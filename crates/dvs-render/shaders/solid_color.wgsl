// Fullscreen oversized triangle solid-color fill for destination rectangles.

struct ColorUniform {
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> color_uniform: ColorUniform;

const CLIP_VERTICES: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
);

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let clip = CLIP_VERTICES[vertex_index];
    return vec4<f32>(clip, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return color_uniform.color;
}
