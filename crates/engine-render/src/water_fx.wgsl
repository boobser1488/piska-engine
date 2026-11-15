// Вода с эффектами (компонент `WaterEffects`, water_fx.rs). Перед файлом приклеиваются
// common.wgsl и water_scene.wgsl (кадр и глубина под водой) или water_noscene.wgsl
// (WebGL2: без них). Квадрат воды — как в water.wgsl.

struct WaterFx {
    center: vec4<f32>,  // x, уровень, z, половина стороны (0 — следовать за камерой)
    deep: vec4<f32>,    // rgb — цвет глубины, a — непрозрачность сверху
    wave: vec4<f32>,    // x — время, y — высота волн, z — скорость
    shallow: vec4<f32>, // rgb — мелкая вода, a — прозрачность (м)
    foam: vec4<f32>,    // x — пена, y — ширина пены (м), z — мелкие волны, w — их размер (м)
    look: vec4<f32>,    // x — преломление, y — блик, z — отражения сцены, w — плоский стиль
    detail: vec4<f32>,  // x — 0 волны и блик, 1 — + глубина и пена, 2 — + отражения сцены
};
@group(1) @binding(0) var<uniform> water: WaterFx;

struct WOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> WOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    var center = water.center.xz;
    var half = water.center.w;
    if (half <= 0.0) {
        center = globals.camera_position.xz;
        half = 6000.0;
    }
    let c = corners[vi] * half + center;
    let world = vec3<f32>(c.x, water.center.y, c.y);
    var o: WOut;
    o.clip = globals.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    return o;
}

fn wave(p: vec2<f32>, dir: vec2<f32>, freq: f32, amp: f32, speed: f32, t: f32) -> vec2<f32> {
    let d = normalize(dir);
    let ph = dot(d, p) * freq + t * speed;
    return d * (cos(ph) * amp * freq);
}

fn w_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Шум значений и его градиент (аналитически) — «карта нормалей» мелких волн.
fn noise_grad(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    let a = w_hash(i);
    let b = w_hash(i + vec2<f32>(1.0, 0.0));
    let c = w_hash(i + vec2<f32>(0.0, 1.0));
    let d = w_hash(i + vec2<f32>(1.0, 1.0));
    let value = a + (b - a) * u.x + (c - a) * u.y + (a - b - c + d) * u.x * u.y;
    let grad = du * vec2<f32>(b - a + (a - b - c + d) * u.y, c - a + (a - b - c + d) * u.x);
    return vec3<f32>(value, grad);
}

@fragment
fn fs_main(i: WOut) -> @location(0) vec4<f32> {
    let rel = i.world - globals.camera_position.xyz;
    let dist = length(rel);
    let p = i.world.xz;
    let t = water.wave.x * water.wave.z;
    let stylized = water.look.w > 0.5;

    // Крупные волны (как у воды без эффектов) и мелкая рябь — два слоя шума навстречу.
    var grad = vec2<f32>(0.0);
    grad += wave(p, vec2<f32>(1.0, 0.3), 0.08, 0.6, 1.1, t);
    grad += wave(p, vec2<f32>(-0.4, 1.0), 0.13, 0.35, 1.5, t);
    grad += wave(p, vec2<f32>(0.7, -0.8), 0.31, 0.12, 2.3, t);
    grad += wave(p, vec2<f32>(-0.9, -0.2), 0.57, 0.05, 3.1, t);
    let fade = water.wave.y / (1.0 + dist * 0.004);
    grad *= fade;
    let scale = water.foam.w;
    let r1 = noise_grad(p / scale + vec2<f32>(t * 0.09, t * 0.05));
    let r2 = noise_grad(p / (scale * 0.53) - vec2<f32>(t * 0.07, -t * 0.11));
    let ripple_fade = water.foam.z / (1.0 + dist * 0.03);
    grad += (r1.yz / scale + r2.yz / (scale * 0.53)) * 0.35 * ripple_fade * water.wave.y;
    var n = normalize(vec3<f32>(-grad.x, 1.0, -grad.y));
    if (stylized) {
        // Гранёные волны: одна нормаль на треугольник сетки 2.5 м (как у низкополигональной воды).
        let cell_size = 2.5;
        let q = p / cell_size;
        let cell = floor(q);
        let upper = select(0.0, 1.0, fract(q.x) + fract(q.y) > 1.0);
        let id = cell * 2.0 + vec2<f32>(upper, 0.0);
        let phase = w_hash(id) * 6.2832 + t * (0.6 + w_hash(id + 7.0) * 0.6);
        let tilt = 0.25 * water.wave.y / (1.0 + dist * 0.02);
        n = normalize(vec3<f32>(sin(phase) * tilt, 1.0, cos(phase * 1.3) * tilt));
    }

    let v = -rel / max(dist, 1e-4);
    let ndv = max(dot(n, v), 0.0);
    var fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    let r = reflect(-v, n);
    var reflection = sky_color(normalize(vec3<f32>(r.x, abs(r.y), r.z)));
    let sun = globals.sun_color.rgb;
    let to_sun = -normalize(globals.sun_direction.xyz);
    let deep = water.deep.rgb * (sun + globals.sky_zenith.rgb);
    var below = deep;
    var alpha = mix(water.deep.a, 1.0, fresnel);

    // Под водой: кадр и глубина (не в WebGL2 и не на пресете Low).
    let size = max(globals.cluster.xy, vec2<f32>(1.0));
    let screen = i.clip.xy / size;
    var shore = 1.0;
    if (WATER_SCENE && water.detail.x > 0.5) {
        let floor_world = scene_world(screen);
        let thickness = distance(floor_world, i.world);
        let height = max(i.world.y - floor_world.y, 0.0);
        // Преломление: сдвиг выборки по волнам, сильнее на толстом слое; если по сдвигу
        // попадаем на то, что над водой, — без сдвига.
        var uv = screen + n.xz * water.look.x * 0.04 * clamp(thickness * 0.5, 0.0, 1.0) / (1.0 + dist * 0.02);
        if (scene_world(uv).y > i.world.y) {
            uv = screen;
        }
        let behind = scene_color(uv);
        let refracted = distance(scene_world(uv), i.world);
        // Поглощение по толщине (красный гаснет быстрее) и свет, рассеянный в толще:
        // у берега — цвет мелкой воды, глубже — цвет глубины.
        let absorb = exp(-refracted / water.shallow.a * vec3<f32>(1.6, 1.0, 0.8));
        let lit = sun * max(to_sun.y, 0.0) * 0.35 + globals.sky_zenith.rgb * 0.6;
        let inscatter = mix(water.shallow.rgb * lit, deep, smoothstep(0.0, water.shallow.a * 2.5, refracted));
        below = behind * absorb + inscatter * (1.0 - absorb);
        alpha = 1.0;
        shore = height;
        // Отражения сцены по экрану (пресет High).
        if (water.detail.x > 1.5 && water.look.z > 0.0 && !stylized) {
            // Луч отражения — по сглаженной нормали: рябь не превращает отражения в кляксы.
            let calm = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), n, 0.3));
            let hit = scene_reflection(i.world, reflect(-v, calm));
            reflection = mix(reflection, hit.rgb, hit.a * water.look.z);
        }
    }
    if (stylized) {
        // Плоский стиль: непрозрачная вода полосами цвета (мелко — глубоко), отражение —
        // только мягкий тон неба. Дно не просвечивает — как у низкополигональной воды.
        let lit = sun * max(to_sun.y, 0.0) * 0.35 + globals.sky_zenith.rgb * 0.6;
        let band = floor(clamp(shore / max(water.shallow.a, 0.05), 0.0, 1.0) * 3.0) / 3.0;
        below = mix(water.shallow.rgb * lit * 1.2, deep, select(1.0, band, WATER_SCENE && water.detail.x > 0.5));
        // Грани различимы по свету солнца.
        below *= 0.75 + 0.5 * max(dot(n, to_sun), 0.0);
        fresnel = step(0.5, fresnel) * 0.3;
        alpha = 1.0;
    }
    var col = mix(below, reflection, fresnel);

    // Блик солнца: узкий, размером с диск солнца (`Sky`), с искрами на ряби.
    let cos_disc = select(0.9997, globals.sky_sun.y, globals.sky_sun.x > 0.5);
    let toward = max(dot(r, to_sun), 0.0);
    let sharp = 2.0 / max(1.0 - cos_disc, 1e-5);
    let glint = pow(toward, sharp) * 30.0 + pow(toward, 300.0) * 3.0;
    col += sun * glint * water.look.y * select(1.0, 0.4, stylized);

    // Пена: у берега и предметов, рваная по шуму.
    if (water.foam.x > 0.0 && shore < water.foam.y * 1.5) {
        let edge = pow(1.0 - smoothstep(0.0, water.foam.y, shore), 1.5);
        let pattern = noise_grad(p * 2.2 + vec2<f32>(t * 0.15, t * 0.1)).x * 0.6 + noise_grad(p * 5.3 - vec2<f32>(t * 0.2, 0.0)).x * 0.4;
        var foam = smoothstep(0.35, 0.65, pattern + edge * 0.6) * edge;
        if (stylized) {
            foam = step(0.5, edge + (pattern - 0.5) * 0.3);
        }
        let foam_light = sun * max(to_sun.y, 0.0) * 0.8 + globals.sky_zenith.rgb * 1.2 + vec3<f32>(0.05);
        col = mix(col, foam_light * 0.9, clamp(foam * water.foam.x, 0.0, 1.0));
        alpha = max(alpha, foam * water.foam.x);
    }

    col = apply_fog(col, i.world);
    return vec4<f32>(col, alpha);
}
