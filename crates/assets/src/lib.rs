//! CPU-side asset loading for Mikan.
//!
//! Two things live here, both deliberately free of GPU code:
//!
//! - [`image`]: PNG files decoded to the engine's canonical RGBA8
//!   representation ([`Image`]), with every PNG color type normalized, bounded
//!   allocations, and descriptive errors.
//! - [`sheet`]: sprite-sheet metadata, parsed from the documented subset of
//!   Aseprite's JSON export ([`SpriteSheet`]), validated and converted to the
//!   texture-coordinate rectangles the renderer samples.
//!
//! The split between this crate and `mikan-render` is deliberate: decoding a
//! PNG is CPU work with no window and no device, so it is testable and usable
//! without either. The render layer takes the bytes from an [`Image`] and
//! uploads them; the asset layer never talks to a GPU, and nothing here can
//! fail because a device is missing.
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

pub use image::{Image, ImageError, MAX_IMAGE_DIMENSION};
pub use sheet::{SheetError, SheetFrame, SpriteSheet};
