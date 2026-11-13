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
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv) * in.color;
}
