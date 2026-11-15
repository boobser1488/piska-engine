// piska:variant water_scene — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Кадр и глубина под водой для water_fx.wgsl: копия кадра до воды и многовыборочная
// глубина сцены (только чтение). Не для WebGL2 — там water_noscene.wgsl.

@group(2) @binding(0) var scene_copy: texture_2d<f32>;
@group(2) @binding(1) var scene_sampler: sampler;
@group(2) @binding(2) var scene_depth: texture_depth_multisampled_2d;

const WATER_SCENE: bool = true;

fn scene_depth_at(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth));
    let pixel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(scene_depth, pixel, 0);
}

// Точка сцены под пикселем `uv` (0…1, сверху вниз). Небо (глубина 0) — очень далеко.
fn scene_world(uv: vec2<f32>) -> vec3<f32> {
    let depth = max(scene_depth_at(uv), 1e-7);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let p = globals.inv_view_proj * ndc;
    return p.xyz / p.w;
}

fn scene_color(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scene_copy, scene_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

// Отражение сцены по экрану: луч от точки воды вдоль `r` шагами растущей длины; попадание —
// когда точка луча оказалась за поверхностью сцены (обратная глубина: меньше — дальше).
// Возвращает (цвет, вес): у краёв кадра и для лучей в небо вес гаснет.
fn scene_reflection(origin: vec3<f32>, r: vec3<f32>) -> vec4<f32> {
    var step_len = 0.4;
    var travelled = 0.0;
    for (var k = 0; k < 16; k += 1) {
        travelled += step_len;
        step_len *= 1.35;
        let pos = origin + r * travelled;
        let clip = globals.view_proj * vec4<f32>(pos, 1.0);
        if (clip.w <= 0.0) {
            break;
        }
        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
            break;
        }
        let depth = scene_depth_at(uv);
        if (depth > ndc.z && depth > 0.0) {
            // Насколько луч ушёл за поверхность: толстые «стены» глубины — промах.
            let surface = scene_world(uv);
            if (distance(surface, pos) < step_len * 1.5 + 0.3) {
                let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
                let weight = smoothstep(0.0, 0.08, edge) * (1.0 - smoothstep(0.6, 1.0, f32(k) / 16.0));
                return vec4<f32>(scene_color(uv), weight);
            }
            break;
        }
    }
    return vec4<f32>(0.0);
}
