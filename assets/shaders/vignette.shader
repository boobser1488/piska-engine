//! kind: post
// Виньетка: края экрана темнее и слегка окрашены.

/// Сила затемнения.
//! param strength: float = 0.55 [0, 1]
/// Где начинается затемнение (расстояние от центра).
//! param radius: float = 0.3 [0, 1]
/// Мягкость перехода.
//! param softness: float = 0.45 [0.01, 1]
/// Оттенок краёв.
//! param tint: color = (0.15, 0.05, 0.1, 1.0)

fn post(in: PostIn) -> vec4<f32> {
    // Учитываем пропорции экрана: виньетка круглая, а не овальная.
    let aspect = in.resolution.x / max(in.resolution.y, 1.0);
    let d = length((in.uv - 0.5) * vec2<f32>(aspect, 1.0)) / max(aspect, 1.0) * 1.4;
    let shade = smoothstep(params.radius, params.radius + params.softness, d) * params.strength;
    let color = mix(in.color.rgb, in.color.rgb * params.tint.rgb, shade);
    return vec4<f32>(color, in.color.a);
}
