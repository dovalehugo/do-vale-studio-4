// BT.709 limited-range YUV (NV12) → RGB conversion on the GPU.
// Assumptions:
// - 8-bit limited range: Y in [16/255, 235/255], UV centered at 0.5
// - BT.709 primaries (HD/UHD)
// - No gamma correction (linear matrix on normalized values)

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

    // ITU-R BT.709 limited range (8-bit normalized)
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
    let y = textureSample(y_plane, tex_sampler, input.tex_coords).r;
    let uv = textureSample(uv_plane, tex_sampler, input.tex_coords).rg;
    let rgb = bt709_limited_yuv_to_rgb(vec3<f32>(y, uv.x, uv.y));
    return vec4<f32>(rgb, 1.0);
}
