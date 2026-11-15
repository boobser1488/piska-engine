//! kind: sprite
// Шейдер спрайта. Добавьте объекту со SpriteRenderer компонент SpriteShader и выберите
// этот файл. Спрайты с одним шейдером и одинаковыми параметрами рисуются одной пачкой.
// Справка: docs/api/shaders.md.

/// Цвет вспышки (например, при попадании).
//! param flash_color: color = (1.0, 1.0, 1.0, 1.0)
/// Сила вспышки: 0 — обычный спрайт, 1 — весь цветом вспышки.
//! param flash: float = 0.0 [0, 1]

fn sprite(in: SpriteIn) -> vec4<f32> {
    // Что нарисовал бы обычный спрайт: текстура × цвет.
    let base = default_sprite(in);
    // Прозрачные пиксели остаются прозрачными: меняем только цвет.
    let rgb = mix(base.rgb, params.flash_color.rgb, params.flash);
    return vec4<f32>(rgb, base.a);
}
