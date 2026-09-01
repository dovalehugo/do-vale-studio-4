// Production NV12 → nonlinear video RGB (SDR) for Integration 5.
// Fullscreen oversized triangle with clip-derived base UVs; aspect-fit uses
// set_viewport/set_scissor on the CPU path.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

struct RenderUniforms {
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
    y_scale: f32,
    y_offset: f32,
    uv_scale: f32,
    uv_offset: f32,
    mat_r: vec4<f32>,
    mat_g: vec4<f32>,
    mat_b: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: RenderUniforms;
@group(0) @binding(1) var y_plane: texture_2d<f32>;
@group(0) @binding(2) var uv_plane: texture_2d<f32>;
@group(0) @binding(3) var tex_sampler: sampler;

const CLIP_VERTICES: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
);

const CLIP_TO_BASE_UV: vec2<f32> = vec2<f32>(0.5, -0.5);
const BASE_UV_OFFSET: vec2<f32> = vec2<f32>(0.5, 0.5);

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let clip = CLIP_VERTICES[vertex_index];
    output.position = vec4<f32>(clip, 0.0, 1.0);
    output.tex_coords = clip * CLIP_TO_BASE_UV + BASE_UV_OFFSET;
    return output;
}

fn yuv_to_rgb(yuv: vec3<f32>) -> vec3<f32> {
    let y = yuv.x * uniforms.y_scale + uniforms.y_offset;
    let u = yuv.y * uniforms.uv_scale + uniforms.uv_offset;
    let v = yuv.z * uniforms.uv_scale + uniforms.uv_offset;

    let r = dot(vec4<f32>(y, u, v, 1.0), uniforms.mat_r);
    let g = dot(vec4<f32>(y, u, v, 1.0), uniforms.mat_g);
    let b = dot(vec4<f32>(y, u, v, 1.0), uniforms.mat_b);

    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let tc = mix(uniforms.uv_min, uniforms.uv_max, input.tex_coords);
    let y = textureSample(y_plane, tex_sampler, tc).r;
    let uv = textureSample(uv_plane, tex_sampler, tc).rg;
    let rgb = yuv_to_rgb(vec3<f32>(y, uv.x, uv.y));
    return vec4<f32>(rgb, 1.0);
}
