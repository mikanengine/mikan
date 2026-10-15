//! Errors raised by the render layer.

use thiserror::Error;

/// Failures that can occur while initializing or driving the renderer.
///
/// Recoverable swap chain conditions (outdated, suboptimal, timeout, occlusion)
/// are handled internally and are *not* errors: they surface as
/// [`FrameOutcome`](mikan_core::FrameOutcome) values instead. Everything in this
/// enum is a real failure that the application should report and act on.
#[derive(Debug, Error)]
pub enum RenderError {
    /// A presentable surface could not be created for the window.
    #[error("could not create a rendering surface: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),

    /// The surface was lost and could not be recreated.
    #[error("the rendering surface was lost and could not be recreated: {0}")]
    SurfaceLost(#[source] wgpu::CreateSurfaceError),

    /// The surface exists but is unusable, for example because a previous
    /// recreation attempt failed.
    #[error("the rendering surface is unavailable")]
    SurfaceUnavailable,

    /// The surface reported a validation error while acquiring a frame. This
    /// indicates a bug in the renderer or a driver fault, not a transient state.
    #[error("the swap chain reported a validation error while acquiring a frame")]
    SurfaceValidation,

    /// No graphics adapter supports the requested surface.
    #[error("no graphics adapter is available for this window")]
    NoAdapter,

    /// The adapter exposes no usable surface format or present mode.
    #[error("the graphics adapter exposes no usable surface configuration")]
    NoSurfaceFormat,

    /// The logical device could not be created.
    #[error("could not create the graphics device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    /// The graphics device was lost and cannot be recovered by reconfiguring.
    #[error("the graphics device was lost: {0}")]
    DeviceLost(String),

    /// Waiting for the graphics device failed.
    #[error("waiting for the graphics device failed: {0}")]
    DevicePoll(#[from] wgpu::PollError),

    /// The back buffer could not be mapped for reading.
    #[error("capturing the frame failed: {0}")]
    CaptureReadback(#[from] wgpu::BufferAsyncError),

    /// The mapped readback range could not be accessed.
    #[error("the captured frame could not be mapped: {0}")]
    CaptureMapRange(#[from] wgpu::MapRangeError),

    /// The readback callback was dropped before reporting a result.
    #[error("the frame capture callback was dropped before reporting a result")]
    CaptureCancelled,

    /// Frame capture was requested, but the surface cannot be copied from.
    #[error("this surface does not support frame capture (missing COPY_SRC usage)")]
    CaptureUnsupported,

    /// The surface format cannot be converted into the RGBA8 layout used by
    /// [`CaptureBuffer`](crate::CaptureBuffer).
    #[error("the surface format {0:?} cannot be captured as RGBA8")]
    CaptureFormatUnsupported(wgpu::TextureFormat),
}
