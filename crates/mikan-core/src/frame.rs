//! The outcome of a frame, shared between the platform and render layers.

/// What happened to a frame the render layer attempted to draw.
///
/// A renderer returns this instead of a bare `Result` so the platform layer can
/// decide whether to keep drawing (and therefore keep the event loop busy) or to
/// idle until the operating system reports that drawing is possible again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was rendered and presented to the display.
    Presented,
    /// The frame was dropped for a recoverable reason, for example a swap chain
    /// acquire timeout or a swap chain reconfiguration. Keep running.
    Skipped,
    /// Drawing is impossible right now, for example because the window is
    /// occluded, minimized, or has zero area. The caller should stop requesting
    /// frames and wait for events instead of spinning.
    Idle,
}

impl FrameOutcome {
    /// Returns `true` when the caller should keep requesting frames.
    #[must_use]
    pub const fn should_continue(self) -> bool {
        matches!(self, Self::Presented | Self::Skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_idle_stops_frame_requests() {
        assert!(FrameOutcome::Presented.should_continue());
        assert!(FrameOutcome::Skipped.should_continue());
        assert!(!FrameOutcome::Idle.should_continue());
    }
}
