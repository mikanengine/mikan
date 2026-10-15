//! Single-frame readback used for tests, screenshots, and tooling.
//!
//! GPU readback is asynchronous on every backend and rows in a copy buffer must
//! be padded to [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`]. The padding removal and
//! channel swizzling live in [`decode_rows`], which is pure and therefore
//! unit-testable without a GPU.

use wgpu::TextureFormat;

use crate::error::RenderError;

/// CPU-side storage for a captured frame, tightly packed as RGBA8.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CaptureBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl CaptureBuffer {
    /// Creates an empty capture buffer; it is filled by
    /// [`Renderer::render_frame`](crate::Renderer::render_frame).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Width of the captured image in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height of the captured image in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether a frame has been captured into this buffer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// Pixels in RGBA8 order, row-major, top row first.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Color of a single pixel as `[r, g, b, a]`, or `None` if the coordinates
    /// are outside the captured image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y as usize * self.width as usize) + x as usize) * 4;
        let slice = self.pixels.get(index..index + 4)?;
        Some([slice[0], slice[1], slice[2], slice[3]])
    }

    pub(crate) fn prepare(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let length = width as usize * height as usize * 4;
        self.pixels.clear();
        self.pixels.resize(length, 0);
    }

    pub(crate) fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
}

/// Rounds `value` up to the next multiple of `alignment`.
pub(crate) fn align_to(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

/// Copies a padded, possibly BGRA-ordered GPU readback into tightly packed RGBA8.
///
/// `source` must contain at least `bytes_per_row * height` bytes, where
/// `bytes_per_row` is the padded row stride used by the copy. Rows are copied
/// into `destination`, which must hold `width * height * 4` bytes.
///
/// # Errors
///
/// Returns [`RenderError::CaptureFormatUnsupported`] for formats that do not
/// have a defined RGBA8 conversion, or a length error if the buffers are too
/// small for the requested dimensions.
pub(crate) fn decode_rows(
    source: &[u8],
    bytes_per_row: usize,
    width: u32,
    height: u32,
    format: TextureFormat,
    destination: &mut [u8],
) -> Result<(), RenderError> {
    let row_bytes = width as usize * 4;
    let required_source = bytes_per_row * height as usize;
    if source.len() < required_source || destination.len() < row_bytes * height as usize {
        return Err(RenderError::CaptureFormatUnsupported(format));
    }

    let swap_channels = match format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        other => return Err(RenderError::CaptureFormatUnsupported(other)),
    };

    for row in 0..height as usize {
        let source_row = &source[row * bytes_per_row..row * bytes_per_row + row_bytes];
        let destination_row = &mut destination[row * row_bytes..(row + 1) * row_bytes];
        if swap_channels {
            for (source_pixel, destination_pixel) in source_row
                .chunks_exact(4)
                .zip(destination_row.chunks_exact_mut(4))
            {
                destination_pixel[0] = source_pixel[2];
                destination_pixel[1] = source_pixel[1];
                destination_pixel[2] = source_pixel[0];
                destination_pixel[3] = source_pixel[3];
            }
        } else {
            destination_row.copy_from_slice(source_row);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_to_rounds_up_to_alignment() {
        assert_eq!(align_to(0, 256), 0);
        assert_eq!(align_to(1, 256), 256);
        assert_eq!(align_to(256, 256), 256);
        assert_eq!(align_to(257, 256), 512);
    }

    #[test]
    fn decode_strips_row_padding() {
        // Two 2-pixel rows with 4 bytes of padding after each row.
        let bytes_per_row = 8 + 4;
        let source = [
            1, 2, 3, 4, 5, 6, 7, 8, 255, 255, 255, 255, // row 0 + padding
            9, 10, 11, 12, 13, 14, 15, 16, 255, 255, 255, 255, // row 1 + padding
        ];
        let mut destination = [0u8; 16];
        decode_rows(
            &source,
            bytes_per_row,
            2,
            2,
            TextureFormat::Rgba8UnormSrgb,
            &mut destination,
        )
        .unwrap();
        assert_eq!(
            destination,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn decode_swaps_bgra_to_rgba() {
        let source = [10, 20, 30, 40, 50, 60, 70, 80];
        let mut destination = [0u8; 8];
        decode_rows(
            &source,
            8,
            2,
            1,
            TextureFormat::Bgra8UnormSrgb,
            &mut destination,
        )
        .unwrap();
        assert_eq!(destination, [30, 20, 10, 40, 70, 60, 50, 80]);
    }

    #[test]
    fn decode_rejects_unsupported_formats_and_short_buffers() {
        let mut destination = [0u8; 4];
        assert!(matches!(
            decode_rows(&[0; 4], 4, 1, 1, TextureFormat::R16Float, &mut destination),
            Err(RenderError::CaptureFormatUnsupported(
                TextureFormat::R16Float
            ))
        ));
        assert!(matches!(
            decode_rows(
                &[0; 2],
                4,
                1,
                1,
                TextureFormat::Rgba8Unorm,
                &mut destination
            ),
            Err(RenderError::CaptureFormatUnsupported(_))
        ));
    }

    #[test]
    fn capture_buffer_reports_pixels_by_coordinate() {
        let mut buffer = CaptureBuffer::new();
        assert!(buffer.is_empty());
        buffer.prepare(2, 2);
        for (index, byte) in buffer.pixels_mut().iter_mut().enumerate() {
            *byte = index as u8;
        }
        assert_eq!(buffer.width(), 2);
        assert_eq!(buffer.height(), 2);
        assert_eq!(buffer.pixel(0, 0), Some([0, 1, 2, 3]));
        assert_eq!(buffer.pixel(1, 1), Some([12, 13, 14, 15]));
        assert_eq!(buffer.pixel(2, 0), None);
        assert_eq!(buffer.pixel(0, 2), None);
        assert_eq!(buffer.pixels().len(), 16);
    }
}
