// Вода (перенос шейдера образца): большой квадрат на уровне воды; волны — только в
// нормали (сумма косинусов), отражение неба по Френелю, блик солнца, туман.
// Перед этим файлом приклеивается common.wgsl.

struct WaterParams {
    center: vec4<f32>, // x, уровень, z, половина стороны (0 — следовать за камерой)
    deep: vec4<f32>,   // rgb — цвет глубины, a — непрозрачность сверху
    wave: vec4<f32>,   // x — время, y — высота волн, z — скорость
};
@group(1) @binding(0) var<uniform> water: WaterParams;

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
        // Море до горизонта: квадрат следует за камерой.
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

@fragment
fn fs_main(i: WOut) -> @location(0) vec4<f32> {
    let rel = i.world - globals.camera_position.xyz;
    let dist = length(rel);
    let p = i.world.xz;
    let t = water.wave.x * water.wave.z;

    var grad = vec2<f32>(0.0);
    grad += wave(p, vec2<f32>(1.0, 0.3), 0.08, 0.6, 1.1, t);
    grad += wave(p, vec2<f32>(-0.4, 1.0), 0.13, 0.35, 1.5, t);
    grad += wave(p, vec2<f32>(0.7, -0.8), 0.31, 0.12, 2.3, t);
    grad += wave(p, vec2<f32>(-0.9, -0.2), 0.57, 0.05, 3.1, t);
    // Вдали волны гаснут: без мерцания.
    let fade = water.wave.y / (1.0 + dist * 0.004);
    let n = normalize(vec3<f32>(-grad.x * fade, 1.0, -grad.y * fade));

    let v = -rel / max(dist, 1e-4);
    let ndv = max(dot(n, v), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    let r = reflect(-v, n);
    let refl = sky_color(normalize(vec3<f32>(r.x, abs(r.y), r.z)));
    let deep = water.deep.rgb * (globals.sun_color.rgb + globals.sky_zenith.rgb);
    var col = mix(deep, refl, fresnel);

    let l = -normalize(globals.sun_direction.xyz);
    col += globals.sun_color.rgb * pow(max(dot(r, l), 0.0), 300.0) * 4.0;

    col = apply_fog(col, i.world);
    return vec4<f32>(col, mix(water.deep.a, 1.0, fresnel));
}
