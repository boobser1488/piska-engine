//! kind: post
// Чёрно-белый кадр, в котором остаётся цветным один оттенок (красный по умолчанию).

/// Какой цвет оставить цветным.
//! param keep: color = (0.9, 0.15, 0.1, 1.0)
/// Насколько широк сохраняемый оттенок.
//! param tolerance: float = 0.1 [0.01, 0.5]
/// Насколько обесцветить остальное (1 — полностью).
//! param amount: float = 1.0 [0, 1]

fn post(in: PostIn) -> vec4<f32> {
    let hsv = rgb_to_hsv(in.color.rgb);
    let key = rgb_to_hsv(params.keep.rgb);
    let dh = abs(fract(hsv.x - key.x + 0.5) - 0.5);
    let keep = (1.0 - smoothstep(params.tolerance, params.tolerance * 1.8, dh)) * smoothstep(0.08, 0.2, hsv.y);
    let gray = vec3<f32>(luminance(in.color.rgb));
    let color = mix(in.color.rgb, gray, params.amount * (1.0 - keep));
    return vec4<f32>(color, in.color.a);
}
