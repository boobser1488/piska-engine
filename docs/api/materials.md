# Материалы

Материал — описание поверхности в модели metallic-roughness (как в glTF): базовый цвет и
текстура, металличность и шероховатость, карта нормалей, свечение, ambient occlusion,
тайлинг, прозрачность (вырез / смешивание), двусторонность, тени, «без освещения».

Материал живёт в ассете `.material` (текст RON рядом с `.meta`), создаётся из кода
(`Material::new()…`) или берётся из файла модели (glTF, `.mtl`). `MeshRenderer` получает
по материалу на каждую **часть** меша (sub-mesh): у модели из файла столько частей,
сколько в ней групп материалов.

Всё из `engine::prelude::*`: `Material`, `Materials`, `AlphaMode`, `MeshRenderer`,
`AssetHandle`, `AssetDatabase`.

## Быстрый старт

```rust
use engine::prelude::*;

fn build(app: &mut App) -> Result<(), EngineError> {
    engine::mount_assets(app, "assets")?;
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();

    // 1. Ассет из каталога ассетов.
    let stone = db.handle::<Material>("materials/rough_stone.material")?;
    app.spawn("Pedestal").with(MeshRenderer::cylinder().with_material_asset(stone));

    // 2. Материал из кода (живёт в ресурсе `Materials`, на диск не пишется).
    let gold = app
        .world
        .resources
        .get_or_insert_with(Materials::default)
        .add(Material::new().color(Color::hex(0xffc933)).metallic(1.0).roughness(0.25));
    app.spawn("Coin").with(MeshRenderer::sphere().with_material_asset(gold));

    // 3. Модель со своими материалами из файла (каждая часть — своим).
    let stairs = db.handle::<Mesh>("models/stairs.glb")?;
    app.spawn("Stairs").with(MeshRenderer::model(stairs));
    Ok(())
}
```

## Поля материала

| поле | по умолчанию | смысл |
|---|---|---|
| `base_color: Color` | белый | базовый цвет (линейный), умножается на текстуру; альфа — прозрачность |
| `base_color_texture` | нет | текстура цвета (sRGB) |
| `metallic`, `roughness` | 0, 0.6 | умножаются на синий и зелёный каналы `metallic_roughness_texture` |
| `metallic_roughness_texture` | нет | как в glTF: G — шероховатость, B — металл (линейные данные) |
| `normal_texture`, `normal_strength` | нет, 1 | карта нормалей в касательном пространстве, OpenGL-соглашение (зелёный — «вверх» по картинке) |
| `emissive: Color`, `emissive_intensity` | чёрный, 1 | свечение; яркость больше 1 даёт блум |
| `emissive_texture` | нет | умножается на свечение (sRGB) |
| `occlusion_texture`, `occlusion_strength` | нет, 1 | красный канал гасит рассеянный свет и отражение неба |
| `uv_scale`, `uv_offset: Vec2` | (1, 1), (0, 0) | повторение и сдвиг всех текстур по UV меша |
| `world_tiling: f32` | 0 | базовая текстура «в метрах» с шагом N метров (как `MeshRenderer::tiling`); 0 — по UV. Проекция по трём осям **объекта**: у неповёрнутых объектов это мировые координаты (соседние плиты продолжают узор), у повёрнутого бруса полосы идут вдоль его граней, у объекта, вращающегося вокруг своей точки (метла, карусель), узор едет вместе с ним. Масштаб объекта не растягивает узор. Объект, который **перемещается**, по-прежнему «проезжает» по узору — для движущихся платформ берите UV |
| `alpha_mode: AlphaMode` | `Opaque` | `Opaque`, `Cutout` (вырез, порог `alpha_cutoff`), `Blend` (смешивание) |
| `alpha_cutoff` | 0.5 | порог выреза |
| `double_sided` | false | рисовать обратные грани; их нормаль переворачивается — освещены со своей стороны |
| `cast_shadows`, `receive_shadows` | true, true | |
| `unlit` | false | без освещения: цвет как есть (плюс свечение) |
| `grid` | false | dev-сетка в метрах поверх материала |

Конструктор по цепочке:

```rust
let leaf = Material::new()
    .texture(db.handle::<Texture>("materials/leaves.png")?)
    .cutout(0.5)
    .double_sided(true)
    .roughness(0.8);
let glass = Material::new().color(Color::rgba(0.55, 0.78, 1.0, 0.3)).transparent().roughness(0.05);
let lamp = Material::new().color(Color::rgb(0.08, 0.07, 0.06)).emissive(Color::rgb(1.0, 0.55, 0.2), 9.0);
let stone = Material::new()
    .texture(db.handle("materials/stone_albedo.png")?)
    .normal_map(db.handle("materials/stone_normal.png")?, 1.0)
    .occlusion(db.handle("materials/stone_ao.png")?, 1.0)
    .tiling(2.0, 1.0)
    .roughness(0.95);
// ещё: .metallic(..), .metallic_roughness_texture(..), .emissive_texture(..), .offset(u, v),
//      .world_tiling(metres), .unlit(true), .shadows(cast, receive)
```

## Фильтр текстуры (`.meta`)

Настройка импорта `"filter"` в `.meta` картинки:

| значение | вблизи | вдали | для чего |
|---|---|---|---|
| `Enum("Linear")` (по умолчанию) | сглаживание | мипмапы, трилинейно, анизотропия 16x | обычные текстуры |
| `Enum("Nearest")` | резкие пиксели | без мипмапов | спрайты, интерфейс, атласы моделей |
| `Enum("Pixel")` | резкие пиксели | мипмапы, трилинейно | пиксель-арт на поверхностях мира: мостовая и стены вдали не рябят |

`Pixel` без анизотропии (в wgpu она требует сглаживания при увеличении), поэтому пол под
острым углом вдали чуть мягче, чем у `Linear`. Узор пиксель-арта на больших поверхностях —
спокойный: однотексельная «шахматка» и тонкие контрастные линии усредняются мипмапами в
ровный цвет, а без мипмапов (`Nearest`) мерцают при каждом повороте камеры.

## Файл `.material`

Текст RON; отсутствующие поля получают значения по умолчанию, текстуры — GUID строкой
(пустая строка — нет текстуры):

```ron
// Material (Piska Engine). Textures are referenced by GUID.
(
    base_color: (1.0, 1.0, 1.0, 1.0),
    base_color_texture: "6d3f1a2b8c4e4f5a9b0c1d2e3f405163",
    roughness: 0.95,
    normal_texture: "6d3f1a2b8c4e4f5a9b0c1d2e3f405164",
    alpha_mode: Opaque,   // Opaque | Cutout | Blend
)
```

Из кода:

```rust
let material = db.load_material(guid)?;                    // прочитать
let text = material.to_ron();                              // в текст
let same = Material::from_ron(&text)?;                     // из текста
let handle = db.save_material("materials/red.material", &material)?; // записать (+ .meta)
```

`save_material` работает только с каталогом ассетов (не с паком в браузере / APK).
Сломанный файл не роняет игру: объект рисуется пурпурным, причина один раз уходит в лог.

## `MeshRenderer` и материалы

```rust
MeshRenderer::sphere().with_material_asset(handle)       // один материал на весь меш
MeshRenderer::asset(mesh).with_materials(vec![a, b])     // по частям модели
MeshRenderer::model(mesh)                                // материалы из файла модели
renderer.materials.push(handle);                         // поле `materials: Vec<AssetHandle<Material>>`
```

Как выбирается материал части `i`:

1. `materials[i]`; частям без своего слота достаётся последний элемент списка;
2. иначе (список пуст, а `model_materials = true`, или слот пустой) — материал из файла
   модели, на который ссылается эта часть;
3. иначе — **встроенный материал** `MeshRenderer`: поля `color`, `texture`, `metallic`,
   `roughness`, `emissive`, `tiling`, `grid`. Так рисуются все сцены, сохранённые до
   материалов: у них `materials` пуст и `model_materials = false` — картинка та же, что
   раньше (это проверяет тест `old_scenes_render_like_before_materials`).

`cast_shadows` компонента выключает тени объекта при любом материале.

Части модели: `db.load_mesh(guid)?.submeshes` — диапазон индексов, номер материала в
файле и имя (`usemtl` в OBJ, имя материала glTF). Материалы файла как `Material`:
`db.model_materials(guid)?` (текстуры находятся по путям рядом с моделью; картинки,
встроенные в `.glb`, не поддерживаются — у такой части остаётся цвет).

## Перекрасить один объект: `tint` и `set_color`

```rust
if let Some(r) = ctx.get_mut::<MeshRenderer>() {
    r.set_color(Color::rgb(1.0, 0.2, 0.2)); // с материалом-ассетом меняет `tint`, со встроенным — `color`
}
```

`tint` умножается на базовый цвет материала только у этого объекта; общий материал не
меняется, и объекты с ним по-прежнему рисуются одной пачкой.

Так же работает свечение: у объекта с материалами-ассетами `MeshRenderer::emissive`
**добавляется** к свечению материала (материал задаёт своё, поле объекта — добавку только
этому объекту: вспышка бампера, лампа контрольной точки). Чёрный (по умолчанию) — без
добавки; пачки не рвутся. Если у материала есть `emissive_texture`, добавка умножается на неё.
Без материалов-ассетов поле — просто свечение встроенного материала.

## Изменить материал из кода во время игры

Материал в `Materials` можно менять — рендерер увидит изменение в следующем кадре у
всех объектов с ним:

```rust
let materials = ctx.world.resources.get_or_insert_with(Materials::default);
if let Some(m) = materials.get_mut(gold) {
    m.emissive_intensity = 4.0;
}
// Подменить ассет-файл из кода: тот же GUID, другой материал.
materials.set(stone, Material::new().color(Color::rgb(0.2, 0.2, 0.25)));
```

Файл `.material`, изменённый на диске, рендерер перечитывает после
`renderer.reload_material(guid)` (одного) или `renderer.reload_assets()` (всё). Редактор
делает это сам: при правке в инспекторе и когда окно получает фокус.

## Цена и пакетная отрисовка

Что едет в **данных экземпляра** (не мешает рисовать объекты одной пачкой): базовый цвет ×
`tint`, металличность, шероховатость, свечение × яркость, `world_tiling`, dev-сетка.

Что лежит в **группе привязок материала** (bind group): пять текстур и общие параметры —
`uv_scale`/`uv_offset`, сила карты нормалей и AO, порог выреза, флаги (unlit, приём теней,
двусторонность, режим альфы). Одинаковые группы делятся, поэтому:

* 1000 кубов с четырьмя материалами, у которых одна текстура и разные цвет/металл —
  **один** draw call (плюс один для теней);
* другая текстура, карта нормалей, тайлинг UV или режим альфы — ещё одна группа и ещё
  одна пачка на каждый меш, где она встречается;
* модель из двух частей с разными материалами — два вызова на пачку таких моделей; если
  все части получили один материал, модель рисуется одним вызовом целиком;
* полупрозрачные (`Blend`) рисуются после непрозрачных и сетки, от дальних к ближним, и
  поэтому склеиваются в пачки только подряд идущие одинаковые;
* тени: `Opaque` — как раньше; двусторонние — свой конвейер без отсечения граней;
  `Cutout` и `Blend` — конвейер с выборкой альфы (у `Blend` порог 0.5: стекло с альфой
  0.3 тени не даёт).

`RenderStats`: `material_bind_groups` — сколько групп создано, `transparent_meshes` —
сколько полупрозрачных частей в кадре; `meshes` считает части (у старых сцен — объекты).
Встроенный материал — та же группа с одной текстурой, поэтому старые сцены дают те же
draw call'ы, что и раньше (`stress meshes`: 8 до и после).

Ограничения WebGL2 соблюдены: экземпляр — 10 атрибутов, вершина — 3 + касательная
(14 из 16), шесть текстур на материал с картой теней (лимит 16), без storage-буферов.

## Не поддерживается

Clear coat, sheen, подповерхностное рассеяние, анизотропия, преломление, параллакс /
height-карты, вторые UV, отдельный `KHR_texture_transform` на каждую текстуру, картинки,
встроенные в `.glb`, точное OIT для пересекающихся прозрачных объектов (сортировка — по
центрам объектов). Карта нормалей не применяется к `world_tiling` (только по UV меша).

## В редакторе

* **Project → правый клик по каталогу → New Material** создаёт `New Material.material`.
  Плитка материала — отрисованная сфера; выделенный материал в панели сведений — большой
  вид с вращением мышью и переключателем **Sphere / Cube / Plane**.
* Щелчок по материалу открывает его **в инспекторе**: цвета, ползунки, слоты текстур с
  миниатюрами (выбор из списка или перетаскивание из Project), режим альфы, флажки.
  Каждое изменение сразу пишется в файл и видно в сцене; **Undo / Redo** (и Ctrl+Z /
  Ctrl+Y, пока открыт материал) отменяют правки материала. Выделение объекта возвращает
  инспектор к объекту.
* У `MeshRenderer` в инспекторе — раздел **Materials**: по слоту на часть модели (имена
  частей из файла), перетаскивание `.material` на слот, **Edit** открывает материал,
  **Extract materials from model** создаёт `.material` рядом с моделью из её встроенных
  материалов и назначает их. Встроенный материал — в группе **Inline material (legacy)**,
  она видна, только пока материалы не назначены.
* `.material`, брошенный на объект в виде Scene или на строку Hierarchy, назначается
  первому слоту его `MeshRenderer`.

API для тестов и автоматизации: `editor.inspect_material("materials/x.material")`,
`editor.edit_material(|m| m.metallic = 1.0)` (тот же путь, что у полей инспектора: шаг
Undo, запись файла, перерисовка), `editor.undo_material()`, `editor.redo_material()`,
`editor.inspected_material()`.

## Образцы

`assets/materials/` (генератор — `python tools/gen_sample_materials.py`): `painted_metal`
(краска с царапинами до металла через metallic-roughness карту), `rough_stone` (цвет +
карта нормалей + AO), `emissive_lamp` (карта свечения, блум), `glass` (Blend), `foliage`
(Cutout, двусторонний), `checker` (тайлинг UV). В `scenes/demo3d.scene` ими одеты
постамент, куб, вращающийся блок, лампа и стальной шар.

## Пример: витрина

Сцены `scenes/showcase/*.scene` (`cargo run --example showcase`, `docs/api/showcase.md`)
одеты четырьмя материалами на одну палитру-текстуру (`showcase/materials/kit | glow | metal
| glass`): все модели набора делят 4 группы привязок, свечение окон и огня даёт блум.

Модели Blockbench (`.bbmodel`), Minecraft (`.model.json`), MagicaVoxel (`.vox`) и `.ply` с цветами
приносят свою текстуру-атлас, как `.pmodel`: `MeshRenderer` без своей текстуры рисует ею.
Настройки их импорта и таблица возможностей — `docs/api/model-formats.md`.

## Свой шейдер

Поле `shader` материала (в инспекторе — раздел **Shader**) подключает файл `.shader`: одна
функция на WGSL меняет цвет, нормаль, свечение или прозрачность, а свет, тени и туман
остаются движка. Параметры шейдера хранятся в `shader_params` и видны в инспекторе.
`Material::new().shader(handle).param("amount", 0.5)`. Подробно — `docs/api/shaders.md`.
