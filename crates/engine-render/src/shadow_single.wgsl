// piska:variant shadow default — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Тени солнца одной картой — вариант шейдеров мешей и рельефа для кадров без
// `SoftShadows` (renderer.rs выбирает вариант по кадру). Приклеивается после common.wgsl
// вместо shadow.wgsl: обычная 2D-карта и PCF 3×3 без кода каскадов — шейдер не платит
// регистрами и выборками из массива за то, чем кадр не пользуется. Результат тот же до
// бита, что у одной карты в shadow.wgsl.

@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

// Доля солнечного света, дошедшая до точки: 1 — на свету, 0 — в тени. К краю области
// теней тень плавно гаснет.
fn sun_shadow(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    if (globals.shadow_params.x < 0.5) {
        return 1.0;
    }
    // Сдвиг вдоль нормали убирает «акне», не отрывая тень от объекта.
    let clip = globals.light_view_proj * vec4<f32>(world_position + n * globals.shadow_params.z, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let depth = clip.z - globals.shadow_params.w;
    let texel = globals.shadow_params.y;
    var sum = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, depth);
        }
    }
    let edge = max(max(abs(clip.x), abs(clip.y)), clip.z);
    return mix(sum / 9.0, 1.0, smoothstep(0.85, 1.0, edge));
}
