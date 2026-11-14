// Сетка вида редактора: «бесконечная» плоскость одним треугольником на весь экран.
// Приклеивается после common.wgsl (нужны `globals`). Рисуется внутри 3D-прохода после
// непрозрачных мешей и до спрайтов: проверка глубины идёт по настоящей MSAA-глубине, а
// шейдер сам пишет глубину точки на плоскости. Без освещения, теней и тумана.

struct Grid {
    // x — плоскость (0 — земля XZ, 1 — стена XY), y — высота плоскости,
    // z — дальность затухания, w — непрозрачность линий.
    params: vec4<f32>,
    color: vec4<f32>,      // rgb — цвет линий; w — сила осей
    axis_u: vec4<f32>,     // rgb — цвет линии u = 0
    axis_v: vec4<f32>,     // rgb — цвет линии v = 0, w — 1 для ортографического вида
    // Луч через пиксель с NDC (x, y), посчитанный на CPU из хорошо обусловленных строк
    // матрицы вида (см. grid_pass.rs): начало = base + origin_x·x + origin_y·y,
    // направление = dir_0 + dir_x·x + dir_y·y. Всё, что меняется от пикселя к пикселю, —
    // маленькие числа относительно `base` (камеры), поэтому вдали от начала координат
    // точка на плоскости не шумит и линии не рассыпаются в точки.
    base: vec4<f32>,       // xyz — камера (орто: центр ближней плоскости); w — ближняя граница t
    origin_x: vec4<f32>,
    origin_y: vec4<f32>,
    dir_0: vec4<f32>,      // w — дальняя граница t
    dir_x: vec4<f32>,
    dir_y: vec4<f32>,
};

@group(1) @binding(0) var<uniform> grid: Grid;

struct GridVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) index: u32) -> GridVertex {
    var out: GridVertex;
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    out.clip_position = vec4<f32>(corner, 0.5, 1.0);
    out.ndc = corner;
    return out;
}

// Насколько пиксель «попадает» в линии клетки `cell` (0…1), с учётом плотности линий:
// слишком частые по экрану линии гаснут, иначе вдали получается муар.
//
// `footprint` — размер пикселя на плоскости (`fwidth(uv)`), посчитанный один раз снаружи.
// Брать здесь `fwidth(uv / cell)` нельзя: на границе уровней детализации `cell` у соседних
// пикселей отличается в 10 раз, производная получается мусорной, и вдоль каждой такой
// границы сетка рассыпается в ряд случайных точек.
fn grid_lines(uv: vec2<f32>, cell: f32, footprint: vec2<f32>) -> f32 {
    let c = uv / cell;
    let d = max(footprint / cell, vec2<f32>(1e-6));
    let distance_px = abs(fract(c - 0.5) - 0.5) / d;
    let line = 1.0 - clamp(distance_px - 0.35, vec2<f32>(0.0), vec2<f32>(1.0));
    let density = 1.0 - smoothstep(vec2<f32>(0.18), vec2<f32>(0.45), d);
    return max(line.x * density.x, line.y * density.y);
}

struct GridOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_grid(in: GridVertex) -> GridOutput {
    // Луч через пиксель — относительно камеры (см. `Grid`).
    let origin = grid.origin_x.xyz * in.ndc.x + grid.origin_y.xyz * in.ndc.y;
    let ray = grid.dir_0.xyz + grid.dir_x.xyz * in.ndc.x + grid.dir_y.xyz * in.ndc.y;
    let wall = grid.params.x > 0.5;
    let height = grid.params.y - select(grid.base.y, grid.base.z, wall);
    let normal_0 = select(origin.y, origin.z, wall);
    let normal_ray = select(ray.y, ray.z, wall);
    let t = (height - normal_0) / select(normal_ray, 1e-12, abs(normal_ray) < 1e-12);
    let relative = origin + ray * t;
    let position = grid.base.xyz + relative;
    let uv = select(position.xz, position.xy, wall);

    // Производные считаем до любых выходов (нужен равномерный поток управления) и от
    // координат относительно камеры: у абсолютных вдали от нуля не хватает разрядов.
    let footprint = fwidth(select(relative.xz, relative.xy, wall));
    let level_size = max(max(footprint.x, footprint.y), 1e-6);
    let lod = log10_(level_size * 48.0);
    let level = floor(lod);
    let blend = lod - level;
    let cell = pow(10.0, level);
    let fine = grid_lines(uv, cell, footprint) * (1.0 - blend);
    let medium = grid_lines(uv, cell * 10.0, footprint);
    let coarse = grid_lines(uv, cell * 100.0, footprint) * blend;
    var lines = clamp(fine + medium + coarse, 0.0, 2.0) * 0.5;
    lines = clamp(lines * 1.15, 0.0, 1.0);

    // Оси мира: линия толще и цветная.
    let axis_width = max(footprint, vec2<f32>(1e-6));
    let on_u = 1.0 - clamp(abs(uv.x) / axis_width.x - 0.6, 0.0, 1.0); // линия u = 0
    let on_v = 1.0 - clamp(abs(uv.y) / axis_width.y - 0.6, 0.0, 1.0); // линия v = 0

    let ortho = grid.axis_v.w > 0.5; // параллельные лучи (2D-вид): расстояния до камеры не нужны
    let distance_to_camera = length(relative);
    var fade = 1.0;
    if (!ortho) {
        fade = 1.0 - smoothstep(grid.params.z * 0.35, grid.params.z, distance_to_camera);
    }
    // Вид с обратной стороны плоскости — тусклее.
    let camera_side = select(globals.camera_position.y, globals.camera_position.z, wall) - grid.params.y;
    let side = select(0.45, 1.0, camera_side >= 0.0 || ortho);

    let line_alpha = lines * grid.params.w * fade * side;
    let axis_strength = grid.color.w * grid.params.w * fade * side;
    let alpha_u = on_u * axis_strength;
    let alpha_v = on_v * axis_strength;
    let axis_alpha = 1.0 - (1.0 - alpha_u) * (1.0 - alpha_v);
    let axis_color = (grid.axis_u.rgb * alpha_u + grid.axis_v.rgb * alpha_v) / max(alpha_u + alpha_v, 1e-4);
    let total = 1.0 - (1.0 - line_alpha) * (1.0 - axis_alpha);
    let color = (grid.color.rgb * line_alpha * (1.0 - axis_alpha) + axis_color * axis_alpha) / max(total, 1e-4);

    // Глубина точки; чуть ближе к камере, чтобы линии не мерцали на поверхностях,
    // лежащих в той же плоскости (пол-меш на y = 0).
    let toward_camera = -normalize(ray);
    let bias = select(distance_to_camera * 4e-4 + 1e-3, 1e-3, ortho);
    let biased = position + toward_camera * bias;
    let clip = globals.view_proj * vec4<f32>(biased, 1.0);

    let visible = t > grid.base.w && t < grid.dir_0.w && total > 0.002;
    if (!visible) {
        discard;
    }
    var out: GridOutput;
    out.color = vec4<f32>(color, total);
    out.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return out;
}

fn log10_(x: f32) -> f32 {
    return log2(x) * 0.30102999566;
}
