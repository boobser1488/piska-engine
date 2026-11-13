// Общее для шейдеров мешей и неба: параметры кадра, цвет неба, туман, ambient.
// Приклеивается к началу mesh.wgsl и sky.wgsl при сборке шейдеров.

struct PointLight {
    position_range: vec4<f32>, // xyz — позиция, w — дальность
    color: vec4<f32>,          // rgb — цвет × яркость
};

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,  // куда летит свет
    sun_color: vec4<f32>,      // rgb — цвет × яркость, a — есть ли солнце
    sky_zenith: vec4<f32>,     // rgb, w — сила ambient
    sky_horizon: vec4<f32>,    // rgb, w — плотность тумана
    shadow_params: vec4<f32>,  // x — тени включены, y — тексель (uv), z — сдвиг по нормали (м), w — bias глубины
    counts: vec4<f32>,         // x — число точечных источников
    points: array<PointLight, 4>,
};

// Параметры экземпляра меша приходят вторым вершинным буфером (шаг — экземпляр):
// одинаковые меши рисуются одним draw call'ом (инстансинг), а данные всех объектов
// уходят на GPU одним сплошным куском. Работает и на WebGL2 (без storage-буферов).
struct Instance {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) normal_0: vec4<f32>,  // столбцы матрицы нормалей (обратная транспонированная 3×3)
    @location(8) normal_1: vec4<f32>,
    @location(9) normal_2: vec4<f32>,
    @location(10) base_color: vec4<f32>,
    @location(11) emissive: vec4<f32>,
    @location(12) params: vec4<f32>,   // x — metallic, y — roughness, z — шаг тайлинга (м), w — dev-сетка
};

// Только матрица модели — проходу теней остальное не нужно.
struct InstanceModel {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

// Цвет неба в направлении `dir`: градиент горизонт → зенит, ореол солнца, дымка ниже горизонта.
fn sky_color(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.y, 0.0, 1.0);
    var color = mix(globals.sky_horizon.rgb, globals.sky_zenith.rgb, pow(up, 0.5));
    let toward_sun = max(dot(dir, -normalize(globals.sun_direction.xyz)), 0.0);
    let sun = globals.sun_color.rgb * globals.sun_color.a;
    color += sun * (pow(toward_sun, 8.0) * 0.05 + pow(toward_sun, 64.0) * 0.14);
    let below = clamp(-dir.y * 4.0, 0.0, 1.0);
    return mix(color, globals.sky_horizon.rgb * 0.5, below);
}

// Туман растворяет даль ровно в цвет неба у горизонта, поэтому край земли не виден.
fn apply_fog(color: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let offset = world_position - globals.camera_position.xyz;
    let distance = length(offset);
    let direction = offset / max(distance, 1e-4);
    let amount = 1.0 - exp(-distance * globals.sky_horizon.w);
    let fog_color = sky_color(normalize(vec3<f32>(direction.x, 0.02, direction.z)));
    return mix(color, fog_color, clamp(amount, 0.0, 1.0));
}

// Полусферный ambient: приглушённое небо сверху, тёплый отсвет земли снизу —
// серое в тени остаётся серым, а не синеет.
fn ambient_light(n: vec3<f32>) -> vec3<f32> {
    let sky = mix(globals.sky_horizon.rgb, globals.sky_zenith.rgb, 0.5);
    let luminance = dot(sky, vec3<f32>(0.2126, 0.7152, 0.0722));
    let from_sky = mix(vec3<f32>(luminance), sky, 0.45) * 1.3;
    let from_ground = vec3<f32>(0.42, 0.38, 0.30) * luminance;
    return mix(from_ground, from_sky, n.y * 0.5 + 0.5) * globals.sky_zenith.w;
}
