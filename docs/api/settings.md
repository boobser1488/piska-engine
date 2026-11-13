# Настройки проекта (`project.settings`)

Unity: **Edit → Project Settings**. Настройки проекта — один файл `project.settings` (RON) в
корне ассетов игры (`games/<имя>/assets/project.settings`). Его пишет редактор, его можно
править руками, он попадает в экспорт вместе с ассетами (пак браузера и Android, каталог
`assets/` на десктопе) и читается при старте игры.

* [1. Как настройки попадают в игру](#1-как-настройки-попадают-в-игру)
* [2. Формат файла](#2-формат-файла)
* [3. Player — окно и сцена по умолчанию](#3-player--окно-и-сцена-по-умолчанию)
* [4. Time](#4-time)
* [5. Physics 2D и Physics (3D)](#5-physics-2d-и-physics-3d)
* [6. Tags and Layers](#6-tags-and-layers)
* [7. Input](#7-input)
* [8. Graphics / Quality](#8-graphics--quality)
* [9. Audio](#9-audio)
* [10. Script Execution Order](#10-script-execution-order)
* [11. Что не настраивается (пока)](#11-что-не-настраивается-пока)

---

## 1. Как настройки попадают в игру

```rust
pub fn build() -> Result<App, EngineError> {
    let mut app = engine::app();
    engine::mount_assets(&mut app, engine::assets_dir!())?; // ← здесь читается project.settings
    // всё, что код делает дальше, перекрывает настройки («код побеждает»)
    Ok(app)
}
engine::game_main!(build, WindowConfig::titled("My Game"));
```

Порядок:

1. `engine::mount_assets` читает `project.settings` и вызывает
   `World::apply_project_settings(settings)`. Нет файла — всё по умолчанию; файл с ошибкой —
   предупреждение в логе и тоже всё по умолчанию.
2. `apply_project_settings` применяет то, что принадлежит ядру (время, ввод, порядок
   скриптов), кладёт в `world.resources` ресурсы `ProjectSettings` и `GraphicsSettings` и
   вызывает **хуки подсистем**: физика 2D и 3D, звук (их регистрирует `engine::app()` —
   `World::add_settings_hook`). Подсистема, выключенная возможностью сборки (`physics2d`,
   `physics3d`, `audio`), просто не регистрирует хук.
3. Рендер читает ресурс `GraphicsSettings` **каждый кадр**.
4. `engine::run` (и `game_main!` на всех платформах) накладывает раздел `player` на
   `WindowConfig` из кода, а `run_game` после `build()` загружает сцену по умолчанию, если
   мир пуст.

Всё применяется **один раз** — при `mount_assets`. Значит, код игры, выполненный после
`mount_assets` (в `build()` или в `on_start`), побеждает: гравитация, привязки кнопок,
громкость шин, `time_scale`, качество — что код задал, то и действует. Шаги физики
настройки не перечитывают.

Редактор, когда пользователь меняет настройки, вызывает тот же
`world.apply_project_settings(settings)` — изменения сразу действуют в открытой сцене (в том
числе на уже созданные тела: фильтры слоёв и пороги засыпания пересобираются).

Прочитать действующие настройки из кода: `ctx.world.project_settings()` (копия) или
`ctx.resource::<ProjectSettings>()`.

## 2. Формат файла

RON-структура, все разделы и поля необязательны: отсутствующее поле берёт значение по
умолчанию, старый файл без новых разделов читается как раньше.

```ron
(
    script_order: { "InputReader": -100 },
    player: (title: "Lantern", width: 1600, height: 900, fps_cap: 120, default_scene: "scenes/menu.scene"),
    time: (fixed_timestep: 0.016666668, max_timestep: 0.33333334, time_scale: 1.0),
    physics2d: (gravity: (0.0, -20.0), solver_iterations: 8),
    physics3d: (gravity: (0.0, -9.81, 0.0), layer_matrix: [ /* 32 числа-маски */ ]),
    tags_and_layers: (tags: ["Enemy", "Pickup"], layers: ["Default", "TransparentFX", /* … 32 имени */]),
    input: (actions: [ (name: "Jump", kind: Button, bindings: ["Key:Space", "Pad:South"], dead_zone: 0.2) ]),
    graphics: (quality: Medium, shadows: true, shadow_distance: 30.0, bloom: true, bloom_intensity: 0.6),
    audio: (master_volume: 0.8, music_volume: 0.5, mute_when_unfocused: true),
)
```

Из кода: `ProjectSettings::load(path)`, `ProjectSettings::from_ron(text, origin)`,
`settings.save(path)`, `settings.to_ron()` (`engine_core::settings`).

## 3. Player — окно и сцена по умолчанию

Раздел `player` (`PlayerSettings`). Применяет `engine::run` → `engine::player_window_config`
→ `WindowConfig::with_player_settings`.

| поле | по умолчанию | что делает |
|---|---|---|
| `title` | `""` | заголовок окна; пусто — заголовок из кода (`WindowConfig::titled`) |
| `company` | `""` | название компании — только данные (окно его не показывает) |
| `version` | `"0.1.0"` | версия игры — только данные |
| `width`, `height` | 1280, 720 | размер окна в логических пикселях (десктоп) |
| `resizable` | `true` | можно ли тянуть окно мышью (десктоп) |
| `fullscreen` | `Windowed` | `Borderless` — полный экран без рамки при старте (десктоп) |
| `vsync` | `false` | синхронизация с монитором |
| `fps_cap` | 0 | не чаще N кадров в секунду; 0 — без ограничения (десктоп и Android; браузер всегда идёт с частотой монитора) |
| `icon` | `""` | значок окна: путь PNG относительно корня ассетов; пусто — `icon.png`, если есть (`game_main!`), иначе значок движка |
| `default_scene` | `""` | сцена (путь относительно ассетов, `scenes/main.scene`), которую игра загружает, если `build()` не создала ни одного объекта |

**Правило наложения.** Основа — `WindowConfig` из кода. Поле настроек побеждает, только
если оно **изменено** (отличается от значения по умолчанию из таблицы); заголовок — только
непустой. Поэтому файл без раздела `player` оставляет окно таким, как его задал код, а
настройка «размер 1280×720» не может перебить размер из кода (это и есть значение по
умолчанию) — поменяйте размер в коде.

Сцена по умолчанию загружается только в **пустой** мир: если `build()` создала хоть один
объект, настройка игнорируется. Неверный путь — ошибка запуска с понятным текстом.

`WindowConfig` получил поля `resizable`, `fullscreen: FullscreenMode`, `fps_cap` и
построители `with_resizable`, `with_fullscreen`, `with_fps_cap`, `with_vsync` — ими можно
пользоваться и без настроек.

## 4. Time

Раздел `time` (`TimeSettings`). Применяет ядро (`apply_project_settings`).

| поле | по умолчанию | что делает |
|---|---|---|
| `fixed_timestep` | 1/60 | `Time::fixed_delta` — шаг `on_fixed_update` и физики (0.001…0.5) |
| `max_timestep` | 1/3 | `Time::max_delta` — кадр длиннее обрезается (после подвисания игра не «прыгает»), 0.01…1 |
| `time_scale` | 1 | начальный `Time::time_scale` (0…100) |

## 5. Physics 2D и Physics (3D)

Разделы `physics2d` (`Physics2dSettings`) и `physics3d` (`Physics3dSettings`). Применяют
хуки `engine_physics::apply_project_settings_2d` / `apply_project_settings_3d`
(→ `Physics2D::apply_settings`, `Physics3D::apply_settings`).

| поле | по умолчанию | что делает |
|---|---|---|
| `gravity` | (0, −9.81) / (0, −9.81, 0) | `Physics2D::gravity` / `Physics3D::gravity`; код, задавший гравитацию позже, побеждает |
| `solver_iterations` | 4 | итерации решателя контактов Rapier (1…64): больше — жёстче стопки, дороже |
| `sleep_linear_threshold` | 0.4 | скорость, ниже которой тело может уснуть (Rapier: нормализованный порог); 0 — не засыпает |
| `sleep_angular_threshold` | 0.5 | то же для вращения, рад/с |
| `layer_matrix` | все со всеми | матрица столкновений слоёв (32 маски, всегда симметрична) |

**Матрица слоёв.** Коллайдер — член слоя своего объекта (`GameObject::layer`, 0–31), а
сталкивается с `collides_with & matrix.mask(layer)`: снятая галочка «слой 8 ↔ слой 9»
пропускает объекты этих слоёв друг сквозь друга, маска коллайдера по-прежнему может сузить
список ещё сильнее. Действует на тела и триггеры 2D и 3D, контроллер персонажа 3D
(`CharacterController` проходит сквозь запрещённые слои; при матрице по умолчанию — как
раньше) и плитки рельефа (`Terrain`, слой объекта рельефа). Лучи (`raycast`,
`overlap_sphere`) матрицу не учитывают — как в Unity, у них своя маска (сейчас только
`ignore`).

Пороги засыпания задаются телу при создании; при повторном применении (редактор)
пересчитываются и у существующих тел. Если игра заменяет ресурс целиком
(`resources.insert(Physics2D::with_gravity(..))`) после `mount_assets`, настройки физики
теряются — меняйте поля существующего ресурса.

## 6. Tags and Layers

Раздел `tags_and_layers` (`TagsAndLayers`): список своих тегов и 32 имени слоёв. Это данные
для редактора (выпадающие списки Tag / Layer в инспекторе, подписи матрицы слоёв); сцены
по-прежнему хранят тег строкой и слой числом. Рантайм раздел не читает.

## 7. Input

Раздел `input` (`InputSettings`): список действий `InputAction` — `name`, `kind`
(`Button` / `Axis`), `bindings` (кнопка), `negative` / `positive` / `analog` (ось),
`dead_zone`. Привязки — текстом `Binding::to_text`: `Key:Space`, `Mouse:Left`, `Pad:South`,
`PadAxis:LeftStickX+`; аналоговая ось — `LeftStickX` или `-LeftStickY` (инверсия).
По умолчанию — те же действия, что движок создаёт сам (`Horizontal`, `Vertical`, `Jump`,
`Fire1`, `Submit`, `Cancel`).

Применяет ядро: действие из настроек **заменяет** одноимённое; действия, которых в
настройках нет, остаются от кода. Исключение — встроенные действия (`Horizontal`,
`Vertical`, `Jump`, `Fire1`, `Submit`, `Cancel`): если такого действия в списке нет, его
удалили на странице Input, и `apply` убирает его из `Input`. **Код побеждает:**
`bind_button` / `set_button_bindings` / `bind_axis`, вызванные после `mount_assets`,
перекрывают настройки (и возвращают удалённое встроенное действие, если код его привязал).

## 8. Graphics / Quality

Раздел `graphics` (`GraphicsSettings`). При применении он кладётся отдельным ресурсом
`GraphicsSettings`, рендер читает его **каждый кадр**. Ресурса нет (игра без
`mount_assets`) — всё как задано компонентом `Environment`.

| поле | High (по умолчанию) | Medium | Low | что делает |
|---|---|---|---|---|
| `quality` | `High` | `Medium` | `Low` | имя пресета |
| `shadows` | да | да | нет | тени от солнца (выкл. — проход теней пуст) |
| `shadow_distance` | 1000 | 30 | 20 | потолок дальности теней: действует меньшее из него и `Environment::shadow_distance` |
| `bloom` | да | да | нет | блум |
| `bloom_intensity` | 1.0 | 0.6 | 1.0 | множитель `Environment::bloom` |
| `bloom_threshold` | 1.0 | 1.0 | 1.0 | яркость (HDR), с которой начинается свечение |
| `fog_scale` | 1.0 | 1.0 | 1.0 | множитель `Environment::fog_density`; 0 — без тумана |

Пресет High совпадает с тем, как движок рисовал до появления настроек. Поля можно
подправить вручную после выбора пресета. 2D-камера без `Environment` пост-обработку не
включает вовсе (блум там не действует).

Из кода (Unity: `QualitySettings.SetQualityLevel`):

```rust
ctx.set_quality(QualityLevel::Low);          // заменяет ресурс значениями пресета
let level = ctx.quality();                   // Low / Medium / High
ctx.resource_mut::<GraphicsSettings>().unwrap().fog_scale = 0.5; // тонкая подстройка
```

## 9. Audio

Раздел `audio` (`AudioSettings`). Применяет хук `engine_audio::apply_project_settings`
(→ `Audio::apply_settings`).

| поле | по умолчанию | что делает |
|---|---|---|
| `master_volume` | 1.0 | громкость шины `Bus::Master` |
| `music_volume` | 1.0 | `Bus::Music` |
| `sfx_volume` | 1.0 | `Bus::Sfx` |
| `ui_volume` | 1.0 | `Bus::Ui` |
| `mute_when_unfocused` | `false` | глушить весь звук, пока окно игры не в фокусе |

Громкость, заданная кодом после `mount_assets` (`audio.set_bus_volume`), побеждает.
`mute_when_unfocused`: оконный цикл кладёт ресурс `WindowFocus { focused }` при смене
фокуса, `AudioSystem` глушит микшер отдельным флагом (`Audio::is_focus_muted`), не трогая
заглушки шин самой игры. Без окна (тесты, `ENGINE_SCREENSHOT`) ничего не глушится. В
браузере фокус — фокус холста.

## 10. Script Execution Order

Раздел `script_order` — см. `docs/api/script-order.md`.

## 11. Что не настраивается (пока)

Честно: этих ручек в файле нет, потому что рантайм их не умеет менять без переделки.

* **MSAA** — рендер всегда рисует с 4 выборками (константа рендера; цели, конвейеры и
  разрешение MSAA создаются один раз под это число).
* **Размер карты теней** — всегда 2048² (одна карта, без каскадов).
* **Render scale**, **тонмаппинг** (всегда ACES), **фильтрация текстур по умолчанию**
  (задаётся импортом каждой текстуры), **max FPS для Quality** (есть `player.fps_cap`).
* **Материал по умолчанию физики** (трение / упругость): у каждого `Collider2D` /
  `Collider` свои поля `friction` / `restitution`.
* **Max voices / частота дискретизации звука**: микшер — 64 голоса, частота подстраивается
  под устройство.
* `player.company` и `player.version` — только данные (их читает экспорт / редактор).

## Editor: Project Settings

**Edit → Project Settings…** в редакторе правит этот файл. Окно и его поиск описаны в
`editor.md` («Окна настроек»); здесь — что именно происходит с файлом.

* Страницы: **Player**, **Physics 2D**, **Physics 3D**, **Tags and Layers**, **Time**,
  **Graphics**, **Audio**, **Script Execution Order** — по одной на раздел файла; **Input** и
  **Build** описаны ниже («Editor: Input», «Editor: Build»; Build пишет не этот файл, а
  `Cargo.toml` игры). Строки соответствуют полям из таблиц выше; описание под строкой и
  подсказка говорят то же, что колонка «что делает».
* **Сразу в мир, потом в файл.** Любое изменение тут же применяется к открытой сцене через
  `World::apply_project_settings` — ядро (время, ввод, порядок скриптов, ресурс
  `GraphicsSettings`) и хуки подсистем (физика 2D / 3D, звук). Поэтому гравитация, матрица
  слоёв, громкости и качество графики меняются и в режиме Play, без перезапуска. Файл
  переписывается целиком (`ProjectSettings::to_ron`), когда пользователь отпустил мышь, или
  через полсекунды после последней набранной буквы. Нет каталога ассетов или файл не
  записался — ошибка в консоли и в строке состояния окна, правка остаётся в мире.
* **Script Execution Order** — исключение: правки ждут **Apply** (см. `script-order.md`).
  Пока они не применены, другие страницы пишут файл с действующим порядком.
* **Стрелка ⟲** у изменённой строки, **Reset page** и **Reset all** (с подтверждением)
  возвращают значения по умолчанию из таблиц выше и сразу переписывают файл. Reset всего на
  странице Tags and Layers удаляет свои теги и возвращает имена слоёв по умолчанию
  (`Default`, `TransparentFX`, `Ignore Raycast`, `Water`, `UI`).
* **Матрица слоёв** показывает только слои с именем (страница Tags and Layers); снятая галочка
  вызывает `LayerMatrix::set(a, b, false)` — матрица симметрична. Enable All возвращает
  «все со всеми».
* **Player → Icon / Default scene** — списки PNG-файлов и `.scene` из ассетов проекта
  (пусто — `None`); путь, которого больше нет, показан как «(missing)».
* **Graphics → Low / Medium / High** записывает в раздел целиком `GraphicsSettings::preset(..)`;
  поля после этого можно подправить, `quality` остаётся именем выбранного пресета.
* **Import… / Export…** — файл в формате `project.settings` где угодно (относительный путь — от
  папки проекта). Import заменяет все разделы, применяет их и записывает `project.settings`.
* Старый файл (например, только `script_order`) открывается: отсутствующие разделы берут
  значения по умолчанию, и при первой правке файл дописывается всеми разделами.

Из кода редактора (тесты, инструменты):

```rust
use engine::editor::{Editor, SettingsScope};

editor.edit_project_settings(|s| {
    s.time.fixed_timestep = 1.0 / 50.0;
    s.tags_and_layers.add_tag("Enemy");
    s.physics2d.layer_matrix.set(8, 9, false);
})?;                                                   // применено и записано
let current = editor.project_settings_draft();          // то, что показывает окно
editor.show_project_settings_page("physics2d");         // открыть окно на странице
editor.reset_settings(SettingsScope::Project, Some("time"))?; // как Reset page без вопроса
```

## Editor: Input

Страница **Project Settings → Input** правит раздел `input` (§7) — карту действий, которые
игра читает через `ctx.input().button("Jump")` и `ctx.input().axis("Horizontal")`.

* **Action list** — все действия карты: имя, вид (`Button` / `Axis`) и краткий список
  привязок. Щелчок выбирает действие, корзина справа удаляет его. Внизу — поле имени и
  **Add Button** / **Add Axis**. Имена уникальны и не пустые: повтор или пустое имя не
  добавляется, ошибка — в строке состояния окна. Под списком — действия, которые задаёт только
  код игры (их в карте нет, страница их не меняет).
* Выбранное действие:
  * **Name** — переименование по Enter или при уходе из поля; привязки сохраняются, старое
    имя убирается из `world.input` сразу.
  * **Type** — Button / Axis. Кнопка → ось: привязки становятся «плюсом»; ось → кнопка:
    «минус» и «плюс» сливаются в один список, аналоговые оси удаляются.
  * **Bindings** (кнопка) или **Negative** / **Positive** (ось) — списки источников: клавиша,
    кнопка мыши, кнопка геймпада, стик или триггер «как кнопка» (`PadAxis:LeftStickX+`).
    Щелчок по привязке — ожидание: следующее нажатие **заменяет** её. **Listen** — то же, но
    нажатие **добавляется**. **Add ▾** — выбрать источник из меню (клавиатура по группам,
    мышь, кнопки геймпада, стики и триггеры). Крестик удаляет привязку, повторы не
    добавляются.
  * Ожидание слышит клавиши (в том числе Shift / Ctrl / Alt — как левые), кнопки мыши (щелчок
    где угодно) и геймпад. **Esc** отменяет ожидание (клавишу Escape можно выбрать в
    **Add ▾**). Геймпад слышен через `world.input`, то есть пока игра запущена (Play) —
    в режиме Edit выберите кнопку геймпада из меню.
  * **Analog axes** (ось) — стики и триггеры, которые читаются плавно; флажок **Invert**
    переворачивает знак (`-LeftStickY`). **Dead zone** — мёртвая зона этих осей (0…0.95).
  * **Revert action** — встроенному действию возвращает привязки по умолчанию, у своего —
    очищает их. Стрелка ⟲ у каждого списка возвращает только этот список.
* **Reset page** (и стрелка у Action list) — карта по умолчанию: шесть встроенных действий.
* Правки сразу применяются к открытому миру (как остальные страницы): действие из карты
  заменяет одноимённое в `world.input`, удалённые и переименованные убираются. Файл
  `project.settings` записывается, когда отпущена мышь и никто не печатает.
* **Код побеждает** (§7): при запуске игры карта применяется в `mount_assets`, а привязки,
  которые код делает после него (`bind_button`, `set_axis_bindings`…), действуют поверх.
  Удалённое на странице встроенное действие игра не получает, если код не привяжет его сам.
  В открытом редакторе каждая правка карты снова применяет её к миру — привязки из кода для
  действий карты при этом заменяются до следующего запуска.

Из кода редактора и тестов: `Editor::select_input_action(name)`,
`selected_input_action()`, `is_listening_for_input()`, идентификаторы
`input_action_row_id(name)`, `input_remove_action_id(name)`, `input_new_action_field_id()`,
`input_add_action_id(ActionKind::Button)`, `input_rename_field_id()`,
`input_binding_id(InputBindingList::Bindings, 0)`, `input_remove_binding_id(..)`,
`input_listen_id(list)`, `input_revert_action_id()`. Карту можно менять и без окна:

```rust
use engine::core::settings::InputAction;
use engine::prelude::*; // Binding, Key

editor.edit_project_settings(|s| {
    s.input.actions.push(InputAction::button("Dash", &[Binding::Key(Key::Q)]));
})?; // применено к world.input и записано в project.settings
```

## Editor: Build

Страница **Project Settings → Build** показывает и правит настройки экспорта, которые
понимает `piska-export` (`docs/api/export.md`, §1.2 и §9.3), — таблицу
`[package.metadata.piska]` в `Cargo.toml` **крейта игры**. Это не `project.settings`: значения
живут в манифесте игры, а экспорт читает их оттуда.

Крейт игры — `Cargo.toml` в папке над каталогом ассетов (`games/<имя>/assets` →
`games/<имя>/Cargo.toml`), а если там не игра — `games/<game_crate>/Cargo.toml` рабочего
пространства (`EditorConfig::game_crate("<имя>")`). Игрой считается пакет (не `engine`),
который зависит от `engine`. Нет такого крейта — страница показывает пояснение вместо полей.

| строка | ключ | по умолчанию (ключа нет) | проверка |
|---|---|---|---|
| Title | `title` | имя пакета | не пустой |
| Icon | `icon` | `assets/icon.png`, иначе значок движка | PNG из ассетов (список) |
| Package id | `android.package` | `org.piska.<каталог игры>` | ≥ 2 частей через точку, каждая с буквы, буквы / цифры / `_` |
| Version code | `android.version_code` | `1` | 1…2 100 000 000 |
| Version name | `android.version_name` | версия пакета | не пустой |
| Orientation | `android.orientation` | `landscape` | `landscape` / `portrait` / `any` |
| Minimum SDK | `android.min_sdk` | `26` | 26…99 (ниже 26 нет AAudio) |

* Версия glibc для Linux — параметр самого экспорта (`piska-export linux --glibc 2.31`), не
  ключ манифеста; страница об этом только напоминает. Windows и Linux берут `title` и `icon`.
* **Запись только своей таблицы.** Файл правится через `toml_edit`: меняются лишь ключи, чьё
  значение изменилось; комментарии, порядок строк, `[dependencies]` и другие таблицы остаются
  как были, комментарий в конце изменённой строки сохраняется. Новая таблица появляется сразу
  после `[package]` (`[package.metadata.piska]`, `[package.metadata.piska.android]`); таблица
  Android, оставшаяся пустой, удаляется.
* Пустое поле или стрелка ⟲ **удаляют ключ** — экспорт берёт значение по умолчанию. Строка
  со значением по умолчанию без ключа помечена «default».
* **Неверное значение не записывается**: поле показывает ошибку (полный текст — в подсказке),
  в файле остаётся прежнее значение. Ключи в файле с неверным типом или значением (например,
  `orientation = "sideways"`) перечислены над полями — экспорт на них остановится.
* Файл записывается, когда отпущена мышь и никто не печатает (Enter или уход из поля), и
  перечитывается, если его поменяли на диске. **Reset page** и **Reset all** удаляют все ключи
  этой таблицы из `Cargo.toml` (остальной файл не трогается).

Из кода редактора и тестов:

```rust
use engine::editor::{AndroidOrientation, BuildOptions, Editor};

let mut options = editor.build_options()?;              // прочитать из Cargo.toml игры
options.android_package = Some("com.example.lantern".into());
options.android_orientation = Some(AndroidOrientation::Portrait);
editor.set_build_options(&options)?;                    // проверить и записать только эту таблицу
let path = editor.game_manifest_path();                 // какой Cargo.toml правится
let text = engine::editor::write_build_options(&manifest_text, &options)?; // без файла
```

Ошибки — `EditorError` с текстом, что исправить (`validate_android_package`,
`BuildOptions::validate` — те же правила, что у экспорта). Идентификаторы для тестов:
`Editor::build_field_id("android.package")`, ошибка поля — `editor.build_field_error(key)`.

## Editor: Preferences

**Edit → Preferences…** (Ctrl+,) — личные настройки редактора, не проекта: они не попадают в
`project.settings` и в экспорт игры. Хранятся в `%APPDATA%/PiskaEngine/editor.ron` (Linux,
macOS — `~/.config/PiskaEngine/editor.ron`), по строке `ключ: значение,` на настройку;
незнакомые и испорченные строки пропускаются, числа вне пределов обрезаются. Всё действует
сразу, без Apply. Окно, поиск, ⟲, Reset page / Reset all и Import / Export — те же, что у
Project Settings (`editor.md`, «Окна настроек»).

| страница | строка | ключ в файле | по умолчанию | что делает |
|---|---|---|---|---|
| General | Undo steps | `undo_limit` | 100 | сколько шагов помнит Undo сцены (10–1000) |
| General | Save before Play | `autosave_on_play` | false | вход в Play сначала сохраняет сцену, если у неё уже есть файл |
| General | Auto-save after | `autosave_minutes` | 0 (выкл.) | через столько минут после первой несохранённой правки сцена сохраняется сама (только Edit и только сцена с файлом) |
| General | External code editor | `external_editor` | пусто | команда внешнего редактора: `{file}` и `{line}` подставляются, без `{file}` путь идёт последним; кавычки `"…"` держат пробелы. Пусто — файлы открываются во вкладке Code. **Test** запускает команду с файлом вкладки Code (или папкой ассетов), итог — в строке состояния |
| Appearance | UI scale, Accent colour | `ui_scale`, `accent` | 100 %, Blue | масштаб интерфейса и цвет акцента |
| Appearance | Play mode tint | `play_tint` | 100 % | насколько панель инструментов окрашивается акцентом в Play (0 % — только полоса снизу) |
| Scene View | Grid colour, Grid opacity | `grid_color`, `grid_opacity` | светло-серый, 100 % | цвет линий сетки и её яркость относительно обычной (2D и 3D) |
| Scene View | Background | `scene_background` | `#222222` | фон вида Scene (вид Game берёт фон своей камеры) |
| Scene View | Move / Rotate / Scale step | `snap_move`, `snap_rotate`, `snap_scale` | 0.5 м, 15°, 0.1 | шаги привязки гизмо (включена кнопкой-магнитом, Ctrl — наоборот) |
| Scene View | Orbit speed, Invert vertical orbit | `orbit_speed`, `invert_orbit_y` | 1×, нет | скорость и направление вращения 3D-камеры правой кнопкой |
| Scene View | Zoom speed | `zoom_speed` | 1× | шаг приближения колесом, 2D и 3D |
| Game View | Vertical sync, Play mode limit, Redraw on demand | `vsync`, `play_fps_cap`, `redraw_on_demand` | нет, без ограничения, да | частота кадров окна редактора |
| Console & Diagnostics | Show messages / warnings / errors, Collapse, Error Pause | `console_info`, `console_warn`, `console_error`, `console_collapse`, `console_error_pause` | да, да, да, нет, нет | с чем открывается Console; изменение на странице сразу переключает и Console |
| Console & Diagnostics | Clear on Play | `console_clear_on_play` | нет | вход в Play очищает Console |
| Console & Diagnostics | Max messages | `console_max` | 2000 | сколько последних сообщений держит Console (50–2000) |
| Console & Diagnostics | History length | `profiler_history` | 300 | сколько последних кадров охватывают график и средние Profiler (30–3000) |
| Build & Tools | cargo, CMake | `tool_cargo`, `tool_cmake` | пусто | программа вместо `cargo` / `cmake` в командах сборки (Build, Ctrl+B) и в экспорте (File → Build…) |
| Build & Tools | Android SDK / NDK, Java (JDK), zig, C++ compiler | `tool_android_sdk`, `tool_android_ndk`, `tool_java`, `tool_zig`, `tool_cxx` | пусто | уходят в окружение этих команд: `ANDROID_HOME` и `ANDROID_SDK_ROOT`, `ANDROID_NDK_HOME` и `ANDROID_NDK_ROOT`, `JAVA_HOME`, `PISKA_ZIG`, `CXX` |
| Build & Tools | Export folder | `export_folder` | `dist` | File → Build… кладёт сборку в `<папка>/<игра>/<платформа>`, если в окне Build не задан свой каталог |
| Shortcuts | (команды) | `shortcut.<команда>` | см. `editor.md` | сочетания клавиш: в файле только изменённые, `none` — сочетание снято |

У инструментов пустое поле значит «искать как обычно» (PATH и переменные окружения); бледный
текст в поле показывает, что нашлось бы. **Detect** вписывает найденный путь (переменные
окружения, PATH, обычные папки установки: `%LOCALAPPDATA%/Android/Sdk`, самая новая версия в
`<SDK>/ndk/`, `jbr` Android Studio). **Validate** запускает программу (`cargo --version`,
`zig version`, `java -version`…) или заглядывает в папку (SDK — `platform-tools` / `platforms`,
NDK — `source.properties`) и показывает итог значком в строке и текстом внизу окна.

Файл можно править руками; строки и пути с запятыми пишутся с `%2C` (`%25` — сам `%`):

```text
(
    undo_limit: 200,
    autosave_minutes: 5,
    external_editor: code -g {file}:{line},
    grid_color: #e7eaeeff,
    snap_move: 0.25,
    console_clear_on_play: true,
    tool_android_sdk: C:\Users\me\AppData\Local\Android\Sdk,
    export_folder: builds,
    shortcut.play: F5,
    shortcut.quit: none,
)
```

Из кода редактора (тесты, инструменты):

```rust
use engine::editor::{Editor, KeyBinding, Preferences};

let mut prefs = editor.preferences().clone();
prefs.snap_move = 0.25;
prefs.external_editor = "code -g {file}:{line}".to_string();
editor.set_preferences(prefs);                          // действует сразу
editor.set_shortcut("play", Some(KeyBinding::key(egui::Key::F5)));
let text = editor.preferences().to_text();              // то, что пишется в файл
let back = Preferences::parse(&text);                   // и читается при запуске
editor.open_in_external_editor(Path::new("src/player.rs"), 12)?;
let grid = editor.scene_grid();                         // сетка, какой её получит рендер
```
