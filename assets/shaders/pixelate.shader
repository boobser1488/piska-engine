//! kind: post
// Пиксельный экран: кадр крупными квадратами и с урезанной палитрой (ретро).

/// Размер «пикселя» в пикселях экрана.
//! param pixel_size: float = 6.0 [1, 32]
/// Сколько уровней яркости на канал (0 — без урезания).
//! param levels: int = 12 [0, 64]

fn post(in: PostIn) -> vec4<f32> {
    let cell = max(params.pixel_size, 1.0) * in.texel_size;
    let uv = (floor(in.uv / cell) + 0.5) * cell;
    var color = scene(uv).rgb;
    if (params.levels > 0) {
        let n = f32(params.levels);
        color = floor(color * n + 0.5) / n;
    }
    return vec4<f32>(color, 1.0);
}
