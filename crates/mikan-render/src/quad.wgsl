// Mikan quad shader.
//
// Draws flat, screen-space colored rectangles. All coordinates are in physical
// pixels with the origin at the top-left corner of the window and y growing
// downward, matching how windowing systems report cursor positions.

struct Viewport {
    // Width and height of the drawable surface, in pixels.
    size: vec2<f32>,
    _padding: vec2<f32>,
};

struct Instance {
    center: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// Pixel space (origin top-left, y down) to clip space (x right, y up).
fn pixel_to_clip(pixel: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
    );
}

@vertex
fn vs_main(
    // Unit quad corner in [-0.5, 0.5].
    @location(0) position: vec2<f32>,
    // Per-instance transform and color.
    @location(1) center: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
) -> VertexOutput {
    let pixel = center + position * size;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(pixel_to_clip(pixel), 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
