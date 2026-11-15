// piska:variant water_scene default — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Вода с эффектами без кадра под водой (WebGL2: многовыборочную глубину читать нельзя):
// те же имена, что в water_scene.wgsl, код отключён константой. Остаются волны, рябь,
// блик и плоский стиль.

const WATER_SCENE: bool = false;

fn scene_world(uv: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(0.0, -1e6, 0.0);
}

fn scene_color(uv: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(0.0);
}

fn scene_reflection(origin: vec3<f32>, r: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}
