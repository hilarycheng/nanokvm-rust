// UVC MJPEG frame is uploaded to an sRGB GPU texture (hardware sRGB decode).
// Color corrections are applied in linear-light RGB. All RGB channels share
// the same luminance scaling for gamma/white/highlights -> minimizes hue shift.
struct ColorControls {
    // Exposure in stops; midtone gamma; highlight compression; white peak (sRGB)
    p0: vec4<f32>,
    // saturation; target_format_is_srgb; spare; spare
    p1: vec4<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> ctl: ColorControls;

struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VOut {
    // Fullscreen triangle. egui callback viewport restricts it to video rect.
    let p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    let xy = p[index];
    var out: VOut;
    out.position = vec4<f32>(xy, 0.0, 1.0);
    out.uv = vec2<f32>((xy.x + 1.0) * 0.5, (1.0 - xy.y) * 0.5);
    return out;
}

fn srgb_encode_channel(x: f32) -> f32 {
    let v = max(x, 0.0);
    if (v < 0.0031308) {
        return v * 12.92;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

@fragment
fn fs_main(input: VOut) -> @location(0) vec4<f32> {
    // Texture hardware decodes sRGB to linear. Shader controls are truly GPU-side.
    var rgb = textureSample(tex, samp, input.uv).rgb;
    rgb = rgb * exp2(ctl.p0.x);

    let luma = max(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0);
    let sat = clamp(ctl.p1.x, 0.0, 2.0);
    rgb = mix(vec3<f32>(luma), rgb, sat);

    // Luminance-only nonlinear curve. Does not separate R/G/B gains.
    let shaped = pow(max(luma, 0.000001), max(ctl.p0.y, 0.25));
    let compress = clamp(ctl.p0.z, 0.0, 1.0);
    let output_luma = shaped / (1.0 + compress * shaped);
    rgb = rgb * (output_luma / max(luma, 0.000001));

    // Dim video only, NOT the Ubuntu/Sway desktop or monitor hardware.
    let white = clamp(ctl.p0.w, 0.45, 1.0);
    let white_linear = select(white / 12.92, pow((white + 0.055) / 1.055, 2.4), white > 0.04045);
    rgb = clamp(rgb * white_linear, vec3<f32>(0.0), vec3<f32>(1.0));

    // For non-sRGB surface targets, encode for display ourselves.
    if (ctl.p1.y < 0.5) {
        rgb = vec3<f32>(
            srgb_encode_channel(rgb.r),
            srgb_encode_channel(rgb.g),
            srgb_encode_channel(rgb.b)
        );
    }
    return vec4<f32>(rgb, 1.0);
}
