//! Frame timing and fixed-timestep scheduling.
//!
//! Simulation timing is deliberately separated from presentation timing:
//!
//! - [`Time`] measures how long a frame took on the wall clock and clamps the
//!   result, so that a stalled frame (window drag, debugger breakpoint, laptop
//!   suspend) cannot inject an enormous delta into the simulation.
//! - [`FixedTimestep`] converts that delta into a bounded number of fixed
//!   simulation steps and reports the leftover fraction as an interpolation
//!   alpha for rendering.

use core::time::Duration;
use std::time::Instant;

/// Tracks wall-clock frame timing.
///
/// The first call to [`Time::tick`] reports a zero delta, because no previous
/// frame exists to measure against.
#[derive(Debug)]
pub struct Time {
    start: Instant,
    last: Instant,
    delta: Duration,
    elapsed: Duration,
    frames: u64,
    max_delta: Duration,
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

impl Time {
    /// Creates a clock starting at the current instant with no delta clamp.
    #[must_use]
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            delta: Duration::ZERO,
            elapsed: Duration::ZERO,
            frames: 0,
            max_delta: Duration::MAX,
        }
    }

    /// Creates a clock whose reported delta never exceeds `max_delta`.
    #[must_use]
    pub fn with_max_delta(max_delta: Duration) -> Self {
        Self {
            max_delta,
            ..Self::new()
        }
    }

    /// Advances the clock and returns the delta since the previous tick,
    /// clamped to the configured maximum.
    pub fn tick(&mut self) -> Duration {
        self.tick_at(Instant::now())
    }

    /// The clamped delta reported by the most recent [`Time::tick`].
    #[must_use]
    pub fn delta(&self) -> Duration {
        self.delta
    }

    /// Total time observed since the clock was created, minus any
    /// over-the-limit portion of individual frame deltas.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Number of ticks observed so far.
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frames
    }

    /// Instant at which the clock was created.
    #[must_use]
    pub fn start_instant(&self) -> Instant {
        self.start
    }

    fn tick_at(&mut self, now: Instant) -> Duration {
        if self.frames == 0 {
            // The first tick has no previous frame to measure against. Reporting
            // the time spent since construction instead would inject startup
            // cost into the first frame's delta, for no benefit.
            self.last = now;
            self.frames = 1;
            return Duration::ZERO;
        }

        let raw = now.saturating_duration_since(self.last);
        self.last = now;
        self.delta = raw.min(self.max_delta);
        self.elapsed += self.delta;
        self.frames += 1;
        self.delta
    }
}

/// Converts variable frame deltas into a bounded number of fixed steps.
///
/// The accumulator carries unconsumed time between calls. When a single frame
/// would require more than the configured maximum number of steps, the surplus
/// backlog is discarded and the event is counted in
/// [`FixedTimestep::clamp_count`]. Dropping the backlog rather than replaying it
/// later is what prevents a frame hitch from cascading into a spiral of death.
#[derive(Debug)]
pub struct FixedTimestep {
    step: Duration,
    accumulator: Duration,
    max_steps_per_frame: u32,
    clamp_count: u64,
}

impl FixedTimestep {
    /// Creates a scheduler running `hz` updates per second.
    ///
    /// `hz` is clamped to at least 1 and `max_steps_per_frame` to at least 1, so
    /// construction cannot fail. Validate [`EngineConfig`](crate::EngineConfig)
    /// if you want configuration mistakes to be reported instead.
    #[must_use]
    pub fn new(hz: u32, max_steps_per_frame: u32) -> Self {
        Self {
            step: Duration::from_secs_f64(1.0 / f64::from(hz.max(1))),
            accumulator: Duration::ZERO,
            max_steps_per_frame: max_steps_per_frame.max(1),
            clamp_count: 0,
        }
    }

    /// Duration of a single fixed step.
    #[must_use]
    pub fn step(&self) -> Duration {
        self.step
    }

    /// Maximum number of steps a single [`FixedTimestep::advance`] may return.
    #[must_use]
    pub fn max_steps_per_frame(&self) -> u32 {
        self.max_steps_per_frame
    }

    /// Number of times catch-up has been clamped, for diagnostics.
    ///
    /// A steadily increasing count means frames are arriving slower than the
    /// simulation rate.
    #[must_use]
    pub fn clamp_count(&self) -> u64 {
        self.clamp_count
    }

    /// Adds `delta` to the accumulator and returns how many fixed steps to run.
    ///
    /// The return value is always at most
    /// [`max_steps_per_frame`](Self::max_steps_per_frame).
    pub fn advance(&mut self, delta: Duration) -> u32 {
        self.accumulator += self.min_clamped(delta);
        let step_nanos = self.step.as_nanos();
        let steps = (self.accumulator.as_nanos() / step_nanos).min(u128::from(u32::MAX)) as u32;

        if steps > self.max_steps_per_frame {
            // Drop the backlog instead of replaying it on later frames.
            self.accumulator = Duration::ZERO;
            self.clamp_count += 1;
            return self.max_steps_per_frame;
        }

        self.accumulator -= self.step * steps;
        steps
    }

    /// Fraction of a step that has not been consumed yet, in `[0.0, 1.0)`.
    ///
    /// Renderers use this to interpolate between the previous and current
    /// simulation state, so that a fixed simulation rate still produces smooth
    /// motion at a different display refresh rate.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        let alpha = self.accumulator.as_secs_f32() / self.step.as_secs_f32();
        alpha.clamp(0.0, 1.0)
    }

    fn min_clamped(&self, delta: Duration) -> Duration {
        // `Duration` arithmetic saturates rather than wrapping, but adding an
        // unbounded delta to the accumulator would still defeat the step limit.
        delta.min(self.step * self.max_steps_per_frame.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn millis(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn first_tick_reports_zero_delta() {
        let mut time = Time::new();
        assert_eq!(time.tick(), Duration::ZERO);
        assert_eq!(time.frame_count(), 1);
        assert_eq!(time.elapsed(), Duration::ZERO);
    }

    #[test]
    fn delta_is_measured_between_ticks() {
        let mut time = Time::new();
        let base = time.start;
        time.tick_at(base);
        let delta = time.tick_at(base + millis(16));
        assert_eq!(delta, millis(16));
        assert_eq!(time.delta(), millis(16));
        assert_eq!(time.elapsed(), millis(16));
        assert_eq!(time.frame_count(), 2);
    }

    #[test]
    fn long_frames_are_clamped_but_short_ones_are_not() {
        let mut time = Time::with_max_delta(millis(100));
        let base = time.start;
        time.tick_at(base);
        assert_eq!(time.tick_at(base + millis(20)), millis(20));
        assert_eq!(time.tick_at(base + millis(520)), millis(100));
        // Elapsed time tracks clamped deltas, not raw wall clock time.
        assert_eq!(time.elapsed(), millis(120));
    }

    #[test]
    fn exact_step_produces_exactly_one_update() {
        let mut fixed = FixedTimestep::new(60, 8);
        assert_eq!(fixed.advance(fixed.step()), 1);
        assert_eq!(fixed.alpha(), 0.0);
        assert_eq!(fixed.clamp_count(), 0);
    }

    #[test]
    fn accumulated_remainder_is_carried_between_frames() {
        let mut fixed = FixedTimestep::new(10, 8); // 100 ms per step
        // Four 30 ms frames accumulate to 120 ms: one step, 20 ms of remainder.
        assert_eq!(fixed.advance(millis(30)), 0);
        assert_eq!(fixed.advance(millis(30)), 0);
        assert_eq!(fixed.advance(millis(30)), 0);
        assert_eq!(fixed.advance(millis(30)), 1);
        let alpha = fixed.alpha();
        assert!((alpha - 0.2).abs() < 0.01, "alpha was {alpha}");
    }

    #[test]
    fn alpha_reports_leftover_fraction_and_stays_in_range() {
        let mut fixed = FixedTimestep::new(60, 8);
        let step = fixed.step();
        assert_eq!(fixed.advance(step * 2 + step / 2), 2);
        let alpha = fixed.alpha();
        assert!((alpha - 0.5).abs() < 0.01, "alpha was {alpha}");
        assert!((0.0..1.0).contains(&alpha));
    }

    #[test]
    fn long_frame_is_bounded_and_reports_clamping() {
        let mut fixed = FixedTimestep::new(60, 4);
        let steps = fixed.advance(Duration::from_secs(5));
        assert_eq!(steps, 4);
        assert_eq!(fixed.clamp_count(), 1);
        // The backlog was dropped: the next frame starts from a clean slate.
        assert_eq!(fixed.advance(Duration::ZERO), 0);
        assert_eq!(fixed.alpha(), 0.0);
    }

    #[test]
    fn catch_up_budget_is_never_exceeded() {
        let mut fixed = FixedTimestep::new(120, 3);
        for delta in [millis(1), millis(100), Duration::from_secs(30), millis(7)] {
            assert!(fixed.advance(delta) <= 3);
        }
    }

    #[test]
    fn zero_hz_and_zero_budget_are_clamped_to_one() {
        let mut fixed = FixedTimestep::new(0, 0);
        assert_eq!(fixed.step(), Duration::from_secs(1));
        assert_eq!(fixed.max_steps_per_frame(), 1);
        assert_eq!(fixed.advance(Duration::from_secs(10)), 1);
    }
}
