// Меши: PBR (metallic-roughness, GGX), солнце с картой теней, до четырёх точечных
// источников, ambient от неба, туман. Выход — HDR: тонмаппинг и блум делает post.wgsl.
// Перед этим файлом приклеивается common.wgsl.

@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

// Материал (renderer/material.rs): общие для пачки параметры и текстуры. Цвет, металл,
// шероховатость и свечение — в данных экземпляра, поэтому материалы, отличающиеся
// только ими, рисуются одной пачкой. Пустые слоты — белая текстура 1×1.
struct MaterialParams {
    uv: vec4<f32>,       // xy — масштаб UV, zw — сдвиг
    factors: vec4<f32>,  // x — сила карты нормалей (0 — карты нет), y — сила AO, z — порог выреза
    flags: vec4<f32>,    // x — без освещения, y — принимает тени, z — двусторонний, w — альфа: 0 непрозрачный, 1 вырез, 2 смешивание
};
@group(1) @binding(0) var<uniform> material: MaterialParams;
@group(1) @binding(1) var material_sampler: sampler;
@group(1) @binding(2) var base_texture: texture_2d<f32>;
@group(1) @binding(3) var metallic_roughness_texture: texture_2d<f32>;
@group(1) @binding(4) var normal_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var occlusion_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Материал экземпляра: одинаков для всех вершин, поэтому без интерполяции.
    @location(3) @interpolate(flat) base_color: vec4<f32>,
    @location(4) @interpolate(flat) emissive: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
    // Касательная в мире (xyz) и знак битангенса (w).
    @location(6) tangent: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(13) tangent: vec4<f32>,
    instance: Instance,
) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(instance.model_0, instance.model_1, instance.model_2, instance.model_3);
    let normal_matrix = mat3x3<f32>(instance.normal_0.xyz, instance.normal_1.xyz, instance.normal_2.xyz);
    let world = model * vec4<f32>(position, 1.0);
    out.world_position = world.xyz;
    out.clip_position = globals.view_proj * world;
    out.normal = normal_matrix * normal;
    out.uv = uv;
    out.base_color = instance.base_color;
    out.emissive = instance.emissive;
    out.params = instance.params;
    out.tangent = vec4<f32>((model * vec4<f32>(tangent.xyz, 0.0)).xyz, tangent.w);
    return out;
}

// ---------------- проход теней с вырезом: альфа базовой текстуры прорезает тень ----------------

struct CutoutInstance {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(10) base_color: vec4<f32>,
};

struct CutoutOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) alpha: f32,
};

@vertex
fn vs_shadow_cutout(@location(0) position: vec3<f32>, @location(2) uv: vec2<f32>, instance: CutoutInstance) -> CutoutOutput {
    var out: CutoutOutput;
    let model = mat4x4<f32>(instance.model_0, instance.model_1, instance.model_2, instance.model_3);
    out.clip_position = globals.light_view_proj * model * vec4<f32>(position, 1.0);
    out.uv = uv * material.uv.xy + material.uv.zw;
    out.alpha = instance.base_color.a;
    return out;
}

@fragment
fn fs_shadow_cutout(in: CutoutOutput) {
    let alpha = textureSample(base_texture, material_sampler, in.uv).a * in.alpha;
    // Полупрозрачные материалы отбрасывают тень там, где они скорее непрозрачны.
    let cutoff = select(material.factors.z, 0.5, material.flags.w > 1.5);
    if (alpha < cutoff) {
        discard;
    }
}

const PI: f32 = 3.14159265;

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn geometry_schlick(n_dot_x: f32, k: f32) -> f32 {
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

// Отражённый свет от одного источника (уже умножен на косинус угла падения).
fn brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - v_dot_h, 5.0);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let specular = distribution_ggx(n_dot_h, roughness)
        * geometry_schlick(n_dot_l, k) * geometry_schlick(n_dot_v, k)
        * fresnel / (4.0 * n_dot_l * n_dot_v + 1e-4);
    let diffuse = (1.0 - fresnel) * (1.0 - metallic) * albedo / PI;
    return (diffuse + specular) * n_dot_l;
}

// Доля солнечного света, дошедшая до точки: 1 — на свету, 0 — в тени. PCF 3×3,
// к краю освещённой солнцем области тень плавно исчезает.
fn sun_visibility(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    if (globals.shadow_params.x < 0.5) {
        return 1.0;
    }
    // Сдвиг вдоль нормали убирает «акне», не отрывая тень от объекта.
    let clip = globals.light_view_proj * vec4<f32>(world_position + n * globals.shadow_params.z, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let depth = clip.z - globals.shadow_params.w;
    let texel = globals.shadow_params.y;
    var sum = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, depth);
        }
    }
    let edge = max(max(abs(clip.x), abs(clip.y)), clip.z);
    return mix(sum / 9.0, 1.0, smoothstep(0.85, 1.0, edge));
}

// Цвет материала: UV меша или проекция текстуры по трём осям в мировых координатах
// с шагом `tile` метров — стены любого размера получают одинаковый масштаб кладки.
// Обе выборки делаются всегда, чтобы производные считались в едином потоке управления.
fn material_color(in: VertexOutput, n: vec3<f32>, uv: vec2<f32>) -> vec4<f32> {
    let by_uv = textureSample(base_texture, material_sampler, uv);
    let tile = in.params.z;
    let p = in.world_position / max(tile, 1e-3);
    var weight = pow(abs(n), vec3<f32>(4.0));
    weight /= (weight.x + weight.y + weight.z);
    // Ось V в текстурах смотрит вниз: на вертикальных плоскостях переворачиваем.
    let along_x = textureSample(base_texture, material_sampler, vec2<f32>(p.z, -p.y));
    let along_y = textureSample(base_texture, material_sampler, p.xz);
    let along_z = textureSample(base_texture, material_sampler, vec2<f32>(p.x, -p.y));
    let triplanar = along_x * weight.x + along_y * weight.y + along_z * weight.z;
    return select(by_uv, triplanar, tile > 0.0);
}

// Нормаль с учётом карты нормалей (OpenGL-соглашение: зелёный — «вверх» по картинке).
// Без карты (сила 0) — нормаль вершины как есть.
fn mapped_normal(in: VertexOutput, n: vec3<f32>, uv: vec2<f32>, side: f32) -> vec3<f32> {
    let texel = textureSample(normal_texture, material_sampler, uv).xyz * 2.0 - 1.0;
    let strength = material.factors.x;
    let t_raw = in.tangent.xyz - n * dot(n, in.tangent.xyz);
    let t = t_raw / max(length(t_raw), 1e-6);
    let b = cross(n, t) * select(1.0, -1.0, in.tangent.w < 0.0) * side;
    let mapped = normalize(t * texel.x * strength + b * texel.y * strength + n * max(texel.z, 1e-3));
    return select(n, mapped, strength > 0.0);
}

// Сглаженные линии сетки через каждые `cell` единиц.
fn grid_lines(uv: vec2<f32>, cell: f32, width: f32) -> f32 {
    let p = uv / cell;
    let d = abs(fract(p - 0.5) - 0.5) / max(fwidth(p), vec2<f32>(1e-4));
    return 1.0 - clamp(min(d.x, d.y) - width, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Обратная сторона двустороннего материала освещается со своей стороны.
    let side = select(-1.0, 1.0, front);
    let geometric = normalize(in.normal) * side;
    let uv = in.uv * material.uv.xy + material.uv.zw;
    let texel = material_color(in, geometric, uv);
    let n = mapped_normal(in, geometric, uv, side);
    let surface = textureSample(metallic_roughness_texture, material_sampler, uv);
    let glow = textureSample(emissive_texture, material_sampler, uv);
    let occlusion = mix(1.0, textureSample(occlusion_texture, material_sampler, uv).r, material.factors.y);

    // Dev-сетка в метрах — проекция на главную ось грани, как в редакторах уровней.
    let axis = abs(n);
    var grid_uv = in.world_position.xy;
    if (axis.x > axis.y && axis.x > axis.z) {
        grid_uv = in.world_position.zy;
    } else if (axis.y > axis.z) {
        grid_uv = in.world_position.xz;
    }
    let grid = 0.16 * grid_lines(grid_uv, 0.25, 0.0) + 0.34 * grid_lines(grid_uv, 1.0, 0.5);

    let albedo = in.base_color.rgb * texel.rgb * (1.0 - grid * in.params.w);
    let alpha = in.base_color.a * texel.a;
    let metallic = clamp(in.params.x * surface.b, 0.0, 1.0);
    let roughness = clamp(in.params.y * surface.g, 0.045, 1.0);
    let v = normalize(globals.camera_position.xyz - in.world_position);
    let emitted = in.emissive.rgb * glow.rgb;

    // Солнце (тень — по геометрической нормали: карта нормалей не сдвигает тени).
    let to_sun = -normalize(globals.sun_direction.xyz);
    let shadow = select(1.0, sun_visibility(in.world_position, geometric), material.flags.y > 0.5);
    var color = brdf(n, v, to_sun, albedo, metallic, roughness)
        * globals.sun_color.rgb * shadow;

    // Точечные источники с плавным затуханием до нуля на границе дальности.
    let point_count = i32(globals.counts.x);
    for (var i = 0; i < point_count; i += 1) {
        let light = globals.points[i];
        let to_light = light.position_range.xyz - in.world_position;
        let distance = length(to_light);
        let reach = clamp(1.0 - distance / max(light.position_range.w, 1e-3), 0.0, 1.0);
        let falloff = reach * reach / (1.0 + distance * distance);
        color += brdf(n, v, to_light / max(distance, 1e-4), albedo, metallic, roughness)
            * light.color.rgb * falloff;
    }

    // Ambient от неба; у гладких и металлических поверхностей — отражение неба.
    // Ambient occlusion гасит и то, и другое в щелях.
    color += ambient_light(n) * albedo * (1.0 - metallic) * occlusion;
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let reflection = sky_color(reflect(-v, n)) * globals.sky_zenith.w * 2.0;
    color += reflection * f0 * (1.0 - roughness * 0.75) * occlusion;

    // Без освещения: цвет материала как есть.
    color = select(color, albedo, material.flags.x > 0.5);

    // Собственное свечение — в HDR, поэтому даёт блум.
    color += emitted;

    // Вырез: после всех выборок текстур (производные считаются в едином потоке).
    if (material.flags.w > 0.5 && material.flags.w < 1.5 && alpha < material.factors.z) {
        discard;
    }
    let out_alpha = select(1.0, alpha, material.flags.w > 1.5);
    return vec4<f32>(apply_fog(color, in.world_position), out_alpha);
}
