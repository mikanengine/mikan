//! Device lifecycle, swap chain management, and frame presentation.

use std::sync::{Arc, Mutex, PoisonError};

use mikan_core::{Color, FrameOutcome};
use wgpu::TextureFormat;

use crate::Frame;
use crate::RenderWindow;
use crate::capture::{CaptureBuffer, align_to, decode_rows};
use crate::error::RenderError;
use crate::quad::QuadRenderer;

/// Knobs for renderer initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSettings {
    /// Synchronize presentation with the display refresh rate.
    ///
    /// When `false` the surface uses `AutoNoVsync`, which presents as fast as
    /// possible and may tear. Choose it deliberately, for example to measure
    /// uncapped frame time.
    pub vsync: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { vsync: true }
    }
}

/// Owns the graphics device and a presentable surface for one window.
///
/// The renderer is deliberately the *only* owner of GPU resources. Dropping it
/// releases the device, the surface, and every buffer the quad pipeline owns.
pub struct Renderer {
    window: Arc<dyn RenderWindow>,
    instance: wgpu::Instance,
    /// `None` only if a surface recreation attempt failed; the renderer is
    /// unusable in that case and reports [`RenderError::SurfaceUnavailable`].
    surface: Option<wgpu::Surface<'static>>,
    adapter_info: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    quads: QuadRenderer,
    size: (u32, u32),
    needs_reconfigure: bool,
    capture_supported: bool,
    device_lost: Arc<Mutex<Option<String>>>,
}

impl Renderer {
    /// Creates the graphics device and configures a surface for `window`.
    ///
    /// `size` is the initial drawable size in physical pixels. A zero-sized
    /// window is accepted: the surface is left unconfigured until
    /// [`Renderer::resize`] provides a usable size.
    ///
    /// # Errors
    ///
    /// Fails if no surface can be created for the window, if no adapter supports
    /// that surface, if the adapter exposes no usable configuration, or if the
    /// logical device cannot be created.
    pub fn new(
        window: Arc<dyn RenderWindow>,
        size: (u32, u32),
        settings: RenderSettings,
    ) -> Result<Self, RenderError> {
        // Passing the window as the display handle ties the instance to the same
        // display connection the window uses, which is what the GLES backend
        // requires on Wayland. It is ignored by Vulkan, Metal, and DX12.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window.clone()),
        ));

        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|error| {
            log::error!("no suitable graphics adapter: {error}");
            RenderError::NoAdapter
        })?;

        let adapter_info = adapter.get_info();
        log::info!(
            "adapter: {} ({:?}, {:?})",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.device_type
        );
        if adapter_info.device_type == wgpu::DeviceType::Cpu {
            log::warn!("no hardware acceleration available; using a software rasterizer");
        }

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("mikan device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            }))?;

        let device_lost = Arc::new(Mutex::new(None));
        {
            let device_lost = Arc::clone(&device_lost);
            device.set_device_lost_callback(move |reason, message| {
                log::error!("graphics device lost ({reason:?}): {message}");
                let mut slot = device_lost.lock().unwrap_or_else(PoisonError::into_inner);
                *slot = Some(message);
            });
        }

        let caps = surface.get_capabilities(&adapter);
        let config =
            surface_configuration(&caps, size, settings).ok_or(RenderError::NoSurfaceFormat)?;
        log::info!(
            "surface format {:?}, present mode {:?}, {}x{}",
            config.format,
            config.present_mode,
            config.width,
            config.height
        );

        let capture_supported = config.usage.contains(wgpu::TextureUsages::COPY_SRC);
        let quads = QuadRenderer::new(&device, &queue, config.format);

        let mut renderer = Self {
            window,
            instance,
            surface: Some(surface),
            adapter_info,
            device,
            queue,
            config,
            quads,
            size,
            needs_reconfigure: false,
            capture_supported,
            device_lost,
        };
        renderer.reconfigure();
        Ok(renderer)
    }

    /// Current drawable size in physical pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Format of the swap chain images.
    #[must_use]
    pub fn surface_format(&self) -> TextureFormat {
        self.config.format
    }

    /// Information about the adapter in use, for logs and diagnostics.
    #[must_use]
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Whether frame capture is available on this surface.
    #[must_use]
    pub fn supports_capture(&self) -> bool {
        self.capture_supported
    }

    /// Updates the drawable size and reconfigures the swap chain.
    ///
    /// A zero-sized window (minimized) is recorded but not configured, because
    /// swap chains must have non-zero dimensions. Rendering reports
    /// [`FrameOutcome::Idle`] until a usable size returns.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.size == (width, height) {
            return;
        }
        self.size = (width, height);
        if width == 0 || height == 0 {
            log::debug!("window has zero area; deferring surface configuration");
            return;
        }
        self.reconfigure();
    }

    /// Renders and presents one frame.
    ///
    /// When `capture` is set, the rendered image is also copied back to CPU
    /// memory before presentation (see [`CaptureBuffer`]). This costs a buffer
    /// allocation and a blocking readback, so it is meant for tests, tooling,
    /// and screenshots rather than steady-state rendering.
    ///
    /// Recoverable swap chain conditions are handled here and reported through
    /// the return value:
    ///
    /// | Surface state | Action |
    /// | --- | --- |
    /// | `Success` | draw and present |
    /// | `Suboptimal` | draw, present, reconfigure on the next frame |
    /// | `Timeout` | skip this frame |
    /// | `Occluded` | report idle |
    /// | `Outdated` | reconfigure, skip this frame |
    /// | `Lost` | recreate the surface, reconfigure, skip this frame |
    /// | `Validation` | fail with [`RenderError::SurfaceValidation`] |
    ///
    /// At most one recovery action is taken per frame, so a persistently broken
    /// surface cannot cause an unbounded retry loop inside a single call.
    ///
    /// # Errors
    ///
    /// Fails on unrecoverable conditions: a lost device, a surface that cannot
    /// be recreated, swap chain validation errors, or a failed capture readback.
    pub fn render_frame(
        &mut self,
        frame: &Frame<'_>,
        capture: Option<&mut CaptureBuffer>,
    ) -> Result<FrameOutcome, RenderError> {
        self.check_device_lost()?;

        if self.size.0 == 0 || self.size.1 == 0 {
            return Ok(FrameOutcome::Idle);
        }
        if self.needs_reconfigure {
            log::debug!("reconfiguring surface after suboptimal presentation");
            self.reconfigure();
        }

        let surface_texture = match self.surface()?.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                self.needs_reconfigure = true;
                texture
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::trace!("swap chain acquire timed out; skipping frame");
                return Ok(FrameOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(FrameOutcome::Idle),
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::info!("swap chain is outdated; reconfiguring");
                self.reconfigure();
                return Ok(FrameOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("swap chain was lost; recreating the surface");
                self.recreate_surface()?;
                self.reconfigure();
                return Ok(FrameOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RenderError::SurfaceValidation);
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mikan frame encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mikan frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_wgpu_color(frame.clear_color)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.quads
                .draw(&self.device, &self.queue, &mut pass, frame.quads);
        }

        // Capture must happen before presentation: afterwards the swap chain
        // image belongs to the compositor again.
        let readback = match capture {
            Some(capture) => Some(self.encode_capture(&mut encoder, &surface_texture, capture)?),
            None => None,
        };

        self.queue.submit([encoder.finish()]);

        if let Some(readback) = readback {
            self.complete_capture(readback)?;
        }

        self.queue.present(surface_texture);
        Ok(FrameOutcome::Presented)
    }

    /// Encodes a copy of the swap chain image plus the state needed to complete
    /// the readback after submission.
    fn encode_capture<'a>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_texture: &wgpu::SurfaceTexture,
        capture: &'a mut CaptureBuffer,
    ) -> Result<PendingCapture<'a>, RenderError> {
        if !self.capture_supported {
            return Err(RenderError::CaptureUnsupported);
        }
        let (width, height) = self.size;
        capture.prepare(width, height);

        let bytes_per_row = align_to(
            width as usize * 4,
            wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize,
        );

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan frame capture"),
            size: (bytes_per_row * height as usize) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &surface_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        Ok(PendingCapture {
            buffer,
            bytes_per_row,
            destination: capture,
        })
    }

    /// Blocks until the capture copy has completed, then decodes its rows into
    /// the caller's buffer.
    fn complete_capture(&self, pending: PendingCapture<'_>) -> Result<(), RenderError> {
        let slice = pending.buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            // A closed receiver means the renderer stopped waiting; the mapping
            // still completes and the result is simply dropped.
            let _ = sender.send(result);
        });

        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let result = receiver.recv().map_err(|_| RenderError::CaptureCancelled)?;
        result?;

        let decoded = {
            let mapped = slice.get_mapped_range()?;
            decode_rows(
                &mapped,
                pending.bytes_per_row,
                self.size.0,
                self.size.1,
                self.config.format,
                pending.destination.pixels_mut(),
            )
        };
        // The view must be released before the buffer can be unmapped.
        pending.buffer.unmap();
        decoded
    }

    fn check_device_lost(&self) -> Result<(), RenderError> {
        let lost = self
            .device_lost
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match lost.as_ref() {
            Some(message) => Err(RenderError::DeviceLost(message.clone())),
            None => Ok(()),
        }
    }

    fn surface(&self) -> Result<&wgpu::Surface<'static>, RenderError> {
        self.surface.as_ref().ok_or(RenderError::SurfaceUnavailable)
    }

    /// Applies the current size to the swap chain.
    fn reconfigure(&mut self) {
        if self.size.0 == 0 || self.size.1 == 0 {
            return;
        }
        self.config.width = self.size.0;
        self.config.height = self.size.1;
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        surface.configure(&self.device, &self.config);
        self.quads
            .set_viewport(&self.queue, self.size.0 as f32, self.size.1 as f32);
        self.needs_reconfigure = false;
    }

    /// Recreates the surface from the window handle after a loss.
    fn recreate_surface(&mut self) -> Result<(), RenderError> {
        // Drop the old surface first: two live swap chains for one window are
        // rejected by some backends.
        self.surface = None;
        let surface = self
            .instance
            .create_surface(self.window.clone())
            .map_err(RenderError::SurfaceLost)?;
        self.surface = Some(surface);
        Ok(())
    }
}

/// A capture copy that has been encoded but not yet read back.
struct PendingCapture<'a> {
    buffer: wgpu::Buffer,
    bytes_per_row: usize,
    destination: &'a mut CaptureBuffer,
}

/// Builds a surface configuration from adapter capabilities.
///
/// Prefers an sRGB format, because the clear color and quad colors are linear
/// values and the hardware should encode them for the display.
fn surface_configuration(
    caps: &wgpu::SurfaceCapabilities,
    size: (u32, u32),
    settings: RenderSettings,
) -> Option<wgpu::SurfaceConfiguration> {
    let format = choose_surface_format(&caps.formats)?;
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
        // Enables frame capture for tests and tooling.
        usage |= wgpu::TextureUsages::COPY_SRC;
    }
    if !caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Auto) {
        log::warn!("surface does not support the automatic alpha mode");
    }
    Some(wgpu::SurfaceConfiguration {
        usage,
        format,
        color_space: wgpu::SurfaceColorSpace::Auto,
        width: size.0,
        height: size.1,
        present_mode: present_mode(settings.vsync),
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: Vec::new(),
    })
}

/// Picks the presentation format: the first sRGB format if the surface offers
/// one, otherwise whatever the adapter lists first.
fn choose_surface_format(formats: &[TextureFormat]) -> Option<TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|format| {
            matches!(
                format,
                TextureFormat::Bgra8UnormSrgb | TextureFormat::Rgba8UnormSrgb
            )
        })
        .or_else(|| formats.first().copied())
}

fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync {
        // AutoVsync falls back through FifoRelaxed to Fifo, all of which are
        // tear-free and always supported.
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    }
}

fn to_wgpu_color(color: Color) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(color.r),
        g: f64::from(color.g),
        b: f64::from(color.b),
        a: f64::from(color.a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_formats_are_preferred_but_any_format_is_accepted() {
        assert_eq!(
            choose_surface_format(&[TextureFormat::Bgra8Unorm, TextureFormat::Rgba8UnormSrgb]),
            Some(TextureFormat::Rgba8UnormSrgb)
        );
        assert_eq!(
            choose_surface_format(&[TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba8UnormSrgb]),
            Some(TextureFormat::Bgra8UnormSrgb)
        );
        assert_eq!(
            choose_surface_format(&[TextureFormat::Rgb10a2Unorm]),
            Some(TextureFormat::Rgb10a2Unorm),
            "no sRGB format available: fall back to the adapter's first choice"
        );
        assert_eq!(choose_surface_format(&[]), None);
    }

    #[test]
    fn vsync_selects_the_matching_automatic_present_mode() {
        assert_eq!(present_mode(true), wgpu::PresentMode::AutoVsync);
        assert_eq!(present_mode(false), wgpu::PresentMode::AutoNoVsync);
    }

    #[test]
    fn clear_color_conversion_preserves_components() {
        let converted = to_wgpu_color(Color::new(0.25, 0.5, 0.75, 1.0));
        assert!((converted.r - 0.25).abs() < f64::EPSILON);
        assert!((converted.g - 0.5).abs() < f64::EPSILON);
        assert!((converted.b - 0.75).abs() < f64::EPSILON);
        assert!((converted.a - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn capture_row_stride_is_aligned_to_the_copy_requirement() {
        // 3 pixels => 12 bytes, rounded up to the 256-byte copy alignment.
        assert_eq!(
            align_to(3 * 4, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize),
            wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize
        );
    }
}
