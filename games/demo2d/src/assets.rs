//! Loading the demo's files: the atlas, its sprite sheet, and the star.
//!
//! This is the bridge the asset layer was built for: `mikan-assets` turns bytes
//! into an [`Image`] and a [`SpriteSheet`] on the CPU, and the renderer turns
//! the pixels into a [`Texture`]. Everything the demo needs is validated at
//! startup — a missing file, a corrupt PNG, or a sheet without the frames the
//! scenes ask for stops the demo with a readable error before the first frame.
//!
//! PNGs are uploaded as sRGB textures
//! ([`TextureEncoding::Srgb`]), which is what their stored bytes mean: the GPU
//! decodes them to linear when sampling, so the colors that reach the
//! framebuffer are the ones the generator wrote, not a CPU approximation.

use std::path::{Path, PathBuf};

use mikan::{
    Image, ImageError, RenderError, Renderer, SheetError, SpriteSheet, Texture, TextureDescriptor,
    TextureEncoding, UvRect,
};
use thiserror::Error;

/// Frames the demo's scenes require by name.
const REQUIRED_FRAMES: [&str; 3] = ["player", "marker", "checker"];

/// Why the demo's assets could not be loaded.
#[derive(Debug, Error)]
pub enum AssetError {
    /// The sprite-sheet document could not be read or parsed.
    #[error(transparent)]
    Sheet(#[from] SheetError),

    /// A PNG could not be read or decoded.
    #[error(transparent)]
    Image(#[from] ImageError),

    /// The GPU texture could not be created.
    #[error(transparent)]
    Render(#[from] RenderError),

    /// The sheet does not name a frame the demo draws.
    #[error("sheet {path} has no frame \"{name}\"; it needs {REQUIRED_FRAMES:?}")]
    MissingFrame {
        /// The sheet file that was loaded.
        path: PathBuf,
        /// The frame the demo asked for.
        name: &'static str,
    },
}

/// Everything the scenes draw with.
///
/// The two textures and the frame UVs are loaded once; scenes only read them.
pub struct Assets {
    /// The sheet the atlas frames come from, kept for diagnostics.
    pub sheet: SpriteSheet,
    /// UV rectangle of the `player` frame.
    pub player_uv: UvRect,
    /// UV rectangle of the `marker` frame.
    pub marker_uv: UvRect,
    /// UV rectangle of the `checker` frame.
    pub checker_uv: UvRect,
    /// The sheet's texture (sRGB).
    pub atlas: Texture,
    /// The star texture with transparent corners (sRGB).
    pub star: Texture,
}

impl Assets {
    /// Loads `atlas.json`, the PNG it names, and `star.png` from `directory`.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] when a file is missing or malformed, the sheet
    /// lacks a required frame, or a texture cannot be created.
    pub fn load(renderer: &Renderer, directory: &Path) -> Result<Self, AssetError> {
        let sheet_path = directory.join("atlas.json");
        let sheet = SpriteSheet::load(&sheet_path)?;
        let atlas = upload_png(renderer, sheet.image_path())?;
        let star = upload_png(renderer, &directory.join("star.png"))?;

        let uv = |name: &'static str| -> Result<UvRect, AssetError> {
            sheet
                .frame_uv(name)
                .ok_or_else(|| AssetError::MissingFrame {
                    path: sheet_path.clone(),
                    name,
                })
        };
        let player_uv = uv("player")?;
        let marker_uv = uv("marker")?;
        let checker_uv = uv("checker")?;

        Ok(Self {
            sheet,
            player_uv,
            marker_uv,
            checker_uv,
            atlas,
            star,
        })
    }
}

/// Decodes a PNG and uploads it as an sRGB texture.
fn upload_png(renderer: &Renderer, path: &Path) -> Result<Texture, AssetError> {
    let image = Image::load_png(path)?;
    let descriptor = TextureDescriptor::new(image.width(), image.height(), image.pixels())
        .with_encoding(TextureEncoding::Srgb);
    Ok(renderer.create_texture(descriptor)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed atlas, decoded at test time.
    fn atlas_image() -> Image {
        Image::decode_png(include_bytes!("../assets/atlas.png")).expect("committed atlas decodes")
    }

    fn sheet() -> SpriteSheet {
        SpriteSheet::parse(include_str!("../assets/atlas.json")).expect("committed sheet parses")
    }

    #[test]
    fn the_committed_sheet_matches_the_committed_texture() {
        let sheet = sheet();
        let image = atlas_image();
        assert_eq!(sheet.size(), image.size());
        assert_eq!(sheet.image_path(), Path::new("atlas.png"));
        assert_eq!(sheet.len(), REQUIRED_FRAMES.len());
        for name in REQUIRED_FRAMES {
            let frame = sheet.frame(name).expect("required frame");
            let [x, y, w, h] = frame.rect();
            assert!(
                x + w <= image.width() && y + h <= image.height(),
                "frame {name} is inside the texture"
            );
        }
    }

    #[test]
    fn the_committed_tiles_carry_the_documented_colors() {
        let image = atlas_image();
        // Player tile: the generator's linear colors, sRGB-encoded for display.
        assert_eq!(
            image.pixel(16, 16),
            Some([253, 196, 108, 255]),
            "player base"
        );
        assert_eq!(
            image.pixel(16, 1),
            Some([179, 129, 63, 255]),
            "player border"
        );
        assert_eq!(
            image.pixel(4, 4),
            Some([255, 237, 196, 255]),
            "player highlight"
        );
        // Marker tile starts at x = 32 and is bluish.
        assert_eq!(
            image.pixel(48, 16),
            Some([160, 225, 249, 255]),
            "marker base"
        );
        // Checker tile starts at x = 64: 8-pixel black and white squares.
        assert_eq!(image.pixel(64, 0), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(72, 0), Some([0, 0, 0, 255]));
        assert_eq!(image.pixel(64, 8), Some([0, 0, 0, 255]));
    }

    #[test]
    fn the_committed_star_has_opaque_middle_and_transparent_corners() {
        let star = Image::decode_png(include_bytes!("../assets/star.png")).expect("star decodes");
        assert_eq!(star.size(), [24, 24]);
        let center = star.pixel(12, 12).expect("center");
        assert_eq!(center[3], 255, "the middle of the star is opaque");
        assert!(
            center[0] > 200 && center[1] > 200 && center[2] < 150,
            "yellow: {center:?}"
        );
        assert_eq!(
            star.pixel(0, 0),
            Some([0, 0, 0, 0]),
            "the corner is transparent"
        );
        assert_eq!(star.pixel(23, 23), Some([0, 0, 0, 0]));
        // The edges are anti-aliased, so they land strictly between.
        let edge = star.pixel(12, 2).expect("edge");
        assert!(
            edge[3] > 0 && edge[3] < 255,
            "the tip should be partially transparent: {edge:?}"
        );
    }

    #[test]
    fn frame_uvs_are_inside_the_texture() {
        let sheet = sheet();
        for name in REQUIRED_FRAMES {
            let uv = sheet.frame_uv(name).expect("required frame");
            assert!(uv.min[0] >= 0.0 && uv.min[1] >= 0.0, "{name}");
            assert!(uv.max[0] <= 1.0 && uv.max[1] <= 1.0, "{name}");
            assert!(uv.min[0] < uv.max[0] && uv.min[1] < uv.max[1], "{name}");
        }
    }
}
