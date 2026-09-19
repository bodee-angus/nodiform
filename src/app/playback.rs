use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RunState {
    Running,
    Finalising,
    Complete,
    Stopped,
    Failed,
}

/// Keep the last run's progress after its timeline or encoder has been released.
/// This measures solver work, not node count or elapsed wall-clock time.
pub(super) struct RunProgress {
    pub(super) recording: bool,
    pub(super) total_ticks: u64,
    pub(super) tick: u64,
    pub(super) state: RunState,
    simulation_finished: bool,
}

impl RunProgress {
    pub(super) fn new(recording: bool, total_ticks: u64) -> Self {
        Self {
            recording,
            total_ticks,
            tick: 0,
            state: RunState::Running,
            simulation_finished: false,
        }
    }

    pub(super) fn observe(&mut self, timeline: &crate::timeline::Timeline) {
        self.tick = timeline.tick;
        self.simulation_finished = timeline.finished();
        if self.simulation_finished && self.state == RunState::Running {
            self.state = if self.recording {
                RunState::Finalising
            } else {
                RunState::Complete
            };
        }
    }

    pub(super) fn stop(&mut self, failed: bool) {
        if matches!(self.state, RunState::Complete | RunState::Finalising) && !failed {
            return;
        }
        self.state = if failed {
            RunState::Failed
        } else {
            RunState::Stopped
        };
    }

    pub(super) fn encoding_finished(&mut self, success: bool) {
        if !success {
            self.state = RunState::Failed;
        } else if self.state == RunState::Finalising && self.simulation_finished {
            self.state = RunState::Complete;
        }
    }

    pub(super) fn fraction(&self) -> f32 {
        if self.simulation_finished {
            1.0
        } else if self.total_ticks == 0 {
            0.0
        } else {
            // A trailing mutation can still be pending at the final tick. Do
            // not round to 100% until the timeline has consumed every event.
            (self.tick as f64 / self.total_ticks as f64).min(0.999) as f32
        }
    }
}

/// Wall-clock preview pacing. Keep fractional ticks between monitor refreshes;
/// a stall may slow the preview, but never builds an unbounded catch-up queue.
#[derive(Default)]
pub(super) struct PreviewClock {
    fractional_tick_nanos: u128,
}

impl PreviewClock {
    pub(super) fn ticks(&mut self, elapsed: Duration, ticks_per_second: u32) -> u32 {
        const NANOS_PER_SECOND: u128 = 1_000_000_000;
        const MAX_TICKS_PER_UPDATE: u128 = 240;
        let elapsed = elapsed.min(Duration::from_millis(100));
        let accumulated =
            self.fractional_tick_nanos + elapsed.as_nanos() * u128::from(ticks_per_second);
        self.fractional_tick_nanos = accumulated % NANOS_PER_SECOND;
        (accumulated / NANOS_PER_SECOND).min(MAX_TICKS_PER_UPDATE) as u32
    }

    pub(super) fn reset(&mut self) {
        self.fractional_tick_nanos = 0;
    }
}

/// Physical pixels occupied by the canvas, including fractional desktop scale.
/// Very large windows are scaled uniformly to the device's texture limit.
pub(super) fn preview_dimensions(
    size: eframe::egui::Vec2,
    pixels_per_point: f32,
    texture_limit: u32,
) -> (u32, u32) {
    let limit = texture_limit.max(1) as f64;
    let width = (f64::from(size.x) * f64::from(pixels_per_point))
        .round()
        .max(1.0);
    let height = (f64::from(size.y) * f64::from(pixels_per_point))
        .round()
        .max(1.0);
    let scale = (limit / width.max(height)).min(1.0);
    (
        (width * scale).round().clamp(1.0, limit) as u32,
        (height * scale).round().clamp(1.0, limit) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Event, Graph, Plan},
        timeline::Timeline,
    };

    fn progress_timeline(
        events: Vec<Event>,
        tail: u32,
        recording: bool,
    ) -> (Timeline, RunProgress) {
        let total_ticks = events
            .iter()
            .map(|event| match event {
                Event::Wait { ticks } => u64::from(*ticks),
                _ => 0,
            })
            .sum();
        let progress = RunProgress::new(recording, total_ticks + u64::from(tail));
        let timeline = Timeline::new(
            Plan {
                events,
                total_ticks,
                node_count: 0,
                edge_count: 0,
            },
            tail,
        );
        (timeline, progress)
    }

    fn birth(id: &str) -> Event {
        serde_json::from_value(serde_json::json!({"op":"batch", "nodes":[{"id":id}]})).unwrap()
    }

    #[test]
    fn progress_counts_birth_waits_script_settling_and_application_tail() {
        let (mut timeline, mut progress) = progress_timeline(
            vec![
                birth("A"),
                Event::Wait { ticks: 3 },
                birth("B"),
                Event::Wait { ticks: 3 },
                Event::Wait { ticks: 11 },
            ],
            7,
            false,
        );
        let mut graph = Graph::new(1);
        assert_eq!(progress.total_ticks, 24);
        for (budget, expected_tick) in [(3, 3), (3, 6), (11, 17)] {
            timeline.next(&mut graph, budget).unwrap();
            progress.observe(&timeline);
            assert_eq!(progress.tick, expected_tick);
            assert_eq!(progress.state, RunState::Running);
            assert!((progress.fraction() - expected_tick as f32 / 24.0).abs() < 1e-6);
        }
        // All nodes already exist, but the final application settling ticks
        // still represent real work and cannot be counted as complete early.
        assert_eq!(graph.nodes.len(), 2);
        timeline.next(&mut graph, 7).unwrap();
        progress.observe(&timeline);
        assert_eq!(progress.tick, 24);
        assert_eq!(progress.state, RunState::Complete);
        assert_eq!(progress.fraction(), 1.0);
        progress.stop(false);
        assert_eq!(progress.state, RunState::Complete);
    }

    #[test]
    fn progress_waits_for_trailing_mutations_at_the_last_tick() {
        let (mut timeline, mut progress) =
            progress_timeline(vec![Event::Wait { ticks: 4 }, birth("last")], 0, false);
        let mut graph = Graph::new(1);
        timeline.next(&mut graph, 4).unwrap();
        progress.observe(&timeline);
        assert_eq!(progress.tick, progress.total_ticks);
        assert!(progress.fraction() < 1.0);
        assert_eq!(progress.state, RunState::Running);
        timeline.next(&mut graph, 0).unwrap();
        progress.observe(&timeline);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(progress.fraction(), 1.0);
        assert_eq!(progress.state, RunState::Complete);
    }

    #[test]
    fn zero_duration_progress_is_finite_and_completes_after_events() {
        let (mut timeline, mut progress) =
            progress_timeline(vec![birth("instant"), Event::Wait { ticks: 0 }], 0, false);
        progress.observe(&timeline);
        assert_eq!(progress.fraction(), 0.0);
        timeline.next(&mut Graph::new(1), 0).unwrap();
        progress.observe(&timeline);
        assert_eq!(progress.fraction(), 1.0);
        assert_eq!(progress.state, RunState::Complete);
    }

    #[test]
    fn recording_progress_is_not_complete_until_encoding_finishes() {
        let (mut timeline, mut progress) =
            progress_timeline(vec![Event::Wait { ticks: 4 }], 0, true);
        timeline.next(&mut Graph::new(1), 4).unwrap();
        progress.observe(&timeline);
        assert_eq!(progress.fraction(), 1.0);
        assert_eq!(progress.state, RunState::Finalising);
        progress.stop(false);
        assert_eq!(progress.state, RunState::Finalising);
        progress.encoding_finished(true);
        assert_eq!(progress.state, RunState::Complete);
        progress.encoding_finished(false);
        assert_eq!(progress.state, RunState::Failed);
        progress.encoding_finished(true);
        assert_eq!(progress.state, RunState::Failed);
    }

    #[test]
    fn metadata_failure_survives_successful_video_encoding() {
        let (mut timeline, mut progress) =
            progress_timeline(vec![Event::Wait { ticks: 4 }], 0, true);
        timeline.next(&mut Graph::new(1), 4).unwrap();
        progress.observe(&timeline);
        progress.stop(true);
        progress.encoding_finished(true);
        assert_eq!(progress.state, RunState::Failed);
    }

    #[test]
    fn stopping_and_flushing_a_partial_video_preserves_its_actual_percentage() {
        let (mut timeline, mut progress) =
            progress_timeline(vec![Event::Wait { ticks: 8 }], 0, true);
        timeline.next(&mut Graph::new(1), 2).unwrap();
        progress.observe(&timeline);
        progress.stop(false);
        assert_eq!(progress.fraction(), 0.25);
        progress.encoding_finished(true);
        assert_eq!(progress.state, RunState::Stopped);
        assert_eq!(progress.fraction(), 0.25);
    }

    #[test]
    fn monitor_refresh_rate_does_not_change_preview_speed() {
        for refresh in [30u64, 60, 75, 120, 144, 165, 240, 360] {
            let mut clock = PreviewClock::default();
            let mut previous = 0;
            let mut ticks = 0;
            for frame in 1..=refresh * 10 {
                let now = frame * 1_000_000_000 / refresh;
                ticks += clock.ticks(Duration::from_nanos(now - previous), 240);
                previous = now;
            }
            assert_eq!(ticks, 2400, "refresh rate {refresh}");
        }
    }

    #[test]
    fn irregular_frames_keep_fractional_ticks() {
        let mut clock = PreviewClock::default();
        let frames = [11, 23, 7, 19, 31, 9];
        let ticks: u32 = frames
            .iter()
            .map(|millis| clock.ticks(Duration::from_millis(*millis), 240))
            .sum();
        assert_eq!(ticks, 24);
    }

    #[test]
    fn long_stalls_are_bounded_and_pause_reset_clears_fractional_debt() {
        let mut clock = PreviewClock::default();
        assert_eq!(clock.ticks(Duration::from_secs(10), 240), 24);
        assert_eq!(clock.ticks(Duration::from_secs(10), 28_800), 240);
        assert_eq!(clock.ticks(Duration::ZERO, 240), 0);
        assert_eq!(clock.ticks(Duration::from_millis(3), 240), 0);
        clock.reset();
        assert_eq!(clock.ticks(Duration::from_millis(2), 240), 0);
    }

    #[test]
    fn preview_uses_physical_pixels_and_preserves_aspect_at_device_limit() {
        use eframe::egui::vec2;
        assert_eq!(
            preview_dimensions(vec2(1200.0, 700.0), 1.0, 8192),
            (1200, 700)
        );
        assert_eq!(
            preview_dimensions(vec2(1200.0, 700.0), 2.0, 8192),
            (2400, 1400)
        );
        assert_eq!(
            preview_dimensions(vec2(800.0, 600.0), 1.25, 8192),
            (1000, 750)
        );
        assert_eq!(
            preview_dimensions(vec2(6000.0, 3000.0), 2.0, 8192),
            (8192, 4096)
        );
        assert_eq!(preview_dimensions(vec2(0.0, -1.0), 2.0, 8192), (1, 1));
    }
}
