//! PNG images decoded to the engine's canonical RGBA8 representation.
//!
//! # What decoding produces
//!
//! [`Image`] is tightly packed RGBA8, row-major, top row first:
//!
//! - **Color space:** the bytes are the ones stored in the file. Standard PNG
//!   files store sRGB-encoded samples, and Mikan does not convert them on the
//!   CPU: the render layer uploads them as an sRGB texture and the GPU decodes
//!   to linear when sampling, which is both correct and higher precision than a
//!   CPU round-trip. `gAMA`/`cHRM` chunks are not interpreted; a file that is
//!   not sRGB is treated as if it were.
//! - **Alpha:** straight (non-premultiplied), preserved from the file,
//!   including `tRNS` transparency and fully transparent pixels.
//! - **Row order:** the first row in memory is the top row of the image, and
//!   `y` grows downward, matching the engine's world space.
//!
//! Every PNG color type (grayscale, grayscale + alpha, RGB, RGBA, palette, with
//! or without `tRNS`, 1–16 bits per sample, interlaced) is normalized to RGBA8
//! by the `png` crate before Mikan converts it; 16-bit samples keep their high
//! byte. Only the first frame of an animated PNG is decoded.
//!
//! # Limits
//!
//! Decoding is bounded by [`MAX_IMAGE_DIMENSION`] per side and allocates with
//! `try_reserve`, so a malformed or hostile file fails with an error instead of
//! aborting the process on an enormous allocation. The decode limit is
//! deliberately larger than common GPU texture limits; the renderer still
//! validates the actual device limit before uploading.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Largest image Mikan will decode, per side.
///
/// Larger than the WebGPU-default texture limit (8192) so that decoding is a
/// property of the asset layer, not of one device; the renderer enforces the
/// device limit separately when the image is uploaded.
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;

/// Why an image could not be produced.
#[derive(Debug, Error)]
pub enum ImageError {
    /// The file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The bytes are not a PNG, are truncated, are corrupt, or declare
    /// dimensions the decoder itself rejects (including zero).
    #[error("could not decode PNG data: {source}")]
    Decode {
        /// Error from the decoder, including its byte offset.
        #[source]
        source: png::DecodingError,
    },

    /// The image is larger than the per-side decode limit.
    #[error("image {width}x{height} exceeds the {max}-pixel decode limit per side")]
    TooLarge {
        /// Declared width.
        width: u32,
        /// Declared height.
        height: u32,
        /// The decode limit that was exceeded.
        max: u32,
    },

    /// The decoded image could not be allocated.
    #[error("could not allocate {bytes} bytes for the decoded image")]
    Allocation {
        /// Bytes requested.
        bytes: u64,
    },

    /// The decoder produced a color type or row shape Mikan does not convert.
    ///
    /// This is defensive: every PNG color type normalizes to 8-bit
    /// grayscale/RGB/RGBA before conversion, so hitting this means the decoder
    /// contract changed.
    #[error("unsupported PNG data after normalization: {0}")]
    UnsupportedColorType(String),
}

/// A decoded RGBA8 image.
///
/// See the [module documentation](self) for the exact representation. The
/// pixels of a standard PNG are ready to be uploaded as an sRGB texture
/// (`TextureEncoding::Srgb` in `mikan-render`) once uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// Decodes `bytes` as a PNG image.
    ///
    /// # Errors
    ///
    /// Returns [`ImageError`] when the bytes are not a decodable PNG (including
    /// zero declared dimensions), when a side is above [`MAX_IMAGE_DIMENSION`],
    /// or when the decoded buffer cannot be allocated.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, ImageError> {
        Self::decode_png_with_limit(bytes, MAX_IMAGE_DIMENSION)
    }

    /// [`Image::decode_png`] with an explicit dimension limit, so the limit
    /// logic itself is testable without constructing a 16k-wide image.
    pub(crate) fn decode_png_with_limit(bytes: &[u8], max: u32) -> Result<Self, ImageError> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        // Expand palette and `tRNS` to RGB(A) and strip 16-bit samples to 8,
        // so the conversion below only ever sees the four 8-bit color types.
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        // Bound the decoder's own buffers too; they are row-sized, but a
        // hostile header should not get to choose them.
        decoder.set_limits(png::Limits {
            bytes: (max as usize)
                .saturating_mul(max as usize)
                .saturating_mul(8),
        });
        let mut reader = decoder
            .read_info()
            .map_err(|source| ImageError::Decode { source })?;

        let (width, height) = reader.info().size();
        if width > max || height > max {
            return Err(ImageError::TooLarge { width, height, max });
        }

        let mut image = Self {
            width,
            height,
            pixels: allocate(u64::from(width) * u64::from(height) * 4)?,
        };
        let row = (width as usize) * 4;

        if reader.info().interlaced {
            // Adam7 needs whole-image output; the decoder de-interlaces it.
            let Some(output_size) = reader.output_buffer_size() else {
                return Err(ImageError::TooLarge { width, height, max });
            };
            let mut buffer = allocate(output_size as u64)?;
            let info = reader
                .next_frame(&mut buffer)
                .map_err(|source| ImageError::Decode { source })?;
            let color = reader.output_color_type().0;
            for y in 0..height as usize {
                let source = &buffer[y * info.line_size..(y + 1) * info.line_size];
                convert_row(source, color, &mut image.pixels[y * row..(y + 1) * row])?;
            }
        } else {
            let color = reader.output_color_type().0;
            let mut y = 0usize;
            while let Some(decoded) = reader
                .next_row()
                .map_err(|source| ImageError::Decode { source })?
            {
                if y >= height as usize {
                    break;
                }
                convert_row(
                    decoded.data(),
                    color,
                    &mut image.pixels[y * row..(y + 1) * row],
                )?;
                y += 1;
            }
        }

        Ok(image)
    }

    /// Reads and decodes a PNG file.
    ///
    /// # Errors
    ///
    /// Returns [`ImageError::Io`] when the file cannot be read, and otherwise
    /// the same failures as [`Image::decode_png`].
    pub fn load_png(path: impl AsRef<Path>) -> Result<Self, ImageError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| ImageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::decode_png(&bytes)
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Size in pixels.
    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// Tightly packed RGBA8 pixels, `width * height * 4` bytes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The RGBA8 pixel at `(x, y)`, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        Some([
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ])
    }
}

/// Allocates a zero-filled buffer, turning allocation failure into an error.
fn allocate(bytes: u64) -> Result<Vec<u8>, ImageError> {
    let bytes_usize = usize::try_from(bytes).map_err(|_| ImageError::Allocation { bytes })?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(bytes_usize)
        .map_err(|_| ImageError::Allocation { bytes })?;
    buffer.resize(bytes_usize, 0);
    Ok(buffer)
}

/// Converts one normalized PNG row to RGBA8.
fn convert_row(
    source: &[u8],
    color: png::ColorType,
    destination: &mut [u8],
) -> Result<(), ImageError> {
    let pixels = destination.len() / 4;
    let required = pixels * color.samples();
    if source.len() < required || !destination.len().is_multiple_of(4) {
        return Err(ImageError::UnsupportedColorType(format!(
            "row of {} bytes cannot fill {pixels} {color:?} pixels",
            source.len()
        )));
    }
    match color {
        png::ColorType::Grayscale => {
            for (value, pixel) in source.iter().zip(destination.chunks_exact_mut(4)) {
                pixel.copy_from_slice(&[*value, *value, *value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for (value, pixel) in source.chunks_exact(2).zip(destination.chunks_exact_mut(4)) {
                pixel.copy_from_slice(&[value[0], value[0], value[0], value[1]]);
            }
        }
        png::ColorType::Rgb => {
            for (value, pixel) in source.chunks_exact(3).zip(destination.chunks_exact_mut(4)) {
                pixel.copy_from_slice(&[value[0], value[1], value[2], 255]);
            }
        }
        png::ColorType::Rgba => {
            destination.copy_from_slice(&source[..destination.len()]);
        }
        png::ColorType::Indexed => {
            return Err(ImageError::UnsupportedColorType("indexed".to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use png::Encoder;
    use std::io::Write as _;

    /// Encodes tightly packed samples of one color type as a non-interlaced
    /// PNG, with a filter chosen by the encoder (`None` for all rows).
    fn encode(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        samples: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(samples).expect("data");
            writer.finish().expect("finish");
        }
        out
    }

    /// The PNG signature.
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// The seven Adam7 passes as `(x_start, y_start, x_step, y_step)`.
    const ADAM7: [(u32, u32, u32, u32); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];

    /// Appends a PNG chunk (length, type, data, CRC).
    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(kind);
        hasher.update(data);
        out.extend_from_slice(&hasher.finalize().to_be_bytes());
    }

    /// Assembles a PNG from a header and raw scanline bytes (filter byte plus
    /// samples per row, in the order the decoder expects them).
    fn hand_crafted_png(
        width: u32,
        height: u32,
        depth: png::BitDepth,
        color: png::ColorType,
        interlaced: bool,
        scanlines: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::from(SIGNATURE);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(depth as u8);
        ihdr.push(color as u8);
        ihdr.extend_from_slice(&[0, 0, u8::from(interlaced)]);
        push_chunk(&mut out, b"IHDR", &ihdr);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(scanlines).expect("compress");
        push_chunk(&mut out, b"IDAT", &encoder.finish().expect("finish"));
        push_chunk(&mut out, b"IEND", &[]);
        out
    }

    /// `filter byte 0 + RGBA pixels` rows for each Adam7 pass of an 8-bit RGBA
    /// image, with `pixel(x, y)` supplying the bytes.
    fn adam7_rgba_scanlines(
        width: u32,
        height: u32,
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        for (x_start, y_start, x_step, y_step) in ADAM7 {
            for y in (y_start..height).step_by(y_step as usize) {
                out.push(0);
                for x in (x_start..width).step_by(x_step as usize) {
                    out.extend_from_slice(&pixel(x, y));
                }
            }
        }
        out
    }

    #[test]
    fn decodes_rgba8_with_size_and_pixels_preserved() {
        let samples = [
            255, 0, 0, 255, // red
            0, 255, 0, 128, // half-transparent green
            0, 0, 255, 0, // fully transparent blue
            10, 20, 30, 40,
        ];
        let image = Image::decode_png(&encode(
            2,
            2,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &samples,
        ))
        .expect("decode");
        assert_eq!(image.size(), [2, 2]);
        assert_eq!(image.pixels(), samples);
        assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(image.pixel(1, 1), Some([10, 20, 30, 40]));
    }

    #[test]
    fn decodes_grayscale_to_opaque_rgb() {
        let image = Image::decode_png(&encode(
            3,
            1,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0, 128, 255],
        ))
        .expect("decode");
        assert_eq!(
            image.pixels(),
            [0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn decodes_grayscale_alpha() {
        let image = Image::decode_png(&encode(
            2,
            1,
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            &[64, 0, 64, 200],
        ))
        .expect("decode");
        assert_eq!(image.pixels(), [64, 64, 64, 0, 64, 64, 64, 200]);
    }

    #[test]
    fn decodes_rgb_to_opaque_rgba() {
        let image = Image::decode_png(&encode(
            2,
            1,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &[1, 2, 3, 200, 201, 202],
        ))
        .expect("decode");
        assert_eq!(image.pixels(), [1, 2, 3, 255, 200, 201, 202, 255]);
    }

    #[test]
    fn decodes_palette_with_trns() {
        let mut out = Vec::new();
        {
            let mut encoder = Encoder::new(&mut out, 3, 1);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(vec![255, 0, 0, 0, 255, 0, 0, 0, 255]);
            encoder.set_trns(vec![255, 128, 0]);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[0, 1, 2]).expect("data");
            writer.finish().expect("finish");
        }
        let image = Image::decode_png(&out).expect("decode");
        assert_eq!(
            image.pixels(),
            [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0],
            "palette expands with tRNS alpha; missing entries are opaque"
        );
    }

    #[test]
    fn sixteen_bit_samples_keep_their_high_byte() {
        let samples = [0x80, 0x01, 0xFF, 0x7F];
        let image = Image::decode_png(&encode(
            2,
            1,
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            &samples,
        ))
        .expect("decode");
        assert_eq!(
            image.pixels(),
            [0x80, 0x80, 0x80, 255, 0xFF, 0xFF, 0xFF, 255]
        );
    }

    #[test]
    fn decodes_interlaced_images_row_by_row() {
        // Distinct color per pixel so a de-interlacing mistake cannot pass.
        let pixel = |x: u32, y: u32| [x as u8 * 16 + 8, y as u8 * 16 + 8, 200, 255];
        let png_bytes = hand_crafted_png(
            8,
            8,
            png::BitDepth::Eight,
            png::ColorType::Rgba,
            true,
            &adam7_rgba_scanlines(8, 8, pixel),
        );
        let image = Image::decode_png(&png_bytes).expect("decode");
        assert_eq!(image.size(), [8, 8]);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(image.pixel(x, y), Some(pixel(x, y)), "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn rejects_bytes_that_are_not_a_png() {
        let error = Image::decode_png(b"not a png at all").expect_err("should fail");
        assert!(matches!(error, ImageError::Decode { .. }), "{error}");
    }

    #[test]
    fn rejects_truncated_files() {
        let mut png_bytes = encode(
            4,
            4,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[7; 4 * 4 * 4],
        );
        png_bytes.truncate(png_bytes.len() / 2);
        let error = Image::decode_png(&png_bytes).expect_err("should fail");
        assert!(matches!(error, ImageError::Decode { .. }), "{error}");
    }

    #[test]
    fn rejects_image_data_that_ends_before_all_rows() {
        // A valid zlib stream holding a single row of a two-row image.
        let row = [0u8, 1, 2, 3, 4, 5, 6, 7, 8];
        let png_bytes = hand_crafted_png(
            2,
            2,
            png::BitDepth::Eight,
            png::ColorType::Rgba,
            false,
            &row,
        );
        let error = Image::decode_png(&png_bytes).expect_err("should fail");
        assert!(matches!(error, ImageError::Decode { .. }), "{error}");
    }

    #[test]
    fn rejects_zero_dimensions_in_the_header() {
        let png_bytes = hand_crafted_png(
            0,
            4,
            png::BitDepth::Eight,
            png::ColorType::Grayscale,
            false,
            &[],
        );
        let error = Image::decode_png(&png_bytes).expect_err("should fail");
        assert!(
            matches!(error, ImageError::Decode { .. }),
            "the decoder rejects zero dimensions before Mikan sees them: {error}"
        );
    }

    #[test]
    fn enforces_the_dimension_limit() {
        let png_bytes = encode(
            4,
            4,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0; 16],
        );
        let error =
            Image::decode_png_with_limit(&png_bytes, 2).expect_err("4x4 exceeds a limit of 2");
        match error {
            ImageError::TooLarge { width, height, max } => {
                assert_eq!((width, height, max), (4, 4, 2));
            }
            other => panic!("expected TooLarge, got {other}"),
        }
    }

    #[test]
    fn decodes_within_the_dimension_limit() {
        let png_bytes = encode(
            4,
            4,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0; 16],
        );
        let image = Image::decode_png_with_limit(&png_bytes, 4).expect("4x4 fits a limit of 4");
        assert_eq!(image.size(), [4, 4]);
    }

    #[test]
    fn pixel_access_is_bounds_checked() {
        let image = Image::decode_png(&encode(
            1,
            1,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[1, 2, 3, 4],
        ))
        .expect("decode");
        assert_eq!(image.pixel(0, 0), Some([1, 2, 3, 4]));
        assert_eq!(image.pixel(1, 0), None);
        assert_eq!(image.pixel(0, 1), None);
        assert_eq!(image.pixel(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn load_png_reports_the_path_that_failed() {
        let path = std::env::temp_dir().join("mikan-assets-missing-file.png");
        let error = Image::load_png(&path).expect_err("file does not exist");
        match error {
            ImageError::Io {
                path: reported,
                source,
            } => {
                assert_eq!(reported, path);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io, got {other}"),
        }
    }

    #[test]
    fn load_png_reads_and_decodes_a_file() {
        let path = std::env::temp_dir().join(format!(
            "mikan-assets-load-{}-{:?}.png",
            std::process::id(),
            std::thread::current().id()
        ));
        let samples = [9, 8, 7, 6];
        std::fs::write(
            &path,
            encode(1, 1, png::ColorType::Rgba, png::BitDepth::Eight, &samples),
        )
        .expect("write test image");
        let image = Image::load_png(&path);
        std::fs::remove_file(&path).expect("clean up test image");
        assert_eq!(image.expect("decode").pixels(), samples);
    }
}
