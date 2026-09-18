use std::time::Duration;

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
