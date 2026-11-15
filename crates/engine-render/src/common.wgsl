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
    counts: vec4<f32>,         // x — число точечных источников; yzw — туман: начало (м), спад с высотой (1/м), высота
    points: array<PointLight, 4>,
    // Каскады теней (shadow.wgsl, `SoftShadows`): матрицы и данные слоёв.
    cascade_view_proj: array<mat4x4<f32>, 4>,
    cascade_data: array<vec4<f32>, 4>, // x — дальняя граница (глубина вида), y — сдвиг по нормали, z — смещение глубины, w — глубина карты (м)
    cascade_params: vec4<f32>,         // x — число каскадов (0 — одна карта, как раньше), y — доля перехода, z — фильтр, w — мягкость (тексели)
    // Небо (`Sky`, atmosphere.rs); sky_sun.x = 0 — небо как раньше.
    sky_sun: vec4<f32>,                // x — включено, y — cos радиуса диска, z — ореол, w — рассеяние
    sky_clouds: vec4<f32>,             // x — покрытие, yz — сдвиг ветром, w — масштаб
    sky_cloud_color: vec4<f32>,        // rgb — цвет облаков, w — звёзды
    sky_night: vec4<f32>,              // x — луна, y — время (с), z — дешёвый рельеф (пресет Low)
    // Лампы (lights.wgsl, вариант «с лампами»; без него не читаются).
    cluster: vec4<f32>,                // xy — размер сцены (пиксели), z — ближняя глубина слоёв, w — масштаб логарифма слоя
    lights_info: vec4<f32>,            // x — ламп в кадре, y — тексель атласа теней ламп (uv)
    fill_sky: vec4<f32>,               // rgb — верх заполняющего света, w — включён
    fill_ground: vec4<f32>,            // rgb — низ (отражённый от земли свет)
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

// Рассеяние (компонент `Sky`): светлая дымка у горизонта днём, тёплая полоса заката на
// стороне солнца и мягкий ореол Ми вокруг него. Только для фона неба (sky.wgsl): туман и
// ambient его не получают — передний план сохраняет контраст и насыщенность.
fn sky_scattering(dir: vec3<f32>, up: f32) -> vec3<f32> {
    let to_sun = -normalize(globals.sun_direction.xyz);
    let strength = globals.sky_sun.w;
    let elevation = to_sun.y;
    let day = smoothstep(0.0, 0.25, elevation);
    let low_sun = (1.0 - smoothstep(0.0, 0.35, abs(elevation))) * smoothstep(-0.08, 0.02, elevation);
    let horizon = pow(1.0 - up, 6.0);
    let flat_dir = normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(1e-4, 0.0, 0.0));
    let flat_sun = normalize(vec3<f32>(to_sun.x, 0.0, to_sun.z) + vec3<f32>(1e-4, 0.0, 0.0));
    let side = max(dot(flat_dir, flat_sun), 0.0);
    let toward = max(dot(dir, to_sun), 0.0);
    // Сила рассеяния следует за яркостью солнца: слабый «лунный» направленный свет
    // ночной сцены не делает небо дневным.
    let sun_luma = dot(globals.sun_color.rgb * globals.sun_color.a, vec3<f32>(0.2126, 0.7152, 0.0722));
    let lit = clamp(sun_luma / 2.0, 0.0, 1.0);
    // Дневная дымка: горизонт светлее и чуть голубее.
    var add = vec3<f32>(0.45, 0.55, 0.65) * horizon * day * 0.2 * lit;
    // Закат: оранжево-розовая полоса у горизонта на стороне солнца.
    add += vec3<f32>(1.0, 0.42, 0.16) * horizon * pow(side, 2.0) * low_sun * 1.1 * lit;
    add += vec3<f32>(0.6, 0.25, 0.35) * pow(1.0 - up, 3.0) * low_sun * 0.25 * lit;
    // Ореол Ми: ярче у низкого солнца.
    let sun = globals.sun_color.rgb * globals.sun_color.a;
    add += sun * pow(toward, 24.0) * (0.04 + 0.08 * low_sun) * globals.sky_sun.z;
    return add * strength;
}

// Туман растворяет даль в цвет неба у горизонта (край земли не виден). До `counts.y`
// метров его нет; плотность спадает с высотой над `counts.w` как e^(−counts.z · Δy) —
// интеграл вдоль луча считается точно (экспоненциальный высотный туман).
fn apply_fog(color: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let density = globals.sky_horizon.w;
    if (density <= 0.0) {
        return color;
    }
    let offset = world_position - globals.camera_position.xyz;
    let distance = length(offset);
    let direction = offset / max(distance, 1e-4);
    let travelled = max(distance - globals.counts.y, 0.0);
    // Средняя плотность вдоль пройденного в тумане отрезка.
    let falloff = globals.counts.z;
    let start_y = globals.camera_position.y + direction.y * min(globals.counts.y, distance) - globals.counts.w;
    let k = falloff * direction.y * travelled;
    var height_term = exp(clamp(-falloff * start_y, -20.0, 4.0));
    if (abs(k) > 1e-4) {
        height_term *= (1.0 - exp(clamp(-k, -20.0, 20.0))) / k;
    }
    let amount = 1.0 - exp(-density * travelled * height_term);
    let fog_color = sky_color(normalize(vec3<f32>(direction.x, 0.02, direction.z)));
    return mix(color, fog_color, clamp(amount, 0.0, 1.0));
}

// Половинный вектор без NaN: при v = −l (камера смотрит вдоль лучей солнца) `normalize`
// нуля дал бы NaN, а он в HDR-кадре — чёрная или белая точка.
fn safe_half(v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let s = v + l;
    return s * inverseSqrt(max(dot(s, s), 1e-12));
}

// Нормаль для освещения не смотрит от камеры: у краёв холмов и на склонах против света
// интерполированная нормаль с картой деталей бывает повёрнута от глаза, и множители
// Френеля и геометрии при n·v → 0 превращают блик в одиночный белый пиксель.
fn facing_normal(n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let d = dot(n, v);
    return normalize(n + v * max(0.05 - d, 0.0));
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
