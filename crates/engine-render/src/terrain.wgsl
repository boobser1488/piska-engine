// Рельеф (перенос шейдера образца): слои текстур, смешанные по высоте и уклону
// (трава, земля, скала, песок, снег; пустыня — песок и тёплые скалы), раскраска
// кистью поверх, смешивание «по высоте текселя» (чёткие природные границы),
// трипланарная скала на обрывах, деталь mod2x вблизи, крупный оттенок против
// повторов, запечённый AO. Свет — как у мешей: GGX от солнца с картой теней,
// точечные источники, ambient неба и туман из common.wgsl.
// Перед этим файлом приклеиваются common.wgsl и shadow.wgsl.

// Карта теней и `sun_shadow` — shadow.wgsl (приклеивается перед этим файлом).

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
    // Разброс нормалей внутри пикселя по карте деталей (Токсвиг): усреднённая мипами
    // нормаль короче единицы — значит, в пикселе были разные наклоны.
    variance: f32,
};

// Дисперсия наклонов из длины отфильтрованной нормали карты деталей (сила 0.7).
fn toksvig(t: vec3<f32>) -> f32 {
    let len = clamp(length(t), 1e-3, 1.0);
    // ×2: анизотропная выборка берёт мип по короткой оси следа и недооценивает разброс.
    return clamp((1.0 - len) / len, 0.0, 1.0) * 0.98;
}

// Производные мировой позиции — один раз в равномерном потоке управления. Все слои
// выбираются с явными градиентами (textureSampleGrad), поэтому лишние слои можно
// пропускать обычным `if`: пиксель платит за один-два слоя.
struct Grads {
    dx: vec3<f32>,
    dy: vec3<f32>,
    // Доля второй, повёрнутой выборки плитки (0..1, плавный шум): против повторов.
    alt: f32,
};

// Вторая выборка плитки: повёрнута на 37° и крупнее в 1.3 раза. Смешанная с первой по
// плавному шуму, она прячет сетку повторов (ровные «волны» плиток вдали).
const ALT_C: f32 = 0.6156; // cos(38°) / 1.28
const ALT_S: f32 = 0.4810; // sin(38°) / 1.28
fn alt_uv(uv: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ALT_C * uv.x - ALT_S * uv.y, ALT_S * uv.x + ALT_C * uv.y) + vec2<f32>(0.37, 0.71);
}
fn alt_dir(d: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ALT_C * d.x - ALT_S * d.y, ALT_S * d.x + ALT_C * d.y);
}

// Альбедо плитки с антиповтором (в местах, где вторая выборка не нужна, — одна).
// Две выборки — только в полосе перехода; где одна из них весит меньше 0.5 %, берётся
// одна (разница в цвете меньше 0.5 %, а выборок вдвое меньше — это самая дорогая часть
// шейдера рельефа).
fn sample_albedo(layer: i32, uv: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>, alt: f32) -> vec4<f32> {
    if (alt >= ALT_ONLY) {
        return textureSampleGrad(albedo_tex, tex_samp, alt_uv(uv), layer, alt_dir(gx), alt_dir(gy));
    }
    var a = textureSampleGrad(albedo_tex, tex_samp, uv, layer, gx, gy);
    if (alt > ALT_NONE) {
        let b = textureSampleGrad(albedo_tex, tex_samp, alt_uv(uv), layer, alt_dir(gx), alt_dir(gy));
        a = mix(a, b, alt);
    }
    return a;
}

// Нормаль карты слоя (в осях плитки) с той же логикой одной / двух выборок.
fn sample_normal(layer: i32, uv: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>, alt: f32) -> vec3<f32> {
    var t = vec3<f32>(0.0, 0.0, 1.0);
    if (alt < ALT_ONLY) {
        t = textureSampleGrad(normal_tex, tex_samp, uv, layer, gx, gy).xyz * 2.0 - 1.0;
    }
    if (alt > ALT_NONE) {
        // Нормаль из повёрнутой выборки поворачиваем обратно.
        let t2 = textureSampleGrad(normal_tex, tex_samp, alt_uv(uv), layer, alt_dir(gx), alt_dir(gy)).xyz * 2.0 - 1.0;
        let back = vec3<f32>(vec2<f32>(ALT_C * t2.x + ALT_S * t2.y, -ALT_S * t2.x + ALT_C * t2.y) * 1.28, t2.z);
        t = select(mix(t, back, alt), back, alt >= ALT_ONLY);
    }
    return t;
}

const ALT_NONE: f32 = 0.01;
const ALT_ONLY: f32 = 0.995;
// Проекция трипланара с меньшим весом не выбирается.
const TRI_MIN_WEIGHT: f32 = 0.005;

fn tint(layer: i32) -> vec3<f32> {
    return terrain.tints[layer].rgb;
}

// Плоская (сверху) проекция слоя, нормаль текстуры добавляется к нормали поверхности.
fn planar(layer: i32, p: vec3<f32>, scale: f32, gr: Grads, n: vec3<f32>) -> Layer {
    let uv = p.xz * scale;
    let gx = gr.dx.xz * scale;
    let gy = gr.dy.xz * scale;
    let a = sample_albedo(layer, uv, gx, gy, gr.alt);
    let t = sample_normal(layer, uv, gx, gy, gr.alt);
    var l: Layer;
    l.albedo = a.rgb * tint(layer);
    l.height = a.a;
    l.normal = normalize(n + vec3<f32>(t.x, 0.0, t.y) * 0.7);
    l.variance = toksvig(t);
    return l;
}

// Трипланарная проекция для обрывов — без растяжения на вертикальных стенах.
fn triplanar(layer: i32, wp: vec3<f32>, scale: f32, gr: Grads, n: vec3<f32>) -> Layer {
    var w = pow(abs(n), vec3<f32>(4.0));
    w /= (w.x + w.y + w.z);
    let p = wp * scale;
    let dx = gr.dx * scale;
    let dy = gr.dy * scale;
    // Проекция с весом меньше 0.5 % не выбирается (на крутом склоне обычно работают одна-две).
    var ax = vec4<f32>(0.0);
    var ay = vec4<f32>(0.0);
    var az = vec4<f32>(0.0);
    var tx = vec3<f32>(0.0, 0.0, 1.0);
    var ty = vec3<f32>(0.0, 0.0, 1.0);
    var tz = vec3<f32>(0.0, 0.0, 1.0);
    if (w.x > TRI_MIN_WEIGHT) {
        ax = sample_albedo(layer, p.zy, dx.zy, dy.zy, gr.alt);
        tx = textureSampleGrad(normal_tex, tex_samp, p.zy, layer, dx.zy, dy.zy).xyz * 2.0 - 1.0;
    }
    if (w.y > TRI_MIN_WEIGHT) {
        ay = sample_albedo(layer, p.xz, dx.xz, dy.xz, gr.alt);
        ty = textureSampleGrad(normal_tex, tex_samp, p.xz, layer, dx.xz, dy.xz).xyz * 2.0 - 1.0;
    }
    if (w.z > TRI_MIN_WEIGHT) {
        az = sample_albedo(layer, p.xy, dx.xy, dy.xy, gr.alt);
        tz = textureSampleGrad(normal_tex, tex_samp, p.xy, layer, dx.xy, dy.xy).xyz * 2.0 - 1.0;
    }
    let nx = vec3<f32>(0.0, tx.y, tx.x) * sign(n.x);
    let ny = vec3<f32>(ty.x, 0.0, ty.y);
    let nz = vec3<f32>(tz.x, tz.y, 0.0) * sign(n.z);
    let a = ax * w.x + ay * w.y + az * w.z;
    var l: Layer;
    l.albedo = a.rgb * tint(layer);
    l.height = a.a;
    l.normal = normalize(n + nx * w.x + ny * w.y + nz * w.z);
    l.variance = toksvig(tx) * w.x + toksvig(ty) * w.y + toksvig(tz) * w.z;
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
    o.variance = mix(base.variance, top.variance, f);
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
    let h = safe_half(v, l);
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
    // Где брать вторую выборку плиток: плавный шум с периодом ~150 м.
    let alt_scale = s / 151.0;
    let alt_noise = textureSampleGrad(albedo_tex, tex_samp, wp.xz * alt_scale + vec2<f32>(0.5, 0.25), L_DETAIL, gr.dx.xz * alt_scale, gr.dy.xz * alt_scale).a;
    gr.alt = smoothstep(0.3, 0.7, alt_noise);
    // Пресет Low: без второй выборки против повторов (самая дорогая часть шейдера).
    let cheap = globals.sky_night.z > 0.5;
    if (cheap) {
        gr.alt = 0.0;
    }

    var m = planar(L_GRASS, wp, 0.25 * s, gr, n);
    m.albedo *= mix(vec3<f32>(1.0), vec3<f32>(1.12, 1.05, 0.8), smoothstep(0.55, 0.85, v));

    // Пустыня: песок вместо травы, тёплые скалы и земля, без снега.
    if (desert > 0.001) {
        var dune = planar(L_SAND, wp, 0.25 * s, gr, n);
        dune.albedo *= mix(vec3<f32>(0.92, 0.86, 0.78), vec3<f32>(1.05, 1.0, 0.92), v);
        m.albedo = mix(m.albedo, dune.albedo, desert);
        m.height = mix(m.height, dune.height, desert);
        m.normal = normalize(mix(m.normal, dune.normal, desert));
        m.variance = mix(m.variance, dune.variance, desert);
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
        // Пресет Low: скала — плоской проекцией сверху, а не трипланаром.
        var rock: Layer;
        if (cheap) {
            rock = planar(L_ROCK, wp, 0.125 * s, gr, n);
        } else {
            rock = triplanar(L_ROCK, wp, 0.125 * s, gr, n);
        }
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
        mixed.variance = 0.0;
        for (var k = 0; k < 5; k += 1) {
            let wk = painted[k];
            if (wk > 0.004) {
                let l = sample_layer(k, wp, s, gr, n);
                mixed.albedo += l.albedo * wk;
                mixed.height += l.height * wk;
                mixed.normal += l.normal * wk;
                mixed.variance += l.variance * wk;
            }
        }
        let inv = 1.0 / paint_total;
        mixed.albedo *= inv;
        mixed.height *= inv;
        mixed.variance *= inv;
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

    let ao = i.ao;
    let view = normalize(globals.camera_position.xyz - i.world);
    // Нормаль с картой деталей не смотрит от камеры (иначе против низкого солнца блик
    // по Френелю зажигает одиночные белые пиксели на склонах и у берега).
    let nn = facing_normal(m.normal, view);
    // Блестят только мокрый берег и снег; трава, земля и скала матовые. Шероховатость
    // растёт там, где карта деталей в пикселе разбросана (Токсвиг, см. `toksvig`):
    // вдали мелкие наклоны не дают мерцающих точек. Антиалиасинг по производным
    // нормали не используется — после ветвлений по слоям он стоил ~2.5 мс кадра
    // на GTX 1050 Ti.
    let wet = 1.0 - smoothstep(sea, sea + 2.0, h);
    let snowy = smoothstep(snow_line, snow_line + 30.0, h) * (1.0 - desert);
    let base_roughness = clamp(0.92 - 0.55 * wet - 0.4 * snowy, 0.3, 1.0);
    let toksvig_roughness = sqrt(sqrt(min(pow(base_roughness, 4.0) + m.variance, 1.0)));
    let roughness = toksvig_roughness;

    let to_sun = -normalize(globals.sun_direction.xyz);
    var shadow = 1.0;
    if (dot(n, to_sun) > 0.0) {
        shadow = sun_shadow(i.world, n);
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
    // Лампы сверх четырёх, прожекторы и тени ламп (lights.wgsl, вариант с лампами).
    if (EXTENDED_LIGHTS) {
        let cell = light_cell(i.clip, i.world);
        for (var k = 0u; k < 16u; k += 1u) {
            let index = cell_light(cell, k);
            if (index == 255u) {
                break;
            }
            let lamp = local_light(index, i.world, n, true);
            color += t_brdf(nn, view, lamp.direction, albedo, roughness) * lamp.radiance * ao;
        }
    }

    color += fill_ambient(nn, ambient_light(nn)) * albedo * ao;
    return vec4<f32>(apply_fog(color, i.world), 1.0);
}
