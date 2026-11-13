// Экранный слой (игровой интерфейс): треугольники в пикселях цели поверх готового кадра.
// Фигуры (скруглённые прямоугольники с рамкой и мягкие тени) считаются через
// расстояние до края (SDF), поэтому края гладкие при любом масштабе и без MSAA.

struct Screen {
    // Размер цели в пикселях.
    target_size: vec2<f32>,
    // Во сколько раз растянуть координаты списка до пикселей цели.
    scale: vec2<f32>,
    // 1 — цель в sRGB-формате (кодирует сама), 0 — кодируем в шейдере.
    srgb_target: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var overlay_texture: texture_2d<f32>;
@group(1) @binding(1) var overlay_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) shape: vec4<f32>,
    @location(4) local: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) shape: vec4<f32>,
    @location(5) local: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    let pixel = position * screen.scale;
    let ndc = pixel / screen.target_size * 2.0 - 1.0;
    out.clip_position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.border_color = border_color;
    // Фигура меряется в пикселях цели: растягиваем её вместе со списком.
    let s = min(screen.scale.x, screen.scale.y);
    out.shape = vec4<f32>(shape.xy * screen.scale, shape.z * s, shape.w * s);
    out.local = vec4<f32>(local.xy * screen.scale, local.z * s, local.w);
    return out;
}

// Расстояние до края скруглённого прямоугольника с полуразмерами `half` (< 0 — внутри).
fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0.0, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(overlay_texture, overlay_sampler, in.uv);
    let fill = texel * in.color;
    // Дальше всё с домноженной альфой: так рамка и заливка смешиваются без ореолов.
    var result = vec4<f32>(fill.rgb * fill.a, fill.a);

    if (in.shape.x > 0.0) {
        let d = rounded_box(in.local.xy, in.shape.xy, in.shape.z);
        let softness = in.local.z;
        var coverage: f32;
        if (softness > 0.0) {
            coverage = 1.0 - smoothstep(-softness, softness, d);
        } else {
            coverage = clamp(0.5 - d, 0.0, 1.0);
        }
        let border = in.shape.w;
        if (border > 0.0) {
            let inner = clamp(0.5 - (d + border), 0.0, 1.0);
            let edge = vec4<f32>(in.border_color.rgb * in.border_color.a, in.border_color.a);
            result = result * inner + edge * (1.0 - inner);
        }
        result = result * coverage;
    }

    if (screen.srgb_target < 0.5 && result.a > 0.0) {
        result = vec4<f32>(linear_to_srgb(result.rgb / result.a) * result.a, result.a);
    }
    return result;
}
