//! GPU textures uploaded from CPU pixel data.
//!
//! Textures are created through [`Renderer::create_texture`], which validates
//! the descriptor before touching the GPU. Callers provide 8-bit RGBA pixels,
//! tightly packed and row-major, and state how those bytes are encoded:
//!
//! - [`TextureEncoding::Linear`] — the bytes *are* linear values, the same
//!   space [`Color`](mikan_core::Color) uses. This is what procedurally
//!   generated surfaces (and this workspace's earlier milestones) provide.
//! - [`TextureEncoding::Srgb`] — the bytes are sRGB-encoded, which is what
//!   standard PNG files contain. The GPU decodes them to linear before the
//!   shader samples, so no CPU-side conversion is needed; see
//!   [`mikan_assets::Image`] for the image-loading side.
//!
//! Either way the shader samples linear values, so `texel * color` is uniform
//! across the engine. Alpha is straight (non-premultiplied) in both encodings.
//!
//! [`Renderer::create_texture`]: crate::Renderer::create_texture
//! [`mikan_assets::Image`]: https://docs.rs/mikan-assets

use wgpu::TextureFormat;

use crate::error::RenderError;
use crate::sprite::SamplerMode;

/// How the RGBA8 bytes of a texture are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureEncoding {
    /// The bytes are linear values, matching [`Color`](mikan_core::Color).
    #[default]
    Linear,
    /// The bytes are sRGB-encoded; the GPU decodes them when sampling.
    Srgb,
}

impl TextureEncoding {
    /// The `wgpu` texture format this encoding uploads as.
    #[must_use]
    pub const fn format(self) -> TextureFormat {
        match self {
            Self::Linear => TextureFormat::Rgba8Unorm,
            Self::Srgb => TextureFormat::Rgba8UnormSrgb,
        }
    }
}

/// Describes a texture to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureDescriptor<'a> {
    /// Width in pixels; must be non-zero and within the device limit.
    pub width: u32,
    /// Height in pixels; must be non-zero and within the device limit.
    pub height: u32,
    /// Tightly packed RGBA8 pixels, row-major, top row first.
    ///
    /// The length must be exactly `width * height * 4` bytes. How the bytes are
    /// encoded is [`TextureDescriptor::encoding`].
    pub pixels: &'a [u8],
    /// How the RGB bytes are encoded; alpha is always straight.
    pub encoding: TextureEncoding,
}

impl<'a> TextureDescriptor<'a> {
    /// Creates a descriptor from dimensions and tightly packed RGBA8 pixels,
    /// interpreted as linear values.
    #[must_use]
    pub const fn new(width: u32, height: u32, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            pixels,
            encoding: TextureEncoding::Linear,
        }
    }

    /// Sets how the pixel bytes are encoded.
    #[must_use]
    pub const fn with_encoding(mut self, encoding: TextureEncoding) -> Self {
        self.encoding = encoding;
        self
    }
}

/// Checks a texture descriptor against the device's size limit.
///
/// Pure, so the rules are unit-tested without a GPU; `Texture::create` runs
/// exactly this before creating any resource.
pub(crate) fn validate(
    descriptor: &TextureDescriptor<'_>,
    max_dimension: u32,
) -> Result<(), RenderError> {
    if descriptor.width == 0 || descriptor.height == 0 {
        return Err(RenderError::TextureZeroSize {
            width: descriptor.width,
            height: descriptor.height,
        });
    }
    if descriptor.width > max_dimension || descriptor.height > max_dimension {
        return Err(RenderError::TextureTooLarge {
            width: descriptor.width,
            height: descriptor.height,
            max: max_dimension,
        });
    }
    let expected = u64::from(descriptor.width) * u64::from(descriptor.height) * 4;
    let actual = descriptor.pixels.len() as u64;
    if actual != expected {
        return Err(RenderError::TextureDataLength {
            width: descriptor.width,
            height: descriptor.height,
            expected,
            actual,
        });
    }
    Ok(())
}

/// A texture that can be drawn with [`Sprite`](crate::Sprite).
///
/// Created by [`Renderer::create_texture`](crate::Renderer::create_texture).
/// The texture may be used as often as needed, including across frames, but
/// only with the [`Renderer`](crate::Renderer) that created it: it carries bind
/// groups built from that renderer's pipeline layout.
///
/// Each texture holds one bind group per [`SamplerMode`], so a sprite can pick
/// nearest or linear filtering per draw without recreating anything.
#[derive(Debug)]
pub struct Texture {
    texture: wgpu::Texture,
    /// Indexed by [`SamplerMode::index`].
    bind_groups: [wgpu::BindGroup; 2],
}

impl Texture {
    /// Uploads `descriptor` as an RGBA8 texture with a single mip level.
    pub(crate) fn create(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        samplers: &[wgpu::Sampler; 2],
        label: &str,
        descriptor: &TextureDescriptor<'_>,
    ) -> Result<Self, RenderError> {
        validate(descriptor, device.limits().max_texture_dimension_2d)?;

        let extent = wgpu::Extent3d {
            width: descriptor.width,
            height: descriptor.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: descriptor.encoding.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            descriptor.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(descriptor.width * 4),
                rows_per_image: Some(descriptor.height),
            },
            extent,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_groups = samplers.each_ref().map(|sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        });

        Ok(Self {
            texture,
            bind_groups,
        })
    }

    /// Size of the texture in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    pub(crate) fn bind_group(&self, sampler: SamplerMode) -> &wgpu::BindGroup {
        &self.bind_groups[sampler.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_descriptors_are_accepted() {
        let one = [0u8; 4];
        assert!(validate(&TextureDescriptor::new(1, 1, &one), 8192).is_ok());

        let atlas = vec![0u8; 64 * 32 * 4];
        assert!(validate(&TextureDescriptor::new(64, 32, &atlas), 8192).is_ok());

        let edge = vec![0u8; 16 * 16 * 4];
        assert!(
            validate(&TextureDescriptor::new(16, 16, &edge), 16).is_ok(),
            "a texture exactly at the device limit is allowed"
        );
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        for (width, height) in [(0, 4), (4, 0), (0, 0)] {
            assert!(matches!(
                validate(&TextureDescriptor::new(width, height, &[]), 8192),
                Err(RenderError::TextureZeroSize { .. })
            ));
        }
    }

    #[test]
    fn oversized_textures_are_rejected_before_uploading() {
        let pixels = vec![0u8; 17 * 16 * 4];
        assert!(matches!(
            validate(&TextureDescriptor::new(17, 16, &pixels), 16),
            Err(RenderError::TextureTooLarge {
                width: 17,
                height: 16,
                max: 16
            })
        ));
        // The size check runs before the data check, whatever the data says.
        assert!(matches!(
            validate(&TextureDescriptor::new(1000, 1000, &[]), 64),
            Err(RenderError::TextureTooLarge { .. })
        ));
    }

    #[test]
    fn data_length_must_match_the_dimensions_exactly() {
        let short = [0u8; 15];
        assert!(matches!(
            validate(&TextureDescriptor::new(2, 2, &short), 8192),
            Err(RenderError::TextureDataLength {
                expected: 16,
                actual: 15,
                ..
            })
        ));
        let long = [0u8; 17];
        assert!(matches!(
            validate(&TextureDescriptor::new(2, 2, &long), 8192),
            Err(RenderError::TextureDataLength {
                expected: 16,
                actual: 17,
                ..
            })
        ));
    }

    #[test]
    fn encoding_selects_the_matching_gpu_format() {
        assert_eq!(
            TextureDescriptor::new(1, 1, &[0; 4]).encoding,
            TextureEncoding::Linear,
            "the default keeps the pre-asset behavior"
        );
        assert_eq!(TextureEncoding::Linear.format(), TextureFormat::Rgba8Unorm);
        assert_eq!(
            TextureEncoding::Srgb.format(),
            TextureFormat::Rgba8UnormSrgb
        );
        let descriptor = TextureDescriptor::new(1, 1, &[0; 4]).with_encoding(TextureEncoding::Srgb);
        assert_eq!(descriptor.encoding, TextureEncoding::Srgb);
    }
}
