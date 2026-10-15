//! GPU textures uploaded from CPU pixel data.
//!
//! Textures are created through [`Renderer::create_texture`], which validates
//! the descriptor before touching the GPU. The workspace does not load image
//! files yet; callers provide linear RGBA8 pixels, the same color values
//! [`Color`](mikan_core::Color) uses, so no color conversion is involved.
//!
//! [`Renderer::create_texture`]: crate::Renderer::create_texture

use wgpu::TextureFormat;

use crate::error::RenderError;

/// Format every texture is created with: 8 bits per channel, linear RGBA.
///
/// Linear values match [`Color`](mikan_core::Color) and the rest of the
/// pipeline; an sRGB format belongs to an image-loading milestone, where the
/// loaded encoding is known.
pub const TEXTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Describes a texture to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureDescriptor<'a> {
    /// Width in pixels; must be non-zero and within the device limit.
    pub width: u32,
    /// Height in pixels; must be non-zero and within the device limit.
    pub height: u32,
    /// Tightly packed linear RGBA8 pixels, row-major, top row first.
    ///
    /// The length must be exactly `width * height * 4` bytes.
    pub pixels: &'a [u8],
}

impl<'a> TextureDescriptor<'a> {
    /// Creates a descriptor from dimensions and tightly packed RGBA8 pixels.
    #[must_use]
    pub const fn new(width: u32, height: u32, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// Checks a texture descriptor against the device's size limit.
///
/// Pure, so the rules are unit-tested without a GPU; `Renderer::create_texture`
/// runs exactly this before creating any resource.
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
/// only with the [`Renderer`](crate::Renderer) that created it: it carries a
/// bind group built from that renderer's pipeline layout.
#[derive(Debug)]
pub struct Texture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl Texture {
    /// Uploads `descriptor` as a linear RGBA8 texture with a single mip level.
    pub(crate) fn create(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
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
            format: TEXTURE_FORMAT,
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        });

        Ok(Self {
            texture,
            bind_group,
        })
    }

    /// Size of the texture in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
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
}
