use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use eframe::egui;

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchMode {
    Normal,
    Version,
    SmokeTest,
    RuleWorker,
}

impl LaunchMode {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        match arguments {
            [] => Ok(Self::Normal),
            [argument] => match argument.as_str() {
                "--version" | "-V" => Ok(Self::Version),
                "--smoke-test" => Ok(Self::SmokeTest),
                "--rule-worker" => Ok(Self::RuleWorker),
                _ => Err(format!("Unknown argument: {argument}")),
            },
            _ => Err("Use at most one option: --version or --smoke-test.".into()),
        }
    }
}

type Outcome = Arc<Mutex<Option<Result<String, String>>>>;

/// A process-wide deadline also covers failures before the first GUI update,
/// including a blocked graphics-driver initialisation or readback.
pub struct SmokeTest {
    outcome: Outcome,
    cancel_deadline: mpsc::Sender<()>,
}

pub struct SmokeProbe {
    outcome: Outcome,
    screenshot_path: Option<PathBuf>,
    screenshot_requested: bool,
    screenshot_saved: bool,
    pub frames: u32,
    pub completed: bool,
}

impl SmokeTest {
    pub fn start() -> (Self, SmokeProbe) {
        let outcome = Arc::new(Mutex::new(None));
        let (cancel_deadline, deadline) = mpsc::channel();
        std::thread::spawn(move || {
            if matches!(
                deadline.recv_timeout(Duration::from_secs(30)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                eprintln!("Nodiform smoke test FAILED: exceeded the 30-second deadline.");
                std::process::exit(1);
            }
        });
        (
            Self {
                outcome: outcome.clone(),
                cancel_deadline,
            },
            SmokeProbe {
                outcome,
                screenshot_path: std::env::var_os("NODIFORM_SMOKE_SCREENSHOT").map(PathBuf::from),
                screenshot_requested: false,
                screenshot_saved: false,
                frames: 0,
                completed: false,
            },
        )
    }

    pub fn finish(self, gui_error: Option<String>) -> bool {
        let result = self
            .outcome
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Err("Window closed before the smoke test completed.".into()));
        match gui_error.map_or(result, Err) {
            Ok(summary) => {
                println!("Nodiform smoke test PASSED: {summary}");
                true
            }
            Err(error) => {
                eprintln!("Nodiform smoke test FAILED: {error}");
                false
            }
        }
    }
}

impl Drop for SmokeTest {
    fn drop(&mut self) {
        let _ = self.cancel_deadline.send(());
    }
}

impl SmokeProbe {
    pub fn ready(&mut self, tick: u64, nodes: usize, edges: usize) -> bool {
        self.frames += 1;
        // Give both attraction and repulsion time to move generated nodes,
        // independently of the particular example selected for the test.
        self.frames >= 6 && tick >= 96 && nodes >= 4 && edges >= 2
    }

    /// Capture the actual native viewport after the app has verified its graph.
    /// This is opt-in for smoke tests only and never replaces an existing file.
    /// The caller must keep updating the window while this returns `Ok(false)`.
    pub fn capture(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        let Some(path) = &self.screenshot_path else {
            return Ok(true);
        };
        if self.screenshot_saved {
            return Ok(true);
        }
        if !path.is_absolute() {
            return Err("NODIFORM_SMOKE_SCREENSHOT must be an absolute PPM file path.".into());
        }

        if !self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_requested = true;
        } else if let Some(image) = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot {
                    viewport_id, image, ..
                } if *viewport_id == egui::ViewportId::ROOT => Some(image.clone()),
                _ => None,
            })
        }) {
            validate_image(&image).map_err(|error| format!("Invalid UI screenshot: {error}"))?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|error| {
                    format!("Cannot create UI screenshot {}: {error}", path.display())
                })?;
            write_ppm(BufWriter::new(file), &image).map_err(|error| {
                format!("Cannot write UI screenshot {}: {error}", path.display())
            })?;
            self.screenshot_saved = true;
            println!(
                "Saved native UI screenshot: {} ({} × {} pixels)",
                path.display(),
                image.size[0],
                image.size[1]
            );
            return Ok(true);
        }
        ctx.request_repaint();
        Ok(false)
    }

    pub fn complete(&mut self, result: Result<String, String>) {
        if !self.completed {
            *self.outcome.lock().unwrap() = Some(result);
            self.completed = true;
        }
    }
}

fn validate_image(image: &egui::ColorImage) -> io::Result<()> {
    let [width, height] = image.size;
    if width == 0 || height == 0 || width.checked_mul(height) != Some(image.pixels.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pixel count does not match nonzero image dimensions",
        ));
    }
    Ok(())
}

/// P6 PPM preserves all screenshot pixels without adding an image dependency.
fn write_ppm(mut output: impl Write, image: &egui::ColorImage) -> io::Result<()> {
    validate_image(image)?;
    write!(output, "P6\n{} {}\n255\n", image.size[0], image.size[1])?;
    for pixel in &image.pixels {
        output.write_all(&pixel.to_srgba_unmultiplied()[..3])?;
    }
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_modes_are_explicit_and_unambiguous() {
        assert_eq!(LaunchMode::parse(&[]), Ok(LaunchMode::Normal));
        for option in ["--version", "-V"] {
            assert_eq!(LaunchMode::parse(&[option.into()]), Ok(LaunchMode::Version));
        }
        assert_eq!(
            LaunchMode::parse(&["--smoke-test".into()]),
            Ok(LaunchMode::SmokeTest)
        );
        assert_eq!(
            LaunchMode::parse(&["--rule-worker".into()]),
            Ok(LaunchMode::RuleWorker)
        );
        assert!(LaunchMode::parse(&["--unknown".into()]).is_err());
        assert!(LaunchMode::parse(&["--version".into(), "--rule-worker".into()]).is_err());
    }

    #[test]
    fn smoke_requires_gui_frames_and_graph_progress() {
        let (_test, mut probe) = SmokeTest::start();
        for _ in 0..5 {
            assert!(!probe.ready(96, 4, 2));
        }
        assert!(!probe.ready(40, 4, 2));
        assert!(!probe.ready(96, 1, 2));
        assert!(!probe.ready(96, 4, 0));
        assert!(probe.ready(96, 4, 2));
    }

    #[test]
    fn smoke_records_only_the_first_outcome() {
        let (test, mut probe) = SmokeTest::start();
        probe.complete(Err("first failure".into()));
        probe.complete(Ok("must not replace failure".into()));
        assert_eq!(
            test.outcome.lock().unwrap().as_ref(),
            Some(&Err("first failure".into()))
        );
    }

    #[test]
    fn screenshot_preserves_dimensions_and_rgb_pixel_order() {
        let image = egui::ColorImage {
            size: [2, 2],
            pixels: vec![
                egui::Color32::RED,
                egui::Color32::GREEN,
                egui::Color32::BLUE,
                egui::Color32::WHITE,
            ],
        };
        let mut output = Vec::new();
        write_ppm(&mut output, &image).unwrap();
        let mut expected = b"P6\n2 2\n255\n".to_vec();
        expected.extend([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(output, expected);
    }

    #[test]
    fn screenshot_rejects_invalid_dimensions_before_writing() {
        let image = egui::ColorImage {
            size: [2, 2],
            pixels: vec![egui::Color32::RED],
        };
        let mut output = Vec::new();
        assert_eq!(
            write_ppm(&mut output, &image).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(output.is_empty());
    }
}
