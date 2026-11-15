//! kind: surface
// Подсветка контура (френель): обычный материал, но края объекта светятся — выделение,
// «призрачность», подсказка «можно взять».

/// Цвет подсветки (яркость больше 1 даёт блум).
//! param rim_color: color = (1.0, 0.55, 0.15, 1.0)
/// Ширина подсветки: больше — тоньше полоса.
//! param power: float = 2.2 [0.5, 8]
/// Яркость.
//! param intensity: float = 4.0 [0, 10]
/// Пульсация (0 — ровно).
//! param pulse: float = 0.3 [0, 1]

fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);
    let rim = fresnel(s.normal, in.view_dir, params.power);
    let beat = 1.0 - params.pulse * (0.5 + 0.5 * sin(in.time * 4.0));
    s.emissive += params.rim_color.rgb * rim * params.intensity * beat;
    return s;
}
