// piska:variant shadow — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Тени солнца для мешей и рельефа. Приклеивается после common.wgsl.
//
// Два режима (renderer.rs, `SoftShadows`):
// * одна карта (как раньше): слой 0, матрица `light_view_proj`, PCF 3×3, к краю
//   области теней тень плавно гаснет;
// * каскады (`cascade_params.x` > 0): 1–4 слоя, у каждого своя матрица и своя доля
//   дальности, переход между каскадами смешивается; фильтр — PCF 3×3, 16 выборок по
//   диску Пуассона или «контактное упрочнение» (полутень растёт с расстоянием до
//   заслонителя — оценивается сравнениями с отступами по глубине, без чтения глубины
//   напрямую: так шейдер собирается и для WebGL2).

@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

// Диск Пуассона, 16 точек в единичном круге.
const POISSON: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.94201624, -0.39906216), vec2<f32>(0.94558609, -0.76890725),
    vec2<f32>(-0.09418410, -0.92938870), vec2<f32>(0.34495938, 0.29387760),
    vec2<f32>(-0.91588581, 0.45771432), vec2<f32>(-0.81544232, -0.87912464),
    vec2<f32>(-0.38277543, 0.27676845), vec2<f32>(0.97484398, 0.75648379),
    vec2<f32>(0.44323325, -0.97511554), vec2<f32>(0.53742981, -0.47373420),
    vec2<f32>(-0.26496911, -0.41893023), vec2<f32>(0.79197514, 0.19090188),
    vec2<f32>(-0.24188840, 0.99706507), vec2<f32>(-0.81409955, 0.91437590),
    vec2<f32>(0.19984126, 0.78641367), vec2<f32>(0.14383161, -0.14100790),
);

fn shadow_tap(uv: vec2<f32>, layer: i32, depth: f32) -> f32 {
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, layer, depth);
}

// Одна карта: как было до каскадов (тот же результат до бита).
fn single_map_visibility(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    let clip = globals.light_view_proj * vec4<f32>(world_position + n * globals.shadow_params.z, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let depth = clip.z - globals.shadow_params.w;
    let texel = globals.shadow_params.y;
    var sum = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            sum += shadow_tap(uv + vec2<f32>(f32(x), f32(y)) * texel, 0, depth);
        }
    }
    let edge = max(max(abs(clip.x), abs(clip.y)), clip.z);
    return mix(sum / 9.0, 1.0, smoothstep(0.85, 1.0, edge));
}

// Видимость солнца в каскаде `c`. Данные каскада: `cascade_data[c]` — x: дальняя граница
// (глубина вида, м), y: сдвиг по нормали (м), z: смещение глубины, w: глубина карты в
// метрах (для оценки расстояния до заслонителя).
fn cascade_visibility(world_position: vec3<f32>, n: vec3<f32>, c: i32) -> f32 {
    let data = globals.cascade_data[c];
    let clip = globals.cascade_view_proj[c] * vec4<f32>(world_position + n * data.y, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let depth = clip.z - data.z;
    let texel = globals.shadow_params.y;
    let filter_mode = globals.cascade_params.z;
    if (filter_mode < 0.5) {
        var sum = 0.0;
        for (var y = -1; y <= 1; y += 1) {
            for (var x = -1; x <= 1; x += 1) {
                sum += shadow_tap(uv + vec2<f32>(f32(x), f32(y)) * texel, c, depth);
            }
        }
        return sum / 9.0;
    }
    var poisson = POISSON;
    var radius = globals.cascade_params.w * texel;
    if (filter_mode > 1.5) {
        // Контактное упрочнение: насколько далеко перед точкой заслонитель. Сравнения
        // с глубиной, отодвинутой к солнцу на 0.25 / 0.75 / 2 м: если тень всё ещё есть,
        // заслонитель дальше — полутень шире.
        let metre = 1.0 / max(data.w, 1e-3);
        var far_blocker = 0.0;
        var offsets = array<f32, 3>(0.25, 0.75, 2.0);
        for (var k = 0; k < 3; k += 1) {
            let probe = depth - offsets[k] * metre;
            var lit = 0.0;
            for (var j = 0; j < 4; j += 1) {
                lit += shadow_tap(uv + poisson[j * 4] * radius * 2.0, c, probe);
            }
            far_blocker += (1.0 - lit * 0.25) / 3.0;
        }
        radius *= mix(0.35, 2.5, far_blocker);
    }
    var sum = 0.0;
    for (var i = 0; i < 16; i += 1) {
        sum += shadow_tap(uv + poisson[i] * radius, c, depth);
    }
    return sum / 16.0;
}

// Доля солнечного света, дошедшая до точки: 1 — на свету, 0 — в тени.
fn sun_shadow(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    if (globals.shadow_params.x < 0.5) {
        return 1.0;
    }
    let count = i32(globals.cascade_params.x);
    if (count <= 0) {
        return single_map_visibility(world_position, n);
    }
    // Глубина вида (w после проекции камеры) выбирает каскад.
    let view_depth = (globals.view_proj * vec4<f32>(world_position, 1.0)).w;
    var c = 0;
    for (var k = 0; k < 3; k += 1) {
        if (k + 1 < count && view_depth > globals.cascade_data[k].x) {
            c = k + 1;
        }
    }
    let far = globals.cascade_data[c].x;
    var visibility = cascade_visibility(world_position, n, c);
    // Переход к следующему каскаду: последняя доля `cascade_params.y` каскада.
    let band = far * globals.cascade_params.y;
    let into_band = view_depth - (far - band);
    if (c + 1 < count && into_band > 0.0) {
        let next = cascade_visibility(world_position, n, c + 1);
        visibility = mix(visibility, next, clamp(into_band / max(band, 1e-3), 0.0, 1.0));
    }
    // За дальней границей последнего каскада тень плавно гаснет.
    if (c + 1 == count) {
        visibility = mix(visibility, 1.0, smoothstep(far - band, far, view_depth));
    }
    return visibility;
}
