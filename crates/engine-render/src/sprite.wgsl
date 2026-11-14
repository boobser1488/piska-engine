// Спрайтовый шейдер: вершины уже в мировых координатах (батчинг на CPU),
// шейдер только проецирует их и умножает текстуру на цвет.

// Начало общего блока параметров кадра (полное описание — в mesh.wgsl).
struct Globals {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // centroid: при MSAA центр пикселя на краю квада может лежать вне него, и UV вышли бы
    // за кадр листа — центроидная интерполяция держит выборку внутри примитива.
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // Кадр листа (min uv, max uv) — одинаков у всех вершин квада.
    @location(2) @interpolate(flat) rect: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) rect: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.color = color;
    out.rect = rect;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Вдали выборка идёт из уменьшенной копии (мипа), где тексель крупнее: полтекселя
    // отступа у края кадра уже мало, и билинейный фильтр захватил бы соседний кадр листа
    // (линии между плитками). Держим UV внутри кадра на полтекселя выбранного мипа.
    let size = vec2<f32>(textureDimensions(sprite_texture));
    let texels = in.uv * size;
    let footprint = max(dot(dpdx(texels), dpdx(texels)), dot(dpdy(texels), dpdy(texels)));
    let top = f32(textureNumLevels(sprite_texture) - 1u);
    let lod = clamp(ceil(0.5 * log2(max(footprint, 1e-8)) + 0.25), 0.0, top);
    // Кадр уже сдвинут внутрь на полтекселя нулевого уровня (у мипмапов).
    let margin = select(0.0, 0.5 * (exp2(lod) - 1.0), lod > 0.0) / size;
    let middle = (in.rect.xy + in.rect.zw) * 0.5;
    let low = min(in.rect.xy + margin, middle);
    let high = max(in.rect.zw - margin, middle);
    let uv = clamp(in.uv, low, high);
    return textureSample(sprite_texture, sprite_sampler, uv) * in.color;
}
