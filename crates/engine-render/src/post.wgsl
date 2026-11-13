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
};

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: PostParams;
@group(0) @binding(3) var bloom_texture: texture_2d<f32>;

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

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    let hdr = textureSample(source_texture, source_sampler, in.uv);
    let bloom = textureSample(bloom_texture, source_sampler, in.uv).rgb;
    var graded = aces((hdr.rgb + bloom * params.bloom_strength) * params.exposure);
    let from_center = distance(in.uv, vec2<f32>(0.5));
    graded *= 1.0 - params.vignette * smoothstep(0.35, 0.85, from_center);
    return vec4<f32>(mix(hdr.rgb, graded, params.enabled), 1.0);
}
