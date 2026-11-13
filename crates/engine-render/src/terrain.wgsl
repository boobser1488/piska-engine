// Рельеф (перенос шейдера образца): слои текстур, смешанные по высоте и уклону
// (трава, земля, скала, песок, снег; пустыня — песок и тёплые скалы), раскраска
// кистью поверх, смешивание «по высоте текселя» (чёткие природные границы),
// трипланарная скала на обрывах, деталь mod2x вблизи, крупный оттенок против
// повторов, запечённый AO. Свет — как у мешей: GGX от солнца с картой теней,
// точечные источники, ambient неба и туман из common.wgsl.
// Перед этим файлом приклеивается common.wgsl.

@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

@group(1) @binding(0) var albedo_tex: texture_2d_array<f32>;
@group(1) @binding(1) var normal_tex: texture_2d_array<f32>;
@group(1) @binding(2) var tex_samp: sampler;

struct TerrainParams {
    origin: vec4<f32>,  // xyz — угол рельефа в мире, w — масштаб текстур
    params: vec4<f32>,  // x — уровень моря, y — пустыня, z — высота снега
    tints: array<vec4<f32>, 5>,
};
@group(1) @binding(3) var<uniform> terrain: TerrainParams;

const L_GRASS: i32 = 0;
const L_DIRT: i32 = 1;
const L_ROCK: i32 = 2;
const L_SAND: i32 = 3;
const L_SNOW: i32 = 4;
const L_DETAIL: i32 = 5;
const T_PI: f32 = 3.14159265;

struct VIn {
    @location(0) position: vec3<f32>,
    @location(1) normal_var: vec4<f32>, // snorm8x4
    @location(2) ao_snow: vec4<f32>,    // unorm8x4: AO, снег, опускание юбки (×127.5 м)
    @location(3) paint: vec4<f32>,      // unorm8x4
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) local: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) variation: f32,
    @location(4) ao: f32,
    @location(5) paint: vec4<f32>,
    @location(6) paint_snow: f32,
    @location(7) steep: f32,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    // Юбка опускается здесь: материал у неё — как у края, а не по её высоте.
    let world = v.position + terrain.origin.xyz - vec3<f32>(0.0, v.ao_snow.z * 127.5, 0.0);
    var o: VOut;
    o.clip = globals.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.local = v.position;
    o.normal = v.normal_var.xyz;
    o.variation = v.normal_var.w * 0.5 + 0.5;
    o.ao = v.ao_snow.x;
    o.paint = v.paint;
    o.paint_snow = v.ao_snow.y;
    o.steep = v.ao_snow.w;
    return o;
}

// Только глубина — для карты теней солнца.
@vertex
fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    let world = v.position + terrain.origin.xyz - vec3<f32>(0.0, v.ao_snow.z * 127.5, 0.0);
    return globals.light_view_proj * vec4<f32>(world, 1.0);
}

struct Layer {
    albedo: vec3<f32>,
    height: f32,
    normal: vec3<f32>, // в мире
};

// Производные мировой позиции — один раз в равномерном потоке управления. Все слои
// выбираются с явными градиентами (textureSampleGrad), поэтому лишние слои можно
// пропускать обычным `if`: пиксель платит за один-два слоя.
struct Grads {
    dx: vec3<f32>,
    dy: vec3<f32>,
};

fn tint(layer: i32) -> vec3<f32> {
    return terrain.tints[layer].rgb;
}

// Плоская (сверху) проекция слоя, нормаль текстуры добавляется к нормали поверхности.
fn planar(layer: i32, p: vec3<f32>, scale: f32, gr: Grads, n: vec3<f32>) -> Layer {
    let uv = p.xz * scale;
    let gx = gr.dx.xz * scale;
    let gy = gr.dy.xz * scale;
    let a = textureSampleGrad(albedo_tex, tex_samp, uv, layer, gx, gy);
    let t = textureSampleGrad(normal_tex, tex_samp, uv, layer, gx, gy).xyz * 2.0 - 1.0;
    var l: Layer;
    l.albedo = a.rgb * tint(layer);
    l.height = a.a;
    l.normal = normalize(n + vec3<f32>(t.x, 0.0, t.y) * 0.7);
    return l;
}

// Трипланарная проекция для обрывов — без растяжения на вертикальных стенах.
fn triplanar(layer: i32, wp: vec3<f32>, scale: f32, gr: Grads, n: vec3<f32>) -> Layer {
    var w = pow(abs(n), vec3<f32>(4.0));
    w /= (w.x + w.y + w.z);
    let p = wp * scale;
    let dx = gr.dx * scale;
    let dy = gr.dy * scale;
    let ax = textureSampleGrad(albedo_tex, tex_samp, p.zy, layer, dx.zy, dy.zy);
    let ay = textureSampleGrad(albedo_tex, tex_samp, p.xz, layer, dx.xz, dy.xz);
    let az = textureSampleGrad(albedo_tex, tex_samp, p.xy, layer, dx.xy, dy.xy);
    let tx = textureSampleGrad(normal_tex, tex_samp, p.zy, layer, dx.zy, dy.zy).xyz * 2.0 - 1.0;
    let ty = textureSampleGrad(normal_tex, tex_samp, p.xz, layer, dx.xz, dy.xz).xyz * 2.0 - 1.0;
    let tz = textureSampleGrad(normal_tex, tex_samp, p.xy, layer, dx.xy, dy.xy).xyz * 2.0 - 1.0;
    let nx = vec3<f32>(0.0, tx.y, tx.x) * sign(n.x);
    let ny = vec3<f32>(ty.x, 0.0, ty.y);
    let nz = vec3<f32>(tz.x, tz.y, 0.0) * sign(n.z);
    let a = ax * w.x + ay * w.y + az * w.z;
    var l: Layer;
    l.albedo = a.rgb * tint(layer);
    l.height = a.a;
    l.normal = normalize(n + nx * w.x + ny * w.y + nz * w.z);
    return l;
}

// Смешивание по высоте текселя (Source «blend modulate»): у границы побеждает слой
// с более высоким текселем — чёткие природные края вместо размытого перехода.
fn blend(base: Layer, top: Layer, t: f32) -> Layer {
    let f = smoothstep(-0.12, 0.12, t * 2.0 - 1.0 + (top.height - base.height) * 0.9);
    var o: Layer;
    o.albedo = mix(base.albedo, top.albedo, f);
    o.height = mix(base.height, top.height, f);
    o.normal = normalize(mix(base.normal, top.normal, f));
    return o;
}

// Слой с номером `layer` — той проекцией, что у автоматического слоя.
fn sample_layer(layer: i32, wp: vec3<f32>, s: f32, gr: Grads, n: vec3<f32>) -> Layer {
    if (layer == L_ROCK) {
        return triplanar(L_ROCK, wp, 0.125 * s, gr, n);
    }
    if (layer == L_SNOW) {
        return planar(L_SNOW, wp, 0.125 * s, gr, n);
    }
    return planar(layer, wp, 0.25 * s, gr, n);
}

fn t_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (T_PI * d * d);
}

fn t_schlick(n_dot_x: f32, k: f32) -> f32 {
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

// Та же модель отражения, что у мешей (GGX, неметалл).
fn t_brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, roughness: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);
    let f0 = vec3<f32>(0.04);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - v_dot_h, 5.0);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let specular = t_ggx(n_dot_h, roughness) * t_schlick(n_dot_l, k) * t_schlick(n_dot_v, k)
        * fresnel / (4.0 * n_dot_l * n_dot_v + 1e-4);
    let diffuse = (1.0 - fresnel) * albedo / T_PI;
    return (diffuse + specular) * n_dot_l;
}

// Доля солнечного света (PCF 3×3), как у мешей.
fn t_sun_visibility(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    if (globals.shadow_params.x < 0.5) {
        return 1.0;
    }
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

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    // Крутизна для материала: по нормали или по самому крутому одностороннему уклону
    // вершины (острые гребни не «плоские»).
    let slope = max(1.0 - n.y, i.steep);
    let h = i.local.y;
    let sea = terrain.params.x;
    let desert = terrain.params.y;
    let snow_base = terrain.params.z;
    let s = 1.0 / max(terrain.origin.w, 1e-3);
    let v = i.variation;
    let dist = length(i.world - globals.camera_position.xyz);

    // Текстуры — по локальной позиции: сдвиг объекта не «протаскивает» узор.
    let wp = i.local;
    var gr: Grads;
    gr.dx = dpdx(wp);
    gr.dy = dpdy(wp);

    // Крупный оттенок ломает повторяемость.
    let macro_scale = s / 64.0;
    let macro_tint = textureSampleGrad(albedo_tex, tex_samp, wp.xz * macro_scale, L_DETAIL, gr.dx.xz * macro_scale, gr.dy.xz * macro_scale).r * 2.0;

    var m = planar(L_GRASS, wp, 0.25 * s, gr, n);
    m.albedo *= mix(vec3<f32>(1.0), vec3<f32>(1.12, 1.05, 0.8), smoothstep(0.55, 0.85, v));

    // Пустыня: песок вместо травы, тёплые скалы и земля, без снега.
    if (desert > 0.001) {
        var dune = planar(L_SAND, wp, 0.25 * s, gr, n);
        dune.albedo *= mix(vec3<f32>(0.92, 0.86, 0.78), vec3<f32>(1.05, 1.0, 0.92), v);
        m.albedo = mix(m.albedo, dune.albedo, desert);
        m.height = mix(m.height, dune.height, desert);
        m.normal = normalize(mix(m.normal, dune.normal, desert));
    }
    let warm = mix(vec3<f32>(1.0), vec3<f32>(1.22, 0.96, 0.72), desert);

    // Каждый слой выбирается только там, где у него есть вес.
    let dirt_amt = max(smoothstep(0.14, 0.26, slope), smoothstep(0.72, 0.9, v) * 0.8);
    if (dirt_amt > 0.001) {
        var dirt = planar(L_DIRT, wp, 0.25 * s, gr, n);
        dirt.albedo *= warm;
        m = blend(m, dirt, dirt_amt);
    }
    let sand_amt = 1.0 - smoothstep(sea + 0.5, sea + 2.5, h + v * 1.5);
    if (sand_amt > 0.001) {
        m = blend(m, planar(L_SAND, wp, 0.25 * s, gr, n), sand_amt);
    }
    let rock_amt = smoothstep(0.30, 0.42, slope + max(h - (snow_base - 40.0), 0.0) * 0.0012);
    if (rock_amt > 0.001) {
        var rock = triplanar(L_ROCK, wp, 0.125 * s, gr, n);
        rock.albedo *= warm;
        m = blend(m, rock, rock_amt);
    }
    let snow_line = snow_base + v * 70.0;
    let snow_amt = smoothstep(snow_line, snow_line + 30.0, h) * (1.0 - smoothstep(0.5, 0.7, slope)) * (1.0 - desert);
    if (snow_amt > 0.001) {
        m = blend(m, planar(L_SNOW, wp, 0.125 * s, gr, n), snow_amt);
    }

    // Раскраска кистью поверх автоматических слоёв.
    var painted = array<f32, 5>(i.paint.x, i.paint.y, i.paint.z, i.paint.w, i.paint_snow);
    let paint_total = i.paint.x + i.paint.y + i.paint.z + i.paint.w + i.paint_snow;
    if (paint_total > 0.004) {
        var mixed: Layer;
        mixed.albedo = vec3<f32>(0.0);
        mixed.height = 0.0;
        mixed.normal = vec3<f32>(0.0);
        for (var k = 0; k < 5; k += 1) {
            let wk = painted[k];
            if (wk > 0.004) {
                let l = sample_layer(k, wp, s, gr, n);
                mixed.albedo += l.albedo * wk;
                mixed.height += l.height * wk;
                mixed.normal += l.normal * wk;
            }
        }
        let inv = 1.0 / paint_total;
        mixed.albedo *= inv;
        mixed.height *= inv;
        mixed.normal = normalize(mixed.normal + n * 1e-3);
        m = blend(m, mixed, clamp(paint_total, 0.0, 1.0));
    }

    // Деталь mod2x вблизи, как detail-текстуры Source.
    var albedo = m.albedo;
    let detail_amt = 1.0 - smoothstep(15.0, 60.0, dist);
    if (detail_amt > 0.0) {
        let detail = textureSampleGrad(albedo_tex, tex_samp, wp.xz * s, L_DETAIL, gr.dx.xz * s, gr.dy.xz * s).r * 2.0;
        albedo *= mix(1.0, detail, detail_amt);
    }
    albedo *= mix(1.0, macro_tint, 0.5);
    // Мокрое тёмное дно ниже уровня моря.
    albedo = mix(albedo * vec3<f32>(0.45, 0.6, 0.6), albedo, smoothstep(sea - 4.0, sea - 0.5, h));

    let nn = m.normal;
    let ao = i.ao;
    let view = normalize(globals.camera_position.xyz - i.world);
    // Блестят только мокрый берег и снег; трава, земля и скала матовые.
    let wet = 1.0 - smoothstep(sea, sea + 2.0, h);
    let snowy = smoothstep(snow_line, snow_line + 30.0, h) * (1.0 - desert);
    let roughness = clamp(0.92 - 0.55 * wet - 0.4 * snowy, 0.3, 1.0);

    let to_sun = -normalize(globals.sun_direction.xyz);
    var shadow = 1.0;
    if (dot(n, to_sun) > 0.0) {
        shadow = t_sun_visibility(i.world, n);
    }
    var color = t_brdf(nn, view, to_sun, albedo, roughness) * globals.sun_color.rgb * shadow * mix(0.6, 1.0, ao);

    let point_count = i32(globals.counts.x);
    for (var k = 0; k < point_count; k += 1) {
        let light = globals.points[k];
        let to_light = light.position_range.xyz - i.world;
        let d = length(to_light);
        let reach = clamp(1.0 - d / max(light.position_range.w, 1e-3), 0.0, 1.0);
        let falloff = reach * reach / (1.0 + d * d);
        color += t_brdf(nn, view, to_light / max(d, 1e-4), albedo, roughness) * light.color.rgb * falloff * ao;
    }

    color += ambient_light(nn) * albedo * ao;
    return vec4<f32>(apply_fog(color, i.world), 1.0);
}
