//! Sprites: rectangles that are flat-colored or textured, plus the pipeline,
//! camera uniform, and batching that draw them.
//!
//! Composing sprites with the GPU is intentionally simple:
//!
//! - one pipeline draws every sprite as an instance of a unit quad;
//! - one run of consecutive sprites that share a texture is one instanced draw
//!   call, in submission order, so painter order is never changed by batching;
//! - the camera is a single view/projection matrix in a uniform buffer.
//!
//! The per-instance layout is shared with `sprite.wgsl` and must not change
//! without changing the shader in the same commit;
//! `instance_layout_matches_shader_expectations` guards the agreement.

use bytemuck::{Pod, Zeroable};
use mikan_core::{Camera2d, Color};

use crate::texture::Texture;

/// A rectangle in normalized texture coordinates.
///
/// `[0.0, 0.0]` is the top-left corner of a texture and `[1.0, 1.0]` the
/// bottom-right, matching the y-down world. A sub-rectangle selects one region
/// of a texture atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    /// Top-left corner of the region.
    pub min: [f32; 2],
    /// Bottom-right corner of the region.
    pub max: [f32; 2],
}

impl UvRect {
    /// The whole texture.
    pub const FULL: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    /// Creates a region from two corners.
    #[must_use]
    pub const fn new(min: [f32; 2], max: [f32; 2]) -> Self {
        Self { min, max }
    }

    /// The region of tile `index` in a `columns` by `rows` atlas grid.
    ///
    /// Tiles are numbered row-major from the top-left. Returns `None` when the
    /// grid is empty or `index` is outside it, so callers can report the
    /// mistake instead of sampling the wrong region.
    #[must_use]
    pub fn tile(columns: u32, rows: u32, index: u32) -> Option<Self> {
        if columns == 0 || rows == 0 || u64::from(index) >= u64::from(columns) * u64::from(rows) {
            return None;
        }
        let width = 1.0 / columns as f32;
        let height = 1.0 / rows as f32;
        let column = (index % columns) as f32;
        let row = (index / columns) as f32;
        let min = [column * width, row * height];
        Some(Self {
            min,
            max: [min[0] + width, min[1] + height],
        })
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
/// by `color`; a sprite without one is filled with `color` alone.
#[derive(Debug, Clone, Copy)]
pub struct Sprite<'a> {
    /// Center of the rectangle, in world units.
    pub center: [f32; 2],
    /// Full width and height, in world units.
    pub size: [f32; 2],
    /// Linear RGBA tint multiplied with the sampled texel, or the fill color
    /// when `texture` is `None`.
    pub color: Color,
    /// Region of `texture` to sample; ignored when `texture` is `None`.
    pub uv: UvRect,
    /// Texture to sample, or `None` for a flat colored rectangle.
    pub texture: Option<&'a Texture>,
}

impl<'a> Sprite<'a> {
    /// A textured sprite covering the whole texture, untinted.
    #[must_use]
    pub fn new(center: [f32; 2], size: [f32; 2], texture: &'a Texture) -> Self {
        Self {
            center,
            size,
            color: Color::WHITE,
            uv: UvRect::FULL,
            texture: Some(texture),
        }
    }

    /// A flat colored sprite that samples no texture.
    #[must_use]
    pub fn solid(center: [f32; 2], size: [f32; 2], color: Color) -> Self {
        Self {
            center,
            size,
            color,
            uv: UvRect::FULL,
            texture: None,
        }
    }

    /// Places the sprite by its top-left corner instead of its center.
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
}

impl SpriteInstance {
    fn from_sprite(sprite: &Sprite<'_>) -> Self {
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
        }
    }
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4];

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
/// [`Texture`] value, because each texture has its own bind group.
fn texture_identity(texture: &Texture) -> usize {
    std::ptr::from_ref(texture).addr()
}

/// Groups consecutive sprites that share a texture into `(start, count)` draw
/// ranges, in submission order.
///
/// Pure so the grouping rules are unit-tested without a GPU; the renderer
/// feeds it one `Option<usize>` key per sprite.
fn plan_batches(keys: impl IntoIterator<Item = Option<usize>>, batches: &mut Vec<(u32, u32)>) {
    batches.clear();
    let mut previous: Option<Option<usize>> = None;
    for (index, key) in keys.into_iter().enumerate() {
        if previous != Some(key) {
            batches.push((index as u32, 1));
            previous = Some(key);
        } else if let Some(last) = batches.last_mut() {
            last.1 += 1;
        }
    }
}

/// Owns the sprite pipeline, its buffers, and the fallback texture.
pub(crate) struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    camera_uniform: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// White 1x1 texture sampled by sprites that carry none.
    fallback: Texture,
    vertices: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    /// CPU-side instance data, reused between frames.
    instances: Vec<SpriteInstance>,
    /// `(start, count)` ranges into `instances`, one per draw call.
    batches: Vec<(u32, u32)>,
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

        // Linear filtering keeps scaled sprites smooth; clamping to the edge
        // means a sub-rectangle can never sample past the atlas border.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mikan sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

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
            &sampler,
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
            sampler,
            fallback,
            vertices,
            instance_buffer: Self::create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            instances: Vec::new(),
            batches: Vec::new(),
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
            &self.sampler,
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

    /// Draws `sprites` in order, one draw call per run of sprites that share a
    /// texture, growing the instance buffer if the frame needs to.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        camera: Option<Camera2d>,
        viewport: (u32, u32),
        sprites: &[Sprite<'_>],
    ) {
        self.update_camera(queue, camera, viewport);

        if sprites.is_empty() {
            return;
        }

        self.instances.clear();
        self.instances
            .extend(sprites.iter().map(SpriteInstance::from_sprite));
        plan_batches(
            sprites
                .iter()
                .map(|sprite| sprite.texture.map(texture_identity)),
            &mut self.batches,
        );

        let count = self.instances.len() as u32;
        if count > self.instance_capacity {
            let capacity = count.next_power_of_two();
            log::debug!("growing sprite instance buffer to {capacity} instances");
            self.instance_buffer = Self::create_instance_buffer(device, capacity);
            self.instance_capacity = capacity;
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));

        let stride = size_of::<SpriteInstance>() as u64;
        for &(start, len) in &self.batches {
            // Every batch begins with the sprite that opened it, so that sprite
            // identifies the bind group for the whole run.
            let bind_group = match sprites[start as usize].texture {
                Some(texture) => texture.bind_group(),
                None => self.fallback.bind_group(),
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
        // repr(C) with only f32 members: exactly 48 bytes and no padding.
        assert_eq!(size_of::<SpriteInstance>(), 48);
        assert_eq!(INSTANCE_LAYOUT.array_stride, 48);
        assert_eq!(INSTANCE_STRIDE, 48);
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 4);
        assert_eq!(INSTANCE_ATTRIBUTES[0].shader_location, 1);
        assert_eq!(INSTANCE_ATTRIBUTES[0].offset, 0); // center
        assert_eq!(INSTANCE_ATTRIBUTES[1].shader_location, 2);
        assert_eq!(INSTANCE_ATTRIBUTES[1].offset, 8); // size
        assert_eq!(INSTANCE_ATTRIBUTES[2].shader_location, 3);
        assert_eq!(INSTANCE_ATTRIBUTES[2].offset, 16); // color
        assert_eq!(INSTANCE_ATTRIBUTES[3].shader_location, 4);
        assert_eq!(INSTANCE_ATTRIBUTES[3].offset, 32); // uv rectangle
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
    }

    #[test]
    fn solid_sprites_keep_the_flat_rectangle_behavior() {
        let sprite = Sprite::solid([100.0, 50.0], [30.0, 40.0], Color::WHITE);
        assert_eq!(sprite.center, [100.0, 50.0]);
        assert_eq!(sprite.size, [30.0, 40.0]);
        assert_eq!(sprite.color, Color::WHITE);
        assert_eq!(sprite.uv, UvRect::FULL);
        assert!(
            sprite.texture.is_none(),
            "a solid sprite samples no texture"
        );

        let instance = SpriteInstance::from_sprite(&sprite);
        assert_eq!(instance.center, [100.0, 50.0]);
        assert_eq!(instance.size, [30.0, 40.0]);
        assert_eq!(instance.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(instance.uv, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn builders_replace_color_and_uv_region() {
        let uv = UvRect::tile(2, 4, 5).expect("tile 5 exists in a 2x4 grid");
        let sprite = Sprite::solid([0.0, 0.0], [16.0, 16.0], Color::WHITE)
            .with_color(Color::new(0.25, 0.5, 0.75, 0.5))
            .with_uv(uv);
        assert_eq!(sprite.color, Color::new(0.25, 0.5, 0.75, 0.5));
        assert_eq!(sprite.uv, uv);

        let instance = SpriteInstance::from_sprite(&sprite);
        assert_eq!(instance.color, [0.25, 0.5, 0.75, 0.5]);
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
    fn uv_rect_tile_grid_is_row_major_from_the_top_left() {
        assert_eq!(UvRect::tile(1, 1, 0), Some(UvRect::FULL));
        assert_eq!(
            UvRect::tile(2, 1, 1),
            Some(UvRect::new([0.5, 0.0], [1.0, 1.0]))
        );
        assert_eq!(
            UvRect::tile(2, 2, 2),
            Some(UvRect::new([0.0, 0.5], [0.5, 1.0])),
            "index 2 starts the second row"
        );
        assert_eq!(
            UvRect::tile(2, 2, 3),
            Some(UvRect::new([0.5, 0.5], [1.0, 1.0]))
        );
    }

    #[test]
    fn uv_rect_rejects_empty_grids_and_out_of_range_tiles() {
        assert_eq!(UvRect::tile(0, 4, 0), None);
        assert_eq!(UvRect::tile(4, 0, 0), None);
        assert_eq!(UvRect::tile(2, 2, 4), None);
        assert_eq!(UvRect::tile(2, 2, u32::MAX), None);
    }

    #[test]
    fn batches_group_consecutive_sprites_with_the_same_texture() {
        let mut batches = Vec::new();

        plan_batches([None, None, Some(7), None], &mut batches);
        assert_eq!(batches, [(0, 2), (2, 1), (3, 1)]);

        plan_batches([Some(1), Some(1), Some(1)], &mut batches);
        assert_eq!(batches, [(0, 3)], "equal keys form one batch");

        plan_batches([Some(1), Some(2), Some(1)], &mut batches);
        assert_eq!(
            batches,
            [(0, 1), (1, 1), (2, 1)],
            "interleaving textures splits batches without reordering"
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
