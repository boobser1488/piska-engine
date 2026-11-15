//! kind: surface
// Растворение: поверхность исчезает по шуму, край светится. Тень исчезает вместе с ней.
// Двигайте `amount` из скрипта: 0 — целый объект, 1 — исчез.

/// Доля растворённой поверхности: 0 — целая, 1 — исчезла.
//! param amount: float = 0.35 [0, 1]
/// Цвет светящегося края (яркость больше 1 даёт блум).
//! param edge_color: color = (8.0, 2.6, 0.4, 1.0)
/// Ширина светящегося края.
//! param edge_width: float = 0.12 [0, 0.3]
/// Размер пятен шума.
//! param scale: float = 3.0 [0.5, 30]

fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);
    // Шум по UV: одинаковые пятна на любом размере объекта.
    let n = fbm(in.uv * params.scale);
    if (n < params.amount) {
        discard;
    }
    // Чем ближе к границе растворения, тем ярче край.
    let edge = 1.0 - smoothstep(0.0, params.edge_width, n - params.amount);
    s.emissive += params.edge_color.rgb * edge * step(0.001, params.amount);
    s.albedo = mix(s.albedo, vec3<f32>(0.05), edge);
    return s;
}
