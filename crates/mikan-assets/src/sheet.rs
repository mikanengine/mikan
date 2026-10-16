//! Named frames of a sprite sheet, parsed from a documented subset of
//! Aseprite's JSON export.
//!
//! A sprite sheet is one texture plus a list of named rectangles into it. Mikan
//! reads the format Aseprite (and tools that imitate it) writes:
//!
//! ```json
//! {
//!   "frames": {
//!     "player": {
//!       "frame": { "x": 0, "y": 0, "w": 32, "h": 32 },
//!       "duration": 100,
//!       "pivot": { "x": 0.5, "y": 1.0 }
//!     }
//!   },
//!   "meta": {
//!     "image": "atlas.png",
//!     "size": { "w": 32, "h": 32 }
//!   }
//! }
//! ```
//!
//! # The accepted subset
//!
//! - `frames` must be an object keyed by frame name, as above. The array form
//!   Aseprite can export is not accepted; frames are named in Mikan.
//! - `frame` is the pixel rectangle inside the sheet: `x`, `y`, `w`, `h`.
//! - `duration` (milliseconds, optional) and `pivot` (optional, in `[0, 1]`
//!   with `(0, 0)` at the top-left of the frame) are preserved as metadata.
//!   The default pivot is the center, `(0.5, 0.5)`.
//! - `meta.image` (the texture file, resolved relative to the JSON file by
//!   [`SpriteSheet::load`]) and `meta.size` (the sheet's pixel size) are
//!   required.
//!
//! Anything else — `frameTags`, `layers`, `slices`, `scale`, `spriteSourceSize`
//! — is ignored, so files grow with the exporting tool without breaking the
//! loader. Two things are *not* ignored, because pretending a frame is
//! something it is not would render the wrong pixels: frames stored `rotated`
//! and frames stored `trimmed` are rejected with a specific error. Neither
//! appears in Aseprite's default export settings.
//!
//! # Validation
//!
//! [`SpriteSheet::parse`] and [`SpriteSheet::load`] reject, with a named
//! [`SheetError`]: duplicate frame names, a zero-sized sheet or frame, a frame
//! reaching outside the sheet, a pivot outside `[0, 1]` (or not a finite
//! number), and rotated or trimmed frames.
//!
//! # UVs and atlas bleeding
//!
//! [`SheetFrame::uv`] converts a pixel rectangle to the normalized
//! [`UvRect`] the renderer samples. The rectangle is inset by half a texel on
//! every side, so with a linear sampler no sample touches a neighbouring
//! frame's texels; the cost is a sub-texel shift at the very edge of a frame,
//! which is the standard trade every texture atlas makes when zoomed in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mikan_core::UvRect;
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};

/// Why a sprite sheet could not be parsed.
#[derive(Debug, thiserror::Error)]
pub enum SheetError {
    /// The JSON file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The text is not the accepted sprite-sheet JSON.
    ///
    /// `path` is the file it was read from, or `None` for in-memory text.
    #[error("could not parse sprite-sheet JSON{}: {source}", match path { Some(path) => format!(" from {}", path.display()), None => String::new() })]
    Json {
        /// File the JSON was read from, if it came from one.
        path: Option<PathBuf>,
        /// Error from the JSON parser, including its position in the text.
        #[source]
        source: serde_json::Error,
    },

    /// The same frame name appears twice. A JSON object cannot be trusted to
    /// keep every entry silently, so this is an error rather than a merge.
    #[error("duplicate frame name \"{name}\"")]
    DuplicateFrame {
        /// The name that appears more than once.
        name: String,
    },

    /// The document has no `meta.image`.
    #[error("sprite sheet has no meta.image path")]
    MissingImage,

    /// The sheet declares an empty size.
    #[error("sprite sheet size must be non-zero, got {width}x{height}")]
    ZeroSheetSize {
        /// Declared width.
        width: u32,
        /// Declared height.
        height: u32,
    },

    /// A frame has an empty region.
    #[error("frame \"{name}\" has a zero-sized region")]
    ZeroFrameSize {
        /// Name of the offending frame.
        name: String,
    },

    /// A frame reaching outside the sheet would sample neighbouring texels or
    /// out-of-bounds memory, depending on the texture size.
    #[error("frame \"{name}\" spans {rect:?} outside the {sheet:?} sheet")]
    FrameOutOfBounds {
        /// Name of the offending frame.
        name: String,
        /// The frame's rectangle as `[x, y, w, h]`.
        rect: [u32; 4],
        /// The sheet's size as `[width, height]`.
        sheet: [u32; 2],
    },

    /// The pivot is outside `[0, 1]` or not a finite number.
    #[error("frame \"{name}\" pivot {pivot:?} must be finite and within [0, 1]")]
    PivotOutOfRange {
        /// Name of the offending frame.
        name: String,
        /// The offending pivot as `[x, y]`.
        pivot: [f32; 2],
    },

    /// A frame is stored rotated, which this loader does not support.
    #[error("frame \"{name}\" is stored rotated, which Mikan does not support")]
    FrameRotated {
        /// Name of the offending frame.
        name: String,
    },

    /// A frame is stored trimmed, which this loader does not support.
    #[error("frame \"{name}\" is trimmed, which Mikan does not support")]
    FrameTrimmed {
        /// Name of the offending frame.
        name: String,
    },
}

/// One named rectangle of a sprite sheet.
///
/// Values come from [`SpriteSheet::parse`] validation, so a frame is always
/// inside its sheet with a non-zero size and a usable pivot.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetFrame {
    name: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    duration_ms: Option<u16>,
    pivot: [f32; 2],
}

impl SheetFrame {
    /// The frame's name, as used in the sheet's `frames` object.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The frame's pixel rectangle inside the sheet, `[x, y, w, h]`.
    #[must_use]
    pub fn rect(&self) -> [u32; 4] {
        [self.x, self.y, self.w, self.h]
    }

    /// The frame's size in pixels, `[w, h]`.
    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        [self.w, self.h]
    }

    /// The frame's animation duration in milliseconds, if the document has one.
    #[must_use]
    pub fn duration_ms(&self) -> Option<u16> {
        self.duration_ms
    }

    /// The pivot, normalized within the frame: `(0, 0)` is the top-left corner,
    /// `(1, 1)` the bottom-right. Defaults to the center when absent.
    #[must_use]
    pub fn pivot(&self) -> [f32; 2] {
        self.pivot
    }

    /// The normalized texture region to sample for this frame in a
    /// `sheet_size` texture, inset by half a texel on every side.
    ///
    /// See the [module documentation](self) for what the inset buys and costs.
    #[must_use]
    pub fn uv(&self, sheet_size: [u32; 2]) -> UvRect {
        let size = [sheet_size[0].max(1) as f32, sheet_size[1].max(1) as f32];
        UvRect::new(
            [self.x as f32 / size[0], self.y as f32 / size[1]],
            [
                (self.x + self.w) as f32 / size[0],
                (self.y + self.h) as f32 / size[1],
            ],
        )
        .inset([0.5 / size[0], 0.5 / size[1]])
    }

    /// Where to place the frame's center, as an offset from the position the
    /// pivot marks, in world units (pixels).
    ///
    /// A frame drawn with `center = position + frame.center_for_pivot()` puts
    /// the point the pivot selects — feet, muzzle, hook — at `position`. With
    /// the default center pivot the offset is `[0, 0]`; with the feet pivot
    /// `(0.5, 1.0)` the center sits half the frame's height above `position`,
    /// because world `y` grows downward.
    #[must_use]
    pub fn center_for_pivot(&self) -> [f32; 2] {
        [
            (0.5 - self.pivot[0]) * self.w as f32,
            (0.5 - self.pivot[1]) * self.h as f32,
        ]
    }
}

/// A texture with named pixel regions.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteSheet {
    image: PathBuf,
    size: [u32; 2],
    frames: BTreeMap<String, SheetFrame>,
}

impl SpriteSheet {
    /// Parses a sprite sheet from JSON text.
    ///
    /// The sheet's image path is kept as written; see [`SpriteSheet::load`] to
    /// resolve it against the JSON file's directory.
    ///
    /// # Errors
    ///
    /// Returns [`SheetError`] when the text is not the accepted subset, when a
    /// frame fails validation, or when a name appears twice.
    pub fn parse(json: &str) -> Result<Self, SheetError> {
        Self::from_slice(json.as_bytes(), None)
    }

    /// Reads a sprite sheet from a JSON file, resolving `meta.image` relative
    /// to the file's directory.
    ///
    /// # Errors
    ///
    /// Returns [`SheetError::Io`] when the file cannot be read, and otherwise
    /// the same failures as [`SpriteSheet::parse`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SheetError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| SheetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut sheet = Self::from_slice(&bytes, Some(path))?;
        if let Some(parent) = path.parent() {
            sheet.image = parent.join(&sheet.image);
        }
        Ok(sheet)
    }

    fn from_slice(bytes: &[u8], origin: Option<&Path>) -> Result<Self, SheetError> {
        let raw: RawSheet = serde_json::from_slice(bytes).map_err(|source| SheetError::Json {
            path: origin.map(Path::to_path_buf),
            source,
        })?;
        if let Some(name) = raw.frames.duplicates.first() {
            return Err(SheetError::DuplicateFrame { name: name.clone() });
        }
        let size = raw.meta.size;
        if size.w == 0 || size.h == 0 {
            return Err(SheetError::ZeroSheetSize {
                width: size.w,
                height: size.h,
            });
        }
        let image = raw.meta.image.ok_or(SheetError::MissingImage)?;
        let mut frames = BTreeMap::new();
        for (name, raw_frame) in raw.frames.frames {
            let frame = Self::build_frame(&name, raw_frame, size)?;
            frames.insert(name, frame);
        }
        Ok(Self {
            image: PathBuf::from(image),
            size: [size.w, size.h],
            frames,
        })
    }

    /// Validates one parsed frame and converts it into its final form.
    fn build_frame(name: &str, raw: RawFrame, sheet: RawSize) -> Result<SheetFrame, SheetError> {
        let name = name.to_string();
        if raw.rotated {
            return Err(SheetError::FrameRotated { name });
        }
        if raw.trimmed {
            return Err(SheetError::FrameTrimmed { name });
        }
        let RawRect { x, y, w, h } = raw.frame;
        if w == 0 || h == 0 {
            return Err(SheetError::ZeroFrameSize { name });
        }
        let inside = x.checked_add(w).is_some_and(|right| right <= sheet.w)
            && y.checked_add(h).is_some_and(|bottom| bottom <= sheet.h);
        if !inside {
            return Err(SheetError::FrameOutOfBounds {
                name,
                rect: [x, y, w, h],
                sheet: [sheet.w, sheet.h],
            });
        }
        let pivot = match raw.pivot {
            Some(pivot) => [pivot.x, pivot.y],
            None => [0.5, 0.5],
        };
        let usable = pivot
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value));
        if !usable {
            return Err(SheetError::PivotOutOfRange { name, pivot });
        }
        Ok(SheetFrame {
            name,
            x,
            y,
            w,
            h,
            duration_ms: raw.duration,
            pivot,
        })
    }

    /// The texture file the frames index into: the path as written in
    /// `meta.image`, resolved against the JSON file's directory by
    /// [`SpriteSheet::load`].
    #[must_use]
    pub fn image_path(&self) -> &Path {
        &self.image
    }

    /// The sheet's pixel size, `[width, height]`.
    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The frame with the given name, or `None` if the sheet has no such frame.
    #[must_use]
    pub fn frame(&self, name: &str) -> Option<&SheetFrame> {
        self.frames.get(name)
    }

    /// Every frame, in name order.
    pub fn frames(&self) -> impl Iterator<Item = &SheetFrame> {
        self.frames.values()
    }

    /// The number of frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the sheet has no frames.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// [`SheetFrame::uv`] for the named frame, using the sheet's own size.
    ///
    /// The convenience method that cannot get the size wrong;
    /// [`SheetFrame::uv`] is the underlying conversion.
    #[must_use]
    pub fn frame_uv(&self, name: &str) -> Option<UvRect> {
        self.frame(name).map(|frame| frame.uv(self.size))
    }
}

/// The raw shape of the document, before validation.
#[derive(Deserialize)]
struct RawSheet {
    frames: RawFrames,
    meta: RawMeta,
}

/// `meta` fields Mikan reads; other keys are ignored.
#[derive(Deserialize)]
struct RawMeta {
    image: Option<String>,
    size: RawSize,
}

/// A `{ "w": u32, "h": u32 }` object.
#[derive(Deserialize, Clone, Copy)]
struct RawSize {
    w: u32,
    h: u32,
}

/// `frames` with duplicate detection, in document order.
struct RawFrames {
    /// Frames, keyed by name, in the order they appeared.
    frames: Vec<(String, RawFrame)>,
    /// Names that appeared more than once, in the order they repeated.
    duplicates: Vec<String>,
}

/// Rejects duplicate names instead of silently keeping one of them.
///
/// Implemented manually because a derived map — and `serde_json`'s own object
/// type — overwrites a repeated key without a trace.
impl<'de> Deserialize<'de> for RawFrames {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FramesVisitor;

        impl<'de> Visitor<'de> for FramesVisitor {
            type Value = RawFrames;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of frame name to frame data")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut frames = Vec::with_capacity(map.size_hint().unwrap_or_default());
                let mut seen = BTreeSet::new();
                let mut duplicates = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    let frame = map.next_value::<RawFrame>()?;
                    if seen.insert(name.clone()) {
                        frames.push((name, frame));
                    } else {
                        duplicates.push(name);
                    }
                }
                Ok(RawFrames { frames, duplicates })
            }
        }

        deserializer.deserialize_map(FramesVisitor)
    }
}

/// One entry of `frames`, before validation.
#[derive(Deserialize)]
struct RawFrame {
    frame: RawRect,
    duration: Option<u16>,
    pivot: Option<RawPivot>,
    #[serde(default)]
    rotated: bool,
    #[serde(default)]
    trimmed: bool,
}

/// A `frame` rectangle.
#[derive(Deserialize)]
struct RawRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// A `pivot` point.
#[derive(Deserialize)]
struct RawPivot {
    x: f32,
    y: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative document: custom and default pivots, a duration, and
    /// extra fields of the kinds real exports carry.
    const SHEET_JSON: &str = r#"
    {
      "frames": {
        "player": {
          "frame": { "x": 0, "y": 0, "w": 32, "h": 32 },
          "duration": 120,
          "pivot": { "x": 0.5, "y": 1.0 },
          "spriteSourceSize": { "x": 0, "y": 0, "w": 32, "h": 32 },
          "sourceSize": { "w": 32, "h": 32 }
        },
        "marker": {
          "frame": { "x": 32, "y": 0, "w": 32, "h": 32 }
        },
        "checker": {
          "frame": { "x": 64, "y": 0, "w": 16, "h": 16 }
        }
      },
      "meta": {
        "image": "atlas.png",
        "size": { "w": 96, "h": 32 },
        "scale": "1",
        "layers": []
      }
    }
    "#;

    fn parse_sheet() -> SpriteSheet {
        SpriteSheet::parse(SHEET_JSON).expect("fixture parses")
    }

    /// A minimal document with a `frames` body and sheet size spliced in.
    fn document(frames: &str, sheet: &str) -> String {
        format!(r#"{{ "frames": {frames}, "meta": {{ "image": "a.png", "size": {sheet} }} }}"#)
    }

    #[test]
    fn parses_frames_and_metadata() {
        let sheet = parse_sheet();
        assert_eq!(sheet.size(), [96, 32]);
        assert_eq!(sheet.image_path(), Path::new("atlas.png"));
        assert_eq!(sheet.len(), 3);
        assert!(!sheet.is_empty());

        let player = sheet.frame("player").expect("player exists");
        assert_eq!(player.name(), "player");
        assert_eq!(player.rect(), [0, 0, 32, 32]);
        assert_eq!(player.size(), [32, 32]);
        assert_eq!(player.duration_ms(), Some(120));
        assert_eq!(player.pivot(), [0.5, 1.0]);

        let marker = sheet.frame("marker").expect("marker exists");
        assert_eq!(marker.duration_ms(), None);
        assert_eq!(
            marker.pivot(),
            [0.5, 0.5],
            "the default pivot is the center"
        );
        assert_eq!(sheet.frame("missing"), None);
    }

    #[test]
    fn frames_iterate_in_name_order() {
        let sheet = parse_sheet();
        let names: Vec<_> = sheet.frames().map(SheetFrame::name).collect();
        assert_eq!(names, ["checker", "marker", "player"]);
    }

    #[test]
    fn uv_insets_by_half_a_texel_on_every_side() {
        let sheet = parse_sheet();
        assert_eq!(
            sheet.frame_uv("checker"),
            Some(UvRect::new([0.671875, 0.015625], [0.828125, 0.484375])),
            "64.5/96 .. 79.5/96 by 0.5/32 .. 15.5/32"
        );
        assert_eq!(sheet.frame_uv("missing"), None);
    }

    #[test]
    fn uv_of_a_frame_at_the_sheet_border_stays_inside_it() {
        let sheet = parse_sheet();
        let player = sheet.frame("player").expect("player exists");
        let uv = player.uv(sheet.size());
        assert_eq!(uv.min, [0.5 / 96.0, 0.5 / 32.0], "not [0, 0]");
        assert!(uv.min[0] > 0.0 && uv.min[1] > 0.0);
        let marker_uv = sheet.frame_uv("marker").expect("marker exists");
        assert!(
            (marker_uv.max[0] - 63.5 / 96.0).abs() < 1e-6,
            "not the sheet's right edge: {}",
            marker_uv.max[0]
        );
    }

    #[test]
    fn uv_of_a_single_pixel_frame_is_the_texel_center() {
        let sheet = SpriteSheet::parse(&document(
            r#"{ "dot": { "frame": { "x": 1, "y": 2, "w": 1, "h": 1 } } }"#,
            r#"{ "w": 4, "h": 4 }"#,
        ))
        .expect("parses");
        let uv = sheet.frame_uv("dot").expect("dot exists");
        assert_eq!(
            uv,
            UvRect::new([1.5 / 4.0, 2.5 / 4.0], [1.5 / 4.0, 2.5 / 4.0]),
            "a one-texel frame collapses to its center instead of inverting"
        );
    }

    #[test]
    fn center_for_pivot_defaults_to_the_center() {
        let sheet = parse_sheet();
        assert_eq!(
            sheet.frame("checker").expect("checker").center_for_pivot(),
            [0.0, 0.0]
        );
    }

    #[test]
    fn center_for_pivot_moves_the_quad_opposite_the_pivot() {
        let sheet = parse_sheet();
        assert_eq!(
            sheet.frame("player").expect("player").center_for_pivot(),
            [0.0, -16.0],
            "a feet pivot (y = 1) puts the center half a frame above the position"
        );
    }

    #[test]
    fn rejects_duplicate_frame_names() {
        let json = document(
            r#"{
                "a": { "frame": { "x": 0, "y": 0, "w": 1, "h": 1 } },
                "a": { "frame": { "x": 1, "y": 0, "w": 1, "h": 1 } }
            }"#,
            r#"{ "w": 2, "h": 1 }"#,
        );
        match SpriteSheet::parse(&json) {
            Err(SheetError::DuplicateFrame { name }) => assert_eq!(name, "a"),
            other => panic!("expected DuplicateFrame, got {other:?}"),
        }
    }

    #[test]
    fn rejects_frames_in_array_form() {
        let json = document(
            r#"[ { "filename": "a", "frame": { "x": 0, "y": 0, "w": 1, "h": 1 } } ]"#,
            r#"{ "w": 2, "h": 1 }"#,
        );
        match SpriteSheet::parse(&json) {
            Err(SheetError::Json { source, .. }) => {
                assert!(
                    source.to_string().contains("expected a map of frame name"),
                    "unhelpful message: {source}"
                );
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn rejects_rotated_and_trimmed_frames() {
        for (field, expect) in [
            (r#""rotated": true"#, "rotated"),
            (r#""trimmed": true"#, "trimmed"),
        ] {
            let json = document(
                &format!(
                    r#"{{ "a": {{ "frame": {{ "x": 0, "y": 0, "w": 1, "h": 1 }}, {field} }} }}"#
                ),
                r#"{ "w": 2, "h": 1 }"#,
            );
            let message = SpriteSheet::parse(&json)
                .expect_err("must be rejected")
                .to_string();
            assert!(message.contains(expect), "unexpected message: {message}");
        }
    }

    #[test]
    fn rejects_zero_sheet_size() {
        let json = document(
            r#"{ "a": { "frame": { "x": 0, "y": 0, "w": 1, "h": 1 } } }"#,
            r#"{ "w": 0, "h": 4 }"#,
        );
        match SpriteSheet::parse(&json) {
            Err(SheetError::ZeroSheetSize { width, height }) => {
                assert_eq!((width, height), (0, 4));
            }
            other => panic!("expected ZeroSheetSize, got {other:?}"),
        }
    }

    #[test]
    fn rejects_zero_sized_frames() {
        let json = document(
            r#"{ "a": { "frame": { "x": 0, "y": 0, "w": 4, "h": 0 } } }"#,
            r#"{ "w": 4, "h": 4 }"#,
        );
        match SpriteSheet::parse(&json) {
            Err(SheetError::ZeroFrameSize { name }) => assert_eq!(name, "a"),
            other => panic!("expected ZeroFrameSize, got {other:?}"),
        }
    }

    #[test]
    fn rejects_frames_outside_the_sheet() {
        let cases = [
            (r#"{ "x": 3, "y": 0, "w": 2, "h": 1 }"#, [3, 0, 2, 1]),
            (r#"{ "x": 0, "y": 3, "w": 1, "h": 2 }"#, [0, 3, 1, 2]),
            (
                r#"{ "x": 4294967295, "y": 0, "w": 2, "h": 1 }"#,
                [u32::MAX, 0, 2, 1],
            ),
        ];
        for (rect, expected) in cases {
            let json = document(
                &format!(r#"{{ "a": {{ "frame": {rect} }} }}"#),
                r#"{ "w": 4, "h": 4 }"#,
            );
            match SpriteSheet::parse(&json) {
                Err(SheetError::FrameOutOfBounds { name, rect, sheet }) => {
                    assert_eq!(name, "a");
                    assert_eq!(rect, expected);
                    assert_eq!(sheet, [4, 4]);
                }
                other => panic!("expected FrameOutOfBounds for {rect}, got {other:?}"),
            }
        }
    }

    #[test]
    fn accepts_frames_flush_with_the_sheet_border() {
        let json = document(
            r#"{ "a": { "frame": { "x": 3, "y": 3, "w": 1, "h": 1 } } }"#,
            r#"{ "w": 4, "h": 4 }"#,
        );
        assert!(SpriteSheet::parse(&json).is_ok());
    }

    #[test]
    fn rejects_pivots_outside_the_unit_square() {
        for pivot in [r#"{ "x": 1.5, "y": 0.5 }"#, r#"{ "x": 0.5, "y": -0.1 }"#] {
            let json = document(
                &format!(
                    r#"{{ "a": {{ "frame": {{ "x": 0, "y": 0, "w": 1, "h": 1 }}, "pivot": {pivot} }} }}"#
                ),
                r#"{ "w": 4, "h": 4 }"#,
            );
            match SpriteSheet::parse(&json) {
                Err(SheetError::PivotOutOfRange { name, .. }) => assert_eq!(name, "a"),
                other => panic!("expected PivotOutOfRange for {pivot}, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_non_finite_pivots() {
        // JSON has no `NaN` literal, so this path goes through the typed
        // builder: a pivot that is not a finite number is rejected instead of
        // poisoning every transform that uses it.
        let frame = RawFrame {
            frame: RawRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            duration: None,
            pivot: Some(RawPivot {
                x: f32::NAN,
                y: 0.5,
            }),
            rotated: false,
            trimmed: false,
        };
        match SpriteSheet::build_frame("a", frame, RawSize { w: 4, h: 4 }) {
            Err(SheetError::PivotOutOfRange { name, pivot }) => {
                assert_eq!(name, "a");
                assert!(pivot[0].is_nan());
            }
            other => panic!("expected PivotOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn accepts_pivots_on_the_unit_square_border() {
        for pivot in [r#"{ "x": 0, "y": 0 }"#, r#"{ "x": 1, "y": 1 }"#] {
            let json = document(
                &format!(
                    r#"{{ "a": {{ "frame": {{ "x": 0, "y": 0, "w": 1, "h": 1 }}, "pivot": {pivot} }} }}"#
                ),
                r#"{ "w": 4, "h": 4 }"#,
            );
            assert!(SpriteSheet::parse(&json).is_ok(), "pivot {pivot}");
        }
    }

    #[test]
    fn rejects_documents_without_an_image_path() {
        let json = r#"{ "frames": {}, "meta": { "size": { "w": 4, "h": 4 } } }"#;
        assert!(matches!(
            SpriteSheet::parse(json),
            Err(SheetError::MissingImage)
        ));
    }

    #[test]
    fn accepts_an_empty_frame_list() {
        let json = document(r#"{}"#, r#"{ "w": 4, "h": 4 }"#);
        let sheet = SpriteSheet::parse(&json).expect("an empty sheet is valid");
        assert!(sheet.is_empty());
    }

    #[test]
    fn rejects_missing_required_fields() {
        for json in [
            r#"{ "meta": { "image": "a.png", "size": { "w": 4, "h": 4 } } }"#,
            r#"{ "frames": {}, "meta": { "image": "a.png" } }"#,
            r#"{ "frames": { "a": { "frame": { "x": 0, "y": 0 } } }, "meta": { "image": "a.png", "size": { "w": 4, "h": 4 } } }"#,
        ] {
            assert!(
                matches!(SpriteSheet::parse(json), Err(SheetError::Json { .. })),
                "should be a JSON error: {json}"
            );
        }
    }

    #[test]
    fn rejects_broken_and_non_utf8_documents() {
        assert!(matches!(
            SpriteSheet::parse("{ not json"),
            Err(SheetError::Json { .. })
        ));
        assert!(matches!(
            SpriteSheet::from_slice(&[0xFF, 0xFE], None),
            Err(SheetError::Json { .. })
        ));
    }

    #[test]
    fn load_resolves_the_image_next_to_the_document() {
        let directory = std::env::temp_dir().join(format!(
            "mikan-assets-sheet-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("atlas.json");
        std::fs::write(&path, SHEET_JSON).expect("write test sheet");

        let sheet = SpriteSheet::load(&path);
        std::fs::remove_dir_all(&directory).expect("clean up test directory");

        let sheet = sheet.expect("load");
        assert_eq!(sheet.image_path(), directory.join("atlas.png"));
        assert_eq!(sheet.size(), [96, 32]);
    }

    #[test]
    fn load_reports_the_path_that_failed() {
        let path = std::env::temp_dir().join("mikan-assets-missing-sheet.json");
        match SpriteSheet::load(&path) {
            Err(SheetError::Io {
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
    fn load_reports_json_errors_with_the_file_they_came_from() {
        let path = std::env::temp_dir().join(format!(
            "mikan-assets-broken-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, "{ nope").expect("write test sheet");
        let error = SpriteSheet::load(&path);
        std::fs::remove_file(&path).expect("clean up test sheet");

        match error {
            Err(SheetError::Json {
                path: Some(reported),
                source,
            }) => {
                assert_eq!(reported, path);
                assert!(source.line() >= 1);
            }
            other => panic!("expected Json with a path, got {other:?}"),
        }
    }
}
