//! Minimal binary PPM (P6) writer for captured frames.
//!
//! PPM is used instead of PNG so the demo needs no image-encoding dependency.
//! The format is trivial: a short ASCII header followed by raw RGB bytes, which
//! most image viewers, ImageMagick, and GIMP read directly.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Problems that can occur while encoding or writing a PPM image.
#[derive(Debug, Error)]
pub enum PpmError {
    /// A dimension was zero.
    #[error("image dimensions must be non-zero, got {width}x{height}")]
    ZeroDimension {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The pixel slice does not match the requested dimensions.
    #[error("expected {expected} bytes of RGBA data for {width}x{height}, got {actual}")]
    WrongPixelCount {
        /// Width the buffer was checked against.
        width: u32,
        /// Height the buffer was checked against.
        height: u32,
        /// Required byte count.
        expected: usize,
        /// Byte count provided.
        actual: usize,
    },
    /// The file could not be written.
    #[error("could not write '{path}': {source}")]
    Write {
        /// Destination path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Encodes RGBA8 pixels as a binary PPM image, dropping the alpha channel.
///
/// # Errors
///
/// Returns [`PpmError::ZeroDimension`] or [`PpmError::WrongPixelCount`] if the
/// dimensions and buffer length do not describe a valid image.
pub fn encode_ppm(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, PpmError> {
    if width == 0 || height == 0 {
        return Err(PpmError::ZeroDimension { width, height });
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(PpmError::WrongPixelCount {
            width,
            height,
            expected,
            actual: rgba.len(),
        });
    }

    let mut encoded = Vec::with_capacity(width as usize * height as usize * 3 + 32);
    encoded.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for pixel in rgba.chunks_exact(4) {
        encoded.extend_from_slice(&pixel[..3]);
    }
    Ok(encoded)
}

/// Encodes RGBA8 pixels and writes them to `path`.
///
/// # Errors
///
/// Returns [`PpmError`] if the image is malformed or the file cannot be written.
pub fn write_ppm(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), PpmError> {
    let encoded = encode_ppm(width, height, rgba)?;
    std::fs::write(path, encoded).map_err(|source| PpmError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_pixels_are_encoded_in_order() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128];
        let encoded = encode_ppm(2, 1, &rgba).expect("valid image");
        assert_eq!(&encoded[..11], b"P6\n2 1\n255\n");
        assert_eq!(&encoded[11..], &[255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn malformed_dimensions_are_rejected() {
        assert!(matches!(
            encode_ppm(0, 4, &[]),
            Err(PpmError::ZeroDimension { .. })
        ));
        assert!(matches!(
            encode_ppm(2, 2, &[0, 0, 0, 0]),
            Err(PpmError::WrongPixelCount {
                expected: 16,
                actual: 4,
                ..
            })
        ));
    }

    #[test]
    fn writing_to_a_missing_directory_reports_the_path() {
        let path = Path::new("definitely/not/a/directory/frame.ppm");
        let error = write_ppm(path, 1, 1, &[1, 2, 3, 4]).expect_err("must fail");
        match error {
            PpmError::Write { path: reported, .. } => assert_eq!(reported, path),
            other => panic!("unexpected error: {other}"),
        }
    }
}
