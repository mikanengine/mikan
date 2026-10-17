//! WAV audio decoded to the engine's canonical f32 representation.
//!
//! # What decoding produces
//!
//! [`SoundData`] holds interleaved f32 samples, one channel after another per
//! frame, starting with the frame at the beginning of the file:
//!
//! - **Integers are normalized to `-1.0..=1.0`.** Unsigned 8-bit samples map
//!   `0..=255` to `-1.0..=127/128`, and signed samples divide by the negative
//!   power of two (`i16 / 32768`, `i24 / 8388608`, `i32 / 2147483648`), so
//!   full-scale negative is exactly `-1.0` and the largest positive value
//!   stays below `1.0`.
//! - **Floats are preserved.** 32-bit float samples are used as stored and
//!   64-bit samples are rounded to the nearest f32; values outside
//!   `-1.0..=1.0` (including infinities and NaN) are kept, and clipping is
//!   the mixer's job at the output stage.
//! - **Rates are validated here.** The mixer renders at one rate and
//!   resamples each voice, so a sound may use any rate in `8000..=192000`;
//!   anything else is rejected at decode time.
//!
//! # The supported subset
//!
//! RIFF/WAVE files (little-endian, `RIFF`+`WAVE`), with these sample
//! formats: unsigned 8-bit PCM, signed 16-, 24-, and 32-bit PCM, and 32- and
//! 64-bit IEEE float — either as the plain format tag or wrapped in
//! `WAVE_FORMAT_EXTENSIBLE` with the standard subformat GUID. Mono and
//! stereo are the only layouts (the engine's bus is stereo). Unknown chunks
//! are skipped, chunk padding is honored, and the `fmt` and `data` chunks may
//! appear in either order.
//!
//! Everything else fails with a descriptive [`SoundError`]: more than two
//! channels, rates outside the range, 20-bit or 12-bit depths, ADPCM and
//! other compressed tags, extensible containers whose valid bits differ from
//! the container width (for example 24-in-32), truncated chunks, chunk sizes
//! that overrun the file, and data that is not a whole number of frames.
//!
//! # Limits
//!
//! Decoding is bounded by [`MAX_SOUND_FRAMES`] and allocates with
//! `try_reserve`, so a hostile or mistaken header cannot abort the process
//! on an enormous allocation. At 48 kHz the limit is about 5 minutes and
//! 49 seconds per sound.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Longest sound Mikan will decode, in frames (per channel).
///
/// The mixer's sound registry stores decoded PCM, so this bounds one sound's
/// memory at roughly `MAX_SOUND_FRAMES * 2 * 4` bytes. At 48 kHz it is about
/// 5 minutes and 49 seconds.
pub const MAX_SOUND_FRAMES: u32 = 1 << 24;

/// The mixer's documented sample-rate range, enforced at decode time so a
/// rate the mixer would reject never reaches it.
const MIN_SAMPLE_RATE: u32 = 8_000;
/// The mixer's documented sample-rate range, enforced at decode time.
const MAX_SAMPLE_RATE: u32 = 192_000;

/// The plain PCM format tag.
const FORMAT_PCM: u16 = 1;
/// The IEEE float format tag.
const FORMAT_FLOAT: u16 = 3;
/// The extensible format tag, whose real tag lives in the subformat GUID.
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// The `KSDATAFORMAT_SUBTYPE_PCM` / `..._IEEE_FLOAT` GUID bytes after the
/// first two (which carry the format tag itself).
const SUBFORMAT_GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// Why a sound could not be produced.
#[derive(Debug, Error)]
pub enum SoundError {
    /// The file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The bytes do not start with a little-endian RIFF/WAVE header.
    #[error("the data is not a RIFF/WAVE file")]
    NotWave,

    /// The bytes are structurally wrong: a truncated chunk, a chunk size
    /// that overruns the file, or data that is not a whole number of frames.
    #[error("malformed WAV data: {detail}")]
    Malformed {
        /// What is wrong, in one sentence.
        detail: String,
    },

    /// There is no `fmt` chunk.
    #[error("the fmt chunk is missing")]
    MissingFormat,

    /// There is no `data` chunk, or it contains no frames.
    #[error("no audio frames were found")]
    MissingData,

    /// The file is valid WAV but uses a feature outside the supported
    /// subset (see the [module documentation](self)).
    #[error("unsupported WAV data: {detail}")]
    Unsupported {
        /// What is unsupported, in one sentence.
        detail: String,
    },

    /// The layout has more than two channels.
    #[error("{channels} channels: only mono and stereo are supported")]
    UnsupportedChannels {
        /// Channel count from the file.
        channels: u32,
    },

    /// The sample rate is outside the mixer's range.
    #[error("sample rate {sample_rate} is outside 8000..=192000")]
    UnsupportedSampleRate {
        /// Sample rate from the file.
        sample_rate: u32,
    },

    /// The sound is longer than [`MAX_SOUND_FRAMES`].
    #[error("the sound has {frames} frames, more than the {max}-frame limit")]
    TooLarge {
        /// Frame count from the header.
        frames: u64,
        /// The decode limit that was exceeded.
        max: u32,
    },

    /// The decoded samples could not be allocated.
    #[error("could not allocate {bytes} bytes for the decoded sound")]
    Allocation {
        /// Bytes requested.
        bytes: u64,
    },
}

/// Decoded interleaved f32 samples with their layout.
///
/// See the [module documentation](self) for the exact representation. This is
/// the audio counterpart of [`Image`](crate::Image): CPU-side decoded data
/// that the audio system copies into the mixer, with no device involved.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundData {
    samples: Vec<f32>,
    channels: u32,
    sample_rate: u32,
}

impl SoundData {
    /// Decodes `bytes` as a WAV file.
    ///
    /// # Errors
    ///
    /// Returns [`SoundError`] when the bytes are not RIFF/WAVE, when a chunk
    /// is truncated or overruns the file, when the format is outside the
    /// supported subset, when the sample rate is outside `8000..=192000`,
    /// when the sound exceeds [`MAX_SOUND_FRAMES`], or when the decoded
    /// buffer cannot be allocated.
    pub fn decode_wav(bytes: &[u8]) -> Result<Self, SoundError> {
        Self::decode_wav_with_limit(bytes, MAX_SOUND_FRAMES)
    }

    /// [`SoundData::decode_wav`] with an explicit frame limit, so the limit
    /// logic itself is testable without constructing a five-minute file.
    pub(crate) fn decode_wav_with_limit(bytes: &[u8], max_frames: u32) -> Result<Self, SoundError> {
        let (format, data) = parse_wave(bytes)?;
        let format = format.ok_or(SoundError::MissingFormat)?;
        let data = data.ok_or(SoundError::MissingData)?;
        if data.is_empty() {
            return Err(SoundError::MissingData);
        }
        decode_samples(format, data, max_frames)
    }

    /// Reads and decodes a WAV file.
    ///
    /// # Errors
    ///
    /// Returns [`SoundError::Io`] when the file cannot be read, and otherwise
    /// the same failures as [`SoundData::decode_wav`].
    pub fn load_wav(path: impl AsRef<Path>) -> Result<Self, SoundError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| SoundError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::decode_wav(&bytes)
    }

    /// Takes ownership of already-decoded samples, for sounds that are
    /// generated rather than loaded (a blip, a test tone, an editor preview).
    ///
    /// # Errors
    ///
    /// Returns [`SoundError`] when `channels` is not 1 or 2, when
    /// `sample_rate` is outside `8000..=192000`, when `samples` is empty, or
    /// when its length is not a whole number of frames.
    pub fn from_samples(
        samples: Vec<f32>,
        channels: u32,
        sample_rate: u32,
    ) -> Result<Self, SoundError> {
        if channels != 1 && channels != 2 {
            return Err(SoundError::UnsupportedChannels { channels });
        }
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(SoundError::UnsupportedSampleRate { sample_rate });
        }
        if samples.is_empty() {
            return Err(SoundError::MissingData);
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(SoundError::Malformed {
                detail: format!(
                    "{} samples do not make whole {channels}-channel frames",
                    samples.len()
                ),
            });
        }
        Ok(Self {
            samples,
            channels,
            sample_rate,
        })
    }

    /// Interleaved f32 samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// 1 or 2.
    #[must_use]
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// The rate the samples were recorded at, `8000..=192000`.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The number of frames (sample groups), `samples().len() / channels()`.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

/// A `fmt` chunk's normalized contents.
#[derive(Debug, Clone, Copy)]
struct WaveFormat {
    /// Resolved to `FORMAT_PCM` or `FORMAT_FLOAT`.
    format: u16,
    channels: u32,
    sample_rate: u32,
    bits: u16,
    /// Bytes per sample: 1, 2, 3, 4, or 8.
    bytes_per_sample: usize,
}

/// One field of a RIFF chunk.
fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// One field of a RIFF chunk.
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn malformed(detail: impl Into<String>) -> SoundError {
    SoundError::Malformed {
        detail: detail.into(),
    }
}

fn unsupported(detail: impl Into<String>) -> SoundError {
    SoundError::Unsupported {
        detail: detail.into(),
    }
}

/// Walks the RIFF chunks and returns the `fmt` and `data` chunk bodies if
/// present. Chunk order does not matter; unknown chunks are skipped.
fn parse_wave(bytes: &[u8]) -> Result<(Option<WaveFormat>, Option<&[u8]>), SoundError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(SoundError::NotWave);
    }
    let declared = u32_at(bytes, 4).expect("length checked") as usize + 8;
    if declared > bytes.len() {
        return Err(malformed(format!(
            "the RIFF header declares {declared} bytes but the file holds {}",
            bytes.len()
        )));
    }

    let mut format = None;
    let mut data = None;
    let mut offset = 12;
    while offset + 8 <= declared {
        let id = &bytes[offset..offset + 4];
        let size = u32_at(bytes, offset + 4).expect("checked") as usize;
        let body_start = offset + 8;
        let Some(body_end) = body_start.checked_add(size) else {
            return Err(malformed("a chunk size overflows the address space"));
        };
        if body_end > declared {
            return Err(malformed(format!(
                "a {:?} chunk of {size} bytes overruns the file",
                String::from_utf8_lossy(id)
            )));
        }
        match id {
            b"fmt " => format = Some(parse_format(&bytes[body_start..body_end])?),
            b"data" => data = Some(&bytes[body_start..body_end]),
            _ => {}
        }
        offset = body_end + (size & 1); // chunks are word-aligned
    }
    Ok((format, data))
}

/// Parses and validates one `fmt` chunk body.
fn parse_format(body: &[u8]) -> Result<WaveFormat, SoundError> {
    if body.len() < 16 {
        return Err(malformed(format!(
            "the fmt chunk is {} bytes, shorter than the 16-byte base",
            body.len()
        )));
    }
    let tag = u16_at(body, 0).expect("checked");
    let channels = u32::from(u16_at(body, 2).expect("checked"));
    let sample_rate = u32_at(body, 4).expect("checked");
    let block_align = u16_at(body, 12).expect("checked");
    let bits = u16_at(body, 14).expect("checked");

    let format = match tag {
        FORMAT_PCM | FORMAT_FLOAT => tag,
        FORMAT_EXTENSIBLE => {
            if body.len() < 40 {
                return Err(malformed(format!(
                    "an extensible fmt chunk is {} bytes, shorter than the 40-byte layout",
                    body.len()
                )));
            }
            let extension = u16_at(body, 16).expect("checked");
            if extension < 22 {
                return Err(malformed(format!(
                    "the extensible fmt chunk declares {extension} bytes of extension data"
                )));
            }
            let valid_bits = u16_at(body, 18).expect("checked");
            if valid_bits != 0 && valid_bits != bits {
                return Err(unsupported(format!(
                    "{valid_bits} valid bits in a {bits}-bit container"
                )));
            }
            let subformat = &body[24..40];
            if subformat[2..] != SUBFORMAT_GUID_TAIL {
                return Err(unsupported(
                    "an extensible fmt chunk whose subformat is not the standard PCM or float GUID",
                ));
            }
            match u16::from_le_bytes([subformat[0], subformat[1]]) {
                FORMAT_PCM | FORMAT_FLOAT => u16::from_le_bytes([subformat[0], subformat[1]]),
                other => {
                    return Err(unsupported(format!(
                        "extensible audio format {other}, which is not PCM or IEEE float"
                    )));
                }
            }
        }
        other => {
            return Err(unsupported(format!(
                "audio format {other}, which is not PCM or IEEE float"
            )));
        }
    };

    if channels != 1 && channels != 2 {
        return Err(SoundError::UnsupportedChannels { channels });
    }
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(SoundError::UnsupportedSampleRate { sample_rate });
    }
    let bytes_per_sample = match (format, bits) {
        (FORMAT_PCM, 8) => 1,
        (FORMAT_PCM, 16) => 2,
        (FORMAT_PCM, 24) => 3,
        (FORMAT_PCM, 32) => 4,
        (FORMAT_FLOAT, 32) => 4,
        (FORMAT_FLOAT, 64) => 8,
        (_, bits) => {
            let kind = if format == FORMAT_FLOAT {
                "float"
            } else {
                "PCM"
            };
            return Err(unsupported(format!("{bits}-bit {kind} samples")));
        }
    };
    let expected_align = channels as usize * bytes_per_sample;
    if block_align as usize != expected_align {
        return Err(malformed(format!(
            "the fmt chunk declares a block align of {block_align} bytes for {channels} channel(s) of {bits}-bit samples (expected {expected_align})"
        )));
    }
    Ok(WaveFormat {
        format,
        channels,
        sample_rate,
        bits,
        bytes_per_sample,
    })
}

/// Turns one validated `data` chunk body into samples.
fn decode_samples(
    format: WaveFormat,
    data: &[u8],
    max_frames: u32,
) -> Result<SoundData, SoundError> {
    let frame_bytes = format.channels as usize * format.bytes_per_sample;
    if !data.len().is_multiple_of(frame_bytes) {
        return Err(malformed(format!(
            "the data chunk's {} bytes do not make whole {frame_bytes}-byte frames",
            data.len()
        )));
    }
    let frames = data.len() / frame_bytes;
    if frames as u64 > u64::from(max_frames) {
        return Err(SoundError::TooLarge {
            frames: frames as u64,
            max: max_frames,
        });
    }

    let sample_count = frames * format.channels as usize;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(sample_count)
        .map_err(|_| SoundError::Allocation {
            bytes: sample_count as u64 * 4,
        })?;
    for sample in data.chunks_exact(format.bytes_per_sample) {
        samples.push(sample_to_f32(
            sample,
            format.format,
            format.bits,
            format.bytes_per_sample,
        ));
    }
    debug_assert_eq!(samples.len(), sample_count);
    Ok(SoundData {
        samples,
        channels: format.channels,
        sample_rate: format.sample_rate,
    })
}

/// Converts one stored sample to the canonical f32 normalization.
fn sample_to_f32(bytes: &[u8], format: u16, bits: u16, bytes_per_sample: usize) -> f32 {
    debug_assert_eq!(bytes.len(), bytes_per_sample);
    if format == FORMAT_FLOAT {
        if bits == 32 {
            f32::from_le_bytes(bytes.try_into().expect("4 bytes"))
        } else {
            f64::from_le_bytes(bytes.try_into().expect("8 bytes")) as f32
        }
    } else {
        match bits {
            8 => (f32::from(bytes[0]) - 128.0) / 128.0,
            16 => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
            24 => {
                // Sign-extend the 24-bit value in an i32.
                let raw =
                    i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
                let value = (raw << 8) >> 8;
                value as f32 / 8_388_608.0
            }
            _ => {
                let raw = i32::from_le_bytes(bytes.try_into().expect("4 bytes"));
                raw as f32 / 2_147_483_648.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps chunk bytes in a RIFF/WAVE header with a correct size.
    fn wav_file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::from(*b"WAVE");
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut out = Vec::from(*b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// A chunk: id, little-endian size, body, and the word-align pad byte.
    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + body.len() + 1);
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if !body.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }

    /// A base `fmt` chunk body for a plain PCM/float tag.
    fn fmt_body(tag: u16, channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let bytes_per_sample = bits.div_ceil(8);
        let align = channels * bytes_per_sample;
        let mut body = Vec::new();
        body.extend_from_slice(&tag.to_le_bytes());
        body.extend_from_slice(&channels.to_le_bytes());
        body.extend_from_slice(&sample_rate.to_le_bytes());
        body.extend_from_slice(&(sample_rate * u32::from(align)).to_le_bytes());
        body.extend_from_slice(&align.to_le_bytes());
        body.extend_from_slice(&bits.to_le_bytes());
        body
    }

    /// A complete WAV file with one `fmt` and one `data` chunk.
    fn simple_wav(tag: u16, channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        wav_file(&[
            chunk(b"fmt ", &fmt_body(tag, channels, sample_rate, bits)),
            chunk(b"data", data),
        ])
    }

    /// An extensible `fmt` chunk body with the standard subformat GUID.
    fn extensible_fmt_body(
        sub_tag: u16,
        channels: u16,
        sample_rate: u32,
        bits: u16,
        valid_bits: u16,
    ) -> Vec<u8> {
        let mut body = fmt_body(FORMAT_EXTENSIBLE, channels, sample_rate, bits);
        body.extend_from_slice(&22u16.to_le_bytes());
        body.extend_from_slice(&valid_bits.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&sub_tag.to_le_bytes());
        body.extend_from_slice(&SUBFORMAT_GUID_TAIL);
        body
    }

    fn i16_bytes(values: &[i16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn decodes_mono_pcm16_normalized() {
        let data = i16_bytes(&[-32_768, -16_384, 0, 16_384, 32_767]);
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_PCM, 1, 48_000, 16, &data)).expect("decode");
        assert_eq!(sound.channels(), 1);
        assert_eq!(sound.sample_rate(), 48_000);
        assert_eq!(sound.frames(), 5);
        assert_eq!(
            sound.samples(),
            &[-1.0, -0.5, 0.0, 0.5, 32_767.0 / 32_768.0][..],
            "full-scale negative is exactly -1; the largest positive stays below 1"
        );
    }

    #[test]
    fn decodes_stereo_interleaved_in_frame_order() {
        let data = i16_bytes(&[10, -10, 20, -20]);
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_PCM, 2, 44_100, 16, &data)).expect("decode");
        assert_eq!(sound.channels(), 2);
        assert_eq!(sound.frames(), 2);
        assert_eq!(
            sound.samples(),
            &[
                10.0 / 32_768.0,
                -10.0 / 32_768.0,
                20.0 / 32_768.0,
                -20.0 / 32_768.0
            ][..]
        );
    }

    #[test]
    fn decodes_unsigned_8_bit_centered_at_128() {
        let sound = SoundData::decode_wav(&simple_wav(FORMAT_PCM, 1, 8_000, 8, &[0, 128, 255]))
            .expect("decode");
        assert_eq!(sound.samples(), &[-1.0, 0.0, 127.0 / 128.0][..]);
    }

    #[test]
    fn decodes_signed_24_bit_with_sign_extension() {
        let data = [
            0x00, 0x00, 0x80, // -8388608
            0xFF, 0xFF, 0xFF, // -1
            0xFF, 0xFF, 0x7F, // 8388607
        ];
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_PCM, 1, 48_000, 24, &data)).expect("decode");
        assert_eq!(
            sound.samples(),
            &[-1.0, -1.0 / 8_388_608.0, 8_388_607.0 / 8_388_608.0][..]
        );
    }

    #[test]
    fn decodes_signed_32_bit() {
        let data: Vec<u8> = [i32::MIN, -1, i32::MAX]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_PCM, 1, 48_000, 32, &data)).expect("decode");
        assert_eq!(
            sound.samples(),
            &[
                -1.0,
                -1.0 / 2_147_483_648.0,
                // i32::MAX rounds to 2^31 as f32, so the normalized value is
                // exactly 1.0.
                1.0
            ][..]
        );
    }

    #[test]
    fn decodes_float32_samples_as_stored() {
        let data: Vec<u8> = [0.25_f32, -1.5, 3.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_FLOAT, 1, 48_000, 32, &data)).expect("decode");
        assert_eq!(
            sound.samples(),
            &[0.25, -1.5, 3.0][..],
            "out-of-range and negative values are preserved; the mixer clips later"
        );
    }

    #[test]
    fn decodes_float64_samples_narrowed_to_f32() {
        let data: Vec<u8> = [0.1_f64, 1e300_f64, f64::NAN]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let sound =
            SoundData::decode_wav(&simple_wav(FORMAT_FLOAT, 1, 48_000, 64, &data)).expect("decode");
        let samples = sound.samples();
        assert_eq!(samples[0], 0.1_f32, "0.1 keeps its nearest f32 value");
        assert!(samples[1].is_infinite() && samples[1] > 0.0);
        assert!(samples[2].is_nan());
    }

    #[test]
    fn accepts_extensible_pcm_and_float() {
        let pcm = wav_file(&[
            chunk(b"fmt ", &extensible_fmt_body(FORMAT_PCM, 1, 48_000, 16, 16)),
            chunk(b"data", &i16_bytes(&[32_767])),
        ]);
        let sound = SoundData::decode_wav(&pcm).expect("extensible PCM decodes");
        assert_eq!(sound.samples(), &[32_767.0 / 32_768.0][..]);

        let float = {
            let mut data = Vec::new();
            data.extend_from_slice(&0.5_f32.to_le_bytes());
            data.extend_from_slice(&0.5_f32.to_le_bytes());
            wav_file(&[
                chunk(
                    b"fmt ",
                    &extensible_fmt_body(FORMAT_FLOAT, 2, 48_000, 32, 32),
                ),
                chunk(b"data", &data),
            ])
        };
        let sound = SoundData::decode_wav(&float).expect("extensible float decodes");
        assert_eq!(sound.samples(), &[0.5, 0.5][..]);
    }

    #[test]
    fn rejects_extensible_valid_bits_that_differ_from_the_container() {
        let bytes = wav_file(&[
            chunk(b"fmt ", &extensible_fmt_body(FORMAT_PCM, 1, 48_000, 32, 24)),
            chunk(b"data", &[0; 4]),
        ]);
        match SoundData::decode_wav(&bytes) {
            Err(SoundError::Unsupported { detail }) => {
                assert!(detail.contains("24 valid bits"), "{detail}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_nonstandard_extensible_subformat_guid() {
        let mut body = extensible_fmt_body(FORMAT_PCM, 1, 48_000, 16, 16);
        let last = body.len() - 1;
        body[last] ^= 0xFF;
        let bytes = wav_file(&[chunk(b"fmt ", &body), chunk(b"data", &[0, 0])]);
        match SoundData::decode_wav(&bytes) {
            Err(SoundError::Unsupported { detail }) => {
                assert!(detail.contains("subformat"), "{detail}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn rejects_more_than_two_channels() {
        let bytes = simple_wav(FORMAT_PCM, 3, 48_000, 16, &[0; 6]);
        match SoundData::decode_wav(&bytes) {
            Err(SoundError::UnsupportedChannels { channels }) => assert_eq!(channels, 3),
            other => panic!("expected UnsupportedChannels, got {other:?}"),
        }
    }

    #[test]
    fn rejects_rates_outside_the_mixers_range() {
        for rate in [0, 7_999, 192_001] {
            let bytes = simple_wav(FORMAT_PCM, 1, rate, 16, &[0, 0]);
            match SoundData::decode_wav(&bytes) {
                Err(SoundError::UnsupportedSampleRate { sample_rate }) => {
                    assert_eq!(sample_rate, rate);
                }
                other => panic!("expected UnsupportedSampleRate for {rate}, got {other:?}"),
            }
        }
        for rate in [8_000, 192_000] {
            SoundData::decode_wav(&simple_wav(FORMAT_PCM, 1, rate, 16, &[0, 0]))
                .unwrap_or_else(|error| panic!("{rate} is in range: {error}"));
        }
    }

    #[test]
    fn rejects_unsupported_bit_depths_and_compressed_formats() {
        for (tag, bits) in [(FORMAT_PCM, 20), (FORMAT_FLOAT, 16), (FORMAT_PCM, 12)] {
            let bytes = simple_wav(tag, 1, 48_000, bits, &[0; 8]);
            assert!(
                matches!(
                    SoundData::decode_wav(&bytes),
                    Err(SoundError::Unsupported { .. })
                ),
                "{bits}-bit tag {tag} must be unsupported"
            );
        }
        for tag in [0x0002_u16, 0x0011, 0x0055] {
            let bytes = simple_wav(tag, 1, 48_000, 16, &[0; 8]);
            assert!(
                matches!(
                    SoundData::decode_wav(&bytes),
                    Err(SoundError::Unsupported { .. })
                ),
                "format tag {tag} must be unsupported"
            );
        }
    }

    #[test]
    fn rejects_structural_damage() {
        assert!(matches!(
            SoundData::decode_wav(b"not a wav at all"),
            Err(SoundError::NotWave)
        ));

        let no_format = wav_file(&[chunk(b"data", &[0, 0])]);
        assert!(matches!(
            SoundData::decode_wav(&no_format),
            Err(SoundError::MissingFormat)
        ));

        let no_data = wav_file(&[chunk(b"fmt ", &fmt_body(FORMAT_PCM, 1, 48_000, 16))]);
        assert!(matches!(
            SoundData::decode_wav(&no_data),
            Err(SoundError::MissingData)
        ));

        let empty_data = simple_wav(FORMAT_PCM, 1, 48_000, 16, &[]);
        assert!(matches!(
            SoundData::decode_wav(&empty_data),
            Err(SoundError::MissingData)
        ));

        // Three bytes cannot make one 16-bit frame.
        let uneven = simple_wav(FORMAT_PCM, 1, 48_000, 16, &[1, 2, 3]);
        assert!(matches!(
            SoundData::decode_wav(&uneven),
            Err(SoundError::Malformed { .. })
        ));

        // A RIFF size that promises more than the file holds.
        let mut lying = simple_wav(FORMAT_PCM, 1, 48_000, 16, &[0, 0]);
        let declared = (lying.len() as u32 + 64).to_le_bytes();
        lying[4..8].copy_from_slice(&declared);
        assert!(matches!(
            SoundData::decode_wav(&lying),
            Err(SoundError::Malformed { .. })
        ));

        // A chunk whose size overruns the file.
        let mut overrun = simple_wav(FORMAT_PCM, 1, 48_000, 16, &[0, 0]);
        let data_size_offset = overrun.len() - 4 - 2;
        overrun[data_size_offset..data_size_offset + 4].copy_from_slice(&100u32.to_le_bytes());
        assert!(matches!(
            SoundData::decode_wav(&overrun),
            Err(SoundError::Malformed { .. })
        ));
    }

    #[test]
    fn rejects_a_wrong_block_align() {
        let mut body = fmt_body(FORMAT_PCM, 1, 48_000, 16);
        body[12..14].copy_from_slice(&3u16.to_le_bytes());
        let bytes = wav_file(&[chunk(b"fmt ", &body), chunk(b"data", &[0, 0])]);
        match SoundData::decode_wav(&bytes) {
            Err(SoundError::Malformed { detail }) => {
                assert!(detail.contains("block align"), "{detail}")
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn skips_unknown_chunks_and_odd_padding_in_any_order() {
        // A LIST chunk with an odd body length (so it carries a pad byte), the
        // data before the fmt, and a trailing JUNK chunk.
        let data = chunk(b"data", &i16_bytes(&[-16_384]));
        let format = chunk(b"fmt ", &fmt_body(FORMAT_PCM, 1, 48_000, 16));
        let list = chunk(b"LIST", b"INF");
        let junk = chunk(b"JUNK", &[1, 2, 3]);
        let bytes = wav_file(&[list.clone(), data.clone(), format.clone(), junk.clone()]);
        let sound = SoundData::decode_wav(&bytes).expect("chunk order is free");
        assert_eq!(sound.samples(), &[-0.5][..]);

        // The same file with a truncated LIST body must fail rather than read
        // past the declared chunk.
        let mut damaged = wav_file(&[list, data, format, junk]);
        damaged.truncate(damaged.len() - 2);
        assert!(SoundData::decode_wav(&damaged).is_err());
    }

    #[test]
    fn enforces_the_frame_limit() {
        let bytes = simple_wav(FORMAT_PCM, 1, 48_000, 16, &i16_bytes(&[0, 0, 0]));
        match SoundData::decode_wav_with_limit(&bytes, 2) {
            Err(SoundError::TooLarge { frames, max }) => {
                assert_eq!((frames, max), (3, 2));
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
        SoundData::decode_wav_with_limit(&bytes, 3).expect("three frames fit a limit of three");
    }

    #[test]
    fn load_wav_reports_the_path_that_failed() {
        let path = std::env::temp_dir().join("mikan-assets-missing-file.wav");
        match SoundData::load_wav(&path) {
            Err(SoundError::Io {
                path: reported,
                source,
            }) => {
                assert_eq!(reported, path);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn load_wav_reads_and_decodes_a_file() {
        let path = std::env::temp_dir().join(format!(
            "mikan-assets-sound-{}-{:?}.wav",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &path,
            simple_wav(FORMAT_PCM, 1, 48_000, 16, &i16_bytes(&[16_384])),
        )
        .expect("write test sound");
        let sound = SoundData::load_wav(&path);
        std::fs::remove_file(&path).expect("clean up test sound");
        assert_eq!(sound.expect("decode").samples(), &[0.5][..]);
    }

    #[test]
    fn from_samples_validates_its_input() {
        assert!(matches!(
            SoundData::from_samples(vec![], 1, 48_000),
            Err(SoundError::MissingData)
        ));
        assert!(matches!(
            SoundData::from_samples(vec![0.0; 4], 3, 48_000),
            Err(SoundError::UnsupportedChannels { channels: 3 })
        ));
        assert!(matches!(
            SoundData::from_samples(vec![0.0; 3], 2, 48_000),
            Err(SoundError::Malformed { .. })
        ));
        assert!(matches!(
            SoundData::from_samples(vec![0.0; 2], 1, 100),
            Err(SoundError::UnsupportedSampleRate { sample_rate: 100 })
        ));
        let sound = SoundData::from_samples(vec![0.25; 8], 2, 44_100).expect("valid");
        assert_eq!(sound.frames(), 4);
        assert_eq!(sound.channels(), 2);
        assert_eq!(sound.sample_rate(), 44_100);
    }
}
