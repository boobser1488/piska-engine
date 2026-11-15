//! kind: sprite
// Обводка спрайта: прозрачные пиксели рядом с непрозрачными закрашиваются цветом
// обводки. У картинки должен быть прозрачный отступ шириной в обводку.

/// Цвет обводки.
//! param outline_color: color = (1.0, 0.85, 0.2, 1.0)
/// Толщина в пикселях текстуры.
//! param thickness: float = 1.0 [0, 4]
/// Мигание обводки (0 — ровно).
//! param pulse: float = 0.0 [0, 1]

fn sprite(in: SpriteIn) -> vec4<f32> {
    let base = default_sprite(in);
    let step_uv = in.texel_size * params.thickness;
    // Самая непрозрачная из восьми соседних точек.
    var around = 0.0;
    for (var i = 0; i < 8; i += 1) {
        let angle = f32(i) * PI * 0.25;
        let uv = clamp(in.uv + vec2<f32>(cos(angle), sin(angle)) * step_uv, in.frame_rect.xy, in.frame_rect.zw);
        around = max(around, sample_sprite(uv).a);
    }
    let glow = 1.0 - params.pulse * (0.5 + 0.5 * sin(in.time * 6.0));
    let outline = vec4<f32>(params.outline_color.rgb * glow, params.outline_color.a * around * in.color.a);
    // Поверх обводки — сам спрайт.
    return mix(outline, base, base.a);
}
