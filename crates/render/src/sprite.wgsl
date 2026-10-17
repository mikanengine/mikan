// Mikan sprite shader.
//
// Draws world-space rectangles as instances of a unit quad, rotated around the
// instance center, transformed by the frame's view/projection matrix and (when
// textured) sampled through a UV rectangle. World space has the origin at the
// top-left corner of the viewport and y growing downward unless a camera says
// otherwise, so a positive rotation in the instance data turns clockwise.

struct Camera {
    // Column-major world-to-clip transform built from the frame's 2D camera.
    view_projection: mat4x4<f32>,
};

// Per-instance data, matching `SpriteInstance` in sprite.rs byte for byte.
struct Instance {
    // Center and size in world units.
    center: vec2<f32>,
    size: vec2<f32>,
    // Linear RGBA tint, multiplied with the sampled texel.
    color: vec4<f32>,
    // Region to sample as (min_u, min_v, max_u, max_v).
    uv: vec4<f32>,
    // cos(rotation), sin(rotation).
    rotation: vec2<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(
    // Unit quad corner in [-0.5, 0.5].
    @location(0) corner: vec2<f32>,
    // Per-instance transform, tint, texture region, and rotation.
    @location(1) center: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv_rect: vec4<f32>,
    @location(5) rotation: vec2<f32>,
) -> VertexOutput {
    // Standard 2D rotation by (cos, sin); positive is clockwise in y-down space.
    let rotated = vec2<f32>(
        corner.x * rotation.x - corner.y * rotation.y,
        corner.x * rotation.y + corner.y * rotation.x,
    );
    let world = center + rotated * size;
    // The corner's position inside the sprite, in [0, 1].
    let normalized = corner + vec2<f32>(0.5, 0.5);
    var out: VertexOutput;
    out.clip_position = camera.view_projection * vec4<f32>(world, 0.0, 1.0);
    out.color = color;
    out.uv = vec2<f32>(
        mix(uv_rect.x, uv_rect.z, normalized.x),
        mix(uv_rect.y, uv_rect.w, normalized.y),
    );
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv) * in.color;
}
