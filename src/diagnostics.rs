use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

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
        // The unchanged ABC example first creates edges at tick 72. Waiting to
        // tick 96 tests attraction as well as node generation and repulsion.
        self.frames >= 6 && tick >= 96 && nodes >= 4 && edges >= 2
    }

    pub fn complete(&mut self, result: Result<String, String>) {
        if !self.completed {
            *self.outcome.lock().unwrap() = Some(result);
            self.completed = true;
        }
    }
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
}
