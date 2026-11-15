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
    // Обратная глубина: z = 0 — дальняя плоскость (небо позади всего).
    out.clip_position = vec4<f32>(corner, 0.0, 1.0);
    out.ndc = corner;
    return out;
}

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

// Шум значений и его сумма октав — для облаков.
fn hash2(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 5; i += 1) {
        sum += value_noise(q) * amplitude;
        q = q * 2.03 + vec2<f32>(17.1, 9.2);
        amplitude *= 0.5;
    }
    return sum;
}

// Небо компонента `Sky`: диск солнца с ореолом, облака, луна, звёзды.
fn atmosphere(direction: vec3<f32>) -> vec3<f32> {
    var color = sky_color(direction) + sky_scattering(direction, clamp(direction.y, 0.0, 1.0)) * smoothstep(-0.05, 0.02, direction.y);
    let to_sun = -normalize(globals.sun_direction.xyz);
    let sun = globals.sun_color.rgb * globals.sun_color.a;
    let toward_sun = dot(direction, to_sun);
    let night = 1.0 - smoothstep(-0.2, 0.02, to_sun.y);

    // Диск солнца (ярче единицы — блум даёт сияние) и узкий ореол.
    let cos_size = globals.sky_sun.y;
    let disc = smoothstep(cos_size - 0.00025, cos_size + 0.00005, toward_sun);
    let toward = max(toward_sun, 0.0);
    color += sun * (disc * 12.0 + (pow(toward, 600.0) * 0.6 + pow(toward, 90.0) * 0.08) * globals.sky_sun.z);

    // Луна напротив солнца, когда оно под горизонтом.
    var moon_light = 0.0;
    if (globals.sky_night.x > 0.5 && night > 0.0) {
        let to_moon = -to_sun;
        let toward_moon = dot(direction, to_moon);
        let moon_disc = smoothstep(0.99985, 0.99992, toward_moon);
        let moon_glow = pow(max(toward_moon, 0.0), 300.0) * 0.15;
        moon_light = (moon_disc * 1.6 + moon_glow) * night;
        color += vec3<f32>(0.85, 0.9, 1.0) * moon_light;
    }

    // Облака: два слоя шума на плоскости над камерой, плывут по ветру.
    var cloud_cover = 0.0;
    let coverage = globals.sky_clouds.x;
    if (coverage > 0.0 && direction.y > 0.0) {
        let scale = max(globals.sky_clouds.w, 0.05);
        let plane = direction.xz / (direction.y + 0.12) * (1.6 / scale);
        let wind = globals.sky_clouds.yz / (900.0 * scale);
        let n = fbm(plane + wind) * 0.75 + fbm(plane * 2.3 - wind * 1.7 + vec2<f32>(5.2, 1.3)) * 0.25;
        let threshold = 1.0 - coverage;
        cloud_cover = smoothstep(threshold - 0.08, threshold + 0.22, n) * smoothstep(0.0, 0.18, direction.y);
        // Свет облака: солнце + небо; край к солнцу светится (серебряная кайма).
        let lit = globals.sky_cloud_color.rgb * (sun * 0.32 + globals.sky_zenith.rgb * 0.7 + globals.sky_horizon.rgb * 0.4);
        let shade = mix(1.0, 0.62, smoothstep(threshold + 0.1, threshold + 0.5, n));
        let rim = pow(toward, 8.0) * (1.0 - smoothstep(threshold, threshold + 0.35, n)) * 0.8;
        let night_cloud = vec3<f32>(0.03, 0.035, 0.05) + vec3<f32>(0.1) * moon_light;
        let cloud = mix(lit * shade + sun * rim * 0.25, night_cloud, night * 0.85);
        color = mix(color, cloud, cloud_cover);
    }

    // Звёзды ночью: за облаками не видны, чуть мерцают.
    let stars = globals.sky_cloud_color.w;
    if (stars > 0.0 && night > 0.0 && direction.y > 0.0) {
        let cell = floor(direction * 420.0);
        let star = hash3(cell);
        let twinkle = 0.75 + 0.25 * sin(globals.sky_night.y * 3.0 + star * 40.0);
        color += vec3<f32>(smoothstep(0.996, 1.0, star) * night * stars * 2.5 * twinkle * (1.0 - cloud_cover));
    }
    return color;
}

@fragment
fn fs_sky(in: SkyOutput) -> @location(0) vec4<f32> {
    // Луч взгляда через пиксель: разница точек на дальней и ближней плоскостях.
    // Обратная глубина: дальняя плоскость — z = 0, ближняя — z = 1.
    let far = globals.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let near = globals.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let direction = normalize(far.xyz / far.w - near.xyz / near.w);

    if (globals.sky_sun.x > 0.5) {
        return vec4<f32>(atmosphere(direction), 1.0);
    }
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
