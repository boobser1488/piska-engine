// Эффекты по глубине кадра (post_fx.rs): экранное затенение (SSAO, половинное
// разрешение) и контур по разрывам глубины и нормалей. Итог — текстура `fx` полного
// размера: r — доля рассеянного света (1 — без затенения), g — сила контура. Её
// подмешивает сведение кадра (post.wgsl::fs_composite).
//
// Глубина читается из многовыборочного буфера глубины сцены (выборка 0), поэтому этот
// модуль собирается только там, где такие текстуры можно читать в шейдере (не WebGL2).

struct FxParams {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera: vec4<f32>,   // xyz — позиция камеры, w — 1 для перспективы, 0 для ортографии
    ao: vec4<f32>,       // x — радиус (м), y — сила, z — смещение (м), w — число выборок
    outline: vec4<f32>,  // x — порог глубины (доля расстояния), y — порог нормалей (1 − cos), z — толщина (пиксели), w — включён
    size: vec4<f32>,     // xy — размер кадра (пиксели), zw — 1 / размер
};

@group(0) @binding(0) var depth_texture: texture_depth_multisampled_2d;
@group(0) @binding(1) var<uniform> fx: FxParams;
@group(0) @binding(2) var ao_texture: texture_2d<f32>;
@group(0) @binding(3) var ao_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

fn depth_at(pixel: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_texture));
    return textureLoad(depth_texture, clamp(pixel, vec2<i32>(0), size - 1), 0);
}

// Мировая точка по пикселю и глубине (обратная глубина: 1 — ближняя плоскость).
fn world_at(pixel: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(pixel) + 0.5) * fx.size.zw;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let p = fx.inv_view_proj * ndc;
    return p.xyz / p.w;
}

// Нормаль по соседним точкам: с каждой стороны берётся та, что ближе по глубине —
// на краях объектов нормаль не «перетекает» через разрыв.
fn normal_at(pixel: vec2<i32>, center: vec3<f32>) -> vec3<f32> {
    let r = world_at(pixel + vec2<i32>(1, 0), depth_at(pixel + vec2<i32>(1, 0)));
    let l = world_at(pixel - vec2<i32>(1, 0), depth_at(pixel - vec2<i32>(1, 0)));
    let u = world_at(pixel + vec2<i32>(0, 1), depth_at(pixel + vec2<i32>(0, 1)));
    let d = world_at(pixel - vec2<i32>(0, 1), depth_at(pixel - vec2<i32>(0, 1)));
    var dx = r - center;
    if (length(center - l) < length(dx)) {
        dx = center - l;
    }
    var dy = u - center;
    if (length(center - d) < length(dy)) {
        dy = center - d;
    }
    var n = normalize(cross(dy, dx));
    // К камере.
    let to_eye = fx.camera.xyz - center;
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    return n;
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// ---------------- SSAO: половинное разрешение ----------------

@fragment
fn fs_ao(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.uv * fx.size.xy);
    let depth = depth_at(pixel);
    if (depth <= 0.0) {
        return vec4<f32>(1.0); // небо
    }
    let p = world_at(pixel, depth);
    let n = normal_at(pixel, p);
    // Базис вокруг нормали, повёрнутый на случайный угол в пикселе.
    let angle = hash12(in.clip_position.xy) * 6.2831853;
    var t = normalize(cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9)));
    let b = cross(n, t);
    t = t * cos(angle) + b * sin(angle);
    let bb = cross(n, t);
    let radius = fx.ao.x;
    let count = i32(fx.ao.w);
    var occlusion = 0.0;
    for (var i = 0; i < count; i += 1) {
        // Точки полусферы, гуще к центру.
        let fi = f32(i) + 0.5;
        let phi = fi * 2.399963; // золотой угол
        let r = sqrt(fi / f32(count));
        let h = sqrt(max(1.0 - r * r, 0.0));
        let scale = mix(0.15, 1.0, fi / f32(count));
        let offset = (t * cos(phi) * r + bb * sin(phi) * r + n * h) * radius * scale;
        let sample_point = p + offset;
        let clip = fx.view_proj * vec4<f32>(sample_point, 1.0);
        if (clip.w <= 1e-4) {
            continue;
        }
        let ndc = clip.xyz / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0))) {
            continue;
        }
        let scene_depth = depth_at(vec2<i32>(suv * fx.size.xy));
        let scene_point = world_at(vec2<i32>(suv * fx.size.xy), scene_depth);
        // Обратная глубина: точка поверхности ближе к камере, чем выборка, — заслоняет.
        let eye = fx.camera.xyz;
        let behind = distance(eye, sample_point) - distance(eye, scene_point);
        let range = smoothstep(0.0, 1.0, radius / max(distance(p, scene_point), 1e-3));
        occlusion += select(0.0, 1.0, behind > fx.ao.z) * range;
    }
    let ao = 1.0 - occlusion / max(f32(count), 1.0);
    return vec4<f32>(ao, ao, ao, 1.0);
}

// ---------------- контур ----------------

fn depth_s(pixel: vec2<i32>, s: i32) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_texture));
    return textureLoad(depth_texture, clamp(pixel, vec2<i32>(0), size - 1), s);
}

// Сила контура в выборке `s` пикселя: 0…1. Сосед считается «за краем», если он далеко от
// плоскости центральной точки (синус угла между направлением на соседа и плоскостью):
// плоский пол под скользящим углом даёт ноль, как бы ни менялась глубина, а силуэт и
// излом грани — заметную величину. Линия рисуется только у ближней к камере стороны.
fn sample_edge(pixel: vec2<i32>, s: i32, step: i32) -> f32 {
    let d = depth_s(pixel, s);
    if (d <= 0.0) {
        return 0.0; // небо
    }
    let p = world_at(pixel, d);
    let eye = fx.camera.xyz;
    let dist = distance(p, eye);
    var offsets = array<vec2<i32>, 4>(vec2<i32>(step, 0), vec2<i32>(-step, 0), vec2<i32>(0, step), vec2<i32>(0, -step));
    var points: array<vec3<f32>, 4>;
    var sky: array<bool, 4>;
    for (var i = 0; i < 4; i += 1) {
        // Сосед за краем кадра зажимается в кадр — и для глубины, и для точки.
        let size = vec2<i32>(textureDimensions(depth_texture));
        let qp = clamp(pixel + offsets[i], vec2<i32>(0), size - 1);
        let qd = depth_s(qp, s);
        sky[i] = qd <= 0.0;
        points[i] = world_at(qp, qd);
    }
    // Нормаль центра — по ближним по глубине соседям с каждой стороны.
    // У края кадра сосед зажат в тот же пиксель (нулевой сдвиг) — берётся другая сторона.
    var dx = points[0] - p;
    if (sky[0] || length(dx) < 1e-6 || (!sky[1] && length(p - points[1]) > 1e-6 && length(p - points[1]) < length(dx))) {
        dx = p - points[1];
    }
    var dy = points[2] - p;
    if (sky[2] || length(dy) < 1e-6 || (!sky[3] && length(p - points[3]) > 1e-6 && length(p - points[3]) < length(dy))) {
        dy = p - points[3];
    }
    let c = cross(dy, dx);
    if (dot(c, c) < 1e-20) {
        return 0.0;
    }
    let n = normalize(c);
    let low = sin(acos(clamp(1.0 - fx.outline.y, -1.0, 1.0)) * 0.5);
    var edge = 0.0;
    for (var i = 0; i < 4; i += 1) {
        if (sky[i]) {
            // Силуэт на фоне неба; у поверхности, видимой почти с ребра (пол к горизонту), — нет.
            edge = max(edge, smoothstep(0.05, 0.2, abs(dot(n, normalize(eye - p)))));
            continue;
        }
        let q = points[i];
        if (distance(q, p) < 1e-6) {
            continue; // край кадра
        }
        let qdist = distance(q, eye);
        // Сосед заметно ближе к камере — линия будет на его стороне.
        if (qdist < dist * (1.0 - fx.outline.x)) {
            continue;
        }
        let offset = q - p;
        let off_plane = abs(dot(offset, n)) / max(length(offset), 1e-5);
        // Разрыв глубины (силуэт) — всегда край, излом — по порогу нормалей.
        let jump = smoothstep(fx.outline.x, fx.outline.x * 2.0, (qdist - dist) / max(dist, 1e-3)) * smoothstep(0.2, 0.5, off_plane);
        let crease = smoothstep(low, low * 1.8 + 0.05, off_plane);
        edge = max(edge, max(jump, crease));
    }
    return edge;
}

// ---------------- полный размер: размытие AO по глубине + контур ----------------

fn linear_distance(pixel: vec2<i32>) -> f32 {
    let d = depth_at(pixel);
    if (d <= 0.0) {
        return 1e6;
    }
    return distance(world_at(pixel, d), fx.camera.xyz);
}

@fragment
fn fs_resolve(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.uv * fx.size.xy);
    let depth = depth_at(pixel);
    let center_distance = linear_distance(pixel);

    // AO: 4×4 выборок половинного разрешения с весом по близости глубины (без ореолов
    // вокруг краёв объектов).
    var ao = 1.0;
    if (fx.ao.y > 0.0 && depth > 0.0) {
        var sum = 0.0;
        var weight = 0.0;
        let half_texel = 2.0 * fx.size.zw;
        for (var y = -1; y <= 2; y += 1) {
            for (var x = -1; x <= 2; x += 1) {
                let offset = vec2<f32>(f32(x) - 0.5, f32(y) - 0.5);
                let uv = in.uv + offset * half_texel;
                let tap_pixel = vec2<i32>(uv * fx.size.xy);
                let tap_distance = linear_distance(tap_pixel);
                let w = 1.0 / (1e-3 + abs(tap_distance - center_distance) / max(center_distance * 0.02, 1e-3));
                sum += textureSampleLevel(ao_texture, ao_sampler, uv, 0.0).r * w;
                weight += w;
            }
        }
        ao = sum / max(weight, 1e-4);
    }

    // Контур: по каждой из четырёх выборок MSAA отдельно, затем среднее — линия
    // сглажена так же, как края геометрии (доля покрытия, а не «лестница»).
    var edge = 0.0;
    if (fx.outline.w > 0.5) {
        // Толщина задана для 720p и растёт с разрешением кадра.
        let step = max(i32(fx.outline.z * fx.size.y / 720.0 + 0.5), 1);
        for (var s = 0; s < 4; s += 1) {
            edge += sample_edge(pixel, s, step);
        }
        edge *= 0.25;
    }
    return vec4<f32>(ao, edge, 0.0, 1.0);
}
