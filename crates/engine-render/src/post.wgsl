// Пост-обработка HDR-кадра: блум (цепочка уменьшений и увеличений) и сведение с
// тонмаппингом ACES, экспозицией и виньеткой.

struct PostParams {
    exposure: f32,
    bloom_strength: f32,
    bloom_threshold: f32,
    vignette: f32,
    // 0 — кадр выводится как есть (2D без пост-обработки), 1 — тонмаппинг и блум.
    enabled: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
    // Цветокоррекция (grading.rs). balance.w = 0 — выключена.
    balance: vec4<f32>,  // rgb — множитель баланса белого
    tone: vec4<f32>,     // x — контраст, y — насыщенность
    lift: vec4<f32>,     // rgb — цвет теней
    gain: vec4<f32>,     // rgb — множитель светов
    fx: vec4<f32>,       // x — AO, y — сила контура, z — блум 13 выборок, w — FXAA
    outline_color: vec4<f32>,
};

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: PostParams;
@group(0) @binding(3) var bloom_texture: texture_2d<f32>;
// Эффекты по глубине (post_fx.wgsl): r — доля рассеянного света, g — контур.
@group(0) @binding(4) var fx_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

fn texel_size() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(source_texture));
}

// Четыре билинейные выборки = фильтр 4×4.
fn box4(uv: vec2<f32>) -> vec3<f32> {
    let t = texel_size();
    var color = textureSample(source_texture, source_sampler, uv + vec2<f32>(-t.x, -t.y)).rgb;
    color += textureSample(source_texture, source_sampler, uv + vec2<f32>(t.x, -t.y)).rgb;
    color += textureSample(source_texture, source_sampler, uv + vec2<f32>(-t.x, t.y)).rgb;
    color += textureSample(source_texture, source_sampler, uv + vec2<f32>(t.x, t.y)).rgb;
    return color * 0.25;
}

// Оставить только то, что ярче порога.
@fragment
fn fs_prefilter(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = min(box4(in.uv), vec3<f32>(64.0)); // гасим «светлячков» от бликов
    let brightness = max(max(color.r, color.g), color.b);
    let keep = max(brightness - params.bloom_threshold, 0.0) / max(brightness, 1e-4);
    return vec4<f32>(color * keep, 1.0);
}

@fragment
fn fs_down(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(box4(in.uv), 1.0);
}

// Уменьшение из 13 выборок (Jimenez, CoD: AW): пять перекрывающихся квадратов 4×4 с
// весами — меньше «квадратных» артефактов и мерцания, чем у одного квадрата.
fn down13(uv: vec2<f32>, karis: bool) -> vec3<f32> {
    let t = texel_size();
    let a = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(-2.0, -2.0)).rgb;
    let b = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(0.0, -2.0)).rgb;
    let c = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(2.0, -2.0)).rgb;
    let d = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(-2.0, 0.0)).rgb;
    let e = textureSample(source_texture, source_sampler, uv).rgb;
    let f = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(2.0, 0.0)).rgb;
    let g = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(-2.0, 2.0)).rgb;
    let h = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(0.0, 2.0)).rgb;
    let i = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(2.0, 2.0)).rgb;
    let j = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(-1.0, -1.0)).rgb;
    let k = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(1.0, -1.0)).rgb;
    let l = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(-1.0, 1.0)).rgb;
    let m = textureSample(source_texture, source_sampler, uv + t * vec2<f32>(1.0, 1.0)).rgb;
    let center = (j + k + l + m) * 0.25;
    let q1 = (a + b + d + e) * 0.25;
    let q2 = (b + c + e + f) * 0.25;
    let q3 = (d + e + g + h) * 0.25;
    let q4 = (e + f + h + i) * 0.25;
    if (karis) {
        // Среднее Кариса: вес группы 1 / (1 + яркость) — одиночные яркие пиксели не мерцают.
        let wc = karis_weight(center) * 0.5;
        let w1 = karis_weight(q1) * 0.125;
        let w2 = karis_weight(q2) * 0.125;
        let w3 = karis_weight(q3) * 0.125;
        let w4 = karis_weight(q4) * 0.125;
        return (center * wc + q1 * w1 + q2 * w2 + q3 * w3 + q4 * w4) / max(wc + w1 + w2 + w3 + w4, 1e-4);
    }
    return center * 0.5 + (q1 + q2 + q3 + q4) * 0.125;
}

fn karis_weight(c: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + max(max(c.r, c.g), c.b));
}

@fragment
fn fs_down13(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(down13(in.uv, false), 1.0);
}

@fragment
fn fs_prefilter13(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = min(down13(in.uv, true), vec3<f32>(64.0));
    let brightness = max(max(color.r, color.g), color.b);
    let keep = max(brightness - params.bloom_threshold, 0.0) / max(brightness, 1e-4);
    return vec4<f32>(color * keep, 1.0);
}

// «Шатёр» из девяти выборок; складывается со следующим, более крупным уровнем.
@fragment
fn fs_up(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = texel_size();
    var color = textureSample(source_texture, source_sampler, in.uv).rgb * 4.0;
    color += textureSample(source_texture, source_sampler, in.uv + vec2<f32>(t.x, 0.0)).rgb * 2.0;
    color += textureSample(source_texture, source_sampler, in.uv - vec2<f32>(t.x, 0.0)).rgb * 2.0;
    color += textureSample(source_texture, source_sampler, in.uv + vec2<f32>(0.0, t.y)).rgb * 2.0;
    color += textureSample(source_texture, source_sampler, in.uv - vec2<f32>(0.0, t.y)).rgb * 2.0;
    color += textureSample(source_texture, source_sampler, in.uv + t).rgb;
    color += textureSample(source_texture, source_sampler, in.uv - t).rgb;
    color += textureSample(source_texture, source_sampler, in.uv + vec2<f32>(t.x, -t.y)).rgb;
    color += textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-t.x, t.y)).rgb;
    return vec4<f32>(color / 16.0, 1.0);
}

fn aces(color: vec3<f32>) -> vec3<f32> {
    let numerator = color * (2.51 * color + 0.03);
    let denominator = color * (2.43 * color + 0.59) + 0.14;
    return clamp(numerator / denominator, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Контраст, насыщенность и тонирование — в гамма-пространстве (≈ как видит глаз),
// результат возвращается в линейное.
fn color_grade(linear: vec3<f32>) -> vec3<f32> {
    var c = pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    c = (c - 0.5) * params.tone.x + 0.5;
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, params.tone.y);
    let shade = clamp(1.0 - luma, 0.0, 1.0);
    c += params.lift.rgb * shade * shade * 0.25;
    c *= mix(vec3<f32>(1.0), params.gain.rgb, clamp(luma, 0.0, 1.0));
    return pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    var hdr = textureSample(source_texture, source_sampler, in.uv);
    let bloom = textureSample(bloom_texture, source_sampler, in.uv).rgb;
    let fx = textureSample(fx_texture, source_sampler, in.uv);
    // Затенение (AO) гасит свет сцены до тонмаппинга.
    hdr = vec4<f32>(hdr.rgb * mix(1.0, fx.r, params.fx.x), hdr.a);
    let grading = params.balance.w > 0.5;
    let balance = select(vec3<f32>(1.0), params.balance.rgb, grading);
    var graded = aces((hdr.rgb + bloom * params.bloom_strength) * params.exposure * balance);
    graded = select(graded, color_grade(graded), grading);
    let from_center = distance(in.uv, vec2<f32>(0.5));
    graded *= 1.0 - params.vignette * smoothstep(0.35, 0.85, from_center);
    var out = mix(hdr.rgb, graded, params.enabled);
    // Контур — поверх: затемнённый цвет под линией или свой цвет.
    let line = select(params.outline_color.rgb, out * params.outline_color.rgb, params.outline_color.a > 0.5);
    out = mix(out, line, clamp(fx.g * params.fx.y, 0.0, 1.0));
    return vec4<f32>(out, 1.0);
}

// FXAA (упрощённый, по яркости): сглаживает ступеньки там, где MSAA не помогает — края
// внутри текстур и вырезов, тонкие линии контура.
fn luma(c: vec3<f32>) -> f32 {
    return sqrt(dot(c, vec3<f32>(0.299, 0.587, 0.114)));
}

@fragment
fn fs_fxaa(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = texel_size();
    let center = textureSample(source_texture, source_sampler, in.uv).rgb;
    let nw = luma(textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-t.x, -t.y)).rgb);
    let ne = luma(textureSample(source_texture, source_sampler, in.uv + vec2<f32>(t.x, -t.y)).rgb);
    let sw = luma(textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-t.x, t.y)).rgb);
    let se = luma(textureSample(source_texture, source_sampler, in.uv + vec2<f32>(t.x, t.y)).rgb);
    let m = luma(center);
    let lo = min(m, min(min(nw, ne), min(sw, se)));
    let hi = max(m, max(max(nw, ne), max(sw, se)));
    if (hi - lo < max(0.0312, hi * 0.125)) {
        return vec4<f32>(center, 1.0);
    }
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.03125, 1.0 / 128.0);
    let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * scale, vec2<f32>(-8.0), vec2<f32>(8.0)) * t;
    let a = 0.5 * (textureSample(source_texture, source_sampler, in.uv + dir * (1.0 / 3.0 - 0.5)).rgb
        + textureSample(source_texture, source_sampler, in.uv + dir * (2.0 / 3.0 - 0.5)).rgb);
    let b = a * 0.5 + 0.25 * (textureSample(source_texture, source_sampler, in.uv - dir * 0.5).rgb
        + textureSample(source_texture, source_sampler, in.uv + dir * 0.5).rgb);
    let lb = luma(b);
    return vec4<f32>(select(b, a, lb < lo || lb > hi), 1.0);
}
