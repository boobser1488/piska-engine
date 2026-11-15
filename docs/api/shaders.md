# Свои шейдеры

Свой шейдер — файл `.shader` в каталоге ассетов. В нём пишется **одна функция** на WGSL, а не
шейдер целиком: свет, тени (во всех вариантах), туман, AO, контур и тонмаппинг движок
добавляет сам, как у стандартного материала. Параметры объявляются в заголовке один раз и
сразу появляются в инспекторе.

| вид (`//! kind:`) | функция | где работает |
|---|---|---|
| `surface` | `fn surface(in: SurfaceIn) -> Surface` | 3D-материал: меняете цвет, нормаль, шероховатость, свечение, прозрачность; свет — движка |
| `unlit` | `fn unlit(in: SurfaceIn) -> vec4<f32>` | 3D-материал без освещения: что вернули, то и цвет (туман — движка) |
| `sprite` | `fn sprite(in: SpriteIn) -> vec4<f32>` | `SpriteRenderer` + компонент `SpriteShader` |
| `post` | `fn post(in: PostIn) -> vec4<f32>` | эффект на весь экран: компонент `PostEffect` на камере |

У `surface` и `unlit` есть необязательная `fn vertex(in: VertexIn) -> VertexOut` — сдвиг вершин
(трава на ветру, волны). Сдвиг действует и в проходе теней: тень качается вместе с объектом.

Всё — из `engine::prelude::*`: `Shader`, `ShaderParams`, `ParamValue`, `PostEffect`,
`SpriteShader`, `MaterialParams`, `ShaderParamExt`.

## За пять минут

1. **Project → Create → Shader → Surface Shader.** Появится `New Surface Shader.shader` и
   откроется во вкладке Code. Шаблон работает сразу.
2. **Create → Material**, в инспекторе материала — поле **Shader**: выберите файл (или
   перетащите его из Project). Ниже появятся параметры шейдера: ползунки, цвета, текстуры.
3. Перетащите материал на объект в сцене. Меняйте код и сохраняйте (Ctrl+S) — сцена
   обновится сразу, и в режиме редактирования, и в игре; пересборка игры не нужна.

Тот же шейдер из кода:

```rust
use engine::prelude::*;

fn build(app: &mut App) -> Result<(), EngineError> {
    engine::mount_assets(app, "assets")?;
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let dissolve = db.handle::<Shader>("shaders/dissolve.shader")?;
    let material = app
        .world
        .resources
        .get_or_insert_with(Materials::default)
        .add(Material::new().shader(dissolve).param("amount", 0.2).param("edge_color", Color::rgb(4.0, 1.5, 0.3)));
    app.spawn("Crate").with(MeshRenderer::cube().with_material_asset(material));
    Ok(())
}
```

Поменять параметр из скрипта — у одного объекта:

```rust
component! {
    pub struct Burn {
        pub speed: f32 = 0.3,
        pub amount: f32 = 0.0,
    }
}
impl Component for Burn {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.amount = (self.amount + self.speed * dt).min(1.0);
        ctx.set_shader_param("amount", self.amount); // `use engine::prelude::*` даёт ShaderParamExt
    }
}
```

`set_shader_param` пишет в `SpriteShader` или `PostEffect` объекта, а у мешей — в компонент
`MaterialParams` (он добавляется сам). Такой объект рисуется своей пачкой; общий материал и
остальные объекты с ним не меняются. Поменять значение у всех объектов материала:

```rust
let materials = ctx.world.resources.get_or_insert_with(Materials::default);
if let Some(m) = materials.get_mut(handle) {
    m.shader_params.set("amount", 0.5);
}
```

## Файл `.shader`

```wgsl
//! kind: surface
/// Доля растворённой поверхности: 0 — целая, 1 — исчезла.
//! param amount: float = 0.35 [0, 1]
//! param edge_color: color = (4.0, 1.6, 0.3, 1.0)
//! param noise: texture = gray

fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);            // что дал бы стандартный материал
    let n = textureSample(noise, noise_sampler, in.uv).r;
    if (n < params.amount) {
        discard;                            // вырез — и в тени тоже
    }
    s.emissive += params.edge_color.rgb * step(n, params.amount + 0.05);
    return s;
}
```

* Заголовок — строки комментариев `//!` (файл остаётся обычным WGSL, подсветка работает).
* `//! kind: surface | unlit | sprite | post` — обязательно, один раз.
* `//! param имя: тип [= значение] [[min, max]]` — параметр. Строки `///` над ним — подсказка
  в инспекторе.

| тип | в шейдере | значение по умолчанию | виджет |
|---|---|---|---|
| `float` | `params.имя: f32` | `0`; диапазон `[0, 1]` — ползунок | число / ползунок |
| `int` | `params.имя: i32` | `0`; диапазон — ползунок | число / ползунок |
| `bool` | `params.имя: bool` | `false` | флажок |
| `vec2`, `vec3`, `vec4` | `vec2<f32>`… | нули: `(1, 0.5)` | поля X / Y / Z / W |
| `color` | `params.имя: vec4<f32>` (линейный RGBA) | белый: `(r, g, b)` или `(r, g, b, a)` | цвет |
| `texture` | `имя: texture_2d<f32>` и `имя_sampler: sampler` | `white`, `black`, `gray`, `normal` | слот текстуры |

Текстура с значением по умолчанию `white` / `black` — цветная (sRGB, как базовая текстура
материала). `gray` / `normal` — **данные** (шум, маски, высоты, карты нормалей): читаются
без перевода из sRGB. Текстур-параметров — не больше 4 (лимит WebGL2).

Числа параметров лежат в своём uniform-блоке (выравнивание WGSL считает движок), текстуры —
следом, в отдельной группе привязок. Имена `params`, `in`, `material`, `frame`, `scene` и
слова WGSL (`from`, `type`, `target`…) параметру дать нельзя — заголовок скажет об этом.

### Значения в `.material`

```ron
(
    base_color: (1.0, 1.0, 1.0, 1.0),
    shader: "8d1f…",                       // GUID .shader (пусто — стандартный материал)
    shader_params: {
        "amount": Float(0.35),
        "edge_color": Color(4.0, 1.6, 0.3, 1.0),
        "noise": Texture("51c2…"),         // GUID картинки; пусто — по умолчанию
    },
)
```

Чего нет в `shader_params`, берётся из объявления в `.shader`. Значение другого типа
приводится (`Int(3)` для float — 3.0); не приводится — значение по умолчанию.

## Справочник

### `SurfaceIn` (вход `surface` и `unlit`)

| поле | что |
|---|---|
| `uv: vec2<f32>` | UV меша с тайлингом и сдвигом материала |
| `world_position: vec3<f32>` | точка в мире |
| `world_normal: vec3<f32>` | нормаль грани (без карты нормалей); у двусторонних — к зрителю |
| `view_dir: vec3<f32>` | от точки к камере, единичный |
| `color: vec4<f32>` | цвет материала × оттенок объекта (`MeshRenderer::tint`) — у мешей нет цвета вершин |
| `time: f32` | секунды; в редакторе вне игры — часы рендерера (материалы «живые») |
| `screen_uv: vec2<f32>` | 0..1 по экрану, (0, 0) — левый верхний угол |
| `object_position: vec3<f32>` | точка в осях объекта от его опоры, в метрах (с масштабом) |
| `front_facing: bool` | лицевая сторона |
| `albedo`, `alpha`, `normal`, `metallic`, `roughness`, `emissive`, `occlusion` | что дал бы стандартный материал (текстуры, карта нормалей, AO, dev-сетка) |

### `Surface` (результат `surface`)

`albedo: vec3`, `alpha: f32`, `normal: vec3` (мировая), `metallic: f32`, `roughness: f32`,
`emissive: vec3` (больше 1 — блум), `occlusion: f32`. `default_surface(in)` — стандартный
материал как есть; шейдер `return default_surface(in);` рисует ровно как без шейдера.

Прозрачность и двусторонность задаёт материал (`Alpha`: Opaque / Cutout / Blend,
`Double-Sided`): `alpha` из `Surface` работает с порогом выреза или смешиванием. `discard`
выкидывает пиксель — и в тени.

### `VertexIn` → `VertexOut` (необязательная `vertex`)

`VertexIn`: `position`, `normal` (в осях объекта), `uv`, `world_position`, `world_normal`,
`object_origin` (мировая опора объекта), `time`. `VertexOut`: `world_position`,
`world_normal`, `uv`. `default_vertex(in)` — без изменений. Сдвиг не меняет границы объекта
для отсечения — двигайте вершины на десятки сантиметров, не на десятки метров.

### `SpriteIn` (вход `sprite`)

`uv`, `color` (цвет спрайта), `time`, `texel_size` (пиксель текстуры в UV), `texture_size`,
`world_position`, `screen_uv`, `frame_rect` (кадр листа: xy — min, zw — max),
`texture_color` (пиксель текстуры). `default_sprite(in)` — текстура × цвет;
`sample_sprite(uv)` — пиксель текстуры в любой точке (без мипмапов, можно в циклах).

### `PostIn` (вход `post`)

`uv`, `color` (кадр в этой точке), `time`, `resolution` (пиксели), `texel_size`, `position`
(номер пикселя). `scene(uv)` — кадр в любой точке. Глубины кадра у эффектов нет.

### Встроенные функции

| функция | что |
|---|---|
| `hash(p: vec2) -> f32`, `hash3(p: vec3) -> f32` | псевдослучайное 0..1 |
| `value_noise(p: vec2) -> f32` | плавный шум 0..1 |
| `fbm(p: vec2) -> f32` | шум из четырёх слоёв 0..1 |
| `remap(v, from_min, from_max, to_min, to_max) -> f32` | перевод диапазона |
| `fresnel(normal, view_dir, power) -> f32` | 0 — лицом к камере, 1 — на краях |
| `rotate_uv(uv, angle, center) -> vec2` | поворот UV |
| `luminance(c: vec3) -> f32` | яркость |
| `rgb_to_hsv(c)`, `hsv_to_rgb(c)` | цвет ↔ оттенок, насыщенность, яркость |
| `PI`, `TAU` | константы |
| `sun_direction()`, `sun_color()`, `sun_visibility(in)`, `sky_ambient(normal)` | только `surface` / `unlit`: свет сцены для своего освещения (мультяшный свет); `sun_visibility` учитывает тени |

У `surface` / `unlit` доступны и текстуры стандартного материала: `base_texture`,
`normal_texture`, … с сэмплером `material_sampler`.

## Ошибки

Вкладка Code проверяет `.shader` при наборе той же сборкой, что и движок: опечатка
подчёркнута на вашей строке, строка в Error List; для неизвестного поля `SurfaceIn` или
параметра — «did you mean» с исправлением в один щелчок. Ошибка в заголовке (тип
параметра, диапазон) — тоже на месте.

Если сохранить шейдер с ошибкой, игра не падает: рисуется прошлая рабочая версия (или
стандартный материал, если рабочей ещё не было — не пурпурный), в Console — одна строка
`assets/shaders/x.shader:12:5: …`. Исправили и сохранили — снова свой шейдер.

## Цена

* Без `.shader` в сцене — ни одного лишнего конвейера, группы привязок или проверки на
  объект: кадр тот же до пикселя (тесты `materials`, `graphics` и `shaders`).
* Шейдер собирается при первом использовании: проверка naga всех вариантов — единицы
  миллисекунд на вариант, модуль и конвейер на GPU — ещё десятки миллисекунд (один раз).
* Объекты с одним мешем, материалом и значениями параметров рисуются одной пачкой.
  `MaterialParams` у объекта — своя пачка только для него. Спрайты с одним `SpriteShader`
  и одинаковыми значениями — одна пачка.
* Свой шейдер со своей `vertex`, `discard` или прозрачностью рисует и тень своим
  конвейером (сдвиг и вырез видны в тени); без них — общим проходом теней.

## Платформы

| | Windows / Linux (Vulkan, DX12) | браузер (WebGPU) | браузер (WebGL2) | Android |
|---|---|---|---|---|
| surface / unlit / vertex | да | да | да (проверено naga с лимитами WebGL2, в браузере не запускалось) | да |
| sprite | да | да | да | да |
| post | да | да | да | да |
| горячая перезагрузка | да (файлы на диске) | нет (ассеты в паке) | нет | нет |

`.shader` попадают в пак игры вместе с остальными ассетами (`piska-export`) и собираются при
загрузке. Между стадиями шейдер мешей передаёт 30 значений из 31, доступных в WebGL2, —
своих varying'ов у шейдера нет (`object_position` занимает место точки для текстуры «в
метрах»: с ним `world_tiling` материала привязан к опоре объекта). Текстур-параметров —
до 4.

## Примеры

`assets/shaders/` — у каждого в начале описание:

| файл | вид | что |
|---|---|---|
| `dissolve` | surface | растворение по шуму со светящимся краем (и в тени) |
| `hologram` | unlit | полосы развёртки, края, мерцание (материал — Blend) |
| `toon` | unlit | мультяшный свет ступенями, с тенями и светлым краем |
| `wind_sway` | surface + vertex | качание на ветру (тень качается тоже) |
| `water` | unlit | бегущая вода с пеной |
| `force_field` | unlit | силовое поле: френель, сетка, волны (Blend, двусторонний) |
| `rim_light` | surface | светящийся контур (френель), пульсация |
| `sprite_outline` | sprite | обводка спрайта |
| `sprite_flash` | sprite | вспышка при попадании |
| `sprite_palette` | sprite | смена палитры с сохранением светотени |
| `vignette` | post | виньетка |
| `pixelate` | post | крупные пиксели и урезанная палитра |
| `crt` | post | старый телевизор |
| `color_pop` | post | чёрно-белое с одним цветом |
| `underwater` | post | под водой: колыхание, оттенок, блики |
