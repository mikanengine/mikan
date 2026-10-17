//! CPU-side asset loading for Mikan.
//!
//! Three things live here, all deliberately free of GPU code:
//!
//! - [`image`]: PNG files decoded to the engine's canonical RGBA8
//!   representation ([`Image`]), with every PNG color type normalized, bounded
//!   allocations, and descriptive errors.
//! - [`sheet`]: sprite-sheet metadata, parsed from the documented subset of
//!   Aseprite's JSON export ([`SpriteSheet`]), validated and converted to the
//!   texture-coordinate rectangles the renderer samples.
//! - [`sound`]: WAV files decoded to interleaved f32 samples ([`SoundData`]),
//!   covering the small format set the mixer supports, with the same bounded
//!   allocations and descriptive errors.
//!
//! The split between this crate and its consumers is deliberate: decoding a
//! PNG or a WAV is CPU work with no window, no device, and no audio hardware,
//! so it is testable and usable without any of them. The render layer takes
//! the bytes from an [`Image`] and uploads them; the audio system takes the
//! samples from a [`SoundData`] and copies them into the mixer. Nothing here
//! can fail because a GPU or a sound card is missing.
//!
//! # Example
//!
//! ```no_run
//! use mikan_assets::{Image, SpriteSheet};
//!
//! let sheet = SpriteSheet::load("assets/atlas.json")?;
//! let image = Image::load_png(sheet.image_path())?;
//! let player = sheet.frame("player").expect("the sheet has a player frame");
//! let uv = player.uv(sheet.size());
//! # let _ = (image, player, uv);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Origin
//!
//! PNG decoding is delegated to the [`png`] crate; this crate owns the
//! conversion into Mikan's representation (RGBA8, top row first, straight
//! alpha, sRGB bytes left as stored), the limits, and the errors. See
//! `docs/sprite-sheets.md` for the sheet format and `docs/adr/` for the
//! decision record behind the crate split.

pub mod image;
pub mod sheet;
pub mod sound;

pub use image::{Image, ImageError, MAX_IMAGE_DIMENSION};
pub use sheet::{SheetError, SheetFrame, SpriteSheet};
pub use sound::{MAX_SOUND_FRAMES, SoundData, SoundError};
