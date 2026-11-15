//! kind: post
// Старый телевизор: выпуклый экран, строки развёртки, сдвиг цветов по краям, тёмные углы.

/// Выпуклость экрана.
//! param curvature: float = 0.12 [0, 0.4]
/// Сила строк развёртки.
//! param scanlines: float = 0.35 [0, 1]
/// Сдвиг красного и синего по краям (пиксели).
//! param aberration: float = 1.5 [0, 6]

fn post(in: PostIn) -> vec4<f32> {
    // Выпуклость: точки уходят от центра тем сильнее, чем дальше от него.
    let c = in.uv * 2.0 - 1.0;
    let bent = c * (1.0 + params.curvature * dot(c, c));
    let uv = bent * 0.5 + 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let shift = vec2<f32>(params.aberration * in.texel_size.x * dot(c, c), 0.0);
    let r = scene(uv + shift).r;
    let g = scene(uv).g;
    let b = scene(uv - shift).b;
    var color = vec3<f32>(r, g, b);
    // Строки развёртки по строкам экрана.
    let line = 0.5 + 0.5 * sin(uv.y * in.resolution.y * PI);
    color *= 1.0 - params.scanlines * (1.0 - line);
    // Тёмные углы кинескопа.
    let corner = 1.0 - smoothstep(0.6, 1.4, length(bent));
    return vec4<f32>(color * (0.4 + 0.6 * corner) * 1.15, 1.0);
}
