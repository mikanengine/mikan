//! Sprites: rectangles that are flat-colored or textured, plus the pipeline,
//! camera uniform, samplers, batching, and statistics that draw them.
//!
//! Composing sprites with the GPU is intentionally simple:
//!
//! - one pipeline draws every sprite as an instance of a unit quad;
//! - one run of consecutive sprites that share a texture *and* a sampler mode
//!   is one instanced draw call, in submission order, so painter order is never
//!   changed by batching;
//! - the camera is a single view/projection matrix in a uniform buffer;
//! - every texture carries one bind group per [`SamplerMode`], so switching
//!   between crisp and smooth sampling is a batching decision, not a pipeline
//!   change.
//!
//! The per-instance layout is shared with `sprite.wgsl` and must not change
//! without changing the shader in the same commit;
//! `instance_layout_matches_shader_expectations` guards the agreement.

use bytemuck::{Pod, Zeroable};
use mikan_core::{Camera2d, Color, UvRect};

use crate::texture::Texture;

/// How a sprite samples its texture.
///
/// The mode affects filtering only, so it never costs a pipeline switch:
/// textures carry a bind group for each mode and the batcher treats the mode as
/// part of its grouping key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SamplerMode {
    /// Linear filtering: smooth scaling, the default for ordinary sprites.
    #[default]
    Linear,
    /// Nearest-neighbor filtering: crisp, blocky texels for pixel art.
    Nearest,
}

impl SamplerMode {
    /// Index into the per-texture bind group pair, matching the sampler order
    /// in [`SpriteRenderer`].
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Linear => 0,
            Self::Nearest => 1,
        }
    }
}

/// A sprite: a rectangle drawn in world space.
///
/// `center` and `size` are in world units and are mapped to the viewport by the
/// frame's camera; without a camera, one world unit is one physical pixel with
/// the origin at the top-left corner of the window, which is the coordinate
/// space cursor positions arrive in.
///
/// A sprite with a `texture` samples it through `uv` and multiplies the texel
/// by `color`; a sprite without one is filled with `color` alone. Alpha is
/// straight (non-premultiplied): the shader outputs `texel.rgb * color.rgb`
/// with `texel.a * color.a`, and the pipeline blends with
/// `src_alpha`/`one_minus_src_alpha`.
#[derive(Debug, Clone, Copy)]
pub struct Sprite<'a> {
    /// Center of the rectangle, in world units.
    pub center: [f32; 2],
    /// Full width and height, in world units.
    pub size: [f32; 2],
    /// Rotation around the center, in radians. Positive angles turn clockwise
    /// because world `y` grows downward.
    pub rotation: f32,
    /// Linear RGBA tint multiplied with the sampled texel, or the fill color
    /// when `texture` is `None`.
    pub color: Color,
    /// Region of `texture` to sample; ignored when `texture` is `None`.
    pub uv: UvRect,
    /// Filtering used when sampling `texture`.
    pub sampler: SamplerMode,
    /// Texture to sample, or `None` for a flat colored rectangle.
    pub texture: Option<&'a Texture>,
}

impl<'a> Sprite<'a> {
    /// A textured sprite covering the whole texture, untinted and unrotated.
    #[must_use]
    pub fn new(center: [f32; 2], size: [f32; 2], texture: &'a Texture) -> Self {
        Self {
            center,
            size,
            rotation: 0.0,
            color: Color::WHITE,
            uv: UvRect::FULL,
            sampler: SamplerMode::default(),
            texture: Some(texture),
        }
    }

    /// A flat colored sprite that samples no texture.
    #[must_use]
    pub fn solid(center: [f32; 2], size: [f32; 2], color: Color) -> Self {
        Self {
            center,
            size,
            rotation: 0.0,
            color,
            uv: UvRect::FULL,
            sampler: SamplerMode::default(),
            texture: None,
        }
    }

    /// Places the sprite by its top-left corner instead of its center.
    ///
    /// The corner is the unrotated top-left of the rectangle: rotation happens
    /// around the center, so a rotated sprite is not guaranteed to have its
    /// visual bounds start at `top_left`.
    #[must_use]
    pub fn at_top_left(mut self, top_left: [f32; 2]) -> Self {
        self.center = [
            top_left[0] + self.size[0] * 0.5,
            top_left[1] + self.size[1] * 0.5,
        ];
        self
    }

    /// Replaces the tint (or fill color).
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Restricts sampling to `uv`.
    #[must_use]
    pub fn with_uv(mut self, uv: UvRect) -> Self {
        self.uv = uv;
        self
    }

    /// Rotates the sprite around its center, in radians.
    #[must_use]
    pub fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// Chooses the texture filtering used for this sprite.
    #[must_use]
    pub fn with_sampler(mut self, sampler: SamplerMode) -> Self {
        self.sampler = sampler;
        self
    }
}

/// Two triangles covering a unit square centered on the origin.
const UNIT_QUAD: [[f32; 2]; 6] = [
    [-0.5, -0.5],
    [0.5, -0.5],
    [0.5, 0.5],
    [-0.5, -0.5],
    [0.5, 0.5],
    [-0.5, 0.5],
];

/// Per-instance data uploaded to the GPU, shared with `sprite.wgsl`.
///
/// `repr(C)`, all `f32`: 56 bytes with no padding. The layout test below keeps
/// this in sync with the attribute list and the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct SpriteInstance {
    /// Center in world units.
    center: [f32; 2],
    /// Full width and height in world units.
    size: [f32; 2],
    /// Linear RGBA tint.
    color: [f32; 4],
    /// Region to sample as `min_u, min_v, max_u, max_v`.
    uv: [f32; 4],
    /// `cos(rotation)`, `sin(rotation)`, precomputed so the shader rotates
    /// without trigonometry.
    rotation: [f32; 2],
}

impl SpriteInstance {
    fn from_sprite(sprite: &Sprite<'_>) -> Self {
        let (sin, cos) = sprite.rotation.sin_cos();
        Self {
            center: sprite.center,
            size: sprite.size,
            color: sprite.color.to_array(),
            uv: [
                sprite.uv.min[0],
                sprite.uv.min[1],
                sprite.uv.max[0],
                sprite.uv.max[1],
            ],
            rotation: [cos, sin],
        }
    }
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x2
];

const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<[f32; 2]>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRIBUTES,
};

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<SpriteInstance>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &INSTANCE_ATTRIBUTES,
};

/// Size of the camera uniform: `mat4x4<f32>`.
const CAMERA_UNIFORM_SIZE: wgpu::BufferAddress = 64;

/// Instances allocated on first use.
const INITIAL_INSTANCE_CAPACITY: u32 = 256;

/// What a frame submitted to the GPU actually consisted of.
///
/// Obtained from [`Renderer::last_frame_stats`](crate::Renderer::last_frame_stats)
/// after [`Renderer::render_frame`](crate::Renderer::render_frame). When a
/// frame is skipped before submission (an unusable surface, for example), the
/// statistics describe the last frame that was submitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Sprites in the frame.
    pub sprites: usize,
    /// Batches the sprites were grouped into, in submission order.
    pub batches: usize,
    /// Instanced draw calls issued for those batches.
    pub draw_calls: usize,
}

/// Builds the column-major view/projection matrix that maps world units to
/// clip space.
///
/// The matrix is `translate(-position) * scale(zoom)` followed by the viewport
/// mapping: `camera.position` lands at the center of the viewport, `zoom`
/// physical pixels cover one world unit, and the y axis flips (world y grows
/// downward, clip y grows upward). `Camera2d::world_to_screen` documents the
/// same transformation in screen space; `camera_matrix_agrees_with_world_to_screen`
/// keeps the two in agreement.
fn view_projection(camera: Camera2d, viewport: [f32; 2]) -> [[f32; 4]; 4] {
    let zoom = camera.effective_zoom();
    let scale_x = 2.0 * zoom / viewport[0];
    let scale_y = -2.0 * zoom / viewport[1];
    [
        [scale_x, 0.0, 0.0, 0.0],
        [0.0, scale_y, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [
            -scale_x * camera.position[0],
            -scale_y * camera.position[1],
            0.0,
            1.0,
        ],
    ]
}

/// Batching key: two sprites form one batch only when they reference the same
/// [`Texture`] value *and* the same sampler mode, because each combination has
/// its own bind group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BatchKey {
    texture: Option<usize>,
    sampler: SamplerMode,
}

impl BatchKey {
    fn of(sprite: &Sprite<'_>) -> Self {
        Self {
            // Identity is the address of the texture, because each texture has
            // its own bind groups.
            texture: sprite
                .texture
                .map(|texture| std::ptr::from_ref(texture).addr()),
            sampler: sprite.sampler,
        }
    }
}

/// Groups consecutive sprites that share a batch key into `(start, count)`
/// draw ranges, in submission order.
///
/// Pure so the grouping rules are unit-tested without a GPU; the renderer
/// feeds it one key per sprite.
fn plan_batches(keys: impl IntoIterator<Item = BatchKey>, batches: &mut Vec<(u32, u32)>) {
    batches.clear();
    let mut previous: Option<BatchKey> = None;
    for (index, key) in keys.into_iter().enumerate() {
        if previous != Some(key) {
            batches.push((index as u32, 1));
            previous = Some(key);
        } else if let Some(last) = batches.last_mut() {
            last.1 += 1;
        }
    }
}

/// Uploads and draws the sprite pipeline, plus the fallback texture.
pub(crate) struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    camera_uniform: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    /// Indexed by [`SamplerMode::index`]: linear first, nearest second.
    samplers: [wgpu::Sampler; 2],
    /// White 1x1 texture sampled by sprites that carry none.
    fallback: Texture,
    vertices: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    /// CPU-side instance data, reused between frames.
    instances: Vec<SpriteInstance>,
    /// `(start, count)` ranges into `instances`, one per draw call.
    batches: Vec<(u32, u32)>,
    last_stats: FrameStats,
}

impl SpriteRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mikan sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mikan camera bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mikan texture bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let camera_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan camera uniform"),
            size: CAMERA_UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mikan camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform.as_entire_binding(),
            }],
        });

        // Clamping to the edge means a sub-rectangle can never sample past the
        // texture border. The two filters are the whole difference between the
        // modes; everything else is shared.
        let sampler = |mode: SamplerMode| {
            let filter = match mode {
                SamplerMode::Linear => wgpu::FilterMode::Linear,
                SamplerMode::Nearest => wgpu::FilterMode::Nearest,
            };
            let label = match mode {
                SamplerMode::Linear => "mikan linear sampler",
                SamplerMode::Nearest => "mikan nearest sampler",
            };
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                lod_min_clamp: 0.0,
                lod_max_clamp: 0.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            })
        };
        let samplers = [sampler(SamplerMode::Linear), sampler(SamplerMode::Nearest)];

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mikan sprite pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mikan sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(VERTEX_LAYOUT), Some(INSTANCE_LAYOUT)],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // Straight alpha: the shader outputs non-premultiplied RGBA.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let white = [255u8, 255, 255, 255];
        let fallback = Texture::create(
            device,
            queue,
            &texture_layout,
            &samplers,
            "mikan fallback texture",
            &crate::TextureDescriptor::new(1, 1, &white),
        )
        .expect("a 1x1 texture is always within device limits");

        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan unit quad vertices"),
            size: size_of_val(&UNIT_QUAD) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The unit quad never changes, so it is uploaded exactly once, here.
        queue.write_buffer(&vertices, 0, bytemuck::cast_slice(&UNIT_QUAD));

        Self {
            pipeline,
            camera_uniform,
            camera_bind_group,
            texture_layout,
            samplers,
            fallback,
            vertices,
            instance_buffer: Self::create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            instances: Vec::new(),
            batches: Vec::new(),
            last_stats: FrameStats::default(),
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan sprite instances"),
            size: u64::from(capacity) * size_of::<SpriteInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Statistics for the most recently prepared frame.
    pub(crate) fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    /// Uploads a texture that sprites can sample.
    pub(crate) fn create_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        descriptor: &crate::TextureDescriptor<'_>,
        label: &str,
    ) -> Result<Texture, crate::RenderError> {
        Texture::create(
            device,
            queue,
            &self.texture_layout,
            &self.samplers,
            label,
            descriptor,
        )
    }

    /// Uploads the camera for this frame.
    fn update_camera(&self, queue: &wgpu::Queue, camera: Option<Camera2d>, viewport: (u32, u32)) {
        let size = [viewport.0 as f32, viewport.1 as f32];
        // Without a camera, world space is pixel space, which is the behavior
        // every earlier frame had.
        let camera = camera.unwrap_or_else(|| Camera2d::pixel_space(size));
        let matrix = view_projection(camera, size);
        queue.write_buffer(&self.camera_uniform, 0, bytemuck::cast_slice(&matrix));
    }

    /// Builds and uploads this frame's instance data and batches, growing the
    /// instance buffer when the frame needs more room than previous frames.
    ///
    /// Returns an error — before any GPU work for the frame is recorded — when
    /// the frame cannot fit in a single vertex buffer on this device.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: Option<Camera2d>,
        viewport: (u32, u32),
        sprites: &[Sprite<'_>],
    ) -> Result<(), crate::RenderError> {
        self.update_camera(queue, camera, viewport);

        self.instances.clear();
        self.instances
            .extend(sprites.iter().map(SpriteInstance::from_sprite));
        plan_batches(sprites.iter().map(BatchKey::of), &mut self.batches);

        self.ensure_capacity(device, self.instances.len())?;
        if !self.instances.is_empty() {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.instances),
            );
        }

        self.last_stats = FrameStats {
            sprites: self.instances.len(),
            batches: self.batches.len(),
            draw_calls: self.batches.len(),
        };
        Ok(())
    }

    /// Replaces the instance buffer when `required` instances do not fit in the
    /// current one, growing geometrically and never shrinking.
    ///
    /// The growth is bounded by the device's maximum buffer size, so a frame
    /// with an absurd sprite count fails with an error instead of asking the
    /// GPU for an impossible allocation.
    fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        required: usize,
    ) -> Result<(), crate::RenderError> {
        if required <= self.instance_capacity as usize {
            return Ok(());
        }
        let max_instances = max_instances(device);
        let Some(capacity) = grown_capacity(required, max_instances) else {
            return Err(crate::RenderError::TooManySprites {
                count: required,
                max: max_instances as usize,
            });
        };
        log::debug!("growing sprite instance buffer to {capacity} instances");
        self.instance_buffer = Self::create_instance_buffer(device, capacity);
        self.instance_capacity = capacity;
        Ok(())
    }

    /// Records the prepared frame, one instanced draw call per batch.
    pub(crate) fn record(&self, pass: &mut wgpu::RenderPass<'_>, sprites: &[Sprite<'_>]) {
        if self.batches.is_empty() {
            return;
        }
        debug_assert_eq!(
            self.last_stats.draw_calls,
            self.batches.len(),
            "statistics must describe the batches being recorded"
        );

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));

        let stride = size_of::<SpriteInstance>() as u64;
        for &(start, len) in &self.batches {
            // Every batch begins with the sprite that opened it, so that sprite
            // identifies the bind group for the whole run.
            let opener = &sprites[start as usize];
            let bind_group = match opener.texture {
                Some(texture) => texture.bind_group(opener.sampler),
                None => self.fallback.bind_group(opener.sampler),
            };
            pass.set_bind_group(1, bind_group, &[]);
            let begin = u64::from(start) * stride;
            let end = u64::from(start + len) * stride;
            pass.set_vertex_buffer(1, self.instance_buffer.slice(begin..end));
            pass.draw(0..UNIT_QUAD.len() as u32, 0..len);
        }
    }
}

/// Full size of the instance data uploaded per sprite.
#[cfg(test)]
const INSTANCE_STRIDE: u64 = size_of::<SpriteInstance>() as u64;

/// The largest number of instances a single vertex buffer can hold on this
/// device.
fn max_instances(device: &wgpu::Device) -> u64 {
    (device.limits().max_buffer_size / size_of::<SpriteInstance>() as u64).min(u64::from(u32::MAX))
}

/// Chooses the capacity for `required` instances: the next power of two, never
/// above `max_instances`, or `None` when even `required` does not fit.
///
/// Pure so the growth policy is unit-tested without a GPU.
fn grown_capacity(required: usize, max_instances: u64) -> Option<u32> {
    if required as u64 > max_instances {
        return None;
    }
    // Fits in `u32` because it does not exceed `max_instances`.
    let required = required as u32;
    Some(
        required
            .checked_next_power_of_two()
            .unwrap_or(required)
            .min(max_instances as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: [f32; 2] = [960.0, 600.0];

    fn project(matrix: &[[f32; 4]; 4], world: [f32; 2]) -> [f32; 2] {
        // clip = M * (world, 0, 1), with M in column-major order.
        [
            matrix[0][0] * world[0] + matrix[1][0] * world[1] + matrix[3][0],
            matrix[0][1] * world[0] + matrix[1][1] * world[1] + matrix[3][1],
        ]
    }

    fn close(actual: [f32; 2], expected: [f32; 2]) {
        for axis in 0..2 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-4,
                "axis {axis}: expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn instance_layout_matches_shader_expectations() {
        // repr(C) with only f32 members: exactly 56 bytes and no padding.
        assert_eq!(size_of::<SpriteInstance>(), 56);
        assert_eq!(INSTANCE_LAYOUT.array_stride, 56);
        assert_eq!(INSTANCE_STRIDE, 56);
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 5);
        assert_eq!(INSTANCE_ATTRIBUTES[0].shader_location, 1);
        assert_eq!(INSTANCE_ATTRIBUTES[0].offset, 0); // center
        assert_eq!(INSTANCE_ATTRIBUTES[1].shader_location, 2);
        assert_eq!(INSTANCE_ATTRIBUTES[1].offset, 8); // size
        assert_eq!(INSTANCE_ATTRIBUTES[2].shader_location, 3);
        assert_eq!(INSTANCE_ATTRIBUTES[2].offset, 16); // color
        assert_eq!(INSTANCE_ATTRIBUTES[3].shader_location, 4);
        assert_eq!(INSTANCE_ATTRIBUTES[3].offset, 32); // uv rectangle
        assert_eq!(INSTANCE_ATTRIBUTES[4].shader_location, 5);
        assert_eq!(INSTANCE_ATTRIBUTES[4].offset, 48); // cos/sin of rotation
    }

    #[test]
    fn vertex_layout_describes_two_float_positions() {
        assert_eq!(VERTEX_LAYOUT.array_stride, 8);
        assert_eq!(VERTEX_ATTRIBUTES[0].shader_location, 0);
        assert_eq!(VERTEX_ATTRIBUTES[0].offset, 0);
        assert_eq!(UNIT_QUAD.len(), 6, "two triangles");
    }

    #[test]
    fn shader_matches_the_pipeline_contract() {
        let source = include_str!("sprite.wgsl");
        assert!(source.contains("fn vs_main"), "vertex entry point");
        assert!(source.contains("fn fs_main"), "fragment entry point");
        assert!(source.contains("view_projection"), "camera uniform");
        assert!(source.contains("textureSample"), "textured sampling");
        assert!(source.contains("rotation"), "per-instance rotation");
        assert!(
            source.contains("mix(uv_rect.x, uv_rect.z, normalized.x)"),
            "uv rectangle selection"
        );
    }

    #[test]
    fn sampler_modes_have_distinct_bind_group_indices() {
        assert_eq!(SamplerMode::Linear.index(), 0);
        assert_eq!(SamplerMode::Nearest.index(), 1);
        assert_eq!(SamplerMode::default(), SamplerMode::Linear);
    }

    #[test]
    fn capacity_grows_geometrically_and_stops_at_the_device_limit() {
        let max = 10_000u64;
        assert_eq!(grown_capacity(1, max), Some(1));
        assert_eq!(grown_capacity(5, max), Some(8));
        assert_eq!(grown_capacity(1000, max), Some(1024));
        assert_eq!(
            grown_capacity(9000, max),
            Some(10_000),
            "the power of two is clamped to what the device can allocate"
        );
        assert_eq!(grown_capacity(10_000, max), Some(10_000));
        assert_eq!(
            grown_capacity(10_001, max),
            None,
            "a frame that cannot fit is rejected instead of overflowing"
        );
        // The degenerate end of the u32 range does not panic.
        assert_eq!(grown_capacity(usize::MAX, u64::from(u32::MAX)), None);
        assert_eq!(
            grown_capacity(usize::try_from(u32::MAX).unwrap(), u64::from(u32::MAX)),
            Some(u32::MAX)
        );
    }

    #[test]
    fn solid_sprites_keep_the_flat_rectangle_behavior() {
        let sprite = Sprite::solid([100.0, 50.0], [30.0, 40.0], Color::WHITE);
        assert_eq!(sprite.center, [100.0, 50.0]);
        assert_eq!(sprite.size, [30.0, 40.0]);
        assert_eq!(sprite.color, Color::WHITE);
        assert_eq!(sprite.uv, UvRect::FULL);
        assert_eq!(sprite.rotation, 0.0);
        assert_eq!(sprite.sampler, SamplerMode::Linear);
        assert!(
            sprite.texture.is_none(),
            "a solid sprite samples no texture"
        );

        let instance = SpriteInstance::from_sprite(&sprite);
        assert_eq!(instance.center, [100.0, 50.0]);
        assert_eq!(instance.size, [30.0, 40.0]);
        assert_eq!(instance.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(instance.uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(instance.rotation, [1.0, 0.0], "cos, sin of zero rotation");
    }

    #[test]
    fn builders_replace_color_uv_rotation_and_sampler() {
        let uv = UvRect::tile(2, 4, 5).expect("tile 5 exists in a 2x4 grid");
        let sprite = Sprite::solid([0.0, 0.0], [16.0, 16.0], Color::WHITE)
            .with_color(Color::new(0.25, 0.5, 0.75, 0.5))
            .with_uv(uv)
            .with_rotation(std::f32::consts::FRAC_PI_2)
            .with_sampler(SamplerMode::Nearest);
        assert_eq!(sprite.color, Color::new(0.25, 0.5, 0.75, 0.5));
        assert_eq!(sprite.uv, uv);
        assert_eq!(sprite.sampler, SamplerMode::Nearest);

        let instance = SpriteInstance::from_sprite(&sprite);
        assert_eq!(instance.color, [0.25, 0.5, 0.75, 0.5]);
        let [cos, sin] = instance.rotation;
        assert!(cos.abs() < 1e-6, "cos(PI/2) is zero, got {cos}");
        assert!((sin - 1.0).abs() < 1e-6, "sin(PI/2) is one, got {sin}");
    }

    #[test]
    fn top_left_placement_agrees_with_center_placement() {
        let placed =
            Sprite::solid([0.0, 0.0], [30.0, 40.0], Color::WHITE).at_top_left([10.0, 20.0]);
        let centered = Sprite::solid([25.0, 40.0], [30.0, 40.0], Color::WHITE);
        assert_eq!(placed.center, centered.center);
        assert_eq!(placed.size, centered.size);
    }

    #[test]
    fn batches_group_consecutive_sprites_sharing_texture_and_sampler() {
        let linear = |texture| BatchKey {
            texture,
            sampler: SamplerMode::Linear,
        };
        let nearest = |texture| BatchKey {
            texture,
            sampler: SamplerMode::Nearest,
        };
        let mut batches = Vec::new();

        plan_batches(
            [linear(None), linear(None), linear(Some(7)), linear(None)],
            &mut batches,
        );
        assert_eq!(batches, [(0, 2), (2, 1), (3, 1)]);

        plan_batches(
            [linear(Some(1)), linear(Some(1)), linear(Some(1))],
            &mut batches,
        );
        assert_eq!(batches, [(0, 3)], "equal keys form one batch");

        plan_batches(
            [linear(Some(1)), linear(Some(2)), linear(Some(1))],
            &mut batches,
        );
        assert_eq!(
            batches,
            [(0, 1), (1, 1), (2, 1)],
            "interleaving textures splits batches without reordering"
        );

        plan_batches(
            [
                nearest(Some(1)),
                nearest(Some(1)),
                linear(Some(1)),
                nearest(Some(1)),
            ],
            &mut batches,
        );
        assert_eq!(
            batches,
            [(0, 2), (2, 1), (3, 1)],
            "the sampler mode is part of the batching key"
        );

        plan_batches([], &mut batches);
        assert!(batches.is_empty());
    }

    #[test]
    fn pixel_space_camera_reproduces_screen_space_rendering() {
        let matrix = view_projection(Camera2d::pixel_space(VIEWPORT), VIEWPORT);
        close(project(&matrix, [0.0, 0.0]), [-1.0, 1.0]);
        close(project(&matrix, VIEWPORT), [1.0, -1.0]);
        close(project(&matrix, [480.0, 300.0]), [0.0, 0.0]);
    }

    #[test]
    fn camera_matrix_agrees_with_world_to_screen() {
        let cameras = [
            Camera2d::new([0.0, 0.0], 1.0),
            Camera2d::new([120.0, -40.0], 2.5),
            Camera2d::new([-500.0, 900.0], 0.35),
            Camera2d::pixel_perfect([120.4, 75.9], [960, 600], [240, 150]),
        ];
        let worlds = [[0.0, 0.0], [100.0, 100.0], [-250.0, 480.0], VIEWPORT];
        for camera in cameras {
            let matrix = view_projection(camera, VIEWPORT);
            for world in worlds {
                let screen = camera.world_to_screen(VIEWPORT, world);
                let expected = [
                    screen[0] / VIEWPORT[0] * 2.0 - 1.0,
                    1.0 - screen[1] / VIEWPORT[1] * 2.0,
                ];
                close(project(&matrix, world), expected);
            }
        }
    }

    #[test]
    fn invalid_zoom_renders_one_to_one_instead_of_nan() {
        for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let matrix = view_projection(Camera2d::new([0.0, 0.0], zoom), [100.0, 100.0]);
            for value in matrix.iter().flatten() {
                assert!(
                    value.is_finite(),
                    "zoom {zoom} produced a non-finite matrix"
                );
            }
            // With zoom 1, the camera's position is the viewport center and one
            // world unit is one pixel, so the viewport edges are 50 units away.
            close(project(&matrix, [0.0, 0.0]), [0.0, 0.0]);
            close(project(&matrix, [50.0, 0.0]), [1.0, 0.0]);
            close(project(&matrix, [0.0, 50.0]), [0.0, -1.0]);
        }
    }
}
