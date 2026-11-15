//! kind: unlit
// Силовое поле: прозрачная сфера, светящаяся по краям, с бегущими волнами и шестиугольной
// сеткой. Материалу — Alpha: Blend и Double-Sided.

/// Цвет поля (яркость больше 1 даёт блум).
//! param color: color = (0.3, 1.2, 2.4, 1.0)
/// Насколько светятся края.
//! param rim_power: float = 2.5 [0.5, 8]
/// Плотность сетки.
//! param cells: float = 10.0 [2, 40]
/// Скорость волн.
//! param speed: float = 1.0 [0, 5]

fn unlit(in: SurfaceIn) -> vec4<f32> {
    let rim = fresnel(in.world_normal, in.view_dir, params.rim_power);
    // Шестиугольная сетка по UV: расстояние до ближайшего центра ячейки.
    let p = in.uv * vec2<f32>(params.cells * 1.732, params.cells);
    let a = p - floor(p / vec2<f32>(1.732, 1.0)) * vec2<f32>(1.732, 1.0) - vec2<f32>(0.866, 0.5);
    let q = p + vec2<f32>(0.866, 0.5);
    let b = q - floor(q / vec2<f32>(1.732, 1.0)) * vec2<f32>(1.732, 1.0) - vec2<f32>(0.866, 0.5);
    let d = min(length(a), length(b));
    let grid = smoothstep(0.42, 0.5, d);
    let wave = pow(0.5 + 0.5 * sin(in.world_position.y * 6.0 - in.time * params.speed * 4.0), 8.0);
    let alpha = clamp(rim * 0.9 + grid * 0.25 + wave * 0.25, 0.0, 1.0);
    return vec4<f32>(params.color.rgb * (0.4 + rim + grid * 0.5 + wave), alpha);
}
