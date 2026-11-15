// piska:variant lights default — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Вариант шейдеров мешей и рельефа без ламп сверх четырёх (renderer/variant.rs): те же
// имена, что в lights.wgsl, но код ламп отключён константой — компилятор его выбрасывает,
// кадр без `SpotLight` / `LightShadows` / `ManyLights` / `FillLight` стоит как раньше.

const EXTENDED_LIGHTS: bool = false;

struct LocalLightSample {
    direction: vec3<f32>,
    radiance: vec3<f32>,
};

fn light_cell(frag: vec4<f32>, world: vec3<f32>) -> vec4<u32> {
    return vec4<u32>(0xFFFFFFFFu);
}

fn cell_light(cell: vec4<u32>, k: u32) -> u32 {
    return 255u;
}

fn local_light(index: u32, world: vec3<f32>, n: vec3<f32>, receive: bool) -> LocalLightSample {
    return LocalLightSample(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0));
}

fn fill_ambient(n: vec3<f32>, base: vec3<f32>) -> vec3<f32> {
    return base;
}
