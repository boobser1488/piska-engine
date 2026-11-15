//! kind: unlit
// Мультяшный свет: освещённость разбита на несколько ступеней, тени солнца сохраняются,
// по краю — светлый контур. Свет считается здесь же (`sun_direction`, `sun_visibility`).

/// Цвет поверхности на свету (умножается на текстуру и цвет материала).
//! param tint: color = (1.0, 0.42, 0.18, 1.0)
/// Цвет теневой стороны.
//! param shade: color = (0.22, 0.16, 0.5, 1.0)
/// Сколько ступеней освещённости.
//! param steps: int = 3 [1, 6]
/// Яркость светлого края.
//! param rim: float = 0.35 [0, 2]

fn unlit(in: SurfaceIn) -> vec4<f32> {
    let n = normalize(in.normal);
    // Освещённость 0..1 с тенью, разбитая на ступени.
    let light = max(dot(n, sun_direction()), 0.0) * sun_visibility(in);
    let count = f32(max(params.steps, 1));
    let level = min(floor(light * count + 0.5) / count, 1.0);
    let base = in.albedo * params.tint.rgb;
    let lit = mix(base * params.shade.rgb, base, level) * (0.35 + 0.65 * min(luminance(sun_color()), 1.0));
    let edge = step(0.6, fresnel(n, in.view_dir, 1.0)) * params.rim * level;
    // Блик — резкое пятно, а не плавный переход.
    let half_dir = normalize(sun_direction() + in.view_dir);
    let spec = step(0.985, max(dot(n, half_dir), 0.0)) * level;
    return vec4<f32>(lit + vec3<f32>(edge + spec * 0.8) + in.emissive, in.alpha);
}
