//! kind: post
// Под водой: кадр колышется, синеет и мутнеет к краям, по нему бегут блики.

/// Сила колыхания (доля экрана).
//! param wobble: float = 0.006 [0, 0.03]
/// Цвет воды.
//! param tint: color = (0.2, 0.6, 0.85, 1.0)
/// Насколько кадр окрашен водой.
//! param depth: float = 0.45 [0, 1]
/// Яркость бегущих бликов.
//! param caustics: float = 0.25 [0, 1]

fn post(in: PostIn) -> vec4<f32> {
    let t = in.time;
    let offset = vec2<f32>(
        sin(in.uv.y * 18.0 + t * 1.7) + sin(in.uv.y * 7.0 - t * 1.1),
        cos(in.uv.x * 15.0 + t * 1.3)
    ) * params.wobble;
    var color = scene(in.uv + offset).rgb;
    color = mix(color, color * params.tint.rgb * 1.4, params.depth);
    let edge = smoothstep(0.3, 0.9, distance(in.uv, vec2<f32>(0.5)));
    color = mix(color, params.tint.rgb * 0.25, edge * params.depth * 0.6);
    let light = pow(value_noise(in.uv * vec2<f32>(9.0, 6.0) + vec2<f32>(t * 0.4, t * 0.25)), 6.0);
    color += params.tint.rgb * light * params.caustics * 2.0;
    return vec4<f32>(color, 1.0);
}
