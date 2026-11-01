// Perch compositor: projective quad blit.
//
// Each window is drawn as two triangles. The vertex shader receives the
// corner position already multiplied by its homogeneous weight w (from the
// homography's third row), so the GPU's perspective-correct interpolation of
// `uv` yields an exact projective texture mapping — the window looks like a
// flat rectangle rotated in 3D, not a sheared parallelogram.

struct VsIn {
    @location(0) pos_w: vec3<f32>,   // (x_ndc * w, y_ndc * w, w)
    @location(1) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Params {
    opacity: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var t_win: texture_2d<f32>;
@group(0) @binding(1) var s_win: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(in.pos_w.x, in.pos_w.y, 0.0, in.pos_w.z);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_win, s_win, in.uv);
    return vec4<f32>(c.rgb, c.a * params.opacity);
}
