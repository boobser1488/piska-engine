# Возможности движка (Cargo features)

Игра линкует только те части движка, которые она использует. Это решается Cargo-возможностями
фасада `crates/engine`; подсистемы не нужны в exe, если не нужны игре. Редактор всегда
собирается со всем.

## Как подключать

```toml
# games/<имя>/Cargo.toml
[dependencies]
engine = { path = "../../crates/engine", default-features = false,
           features = ["physics2d", "audio", "ui", "gamepad"] }

[features]
# Редактор в экспорт не входит. Ему нужен весь рантайм: возможность `editor` тянет `full`.
editor = ["engine/editor"]
```

`default-features = false` убирает **всё**, включая рантайм: перечислите нужное. Не хотите
выбирать — `features = ["full"]` (всё, кроме редактора; так собирались игры до разбиения).

По умолчанию у самого `engine` включено всё: `default = ["full", "editor"]`. Примеры,
тесты (`cargo test --workspace`), редактор и `engine::edit` работают как раньше.

## Таблица

| возможность | что включает | что тянет в exe | без неё |
|---|---|---|---|
| `physics2d` | `Rigidbody2D`, `Collider2D`, `Physics2D`, `Physics2DSystem`, `Shape2D`, `RaycastHit2D` | Rapier2D + Parry2D (~0.5 МиБ кода) | нет модуля `engine::physics` 2D-типов, в `prelude` их нет |
| `physics3d` | `Rigidbody`, `Collider`, `CharacterController`, `Physics3D`, `Shape3D`, коллайдеры рельефа (`engine::terrain_physics`) | Rapier3D + Parry3D + nalgebra (~0.8 МиБ) | то же для 3D |
| `physics` | `physics2d` + `physics3d` | | |
| `audio` | микшер, синтезатор, `AudioSource`, `AudioListener`, `Audio`, вывод на устройство (cpal) | ~0.25 МиБ | нет `engine::audio`, `Audio` и пр. в `prelude` |
| `audio-wav` | `AudioClip::from_bytes`/`load` для `.wav`, `save_wav` (hound); включает `audio` | мало (десятки КиБ) | `.wav` даёт `ClipError::Disabled` |
| `audio-ogg` | `.ogg` Vorbis (lewton); включает `audio` | ~70 КиБ | `.ogg` даёт `ClipError::Disabled` |
| `ui` | игровой интерфейс (`engine-ui`): холст, текст, кнопки, раскладка, шрифты | ~0.9 МиБ вместе со шрифтами | нет `engine::ui`, `UiContext` и пр. |
| `cpp` | `engine::cpp`, `add_static_cpp`, `add_cpp_plugins`, `cpp_scripts!`, сервисы `engine::call_service` | мост + C ABI | нет C++-логики |
| `scenario` | язык Scenario, `ScenarioRunner`, `engine::scenario`; **включает `audio` и `ui`** (сценарии играют звук и читают кнопки) | ВМ языка ~0.5 МиБ | нет `ScenarioRunner` |
| `gamepad` | подсистема геймпадов (gilrs на десктопе, Gamepad API в браузере) | gilrs | `Input` без геймпада; клавиатура и мышь работают |
| `mesh-gltf` | модели `.gltf` / `.glb` (`engine-assets/gltf`) | gltf + serde_json (~0.25 МиБ) | `.gltf` даёт ошибку «enable `mesh-gltf`»; `.obj` и `.pmodel` работают всегда |
| `image-jpeg` | текстуры JPEG (`image/jpeg`, zune-jpeg) | ~0.27 МиБ | JPEG не читается; PNG — всегда |
| `webgl` | браузер: запасной WebGL2 (`wgpu/webgl`) | GLES-бэкенд wgpu в wasm | только WebGPU. `piska-export web` включает сам, если нет `--no-webgl` |
| `full` | всё перечисленное, кроме `webgl` и `editor` | | |
| `editor` | редактор (egui); включает `full` | | |

Если нужного типа не нашлось (`cannot find type Rigidbody in this scope`), проверьте эту
таблицу: скорее всего, не включена возможность. `engine::enabled_features()` возвращает
список включённых имён.

Не вынесено в возможности (мало выигрыша или слишком вплетено в рендер): частицы,
тайлмапы, анимации спрайтов, рельеф и вода в `engine-render` — они есть всегда
(см. отчёт об измерениях: рельеф — около 1 % exe).

## Правила для тех, кто пишет код движка

* Код, который трогает подсистему, закрывайте тем же `cfg`: `#[cfg(feature = "physics2d")]`
  и т. д. в крейте `engine`; в самих крейтах — их собственными возможностями
  (`engine-physics`: `2d`, `3d`; `engine-audio`: `wav`, `ogg`, `device`;
  `engine-assets`: `gltf`; `engine-render`: `jpeg`, `webgl`; `engine-ui`: `jpeg`).
* Настройки проекта: хук раздела регистрируется там, где ставится подсистема
  (`engine::app()`), и только под её возможностью. Сам раздел настроек (данные в
  `engine-core::settings`) есть всегда — это обычные данные, их читает и редактор.
* Новый код в `engine::app()` / `prelude` / `cpp_services` / `scenario` — под `cfg`.
* Пути зависимостей: `engine-physics`, `engine-audio`, `engine-assets`, `engine-render` в
  корневом `Cargo.toml` объявлены с `default-features = false`; нужную возможность
  указывает тот, кто зависит (фасад, редактор). У самих крейтов `default` включает всё,
  поэтому `cargo test -p engine-physics` проверяет обе физики.
* Проверка набора: `cargo check -p engine --lib --no-default-features --features "physics2d,audio,ui"`.
  Рабочие наборы игр проверяются тестами игр.

## Бэкенды рендера по платформам (wgpu 23)

Прочитано в `Cargo.toml` wgpu 23.0.1 / wgpu-core / wgpu-hal:

* По умолчанию wgpu включает `wgsl`, `dx12`, `metal`, `webgpu`. Движку нужны WGSL, а DX12 —
  только на Windows; `metal` на Windows / Linux / Android ничего не собирает, `webgpu`
  (писатель WGSL в naga) нужен только в браузере. `engine-render` берёт wgpu без
  умолчаний и добавляет по цели: Windows — `dx12`, wasm — `webgpu` (+ `webgl` по запросу).
* **Vulkan и GLES wgpu 23 подключает сам**, из своих таблиц зависимостей по цели
  (`cfg(windows)`, `cfg(unix)` → `wgpu-core/vulkan` и `gles`): отключить GLES на Windows,
  Linux и Android флагами нельзя, это потребовало бы форка wgpu. Поэтому «Windows без
  GLES» невозможно без патча wgpu.
* Браузер: WebGPU всегда; WebGL2 — `webgl`, его можно убрать (`--no-webgl`).

## Измерения (Windows MSVC, release: opt-level 3, fat LTO)

Размер кода по крейтам считает `tools/size_report_pdb.py` (читает PDB; `cargo bloat` на MSVC
не работает), для wasm — `tools/size_report.py`. Таблица `swarm` (2D, звук, интерфейс),
КиБ кода функций:

| крейт | было | стало | | крейт | было | стало |
|---|---|---|---|---|---|---|
| core (дженерики) | 1369 | 903 | | rapier2d + parry2d | 488 | 0 |
| naga | 1105 | 971 | | rapier3d + parry3d | 799 | 0 |
| wgpu_hal | 569 | 492 | | engine_physics | 118 | 0 |
| wgpu_core | 545 | 446 | | engine_scenario | 452 | 0 |
| engine_render | 456 | 364 | | zune_jpeg | 268 | 0 |
| serde_core | 440 | 41 | | gltf + gltf_json + serde_json | 224 | 0 |
| engine (фасад) | 289 | 43 | | lewton (ogg) | 70 | 0 |
| engine_audio | 157 | 79 | | engine_assets | 179 | 102 |
| тела функций, всего | 10 298 | 5 655 | | не код (данные, таблицы) | 3 068 | 1 803 |

Размеры игр, байты (до — экспорт предыдущей волны, после — измерено; «после» собрано с
`panic = "abort"`, у `marble` тоже, см. ниже):

| игра | платформа | до | после | |
|---|---|---|---|---|
| swarm | Windows exe | 12 042 752 | 7 636 992 | −36,6 % |
| lantern | Windows exe | 12 051 968 | 7 646 208 | −36,6 % |
| marble | Windows exe | 12 541 440 | 9 060 864 | −27,8 % |
| swarm | web `game_bg.wasm` | 6 550 735 | 4 380 110 | −33,1 % |
| lantern | web `game_bg.wasm` | 6 551 032 | 4 384 757 | −33,1 % |

Средний FPS (900 кадров, vsync выключен, стартовая сцена): swarm 437 / 418 против 427 / 380,
lantern 501 против 470, marble 438 / 418 против 437 / 417 (один прогон marble — 262 из-за
помехи на машине). Скорость не упала.

Что дало сколько (по таблице, `swarm`): физика (Rapier 2D и 3D) ≈ 1,4 МиБ, Scenario ≈ 0,55 МиБ
(ВМ 452 КиБ и склейка в фасаде), JPEG 0,27 МиБ, glTF ≈ 0,22 МиБ, Ogg 70 КиБ; обрезка
wgpu (`dx12`/`wgsl` без `metal`, `webgpu`) — часть падения naga / wgpu_hal; `panic = "abort"` —
около 0,45 МиБ в `core` и ≈ 1,2 МиБ таблиц раскрутки («не код»). Вклад `abort` и обрезки wgpu
отдельно не мерили — на это не хватило сборок (их нельзя гонять в два потока бесконечно).
Рельеф в `engine-render` (≈ 130 КиБ ≈ 1 %) и частицы / тайлмапы в возможности не вынесены.
`opt-level = "s"` для десктопа не пробовали: размер и так упал на треть без потери FPS.
