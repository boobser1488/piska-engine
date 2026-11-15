// piska:variant lights — вариант для renderer/variant.rs (проверка шейдеров в редакторе берёт по одному файлу группы).
// Лампы сверх четырёх, прожекторы, тени ламп, заполняющий свет (lights.rs) — вариант
// шейдеров мешей и рельефа «с лампами» (renderer/variant.rs). Приклеивается после
// common.wgsl и shadow*.wgsl (сравнивающий сэмплер `shadow_sampler` — оттуда).
//
// Данные — в текстурах, читаются `textureLoad` (работает и в WebGL2):
// * `light_params` — по 4 текселя на лампу: (позиция, дальность), (цвет, вид: 0 точечная,
//   1 прожектор), (направление, cos внешнего полуугла), (cos внутреннего, первая грань
//   тени или −1, тангенс полуугла грани, сдвиг);
// * `light_cells` — ячейка кластера: 16 байтов-номеров ламп, 0xFF — конец списка;
// * `light_faces` — по 5 текселей на грань тени: столбцы матрицы и прямоугольник плитки;
// * `light_atlas` — глубины граней теней.

@group(0) @binding(3) var light_params: texture_2d<f32>;
@group(0) @binding(4) var light_cells: texture_2d<u32>;
@group(0) @binding(5) var light_atlas: texture_depth_2d;
@group(0) @binding(6) var light_faces: texture_2d<f32>;

const EXTENDED_LIGHTS: bool = true;
const CLUSTER_X: f32 = 16.0;
const CLUSTER_Y: f32 = 9.0;
const CLUSTER_Z: f32 = 16.0;

struct LocalLightSample {
    direction: vec3<f32>,
    radiance: vec3<f32>,
};

// Ячейка кластера пикселя: по положению на экране (`frag` — @builtin(position)) и
// глубине вида точки.
fn light_cell(frag: vec4<f32>, world: vec3<f32>) -> vec4<u32> {
    let size = max(globals.cluster.xy, vec2<f32>(1.0));
    let tile = clamp(floor(frag.xy / size * vec2<f32>(CLUSTER_X, CLUSTER_Y)), vec2<f32>(0.0), vec2<f32>(CLUSTER_X - 1.0, CLUSTER_Y - 1.0));
    let depth = (globals.view_proj * vec4<f32>(world, 1.0)).w;
    let slice = clamp(floor(log(max(depth, globals.cluster.z) / globals.cluster.z) * globals.cluster.w), 0.0, CLUSTER_Z - 1.0);
    return textureLoad(light_cells, vec2<i32>(i32(tile.x), i32(tile.y + slice * CLUSTER_Y)), 0);
}

// Номер `k`-й лампы ячейки (255 — дальше ламп нет).
fn cell_light(cell: vec4<u32>, k: u32) -> u32 {
    return (cell[k / 4u] >> ((k % 4u) * 8u)) & 0xFFu;
}

// Тень лампы: грань `face` атласа, 4 сравнения с линейной фильтрацией (сглаженный край).
fn light_face_visibility(face: i32, world: vec3<f32>) -> f32 {
    let m = mat4x4<f32>(
        textureLoad(light_faces, vec2<i32>(0, face), 0),
        textureLoad(light_faces, vec2<i32>(1, face), 0),
        textureLoad(light_faces, vec2<i32>(2, face), 0),
        textureLoad(light_faces, vec2<i32>(3, face), 0),
    );
    let rect = textureLoad(light_faces, vec2<i32>(4, face), 0);
    let clip = m * vec4<f32>(world, 1.0);
    if (clip.w <= 1e-4) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    let local = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let texel = globals.lights_info.y;
    let uv = clamp(rect.xy + local * rect.zw, rect.xy + vec2<f32>(texel * 1.5), rect.xy + rect.zw - vec2<f32>(texel * 1.5));
    let depth = ndc.z - 0.00002;
    var sum = 0.0;
    sum += textureSampleCompareLevel(light_atlas, shadow_sampler, uv + vec2<f32>(-0.5, -0.5) * texel, depth);
    sum += textureSampleCompareLevel(light_atlas, shadow_sampler, uv + vec2<f32>(0.5, -0.5) * texel, depth);
    sum += textureSampleCompareLevel(light_atlas, shadow_sampler, uv + vec2<f32>(-0.5, 0.5) * texel, depth);
    sum += textureSampleCompareLevel(light_atlas, shadow_sampler, uv + vec2<f32>(0.5, 0.5) * texel, depth);
    return sum * 0.25;
}

// Свет лампы `index` в точке: направление на лампу и пришедший свет (с затуханием,
// конусом прожектора и тенью, если `receive`).
fn local_light(index: u32, world: vec3<f32>, n: vec3<f32>, receive: bool) -> LocalLightSample {
    let row = i32(index);
    let p0 = textureLoad(light_params, vec2<i32>(0, row), 0);
    let p1 = textureLoad(light_params, vec2<i32>(1, row), 0);
    let p2 = textureLoad(light_params, vec2<i32>(2, row), 0);
    let p3 = textureLoad(light_params, vec2<i32>(3, row), 0);
    let to_light = p0.xyz - world;
    let distance = length(to_light);
    let l = to_light / max(distance, 1e-4);
    // Затухание — как у четырёх ламп без компонентов (mesh.wgsl).
    let reach = clamp(1.0 - distance / max(p0.w, 1e-3), 0.0, 1.0);
    var falloff = reach * reach / (1.0 + distance * distance);
    if (p1.w > 0.5) {
        falloff *= smoothstep(p2.w, p3.x, dot(-l, p2.xyz));
    }
    if (receive && p3.y >= 0.0 && falloff > 0.0) {
        // Сдвиг вдоль нормали растёт с расстоянием: тексель грани там крупнее.
        let texel_world = 2.0 * distance * p3.z / 512.0;
        let lifted = world + n * (texel_world * 1.5 + p3.w);
        var face = i32(p3.y);
        if (p1.w < 0.5) {
            // Грань куба по главной оси направления от лампы: +X, −X, +Y, −Y, +Z, −Z.
            let d = lifted - p0.xyz;
            let a = abs(d);
            if (a.x >= a.y && a.x >= a.z) {
                face += select(1, 0, d.x > 0.0);
            } else if (a.y >= a.z) {
                face += select(3, 2, d.y > 0.0);
            } else {
                face += select(5, 4, d.z > 0.0);
            }
        }
        falloff *= light_face_visibility(face, lifted);
    }
    return LocalLightSample(l, p1.rgb * falloff);
}

// Заполняющий свет (`FillLight`): полусфера — верх и отражённый снизу свет; без
// компонента — ambient как есть.
fn fill_ambient(n: vec3<f32>, base: vec3<f32>) -> vec3<f32> {
    if (globals.fill_sky.w < 0.5) {
        return base;
    }
    return mix(globals.fill_ground.rgb, globals.fill_sky.rgb, n.y * 0.5 + 0.5) * globals.sky_zenith.w;
}
