//! Screen-space colored quads: geometry, GPU layout, and the draw call.
//!
//! One pipeline and one instanced draw call cover any number of quads. Each
//! instance carries its own center, size, and color; the pipeline is rebuilt
//! only when the target format changes.

use bytemuck::{Pod, Zeroable};
use mikan_core::Color;

/// A flat, screen-space colored rectangle, measured in physical pixels.
///
/// The coordinate space matches cursor coordinates: the origin is the top-left
/// corner of the window, `x` grows to the right, and `y` grows downward.
///
/// This layout is shared with the vertex shader and must not be changed without
/// changing `quad.wgsl` in the same commit; `layout_matches_shader` guards it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Quad2d {
    /// Center of the rectangle, in pixels.
    pub center: [f32; 2],
    /// Full width and height, in pixels.
    pub size: [f32; 2],
    /// Non-premultiplied linear RGBA color.
    pub color: [f32; 4],
}

impl Quad2d {
    /// Creates a quad from its center, size, and color.
    #[must_use]
    pub const fn new(center: [f32; 2], size: [f32; 2], color: Color) -> Self {
        Self {
            center,
            size,
            color: color.to_array(),
        }
    }

    /// Creates a quad from its top-left corner, size, and color.
    #[must_use]
    pub const fn from_top_left(top_left: [f32; 2], size: [f32; 2], color: Color) -> Self {
        Self {
            center: [top_left[0] + size[0] * 0.5, top_left[1] + size[1] * 0.5],
            size,
            color: color.to_array(),
        }
    }

    /// Color of this quad.
    #[must_use]
    pub const fn color(&self) -> Color {
        Color::new(self.color[0], self.color[1], self.color[2], self.color[3])
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

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Float32x4];

const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<[f32; 2]>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRIBUTES,
};

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<Quad2d>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &INSTANCE_ATTRIBUTES,
};

/// Size of the viewport uniform: `vec2<f32>` rounded up to 16 bytes.
const UNIFORM_SIZE: wgpu::BufferAddress = 16;

/// Instances allocated on first use.
const INITIAL_INSTANCE_CAPACITY: u32 = 256;

/// Owns the quad pipeline and its buffers.
pub(crate) struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: u32,
}

impl QuadRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mikan quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mikan quad bind group layout"),
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

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan viewport uniform"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mikan quad bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mikan quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mikan quad pipeline"),
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
            uniforms,
            bind_group,
            vertices,
            instances: Self::create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mikan quad instances"),
            size: u64::from(capacity) * size_of::<Quad2d>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Updates the viewport used to convert pixels into clip space.
    pub(crate) fn set_viewport(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        let uniforms = [width, height, 0.0, 0.0];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uniforms));
    }

    /// Draws `quads` in the given order, growing the instance buffer if needed.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        quads: &[Quad2d],
    ) {
        let count = quads.len() as u32;
        if count == 0 {
            return;
        }
        if count > self.instance_capacity {
            let capacity = count.next_power_of_two();
            log::debug!("growing quad instance buffer to {capacity} instances");
            self.instances = Self::create_instance_buffer(device, capacity);
            self.instance_capacity = capacity;
        }

        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(quads));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.draw(0..UNIT_QUAD.len() as u32, 0..count);
    }
}

/// Full size of the instance data uploaded per quad.
#[cfg(test)]
const INSTANCE_STRIDE: u64 = size_of::<Quad2d>() as u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_layout_matches_shader_expectations() {
        // repr(C) with only f32 members: exactly 32 bytes and no padding.
        assert_eq!(size_of::<Quad2d>(), 32);
        assert_eq!(INSTANCE_LAYOUT.array_stride, 32);
        assert_eq!(INSTANCE_STRIDE, 32);
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 3);
        assert_eq!(INSTANCE_ATTRIBUTES[0].shader_location, 1);
        assert_eq!(INSTANCE_ATTRIBUTES[0].offset, 0); // center
        assert_eq!(INSTANCE_ATTRIBUTES[1].shader_location, 2);
        assert_eq!(INSTANCE_ATTRIBUTES[1].offset, 8); // size
        assert_eq!(INSTANCE_ATTRIBUTES[2].shader_location, 3);
        assert_eq!(INSTANCE_ATTRIBUTES[2].offset, 16); // color
    }

    #[test]
    fn vertex_layout_describes_two_float_positions() {
        assert_eq!(VERTEX_LAYOUT.array_stride, 8);
        assert_eq!(VERTEX_ATTRIBUTES[0].shader_location, 0);
        assert_eq!(VERTEX_ATTRIBUTES[0].offset, 0);
        assert_eq!(UNIT_QUAD.len(), 6, "two triangles");
    }

    #[test]
    fn shader_entry_points_match_the_pipeline() {
        let source = include_str!("quad.wgsl");
        assert!(source.contains("fn vs_main"), "vertex entry point");
        assert!(source.contains("fn fs_main"), "fragment entry point");
        assert!(source.contains("view_location") || source.contains("viewport"));
    }

    #[test]
    fn constructors_agree_on_geometry() {
        let quad = Quad2d::from_top_left([10.0, 20.0], [30.0, 40.0], Color::WHITE);
        assert_eq!(quad.center, [25.0, 40.0]);
        assert_eq!(quad.size, [30.0, 40.0]);
        assert_eq!(quad.color(), Color::WHITE);

        let same = Quad2d::new([25.0, 40.0], [30.0, 40.0], Color::WHITE);
        assert_eq!(quad, same);
    }
}
