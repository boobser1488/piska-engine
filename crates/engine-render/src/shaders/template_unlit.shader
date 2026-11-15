//! kind: unlit
// Шейдер без освещения: цвет, который вернёт функция, и будет цветом пикселя (туман движок
// добавит сам). Подходит для голограмм, неба на плоскости, бегущей воды, указателей.
// Справка: docs/api/shaders.md.

/// Первый цвет полос.
//! param color_a: color = (0.1, 0.6, 1.0, 1.0)
/// Второй цвет полос.
//! param color_b: color = (0.0, 0.15, 0.4, 1.0)
/// Скорость движения полос.
//! param speed: float = 0.5 [0, 4]

fn unlit(in: SurfaceIn) -> vec4<f32> {
    // Полосы бегут по UV со временем; sin даёт -1..1, переводим в 0..1.
    let wave = sin((in.uv.y + in.time * params.speed) * TAU * 4.0) * 0.5 + 0.5;
    let color = mix(params.color_a.rgb, params.color_b.rgb, wave);
    return vec4<f32>(color, 1.0);
}
