//! An independent Rust reference for the native mixer's arithmetic.
//!
//! The C++ mixer's output is cross-checked bit for bit against this
//! oracle: linear-interpolation resampling with the same operation order,
//! gain application, the final clip, and the s16 quantization the offline
//! device applies. It exists because a test that only asserts "some
//! samples came out" cannot catch an interpolation or rounding mistake;
//! equality against a second implementation can.
//!
//! Keep this file and `audio_runtime.cpp`'s `mix_voice` changing together;
//! `tests/cross_check.rs` is what fails when they drift.

/// Clips a mixed sample to -1..1; NaN becomes silence. Mirrors the
/// native mixer's final pass.
#[must_use]
pub fn clip(value: f32) -> f32 {
    if value > 1.0 {
        1.0
    } else if value < -1.0 {
        -1.0
    } else if value.is_nan() {
        0.0
    } else {
        value
    }
}

/// Clips and quantizes to s16 exactly the way the offline device does:
/// nearest, ties upward (`.floor(x + 0.5)`), NaN as silence.
#[must_use]
pub fn to_s16(sample: f32) -> i16 {
    let clamped = if (-1.0..=1.0).contains(&sample) {
        sample
    } else if sample > 1.0 {
        1.0
    } else if sample < -1.0 {
        -1.0
    } else {
        0.0
    };
    (clamped * 32767.0 + 0.5).floor() as i16
}

/// One voice's playback state and arithmetic, mirroring one native voice
/// slot for a single (non-looping or looping) sound.
#[derive(Debug, Clone)]
pub struct Voice {
    /// Interleaved samples.
    samples: Vec<f32>,
    /// 1 or 2.
    channels: u32,
    /// The sound's rate.
    rate: u32,
    /// Whether the voice wraps at the end.
    looping: bool,
    /// Fractional cursor in source frames.
    position: f64,
}

impl Voice {
    /// Starts a voice at position zero.
    ///
    /// # Panics
    ///
    /// If `channels` is not 1 or 2, or `samples` is not a whole number of
    /// frames for `channels`.
    #[must_use]
    pub fn new(samples: Vec<f32>, channels: u32, rate: u32, looping: bool) -> Self {
        assert!(channels == 1 || channels == 2, "channels must be 1 or 2");
        assert!(
            samples.len().is_multiple_of(channels as usize),
            "samples must be whole frames"
        );
        Self {
            samples,
            channels,
            rate,
            looping,
            position: 0.0,
        }
    }

    /// The sound's length in frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// The published cursor (frames consumed), like the native runtime's
    /// `position_frames`.
    #[must_use]
    pub fn position_frames(&self) -> u64 {
        let length = self.frames() as f64;
        if !self.looping && self.position >= length {
            self.frames() as u64
        } else {
            self.position as u64
        }
    }

    /// Whether the voice ran past its end without looping.
    #[must_use]
    pub fn finished(&self) -> bool {
        !self.looping && self.position >= self.frames() as f64
    }

    /// Mixes up to `out.len() / 2` stereo frames into `out`, at
    /// `out_rate`, with `gain` and `pitch`, exactly like the native
    /// `mix_voice`. Returns the number of frames mixed.
    ///
    /// # Panics
    ///
    /// If `out.len()` is odd.
    pub fn mix_into(&mut self, out: &mut [f32], out_rate: u32, gain: f32, pitch: f32) -> usize {
        assert!(
            out.len().is_multiple_of(2),
            "the output is interleaved stereo"
        );
        let length = self.frames();
        let length_f = length as f64;
        let step = (self.rate as f64 / out_rate as f64) * pitch as f64;
        let mut position = self.position;
        let mut mixed = 0;
        for i in 0..out.len() / 2 {
            if position >= length_f {
                if !self.looping {
                    break;
                }
                position %= length_f;
            }
            let first = position as usize;
            let fraction = (position - first as f64) as f32;
            let (left0, right0, left1, right1) = if self.channels == 2 {
                let left0 = self.samples[first * 2];
                let right0 = self.samples[first * 2 + 1];
                let (left1, right1) = if first + 1 < length {
                    (
                        self.samples[(first + 1) * 2],
                        self.samples[(first + 1) * 2 + 1],
                    )
                } else if self.looping {
                    (self.samples[0], self.samples[1])
                } else {
                    (0.0, 0.0)
                };
                (left0, right0, left1, right1)
            } else {
                let left0 = self.samples[first];
                let left1 = if first + 1 < length {
                    self.samples[first + 1]
                } else if self.looping {
                    self.samples[0]
                } else {
                    0.0
                };
                (left0, left0, left1, left1)
            };
            out[i * 2] += (left0 + (left1 - left0) * fraction) * gain;
            out[i * 2 + 1] += (right0 + (right1 - right0) * fraction) * gain;
            position += step;
            mixed += 1;
        }
        self.position = position;
        mixed
    }
}
