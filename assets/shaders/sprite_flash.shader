//! kind: sprite
// Вспышка при попадании: спрайт заливается цветом, прозрачность сохраняется. Из скрипта
// поставьте `flash` в 1 и плавно опускайте до 0.

/// Цвет вспышки.
//! param flash_color: color = (1.0, 1.0, 1.0, 1.0)
/// Сила: 0 — обычный спрайт, 1 — целиком цветом вспышки.
//! param flash: float = 0.0 [0, 1]

fn sprite(in: SpriteIn) -> vec4<f32> {
    let base = default_sprite(in);
    return vec4<f32>(mix(base.rgb, params.flash_color.rgb, params.flash), base.a);
}
