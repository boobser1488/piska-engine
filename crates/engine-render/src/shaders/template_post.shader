//! kind: post
// Эффект на весь экран. Добавьте камере компонент PostEffect и выберите этот файл.
// Эффектов может быть несколько: они идут по возрастанию `order`.
// Справка: docs/api/shaders.md.

/// Сила затемнения краёв экрана.
//! param strength: float = 0.5 [0, 1]
/// Насколько далеко от центра начинается затемнение.
//! param radius: float = 0.35 [0, 1]

fn post(in: PostIn) -> vec4<f32> {
    // Расстояние от центра экрана (0 в центре, около 0.7 в углах).
    let d = distance(in.uv, vec2<f32>(0.5));
    let shade = 1.0 - params.strength * smoothstep(params.radius, params.radius + 0.45, d);
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}
