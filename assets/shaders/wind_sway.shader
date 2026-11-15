//! kind: surface
// Ветер: верх объекта качается, низ стоит на месте (трава, кусты, флаги). Сдвиг вершин
// действует и в тенях — тень качается вместе с объектом.

/// Насколько сильно качает (метры у верхушки высотой 1 м).
//! param strength: float = 0.25 [0, 1]
/// Скорость ветра.
//! param speed: float = 1.6 [0, 6]
/// Направление ветра по земле (x, z).
//! param direction: vec2 = (1.0, 0.3)
/// Цвет верхушек (смешивается с цветом материала).
//! param tip_color: color = (0.55, 1.0, 0.25, 1.0)

fn vertex(in: VertexIn) -> VertexOut {
    var o = default_vertex(in);
    // Высота точки над опорой объекта: низ не двигается.
    let height = max(in.world_position.y - in.object_origin.y, 0.0);
    // Порывы: медленная волна + быстрая дрожь, у соседних объектов — в разной фазе.
    let phase = dot(in.world_position.xz, vec2<f32>(0.35, 0.21));
    let gust = sin(in.time * params.speed + phase) * 0.7 + sin(in.time * params.speed * 2.7 + phase * 3.0) * 0.3;
    let dir = normalize(vec2<f32>(params.direction.x, params.direction.y) + vec2<f32>(1e-4, 0.0));
    let bend = gust * params.strength * height * height;
    o.world_position += vec3<f32>(dir.x, 0.0, dir.y) * bend;
    return o;
}

fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);
    // Чем выше точка, тем больше цвета верхушек.
    let tip = clamp(in.object_position.y, 0.0, 1.0);
    s.albedo = mix(s.albedo, s.albedo * params.tip_color.rgb * 1.6, tip);
    return s;
}
