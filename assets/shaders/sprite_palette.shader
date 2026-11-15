//! kind: sprite
// Смена палитры: пиксели, близкие по цвету к `source_color`, перекрашиваются в `new_color` с сохранением
// светотени (одежда врагов разных цветов из одной картинки).

/// Какой цвет заменить.
//! param source_color: color = (0.85, 0.2, 0.2, 1.0)
/// На какой цвет.
//! param new_color: color = (0.2, 0.45, 0.95, 1.0)
/// Насколько далёкие оттенки тоже заменяются.
//! param tolerance: float = 0.12 [0, 0.5]

fn sprite(in: SpriteIn) -> vec4<f32> {
    let base = default_sprite(in);
    let hsv = rgb_to_hsv(base.rgb);
    let key = rgb_to_hsv(params.source_color.rgb);
    // Расстояние по оттенку (по кругу) — и только у насыщенных пикселей.
    let dh = abs(fract(hsv.x - key.x + 0.5) - 0.5);
    let pick = (1.0 - smoothstep(params.tolerance * 0.5, params.tolerance, dh)) * smoothstep(0.15, 0.3, hsv.y);
    let target_hsv = rgb_to_hsv(params.new_color.rgb);
    // Новый оттенок и насыщенность, прежняя яркость (светотень остаётся).
    let swapped = hsv_to_rgb(vec3<f32>(target_hsv.x, target_hsv.y, hsv.z * max(target_hsv.z, 0.05) / max(key.z, 0.05)));
    return vec4<f32>(mix(base.rgb, swapped, pick), base.a);
}
