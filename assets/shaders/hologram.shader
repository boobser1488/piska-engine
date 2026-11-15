//! kind: unlit
// Голограмма: бегущие полосы развёртки, яркие края, мерцание. Материалу — Alpha: Blend,
// чтобы сквозь голограмму было видно сцену.

/// Цвет голограммы.
//! param color: color = (0.2, 0.85, 1.4, 1.0)
/// Сколько полос развёртки на метр высоты.
//! param lines: float = 18.0 [2, 80]
/// Скорость бега полос.
//! param speed: float = 1.5 [0, 8]
/// Сила мерцания.
//! param flicker: float = 0.15 [0, 1]

fn unlit(in: SurfaceIn) -> vec4<f32> {
    // Полосы идут по мировой высоте — у всех частей объекта одинаковые.
    let band = fract(in.world_position.y * params.lines - in.time * params.speed);
    let scan = smoothstep(0.0, 0.15, band) * (1.0 - smoothstep(0.5, 0.65, band));
    // Края (там, где поверхность видна «вскользь») ярче.
    let rim = fresnel(in.world_normal, in.view_dir, 2.0);
    let wobble = 1.0 - params.flicker * step(0.93, hash(vec2<f32>(floor(in.time * 24.0), 0.5)));
    let alpha = clamp(0.15 + scan * 0.35 + rim * 0.8, 0.0, 1.0) * wobble;
    return vec4<f32>(params.color.rgb * (0.6 + rim + scan * 0.5) * wobble, alpha);
}
