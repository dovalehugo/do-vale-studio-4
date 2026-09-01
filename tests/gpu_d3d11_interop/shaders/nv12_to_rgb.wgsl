// BT.709 limited-range NV12 → RGB for Experiment 2 (real decoder texture).
// Crops decoder allocation height 2176 to visible video height 2160.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

const VERTICES: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
);

const TEX_COORDS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(0.0, 1.0),
    vec2<f32>(2.0, 1.0),
    vec2<f32>(0.0, -1.0),
);

const VISIBLE_V_SCALE: f32 = 2160.0 / 2176.0;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(VERTICES[vertex_index], 0.0, 1.0);
    output.tex_coords = TEX_COORDS[vertex_index];
    return output;
}

@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var uv_plane: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

fn bt709_limited_yuv_to_rgb(yuv: vec3<f32>) -> vec3<f32> {
    let y = yuv.x;
    let u = yuv.y;
    let v = yuv.z;

    let y_adj = y - (16.0 / 255.0);
    let u_adj = u - 0.5;
    let v_adj = v - 0.5;

    let r = 1.164383 * y_adj + 1.792741 * v_adj;
    let g = 1.164383 * y_adj - 0.213249 * u_adj - 0.532909 * v_adj;
    let b = 1.164383 * y_adj + 2.112402 * u_adj;

    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let tc = vec2<f32>(input.tex_coords.x, input.tex_coords.y * VISIBLE_V_SCALE);
    let y = textureSample(y_plane, tex_sampler, tc).r;
    let uv = textureSample(uv_plane, tex_sampler, tc).rg;
    let rgb = bt709_limited_yuv_to_rgb(vec3<f32>(y, uv.x, uv.y));
    return vec4<f32>(rgb, 1.0);
}
