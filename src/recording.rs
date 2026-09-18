//! Frame-stepped video recording. A full queue returns ownership of the frame;
//! the caller must retry it before advancing the simulation.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUEUED_FRAMES: usize = 2;
const STDERR_BYTES: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const FINALISE_TIMEOUT: Duration = Duration::from_secs(120);
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Exactly `libx264` or `h264_nvenc`. Encoders are never silently replaced.
    pub codec: String,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60,
            codec: "libx264".into(),
        }
    }
}

impl RecordingConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.frame_bytes().map(|_| ())
    }

    fn frame_bytes(&self) -> Result<usize, String> {
        if self.width == 0 || self.height == 0 {
            return Err("Recording width and height must be greater than zero.".into());
        }
        if !self.width.is_multiple_of(2) || !self.height.is_multiple_of(2) {
            return Err("Recording width and height must be even for YUV 4:2:0 video.".into());
        }
        if self.fps == 0 {
            return Err("Recording frame rate must be greater than zero.".into());
        }
        if !matches!(self.codec.as_str(), "libx264" | "h264_nvenc") {
            return Err(format!(
                "Unsupported encoder {:?}. Choose libx264 or h264_nvenc.",
                self.codec
            ));
        }
        let size = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or_else(|| "Recording dimensions exceed addressable frame memory.".to_string())?;
        Ok(size)
    }
}

pub struct Recorder {
    directory: PathBuf,
    frame_bytes: usize,
    frames: u64,
    sender: Option<SyncSender<Vec<u8>>>,
    completion: Receiver<Result<PathBuf, String>>,
    worker: Option<JoinHandle<()>>,
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
    stderr_tail: Arc<Mutex<VecDeque<u8>>>,
    completed: bool,
}

/// Starts the encoder away from the GUI thread. Dropping the job discards its
/// receiver; any recorder subsequently produced is dropped and its child killed.
/// An already-running encoder probe may take up to its ten-second timeout.
pub struct RecorderStartJob {
    receiver: Option<Receiver<Result<Recorder, String>>>,
}

impl RecorderStartJob {
    pub fn start(
        output_dir: PathBuf,
        config: RecordingConfig,
        metadata: Value,
    ) -> Result<Self, String> {
        config.validate()?;
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("nodiform-recorder-start".into())
            .spawn(move || {
                let result = Recorder::start(&output_dir, config, metadata);
                // SendError owns the Recorder, so a cancelled job cannot leave
                // a live encoder orphaned after startup completes.
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Could not start encoder setup worker: {error}"))?;
        Ok(Self {
            receiver: Some(receiver),
        })
    }

    /// Nonblocking; yields the startup result exactly once.
    pub fn poll(&mut self) -> Option<Result<Recorder, String>> {
        let receiver = self.receiver.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The encoder setup worker ended without reporting a result.".into())
            }
        };
        self.receiver.take();
        Some(result)
    }
}

impl Recorder {
    /// Starts one uniquely named session. `metadata` should include the source,
    /// seed, force profile, solver version and simulation-to-video tick ratio.
    /// The selected encoder is tested before any session is created.
    pub fn start(
        output_dir: &Path,
        config: RecordingConfig,
        metadata: Value,
    ) -> Result<Self, String> {
        let frame_bytes = config.frame_bytes()?;
        if !encoders_available()?.contains(&config.codec) {
            return Err(format!(
                "FFmpeg does not provide the selected encoder: {}",
                config.codec
            ));
        }
        verify_encoder(&config.codec)?;
        let directory = create_session_directory(output_dir)?;
        let video = directory.join("simulation.mkv");
        let manifest = json!({
            "format_version": 1,
            "created_unix_millis": unix_millis(),
            "status": "recording",
            "status_scope": "video_encoding",
            "config": &config,
            "metadata": metadata,
            "frames": 0,
            "output": "simulation.mkv",
            "run_outcome_file": "run-outcome.json"
        });
        write_new_json(&directory.join("manifest.json"), &manifest)?;

        let mut command = Command::new("ffmpeg");
        command.args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-nostdin",
            "-n",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
        ]);
        command.arg(format!("{}x{}", config.width, config.height));
        command.arg("-framerate").arg(config.fps.to_string());
        command.args(["-i", "pipe:0", "-an", "-c:v", &config.codec]);
        encoder_options(&mut command, &config.codec);
        command.args([
            "-pix_fmt",
            "yuv420p",
            "-f",
            "matroska",
            "-cluster_time_limit",
            "1000",
            "-flush_packets",
            "1",
        ]);
        command
            .arg(&video)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut process = match command.spawn() {
            Ok(process) => process,
            Err(error) => {
                let message = format!("Could not start FFmpeg: {error}");
                let _ = finalise_manifest(&directory, manifest, 0, Some(&message));
                return Err(message);
            }
        };
        // These handles are guaranteed by Stdio::piped above.
        let stdin = process.stdin.take().expect("piped FFmpeg stdin");
        let stderr = process.stderr.take().expect("piped FFmpeg stderr");
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_BYTES)));
        let reader_tail = Arc::clone(&stderr_tail);
        let stderr_reader = match thread::Builder::new()
            .name("nodiform-ffmpeg-stderr".into())
            .spawn(move || drain_tail(stderr, &reader_tail, STDERR_BYTES))
        {
            Ok(reader) => reader,
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                let message = format!("Could not start FFmpeg diagnostics reader: {error}");
                let _ = finalise_manifest(&directory, manifest, 0, Some(&message));
                return Err(message);
            }
        };
        let child = Arc::new(Mutex::new(process));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(QUEUED_FRAMES);
        let (completion_tx, completion) = mpsc::channel();
        let worker_child = Arc::clone(&child);
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_tail = Arc::clone(&stderr_tail);
        let worker_directory = directory.clone();
        let worker_manifest = manifest.clone();
        let worker = thread::Builder::new()
            .name("nodiform-video-writer".into())
            .spawn(move || {
                let mut written = 0;
                let result = encode_frames(
                    stdin,
                    receiver,
                    &worker_child,
                    &worker_cancelled,
                    &mut written,
                );
                // An encoder error must also close the child, freeing a blocked
                // stdin writer and allowing the stderr reader to reach EOF.
                if result.is_err() {
                    kill_child(&worker_child);
                    let _ = await_exit(&worker_child, Duration::from_secs(5));
                }
                if stderr_reader.is_finished() || child_has_exited(&worker_child) {
                    let _ = stderr_reader.join();
                }
                let result = result.map_err(|message| {
                    let diagnostics = tail_text(&worker_tail);
                    if diagnostics.is_empty() {
                        message
                    } else {
                        format!("{message}\nFFmpeg: {diagnostics}")
                    }
                });
                let manifest_result = finalise_manifest(
                    &worker_directory,
                    worker_manifest,
                    written,
                    result.as_ref().err().map(String::as_str),
                );
                let result = match (result, manifest_result) {
                    (Ok(()), Ok(())) => Ok(video),
                    (Ok(()), Err(error)) => Err(format!(
                        "Video was saved to {}, but its manifest could not be finalised: {error}",
                        video.display()
                    )),
                    (Err(error), Ok(())) => Err(error),
                    (Err(error), Err(manifest_error)) => Err(format!(
                        "{error}\nThe failure manifest could not be saved: {manifest_error}"
                    )),
                };
                let _ = completion_tx.send(result);
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                kill_child(&child);
                let message = format!("Could not start video writer: {error}");
                let _ = finalise_manifest(&directory, manifest, 0, Some(&message));
                return Err(message);
            }
        };
        Ok(Self {
            directory,
            frame_bytes,
            frames: 0,
            sender: Some(sender),
            completion,
            worker: Some(worker),
            child,
            cancelled,
            stderr_tail,
            completed: false,
        })
    }

    /// `Ok(None)` means accepted; `Ok(Some(frame))` means retry this same frame.
    /// Pixels must be tightly packed top-to-bottom RGBA with no row padding.
    /// Calling this never waits for the encoder or the worker thread.
    pub fn try_frame(&mut self, rgba: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
        validate_frame_length(rgba.len(), self.frame_bytes)?;
        if self.completed {
            return Err("This recording has already completed.".into());
        }
        let sender = self.sender.as_ref().ok_or_else(|| {
            "This recording is being finalised and accepts no more frames.".to_string()
        })?;
        match sender.try_send(rgba) {
            Ok(()) => {
                self.frames += 1;
                Ok(None)
            }
            Err(TrySendError::Full(frame)) => Ok(Some(frame)),
            Err(TrySendError::Disconnected(_)) => {
                let diagnostics = tail_text(&self.stderr_tail);
                Err(if diagnostics.is_empty() {
                    "The encoder has stopped. Poll the recording for its final result.".into()
                } else {
                    format!("The encoder has stopped: {diagnostics}")
                })
            }
        }
    }

    /// Saves the simulation's final state independently of encoding success.
    /// Call once before `finish`, with the termination reason and final tick,
    /// event, node and edge counts. Writes the small `run-outcome.json` file
    /// synchronously and never overwrites an existing outcome. The manifest's
    /// status describes video encoding only, so a stopped run may still produce
    /// a successfully completed video. An absent outcome means it is unknown.
    pub fn set_run_outcome(&mut self, outcome: Value) -> Result<(), String> {
        if !outcome.is_object() {
            return Err("The simulation run outcome must be a JSON object.".into());
        }
        write_new_json(&self.directory.join("run-outcome.json"), &outcome)
    }

    /// Closes input without blocking. All accepted frames are drained first.
    /// Keep this Recorder alive and call `poll` until it returns a result.
    pub fn finish(&mut self) {
        self.sender.take();
    }

    /// Returns the final result exactly once. Neither waiting nor joining a
    /// still-running worker occurs on the caller's (usually GUI) thread.
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        if self.completed {
            return None;
        }
        let result = match self.completion.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                kill_child(&self.child);
                Err("The recording worker ended without reporting a result.".into())
            }
        };
        self.completed = true;
        self.sender.take();
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
        Some(result)
    }

    /// Number of frames accepted into the bounded queue.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.sender.take();
        if !self.completed {
            self.cancelled.store(true, Ordering::Release);
            // The writer might be blocked in write_all. Killing the child
            // closes its read side; merely setting a flag would not unblock it.
            kill_child(&self.child);
        }
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

/// Lists supported encoders compiled into the installed FFmpeg. This does not
/// claim that NVIDIA drivers/hardware work: `start` tests the selected encoder.
/// The probe is bounded to ten seconds and has no network or shell access.
pub fn encoders_available() -> Result<Vec<String>, String> {
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-encoders"]);
    let output = run_probe(command, PROBE_TIMEOUT)?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg encoder discovery failed: {}",
            output.stderr
        ));
    }
    let mut encoders = Vec::new();
    for line in output.stdout.lines() {
        let mut words = line.split_whitespace();
        let _flags = words.next();
        if let Some(name) = words.next() {
            if matches!(name, "libx264" | "h264_nvenc") {
                encoders.push(name.to_string());
            }
        }
    }
    // Software first, regardless of FFmpeg's listing order.
    encoders.sort_by_key(|name| if name == "libx264" { 0 } else { 1 });
    encoders.dedup();
    Ok(encoders)
}

fn encoder_options(command: &mut Command, codec: &str) {
    if codec == "libx264" {
        command.args(["-crf", "18", "-preset", "medium"]);
    } else {
        command.args([
            "-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq", "19", "-b:v", "0",
        ]);
    }
}

fn validate_frame_length(actual: usize, expected: usize) -> Result<(), String> {
    if actual != expected {
        Err(format!(
            "Incorrect RGBA frame length: expected {expected} bytes, received {actual}."
        ))
    } else {
        Ok(())
    }
}

fn verify_encoder(codec: &str) -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=256x256:r=1",
        "-frames:v",
        "1",
        "-an",
        "-c:v",
        codec,
    ]);
    encoder_options(&mut command, codec);
    command.args(["-pix_fmt", "yuv420p", "-f", "null", "-"]);
    let output = run_probe(command, PROBE_TIMEOUT)?;
    if !output.status.success() {
        return Err(format!(
            "The selected encoder {codec} failed its runtime test. No recording was started. {}",
            output.stderr.trim()
        ));
    }
    Ok(())
}

fn encode_frames(
    mut stdin: std::process::ChildStdin,
    receiver: Receiver<Vec<u8>>,
    child: &Arc<Mutex<Child>>,
    cancelled: &AtomicBool,
    written: &mut u64,
) -> Result<(), String> {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("Recording was interrupted before finalisation.".into());
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                stdin.write_all(&frame).map_err(|error| {
                    format!("Could not write frame {} to FFmpeg: {error}", *written)
                })?;
                *written += 1;
            }
            Err(RecvTimeoutError::Timeout) => {
                let status = child
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .try_wait()
                    .map_err(|error| format!("Could not inspect FFmpeg: {error}"))?;
                if let Some(status) = status {
                    return Err(format!("FFmpeg exited before input was closed ({status})."));
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    let status = await_exit(child, FINALISE_TIMEOUT)?;
    if cancelled.load(Ordering::Acquire) {
        return Err("Recording was interrupted before finalisation.".into());
    }
    if !status.success() {
        return Err(format!("FFmpeg could not finish the video ({status})."));
    }
    if *written == 0 {
        return Err("The recording contained no frames; no playable video was produced.".into());
    }
    Ok(())
}

fn await_exit(child: &Arc<Mutex<Child>>, timeout: Duration) -> Result<ExitStatus, String> {
    let started = Instant::now();
    loop {
        let status = child
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_wait()
            .map_err(|error| format!("Could not inspect FFmpeg exit status: {error}"))?;
        if let Some(status) = status {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            kill_child(child);
            return Err(format!(
                "FFmpeg did not finish within {} seconds and was stopped.",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn kill_child(child: &Arc<Mutex<Child>>) {
    let mut child = child.lock().unwrap_or_else(|p| p.into_inner());
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
    }
}

fn child_has_exited(child: &Arc<Mutex<Child>>) -> bool {
    matches!(
        child.lock().unwrap_or_else(|p| p.into_inner()).try_wait(),
        Ok(Some(_))
    )
}

fn drain_tail(mut reader: impl Read, tail: &Mutex<VecDeque<u8>>, limit: usize) {
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
                for &byte in &buffer[..count] {
                    if tail.len() == limit {
                        tail.pop_front();
                    }
                    tail.push_back(byte);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn tail_text(tail: &Mutex<VecDeque<u8>>) -> String {
    let bytes: Vec<_> = tail
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .copied()
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

struct ProbeOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!("FFmpeg is unavailable. Install FFmpeg and make it available on PATH. {error}")
        })?;
    let stdout = child.stdout.take().expect("piped probe stdout");
    let stderr = child.stderr.take().expect("piped probe stderr");
    let stdout_tail = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
    let out_tail = Arc::clone(&stdout_tail);
    let err_tail = Arc::clone(&stderr_tail);
    let out_reader = thread::spawn(move || drain_tail(stdout, &out_tail, 128 * 1024));
    let err_reader = thread::spawn(move || drain_tail(stderr, &err_tail, STDERR_BYTES));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_reader.join();
                let _ = err_reader.join();
                return Err(format!(
                    "FFmpeg's encoder probe timed out after {} seconds.",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_reader.join();
                let _ = err_reader.join();
                return Err(format!("Could not inspect FFmpeg's encoder probe: {error}"));
            }
        }
    };
    let _ = out_reader.join();
    let _ = err_reader.join();
    Ok(ProbeOutput {
        status,
        stdout: tail_text(&stdout_tail),
        stderr: tail_text(&stderr_tail),
    })
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn create_session_directory(output_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(output_dir).map_err(|error| {
        format!(
            "Could not create recording destination {}: {error}",
            output_dir.display()
        )
    })?;
    for _ in 0..100 {
        let sequence = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = output_dir.join(format!(
            "run-{}-{}-{sequence}",
            unix_millis(),
            std::process::id()
        ));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create recording session {}: {error}",
                    directory.display()
                ))
            }
        }
    }
    Err("Could not reserve a unique recording session directory.".into())
}

fn write_new_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Could not flush {}: {error}", path.display()))
}

fn finalise_manifest(
    directory: &Path,
    mut manifest: Value,
    frames: u64,
    error: Option<&str>,
) -> Result<(), String> {
    manifest["status"] = json!(if error.is_some() {
        "failed"
    } else {
        "complete"
    });
    manifest["finished_unix_millis"] = json!(unix_millis());
    manifest["frames"] = json!(frames);
    if let Some(error) = error {
        manifest["error"] = json!(error);
    }
    let temporary = directory.join("manifest.final.tmp");
    write_new_json(&temporary, &manifest)?;
    fs::rename(&temporary, directory.join("manifest.json"))
        .map_err(|error| format!("Could not commit final recording manifest: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(width: u32, height: u32, fps: u32) -> RecordingConfig {
        RecordingConfig {
            width,
            height,
            fps,
            codec: "libx264".into(),
        }
    }

    #[test]
    fn rejects_zero_odd_and_overflowing_dimensions() {
        for value in [
            config(0, 64, 30),
            config(64, 0, 30),
            config(63, 64, 30),
            config(64, 63, 30),
            config(64, 64, 0),
            config(u32::MAX - 1, u32::MAX - 1, 30),
        ] {
            assert!(
                value.validate().is_err(),
                "unexpected valid config: {value:?}"
            );
        }
        assert_eq!(config(64, 32, 30).frame_bytes().unwrap(), 64 * 32 * 4);
        assert!(RecordingConfig {
            codec: "anything; echo unsafe".into(),
            ..config(64, 64, 30)
        }
        .validate()
        .is_err());
    }

    #[test]
    fn stderr_is_bounded_and_preserves_tail() {
        let tail = Mutex::new(VecDeque::new());
        drain_tail(std::io::Cursor::new(b"0123456789"), &tail, 4);
        assert_eq!(tail_text(&tail), "6789");
    }

    #[test]
    fn accepts_only_exact_tightly_packed_rgba_lengths() {
        let expected = config(64, 32, 30).frame_bytes().unwrap();
        assert!(validate_frame_length(expected, expected).is_ok());
        assert!(validate_frame_length(expected - 1, expected).is_err());
        assert!(validate_frame_length(expected + 256, expected).is_err());
        assert!(validate_frame_length(0, expected).is_err());
    }

    #[test]
    fn full_queue_returns_the_identical_frame() {
        let (tx, _rx) = mpsc::sync_channel(1);
        tx.try_send(vec![1u8, 2, 3, 4]).unwrap();
        let second = vec![9u8, 8, 7, 6];
        match tx.try_send(second.clone()) {
            Err(TrySendError::Full(returned)) => assert_eq!(returned, second),
            _ => panic!("a full queue must return frame ownership"),
        }
    }

    #[test]
    fn startup_job_reports_completion_only_once() {
        let (sender, receiver) = mpsc::channel();
        let mut job = RecorderStartJob {
            receiver: Some(receiver),
        };
        assert!(job.poll().is_none());
        assert!(sender.send(Err("expected test failure".into())).is_ok());
        match job.poll() {
            Some(Err(error)) => assert_eq!(error, "expected test failure"),
            _ => panic!("startup failure was not delivered"),
        }
        assert!(job.poll().is_none());
    }

    #[test]
    fn startup_job_handles_abandoned_worker() {
        let (sender, receiver) = mpsc::channel();
        let mut job = RecorderStartJob {
            receiver: Some(receiver),
        };
        drop(sender);
        assert!(matches!(job.poll(), Some(Err(_))));
        assert!(job.poll().is_none());
    }

    /// Runs only when FFmpeg with libx264 is installed. No external services,
    /// user directories or existing files are touched.
    #[test]
    fn ffmpeg_records_all_frames_and_commits_manifest() {
        let Ok(encoders) = encoders_available() else {
            return;
        };
        if !encoders.iter().any(|name| name == "libx264") {
            return;
        }
        let test_root = std::env::temp_dir().join(format!(
            "nodiform-recorder-test-{}-{}-{}",
            std::process::id(),
            unix_millis(),
            SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&test_root).unwrap();
        let mut job = RecorderStartJob::start(
            test_root.clone(),
            config(64, 64, 12),
            json!({ "test": true }),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut recorder = loop {
            if let Some(result) = job.poll() {
                break result.unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "encoder startup failed to finish"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert!(job.poll().is_none());
        let session = recorder.directory().to_path_buf();
        assert!(recorder
            .try_frame(vec![0; 3])
            .unwrap_err()
            .contains("frame length"));
        for index in 0..12 {
            let mut frame = vec![0u8; 64 * 64 * 4];
            for pixel in frame.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[index * 20, 80, 160, 255]);
            }
            loop {
                match recorder.try_frame(frame).unwrap() {
                    None => break,
                    Some(returned) => {
                        frame = returned;
                        assert!(Instant::now() < deadline, "encoder stalled");
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }
        assert_eq!(recorder.frames(), 12);
        let outcome = json!({
            "status": "stopped",
            "actual_tick": 24,
            "events": 12,
            "nodes": 3,
            "edges": 2
        });
        assert!(recorder.set_run_outcome(json!("invalid outcome")).is_err());
        recorder.set_run_outcome(outcome.clone()).unwrap();
        assert!(recorder
            .set_run_outcome(json!({ "status": "completed" }))
            .is_err());
        recorder.finish();
        assert!(recorder.try_frame(vec![0; 64 * 64 * 4]).is_err());
        let video = loop {
            if let Some(result) = recorder.poll() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "encoder failed to finish");
            thread::sleep(Duration::from_millis(5));
        };
        assert!(fs::metadata(&video).unwrap().len() > 0);
        let manifest: Value =
            serde_json::from_slice(&fs::read(session.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["frames"], 12);
        assert_eq!(manifest["status"], "complete");
        assert_eq!(manifest["status_scope"], "video_encoding");
        assert_eq!(manifest["run_outcome_file"], "run-outcome.json");
        assert_eq!(manifest["config"]["codec"], "libx264");
        assert_eq!(manifest["metadata"]["test"], true);
        let saved_outcome: Value =
            serde_json::from_slice(&fs::read(session.join("run-outcome.json")).unwrap()).unwrap();
        assert_eq!(saved_outcome, outcome);
        assert!(recorder.poll().is_none());

        // ffprobe is optional, but when present it verifies the actual encoded
        // stream rather than trusting the queue counter or output file size.
        if let Ok(output) = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=nb_read_frames,width,height",
                "-of",
                "json",
            ])
            .arg(&video)
            .output()
        {
            assert!(output.status.success());
            let probe: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(probe["streams"][0]["nb_read_frames"], "12");
            assert_eq!(probe["streams"][0]["width"], 64);
            assert_eq!(probe["streams"][0]["height"], 64);
        }
        drop(recorder);
        fs::remove_dir_all(&test_root).unwrap();
    }
}
