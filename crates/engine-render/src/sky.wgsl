// Небо и проход теней. Оба читают только параметры кадра (без карты теней: во время
// прохода теней она сама является целью отрисовки). Перед файлом приклеивается common.wgsl.

// ---------------- проход теней: только глубина из точки зрения солнца ----------------

@vertex
fn vs_shadow(@location(0) position: vec3<f32>, instance: InstanceModel) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(instance.model_0, instance.model_1, instance.model_2, instance.model_3);
    return globals.light_view_proj * model * vec4<f32>(position, 1.0);
}

// ---------------- небо: один треугольник на весь экран ----------------

struct SkyOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOutput {
    var out: SkyOutput;
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    out.clip_position = vec4<f32>(corner, 1.0, 1.0);
    out.ndc = corner;
    return out;
}

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

@fragment
fn fs_sky(in: SkyOutput) -> @location(0) vec4<f32> {
    // Луч взгляда через пиксель: разница точек на дальней и ближней плоскостях.
    let far = globals.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let near = globals.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let direction = normalize(far.xyz / far.w - near.xyz / near.w);

    var color = sky_color(direction);

    // Диск солнца — ярче единицы, блум превращает его в сияние.
    let to_sun = -normalize(globals.sun_direction.xyz);
    let toward_sun = dot(direction, to_sun);
    color += globals.sun_color.rgb * globals.sun_color.a * smoothstep(0.9994, 0.9997, toward_sun) * 8.0;

    // Звёзды, когда солнце под горизонтом.
    let night = 1.0 - smoothstep(-0.25, 0.0, to_sun.y);
    if (night > 0.0 && direction.y > 0.0) {
        let star = hash3(floor(direction * 450.0));
        color += vec3<f32>(smoothstep(0.997, 1.0, star) * night * 3.0);
    }
    return vec4<f32>(color, 1.0);
}
