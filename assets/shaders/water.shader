//! kind: unlit
// Мультяшная вода: два слоя шума бегут в разные стороны, на гребнях — светлая пена,
// у краёв (под острым углом к камере) вода светлее.

/// Цвет глубокой воды.
//! param deep: color = (0.02, 0.18, 0.32, 1.0)
/// Цвет мелкой воды.
//! param shallow: color = (0.08, 0.55, 0.65, 1.0)
/// Цвет пены.
//! param foam: color = (0.9, 0.97, 1.0, 1.0)
/// Скорость течения.
//! param speed: float = 0.08 [0, 0.5]
/// Размер волн.
//! param scale: float = 5.0 [0.5, 30]

fn unlit(in: SurfaceIn) -> vec4<f32> {
    // Мировые координаты: соседние плоскости воды продолжают друг друга.
    let p = in.world_position.xz / params.scale;
    let a = fbm(p + vec2<f32>(in.time * params.speed, in.time * params.speed * 0.6));
    let b = fbm(p * 1.7 - vec2<f32>(in.time * params.speed * 0.8, -in.time * params.speed * 0.4));
    let waves = (a + b) * 0.5;
    var color = mix(params.deep.rgb, params.shallow.rgb, smoothstep(0.35, 0.75, waves));
    let crest = smoothstep(0.66, 0.72, waves);
    color = mix(color, params.foam.rgb, crest);
    let edge = fresnel(in.world_normal, in.view_dir, 4.0);
    color = mix(color, params.shallow.rgb * 1.4, edge * 0.5);
    return vec4<f32>(color, 1.0);
}
