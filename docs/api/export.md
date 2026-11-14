# Экспорт игры: браузер, Windows, Android, Linux (и как устроен крейт игры)

Игра на Piska Engine — это **крейт рабочего пространства** в `games/<имя>/` со своими
ассетами. Одна функция `build()` собирает приложение, макрос `engine::game_main!` делает
из неё программу для каждой платформы, а инструмент `piska-export` собирает готовую
раздачу в `dist/<имя>/<платформа>/`.

| платформа | команда | что получается | статус |
|---|---|---|---|
| браузер | `piska-export web` | `index.html`, `game.js`, `game_bg.wasm`, `assets.pak` | работает (WebGPU, запасной WebGL2) |
| Windows | `piska-export windows` | `<игра>.exe` + `assets/` | работает (собирать на Windows) |
| Android | `piska-export android` | `<игра>.apk` — подписанный, выровненный, без Gradle | APK собирается и проверяется статически; на устройстве **ни разу не запускался** (§9.7) |
| Linux | `piska-export linux` | программа + `assets/` + значок + `.desktop`, `--tar` — `.tar.gz` | на Linux — обычная сборка; с Windows — через zig (§10.2); собран здесь кросс-сборкой и проверен статически, **не запускался** (§10.6) |

Готовый пример — `games/template` («Звездопад»): спрайты, физика 2D, префаб, сцена из
файла, интерфейс, звук (клип из ассетов и синтезатор), сохранение рекорда.

```bash
cargo run -p template                                          # играть на десктопе
cargo run -p piska-export -- web --game template --release     # собрать для браузера
cargo run -p piska-export -- serve dist/template/web --open    # открыть в браузере
```

---

## 1. Как создать крейт игры

### 1.1. Каталог

```text
games/my_game/
├── Cargo.toml
├── assets/              ← всё, что игра загружает: картинки, звук, сцены, префабы (+ .meta)
│   ├── scenes/main.scene
│   ├── prefabs/…
│   └── sprites/…
├── src/
│   ├── lib.rs           ← компоненты, pub fn build(), engine::game_main!(…)
│   ├── main.rs          ← три строки: вызывает my_game::main()
│   ├── scripts/         ← скрипты на Rust из редактора; mod.rs пишет редактор (§5)
│   └── bin/my_game-editor.rs ← engine::editor_main!(…): игра в редакторе (§5)
└── tests/               ← тесты без окна (app.run_headless)
```

Новый проект удобнее создать в Piska Hub (New Project): шаблоны Empty 2D / Empty 3D
пишут всё дерево ниже, а их поведение — сценарий Scenario, поэтому проект открывается
общим редактором `piska-editor --project <каталог>` **без компиляции** (docs/api/hub.md, §3);
свой код на Rust (`src/scripts/`) или C++ (`cpp/`) включает сборку программы редактора
проекта. Экспорт (`piska-export`) по-прежнему собирает крейт игры.

Проще всего скопировать `games/template` целиком, переименовать каталог и пакет и
заменить игру. Новый каталог в `games/` подхватывается рабочим пространством сам
(`members = ["games/*"]` в корневом `Cargo.toml`) — регистрировать не нужно.

### 1.2. `Cargo.toml`

```toml
[package]
name = "my_game"                     # имя пакета: cargo run -p my_game, --game my_game
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

# Всё необязательно.
[package.metadata.piska]
title = "My Game"                    # заголовок страницы, имя приложения (по умолчанию — имя пакета)
icon = "sprites/icon.png"            # свой значок: PNG из assets/ (иначе assets/icon.png, иначе значок движка), см. §12

[package.metadata.piska.android]     # см. §9.3
package = "com.example.my_game"      # id приложения (по умолчанию org.piska.<каталог>)
version_code = 1                     # растёт с каждой публикацией в магазине
orientation = "landscape"            # "landscape" (по умолчанию) | "portrait" | "any"
# Эту таблицу можно править в редакторе: Project Settings → Build (с проверкой значений;
# остальной Cargo.toml и комментарии не меняются) — см. settings.md, «Editor: Build».

[dependencies]
# Без редактора и только с теми подсистемами, что нужны игре (см. ниже).
engine = { path = "../../crates/engine", default-features = false, features = ["physics2d", "audio", "ui", "gamepad"] }
log.workspace = true
```

`default-features = false` убирает из игры редактор **и** все подсистемы: перечислите в
`features` то, чем игра пользуется, — остальное не попадёт в exe / wasm / apk. Список
возможностей (`physics2d`, `physics3d`, `audio`, `audio-wav`, `audio-ogg`, `ui`, `cpp`,
`scenario`, `gamepad`, `mesh-gltf`, `image-jpeg`) и что каждая даёт — в
`docs/design/engine-features.md`. Не хотите выбирать — `features = ["full"]`: весь рантайм,
как раньше. Не хватает возможности — компилятор скажет «cannot find type `Rigidbody`»:
загляните в таблицу. `piska-export` печатает включённые возможности движка при каждой сборке.

Если хотите запускать редактор для своей игры (см. §5), заведите отдельный пример или
бинарник с `engine = { path = …, features = ["editor"] }` (редактору нужен весь рантайм,
возможность тянет `full`) — в экспорт он не попадает.

Каталог `games/<имя>` должен совпадать с аргументом `--game`; имя пакета может быть любым.

### 1.3. `src/lib.rs`

```rust
use engine::prelude::*;

component! {
    /// Игрок.
    pub struct Player { pub speed: f32 = 6.0 }
}
impl Component for Player {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let run = ctx.input().axis("Horizontal");
        ctx.transform().position.x += run * self.speed * dt;
    }
}

pub fn register(app: &mut App) {
    app.register::<Player>();
}

/// Собрать игру. Тесты вызывают её же (или вариант с `SaveData::in_memory()`).
pub fn build() -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    app.world.resources.insert(engine::save_data("My Game"));   // файл / localStorage
    engine::mount_assets(&mut app, engine::assets_dir!())?;      // каталог / пак
    engine::load_scene(&mut app, "scenes/main.scene")?;          // путь внутри assets
    Ok(app)
}

engine::game_main!(build, WindowConfig::titled("My Game").with_size(1280, 720));
```

`engine::game_main!(build, config)` создаёт точки входа:

| платформа | что генерируется |
|---|---|
| десктоп (Windows, Linux, macOS) | `pub fn main() -> Result<(), EngineError>` — его вызывает `src/main.rs` |
| браузер (`wasm32`) | экспорт `piska_start()`; `index.html` вызывает его после щелчка «Play»: скачивается `assets.pak`, вызывается `build()`, игра привязывается к `<canvas id="piska-canvas">` |
| Android | `#[no_mangle] fn android_main(app)`: пак ассетов из APK, `build()`, окно — экран |

### 1.4. `src/main.rs`

```rust
// В release-сборке на Windows — без окна консоли.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), engine::EngineError> {
    my_game::main()
}
```

### 1.5. Правила переносимого кода

Одна и та же `build()` работает на десктопе, в браузере и на Android, если соблюдать:

| нельзя | нужно | почему |
|---|---|---|
| `std::fs::read`, `File::open`, `path_of(guid)` + чтение файла | `database.read(guid)`, `database.read_path("sounds/x.wav")`, `database.read_text(guid)` | в браузере и APK нет файловой системы — ассеты в паке |
| `app.world.load_scene("assets/scenes/x.scene")` | `engine::load_scene(&mut app, "scenes/x.scene")`; из скрипта — `ctx.load_scene("x")` | первое читает файл с диска |
| `engine::mount_assets(&mut app, "assets")` с путём от текущего каталога | `engine::mount_assets(&mut app, engine::assets_dir!())` | `assets_dir!()` находит `assets/` и при `cargo run`, и рядом с экспортированным exe |
| `SaveData::open("Game")` | `engine::save_data("Game")` | в браузере сохранения живут в `localStorage` |
| `std::time::Instant`, `SystemTime` | `ctx.time()`, таймеры движка; если очень нужно — крейт `web-time` | в браузере `std::time` паникует |
| `std::thread::spawn` | всё в кадре; тяжёлое — по частям | в `wasm32-unknown-unknown` нет потоков |
| `std::env::var`, аргументы командной строки | настройки в `SaveData` или константах | в браузере окружения нет (переменные просто «не заданы») |
| динамические C++-плагины (`add_cpp_plugins`) | Rust; C++ — статически и только не в браузере (§7) | в браузере нет `dlopen` |

Звук из файла ассетов, работающий и из пака:

```rust
let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
let bytes = db.read_path("sounds/catch.wav")?;
let clip = AudioClip::from_bytes(&bytes).map_err(|e| engine::assets::AssetError::Import {
    path: "sounds/catch.wav".into(),
    reason: e.to_string(),
})?;
app.world.resources.get_mut::<Audio>().unwrap().add_clip("catch", clip);
// дальше: audio.play("catch")
```

Синтезированные звуки (`synth::sfx::*`, `synth::music::*`) работают везде.

### 1.6. Ассеты

* Всё, что загружается, лежит в `games/<имя>/assets/`. Рядом с каждым файлом — `.meta`
  с постоянным GUID; сцены и префабы ссылаются на ассеты по GUID (`Asset("<guid>")`).
  Недостающие `.meta` создаёт `AssetDatabase::open` (запуск игры на десктопе, редактор,
  или сам экспорт перед упаковкой). **Кладите `.meta` в систему контроля версий** —
  иначе GUID поменяются.
* Префабы: `ctx.instantiate_at("prefabs/star.prefab", pos)` — путь относительно `assets/`.
* Сцены: `ctx.load_scene("level2")` ищет `level2`, `level2.scene`, `scenes/level2`,
  `scenes/level2.scene` среди ассетов.
* Пути — всегда через `/` и относительно `assets/`.
* Настройки проекта — файл `assets/project.settings` (Edit → Project Settings). Он
  упаковывается вместе с остальными ассетами (пак браузера и Android, каталог `assets/`
  на десктопе) и читается при старте игры в `engine::mount_assets`; раздел `player`
  (заголовок, размер, полный экран, vsync, FPS, значок, сцена по умолчанию) накладывается
  на `WindowConfig` из `game_main!`. Все разделы — `docs/api/settings.md`.

### 1.7. Тест без окна

```rust
// games/my_game/tests/headless.rs
use engine::prelude::*;

#[test]
fn the_game_runs() {
    std::env::set_var("ENGINE_AUDIO", "off");          // не открывать звуковую карту
    let mut app = my_game::build().unwrap();
    app.run_headless(600, 1.0 / 60.0);                  // 10 секунд игры
    assert!(app.world.scene.find_by_name("Player").is_some());
}
```

`cargo test -p my_game`. Чтобы проверить игру «как в браузере» (из пака, без каталога), см.
`games/template/tests/packed.rs`: `engine::assets::install_pack(..)` до `build()` —
отдельным файлом теста (пак ставится на весь процесс).

### 1.8. Проверка, что игра соберётся для браузера

```bash
cargo check -p my_game --target wasm32-unknown-unknown
```

Скрипт `tools/check-wasm.sh` проверяет так все игры из `games/` и движок
(`rustup target add wasm32-unknown-unknown` — один раз).

---

## 2. Экспорт в браузер

```bash
cargo run -p piska-export -- web --game template             # отладочная сборка
cargo run -p piska-export -- web --game template --release   # для публикации
cargo run -p piska-export -- web --game template --out site  # свой каталог
```

Шаги (их же показывает окно Build редактора): проверка тулчейна → пак ассетов →
`cargo rustc --lib --target wasm32-unknown-unknown --crate-type cdylib` →
wasm-bindgen (встроен в `piska-export` как библиотека той же версии, что в
`Cargo.lock`; отдельный `wasm-bindgen-cli` ставить не нужно) → `index.html`.

Результат — `dist/<игра>/web/`:

| файл | что |
|---|---|
| `index.html` | холст на всё окно, полоса загрузки, кнопка **Play**, сообщение, если браузер не умеет ни WebGPU, ни WebGL2, и текст ошибки, если игра упала |
| `game.js` | обвязка wasm-bindgen (ES-модуль) |
| `game_bg.wasm` | код игры и движка |
| `assets.pak` | все ассеты с `.meta` одним файлом |

**Почему кнопка Play.** Браузеры не дают звуку играть, пока игрок не щёлкнул по странице.
Игра стартует по щелчку — звук работает сразу. Пока wasm скачивается и компилируется,
показывается полоса загрузки.

**Почему пак скачивается, а не вшит в wasm.** `assets.pak` — отдельный файл: браузер
кэширует его отдельно от кода (поменяли код — ассеты заново не качаются), wasm не
раздувается данными (меньше памяти: байты не живут дважды — в сегменте данных и в
копии), а смена ассетов не требует пересборки Rust. Игра скачивает пак после «Play»
(индикатор показывает «Loading assets…»); для больших игр это секунды — учитывайте при
выборе размеров текстур и музыки.

### 2.1. Размеры

`games/template` (пак ассетов 19 КиБ), Rust 1.97, wasm-bindgen 0.2.130. Release-сборка
компилируется долго (LTO по всему движку: ~15 минут на этой машине с нуля), отладочная —
~10 минут с нуля и быстро при повторных сборках:

| сборка | `game_bg.wasm` | после gzip | `game.js` |
|---|---|---|---|
| отладочная (`web`) | 11.1 МиБ | 3.1 МиБ | 166 КиБ |
| `--release` (профиль `web-release`: `opt-level = "s"`, LTO) | 5.3 МиБ | 2.0 МиБ | 133 КиБ |

Размер почти целиком — движок (wgpu с бэкендами WebGPU и WebGL2, naga, рендер,
интерфейс, звук); своя игра добавляет сотни КиБ. Подсистемы выбираются возможностями
движка (`docs/design/engine-features.md`): игра без физики и сценариев не тащит Rapier и
виртуальную машину Scenario. После перехода на возможности: `swarm` и `lantern`
`game_bg.wasm` **4.18 МиБ** (было 6.25 МиБ; gzip 1.59 МиБ), десктопные exe — 7.3 МиБ
вместо 11.5 МиБ (см. `docs/design/engine-features.md`, раздел «Измерения»).
`wasm-opt` (binaryen) в поставке нет; если он есть в PATH (или в `PISKA_WASM_OPT`),
`piska-export web --release` применяет `wasm-opt -Oz` сам (не проверено: на машине
разработки его нет). `--no-webgl` убирает запасной WebGL2 и делает wasm меньше ещё.

### 2.2. Проверить локально

```bash
cargo run -p piska-export -- serve dist/template/web --open     # http://127.0.0.1:8080/
cargo run -p piska-export -- serve dist/template/web --port 9000
```

Сервер слушает только `127.0.0.1`, отдаёт `.wasm` как `application/wasm` и запрещает
кэш (после пересборки обновите страницу). Пока сервер запущен через `cargo run`, его exe занят — если
вы меняете сам `piska-export`, остановите сервер перед пересборкой (окно Build редактора
делает это само). Открыть `index.html` двойным щелчком нельзя:
браузер не загружает модули и wasm с `file://`.

### 2.3. Публикация (хостинг)

* Подойдёт любой статический хостинг (GitHub Pages, Netlify, itch.io как HTML5-игра,
  свой nginx): выложите содержимое каталога как есть.
* `.wasm` должен отдаваться с `Content-Type: application/wasm` (иначе браузер не
  компилирует его потоково и пишет предупреждение; игра всё равно запустится).
* Включите gzip или brotli для `.wasm`, `.js`, `.pak` — wasm сжимается примерно втрое.
* **WebGPU работает только в «безопасном контексте»: HTTPS или `localhost`/`127.0.0.1`.**
  По голому `http://` с другого адреса браузер не даст WebGPU, и игра уйдёт на WebGL2
  (если и его нет — страница так и скажет).
* Заголовки COOP/COEP не нужны (потоков и `SharedArrayBuffer` нет).
* itch.io: заархивируйте содержимое `dist/<игра>/web/` в zip, тип проекта — HTML,
  «This file will be played in the browser».

### 2.4. WebGPU и WebGL2

Рендер сначала пробует WebGPU; если браузер его не поддерживает или не даёт адаптер —
WebGL2 (`wgpu`, фича `webgl`). Чем рисует игра — видно в консоли браузера
(`renderer: … (BrowserWebGpu)` / `(Gl)`) и в атрибуте `<body data-piska-backend>`.
Принудительно WebGL2 (проверить запасной путь): `index.html?backend=webgl`.

Что рендер требует от устройства и как с этим у WebGL2:

| что использует рендер | WebGPU | WebGL2 |
|---|---|---|
| HDR-цель `Rgba16Float` | да | только с расширением `EXT_color_buffer_float` (есть почти везде на десктопе и в новых телефонах); без него — понятная ошибка на странице |
| MSAA ×4 этой цели с разрешением выборок | да | с тем же расширением |
| тени: сэмплер со сравнением глубины | да | да |
| анизотропия 16× | да | если есть `EXT_texture_filter_anisotropic`, иначе обычная фильтрация |
| uniform-буферы с динамическим смещением | да | да |
| storage-буферы, compute | не используются | — |

Лимиты устройства берутся под бэкенд: для WebGL2 — `Limits::downlevel_webgl2_defaults()`
(у WebGL2 нет storage-буферов и compute, рендеру они и не нужны). Проверка возможностей
(`Rgba16Float` с MSAA ×4) делается до создания устройства — на слабой видеокарте игра
не падает, а показывает, чего не хватает.

### 2.5. Ввод и холст в браузере

* Холст `<canvas id="piska-canvas">` растянут CSS-ом на всё окно; размер в пикселях
  учитывает `devicePixelRatio` (чёткая картинка на HiDPI и телефонах). `input.screen_size()`
  — в тех же пикселях, что `mouse_position()` и касания.
* Клавиатура, мышь, колесо, набор текста и касания приходят так же, как на десктопе.
  Клавиатура достаётся холсту после щелчка (Play фокусирует его); стрелки и пробел не
  прокручивают страницу.
* Геймпады в браузере пока не поддерживаются (gilrs не умеет веб).
* Паника выводится в консоль браузера целиком и показывается на странице.
* Лог движка (`log::info!` и т. п.) — в консоль браузера.

---

## 3. Экспорт для Windows

```bash
cargo run -p piska-export -- windows --game template --release
```

Результат — `dist/template/windows/`: `template.exe` (в release — без окна консоли) и
каталог `assets/` рядом. Игра находит ассеты через `engine::assets_dir!()`: сначала
`assets/` рядом с exe, иначе каталог крейта (для `cargo run`). Раздавать — каталог
целиком (zip). Собирается только на Windows. С тулчейном `x86_64-pc-windows-msvc` (как на этой машине)
игре нужен «Microsoft Visual C++ Redistributable» — на большинстве машин с Windows он уже
стоит.

---

## 4. Сводка команд `piska-export`

| команда | |
|---|---|
| `web --game <имя> [--release] [--out <каталог>]` | сборка для браузера |
| `windows --game <имя> [--release] [--out <каталог>]` | сборка для Windows |
| `android --game <имя> [--release] [--out <каталог>] [--abi arm64-v8a[,x86_64]]` | APK (§9) |
| `android … --keystore <файл> --key-alias <имя> [--keystore-pass-file <файл>]` | APK, подписанный своим ключом (§9.4) |
| `linux --game <имя> [--release] [--out <каталог>] [--tar] [--zig <zig.exe>] [--glibc 2.31]` | сборка для Linux x86_64 (§10) |
| `serve [<каталог>] [--port 8080] [--open]` | локальный сервер для проверки |
| `pack --game <имя> --out <файл.pak>` | только пак ассетов |
| `list` | игры в `games/` |
| `project-pack <каталог> [-o файл.pks]` / `project-unpack <файл.pks> <каталог>` / `project-info <файл.pks>` | исходник проекта одним файлом и обратно (`docs/api/project-file.md`) |
| `--root <каталог>` | корень рабочего пространства, если запускаете не из него |
| `--no-webgl` | (web) без запасного WebGL2: wasm меньше, но игра идёт только в браузерах с WebGPU |
| `list --json` | игры рабочего пространства с заголовком и включёнными возможностями движка (`engine_features`, `engine_features_effective`) |

Каждая сборка печатает `engine features of <игра>: …` — что игра линкует. `--release` на
десктопе и Android собирается с `panic = "abort"` (код меньше и быстрее), если у игры нет
возможности `cpp`: мосту C++ нужна раскрутка стека (`catch_unwind` вокруг сервисов).
`wasm-opt` из PATH (или `PISKA_WASM_OPT`) применяется к release-веб-сборке сам.

Строки `piska-export: [n/m] …` — шаги экспорта (их показывает редактор). Код выхода не
ноль — экспорт не удался; причина — последней строкой `error: …`.

---

## 5. Экспорт из редактора: File → Build…

```rust
engine::edit(app, EditorConfig::titled("My Game")
    .open_scene("scenes/main.scene")
    .game_crate("my_game"))            // крейт games/my_game
// .workspace("…") — если редактор запущен не из корня рабочего пространства
```

Окно **Build**: игра (`games/<имя>`), платформа (только те, что эта машина умеет
собрать: Web и Android — везде, Windows — на Windows, Linux — на Linux или если zig есть в `PISKA_ZIG` или PATH),
Debug / Release, каталог результата
(пусто — `dist/<имя>/<платформа>`). **Build** запускает `cargo run -p piska-export -- …`
в фоне — вывод компилятора идёт в Console, в окне виден текущий шаг. После удачной
сборки: **Reveal in File Manager** и для веба **Run in Browser** (поднимает
`piska-export serve` на порту 8080 и открывает страницу; сервер останавливается вместе с
редактором). Для Linux окно добавляет `--tar`. Чего не хватает для Android (SDK, NDK,
Java, cargo-ndk) — последней строкой `error: …` в Console. Без `game_crate` пункт меню серый и подсказка объясняет почему.

Из кода и тестов: `Action::ShowBuildDialog(bool)`, `Action::Export(ExportSettings {
platform, release, out_dir })`, `Action::RevealExport`, `Action::RunInBrowser`;
`editor.export_command(&settings)` — какая команда будет запущена.

**Игра в редакторе.** У крейта игры есть своя программа редактора: фича
`editor = ["engine/editor"]`, `[[bin]] name = "<имя>-editor"` с
`required-features = ["editor"]` и файл `src/bin/<имя>-editor.rs` из одной строки
`engine::editor_main!(<имя>::build, |config| config.open_scene("scenes/main.scene"));`.
Запуск: `cargo run -p <имя> --features editor --bin <имя>-editor`. Скрипты на Rust,
созданные в нём (New Rust Script…), ложатся в `src/scripts/`; `src/scripts/mod.rs` пишет
редактор, а `register()` игры один раз вызывает `scripts::register(app)`. Build в этом
редакторе пересобирает программу и перезапускает её с той же сценой — подробно в
`editor.md`, «Скрипты на Rust». Экспорт собирает `--lib` / `--bins` без фичи `editor`,
поэтому программа редактора в игру не попадает. `games/template` уже так устроен.

---

## 6. Ассеты без файловой системы (для авторов движка и инструментов)

```rust
use engine::assets::{AssetDatabase, AssetPack, install_pack};

// Пак: магия "PISKAPK1", число записей, (u16 длина пути, путь, u32 длина, байты)…
let mut builder = AssetPack::builder();
builder.add_dir("games/template/assets")?;          // все файлы + .meta
let bytes = builder.to_bytes();

let pack = AssetPack::from_bytes(bytes)?;           // &'static [u8] не копируется
let db = AssetDatabase::from_pack(pack)?;           // GUID и настройки — из .meta в паке
let png = db.read(db.guid_of("sprites/star.png").unwrap())?;   // Cow<[u8]>
let mesh = db.load_mesh(guid)?;                      // .obj / .gltf (внешние буферы — из пака) / .glb

install_pack(pack);                                  // для всего процесса:
let db = AssetDatabase::mount("assets")?;            // → из пака, путь не используется
```

* `AssetDatabase::read` / `read_path` / `read_text` / `load_mesh` работают одинаково для
  каталога и пака. `path_of` у базы из пака возвращает условный путь
  `<asset pack>/…` — файла на диске нет.
* `PrefabSource` базы (префабы для `instantiate`) и `AssetScenes` (сцены для
  `load_scene`) читают через `read` — работают из пака.
* Рендер грузит текстуры и меши через `read` / `load_mesh`.
* `engine::mount_assets` = `AssetDatabase::mount` + префабы + `AssetScenes`.

Что из движка **ещё читает файлы напрямую** (в браузере не загрузится; в логе будет
ошибка, игра продолжит работать):

| что | где | обход в игре |
|---|---|---|
| звуковые клипы по ссылке-ассету (`AudioSource.clip = Asset(..)`, `audio.play(handle)`) | `engine-audio`, `Audio::load_asset` | `AudioClip::from_bytes(&db.read_path(..)?)` + `add_clip("имя", ..)`, играть по имени |
| шрифты интерфейса из ассетов (`Text.font`) | `engine-ui`, `text.rs` | встроенный шрифт (латиница и кириллица) |
| размер картинки для 9-slice (`Image` с `slice`) | `engine-ui`, `texture_size` | 9-slice без текстуры-ассета или обычные картинки |
| меш-коллайдеры 3D из ассетов | `engine-physics`, `physics3d.rs` | примитивные коллайдеры |

---

## 7. C++ в браузере

Игровая логика на C++ в веб-сборку **не входит**: в `wasm32` `engine-cpp-bridge`
собирается без статических C++-модулей (`noise`, `builtin_gameplay`) и без динамических
плагинов. На десктопе и Android всё как прежде.

Почему:

1. У цели `wasm32-unknown-unknown` нет ни libc, ни стандартной библиотеки C++. Код
   движка на C++ (`plugins/cpp-gameplay`, `cpp/noise.cpp`) использует `<vector>`,
   `<algorithm>`, `<cmath>`, `<cstdio>` — без libc++ он не компилируется.
2. Компилятор есть: clang из Android NDK (уже установлен) умеет `--target=wasm32` и
   содержит `wasm-ld`. Но заголовки libc++ из NDK настроены под bionic (Linux): с ними
   компиляция под wasm32 падает (`No thread API`, `asm/types.h not found`) — проверено.
3. Динамических библиотек в браузере нет: `CppPlugins::load_hot` и `add_cpp_plugins`
   возвращают ошибку «this platform cannot load dynamic libraries».

Самый дешёвый путь включить (оценка: 1–2 дня):

1. Скачать `wasi-sysroot` из релизов wasi-sdk (libc++, libc++abi, wasi-libc под
   `wasm32-wasi`, ≈ 50 МБ) — это единственное, чего нет на машине.
2. В `crates/engine-cpp-bridge/build.rs` для `wasm32` компилировать C++ clang-ом из NDK:
   `--target=wasm32-wasi --sysroot=<wasi-sysroot> -fno-exceptions -fno-rtti` и линковать
   `libc++.a`, `libc++abi.a`, `libc.a` в модуль Rust. ABI совпадает: Rust 1.97 на
   `wasm32-unknown-unknown` использует стандартный C ABI WebAssembly, а граница движка —
   указатели и скаляры.
3. Закрыть импорты WASI, которые тянет libc (`fd_write` для `printf`, `proc_exit`,
   `clock_time_get`): определить их в Rust заглушками (`__imported_wasi_snapshot_preview1_*`)
   — иначе wasm-bindgen оставит неразрешённые импорты.
4. Вызвать статические конструкторы C++ (`__wasm_call_ctors`) до регистрации
   компонентов: на них держится саморегистрация компонентов в `engine_component.hpp`.
5. Включить `cfg(engine_static_cpp)` для wasm в `build.rs`, после чего `builtin_gameplay`
   и `noise` станут доступны и в браузере.

Альтернатива без libc++ — писать веб-совместимый C++ в свободностоящем подмножестве
(без STL, `-ffreestanding`, `-nostdlib`), но существующий код движка этому не
соответствует, и это сильное ограничение для авторов игр — поэтому не сделано.

---

## 8. Ограничения и известные проблемы

* Геймпады в браузере не работают (gilrs не поддерживает веб).
* Звуковые клипы, шрифты, 9-slice и меш-коллайдеры из ассетов в браузере грузятся
  только описанными в §6 обходами (до доработки `engine-audio`, `engine-ui`, `engine-physics`).
* В браузере игра не может выйти (`ctx.quit()` останавливает цикл кадров, вкладка остаётся).
* `ENGINE_FRAMES`, `ENGINE_SCREENSHOT` в браузере не действуют.
* Многопоточности в веб-сборке нет; всё, что движок делает в потоках на десктопе, в
  браузере не используется.
* Экспорт для Windows делается только на Windows (`piska-export` устроен так, что
  платформа — отдельный модуль и строка в `Platform`).
* Android и Linux — свои ограничения в §9.6 и §10.5.

---

## 9. Экспорт для Android

```bash
cargo run -p piska-export -- android --game template                 # отладочная сборка
cargo run -p piska-export -- android --game template --release       # для раздачи
adb install -r dist/template/android/template.apk                    # поставить на телефон
adb logcat -s piska RustStdoutStderr                                 # лог игры
```

Результат — `dist/<игра>/android/<игра>.apk`: подписанный, выровненный (`zipalign`,
страницы `.so` по 16 КиБ), ставится `adb install`. Gradle и Android Studio не нужны.

### 9.1. Что нужно на машине

| что | зачем | как поставить |
|---|---|---|
| Android SDK | `aapt2`, `zipalign`, `apksigner` (build-tools), `android.jar` (platforms) | Android Studio → SDK Manager, или `sdkmanager "build-tools;36.0.0" "platforms;android-35"` |
| Android NDK r23+ | clang для C++-моста, линковщик | SDK Manager → SDK Tools → NDK (Side by side), или `sdkmanager "ndk;28.2.13676358"` |
| Java 11+ | `apksigner`, `keytool` | любой JDK (Temurin, JDK из Android Studio); `JAVA_HOME` или `java` в PATH |
| cargo-ndk | собирает Rust и C++ под Android | `cargo install cargo-ndk` |
| цель Rust | | `rustup target add aarch64-linux-android` |

Где ищется SDK: `ANDROID_HOME`, `ANDROID_SDK_ROOT`, затем стандартные места —
`%LOCALAPPDATA%\Android\Sdk` (Windows), `~/Library/Android/sdk` (macOS), `~/Android/Sdk`
(Linux). NDK: `ANDROID_NDK_HOME` / `ANDROID_NDK_ROOT`, иначе самый новый `<SDK>/ndk/<версия>`.
Build-tools и платформа — самые новые установленные. Если чего-то нет, экспорт
останавливается на первом шаге и пишет, что поставить.

### 9.2. Шаги

1. проверка SDK / NDK / build-tools / платформы / Java / cargo-ndk / цели Rust;
2. пак ассетов (как для веба);
3. `cargo ndk -t arm64-v8a -P 26 rustc -p <пакет> --lib --profile <dev|release> --crate-type cdylib`
   → `lib<пакет>.so` (C++-мост собирается clang-ом NDK);
4. проверка библиотеки: отладочная информация снимается (`llvm-strip --strip-debug`),
   библиотека экспортирует `ANativeActivity_onCreate`, каждая `NEEDED`-библиотека есть
   в Android (`libc`, `libm`, `libdl`, `liblog`, `libandroid`, `libaaudio`, `libEGL`,
   `libGLESv2/3`, `libvulkan`…) или кладётся в APK (`libc++_shared.so` из NDK — её тянет
   C++-мост);
5. `AndroidManifest.xml` и значок → `aapt2 compile` + `aapt2 link` против `android.jar`;
6. в архив добавляются `lib/<abi>/*.so` и `assets/assets.pak` **без сжатия**
   (`extractNativeLibs="false"`: Android загружает библиотеку прямо из APK) → `zipalign -P 16 4`;
7. `apksigner sign` → `apksigner verify`.

Промежуточные файлы — в `<target>/piska-android/<игра>/` (манифест, `base.apk`, значок).

### 9.3. Манифест

Генерируется из `[package.metadata.piska]` (§1.2):

| поле | по умолчанию | в манифесте |
|---|---|---|
| `title` | имя пакета | `android:label` |
| `android.package` | `org.piska.<каталог игры>` | `package` (id приложения: две+ части через точку, буквы / цифры / `_`) |
| `android.version_code` | `1` | `android:versionCode` |
| `android.version_name` | версия пакета | `android:versionName` |
| `android.orientation` | `"landscape"` | `sensorLandscape`; `"portrait"` → `sensorPortrait`; `"any"` → `fullUser` |
| `android.min_sdk` | `26` | `minSdkVersion` (меньше 26 нельзя: звук идёт через AAudio, он появился в Android 8.0) |
| — | установленная платформа | `targetSdkVersion` |
| `icon` | значок движка (буква P на синей плитке) | `@mipmap/ic_launcher`, 192×192 |

Остальное фиксировано: одна `android.app.NativeActivity` без Java-кода
(`android:hasCode="false"`, `android.app.lib_name = <пакет>`), тема
`Theme.Black.NoTitleBar.Fullscreen` (без заголовка и строки состояния), `singleTask`,
`configChanges` на поворот / размер / клавиатуру — при повороте активность не
пересоздаётся, игра получает новый размер экрана; `glEsVersion 3.0` обязателен,
сенсорный экран и геймпад — нет; `appCategory="game"`.

### 9.4. Подпись

**Отладочный ключ** (по умолчанию): `~/.android/debug.keystore` (или
`$ANDROID_USER_HOME/debug.keystore`) — тот же, что у Android Studio; если его нет,
экспорт создаёт его `keytool`-ом. Пароль у этого ключа общеизвестный (`android`) и
передаётся инструментам через переменную окружения. Такой APK годится для своих
устройств и тестировщиков, но не для Google Play.

**Свой ключ.** Создать один раз (keytool спросит пароль сам — в командной строке его нет):

```bash
keytool -genkeypair -keystore my-release.keystore -alias game -keyalg RSA -keysize 2048 -validity 10000
```

Собрать:

```bash
# пароль — в файле (одна строка; файл не кладите в git):
cargo run -p piska-export -- android --game my_game --release \
    --keystore my-release.keystore --key-alias game --keystore-pass-file ~/.secrets/game-ks.txt

# или в переменной окружения (CI: секрет репозитория):
#   PISKA_KEYSTORE_PASSWORD — пароль хранилища, PISKA_KEY_PASSWORD — пароль ключа, если другой
cargo run -p piska-export -- android --game my_game --release --keystore my-release.keystore --key-alias game
```

Без файла и переменной `apksigner` спрашивает пароль в терминале (из окна Build
редактора это не работает — используйте файл или переменную). Пароль никогда не
передаётся в командной строке. Хранилище ключей держите вне репозитория: потеряете —
не сможете обновлять игру в магазине.

### 9.5. Как игра работает на Android

* **Точка входа** — `android_main` из `engine::game_main!`; `assets.pak` читается из
  `assets/` APK (`AAssetManager`), дальше `build()` как везде.
* **Жизненный цикл.** Свернули — поверхность отпускается (устройство, текстуры и мир
  остаются), все клавиши / касания отпускаются, подсистемы получают `idle` — звук
  замирает, игровое время не идёт. Вернулись — новая поверхность, игра продолжается с
  того же кадра.
* **Касания** — `input.touches()`; первый палец ещё и «мышь» (`mouse_position`, левая
  кнопка), так что игра, сделанная под мышь, играется пальцем.
* **Кнопка «Назад»** — `Key::Escape`. Выйти из игры по ней: `ctx.quit()` — активность
  закрывается, процесс завершается.
* **Экран и поворот** — `input.screen_size()` в пикселях экрана; при повороте (если
  ориентация `"any"`) приходит новый размер, поверхность перенастраивается.
* **Экранная клавиатура** открывается, пока редактируется `InputField`
  (`UiEvents::wants_keyboard()`), и закрывается после. Текст приходит событиями клавиш
  — латиница, цифры, Backspace и Enter работают с обычными клавиатурами; составной ввод
  (IME для китайского, японского; подсказки «свайпом») NativeActivity не поддерживает.
* **Сохранения** — `engine::save_data("Game")` пишет во внутренний каталог приложения
  (`/data/user/0/<id>/files/<Game>/save.ron`): другие приложения его не видят, при
  удалении игры он удаляется.
* **Лог** — logcat с тегом `piska`: `adb logcat -s piska`. Паника тоже уходит в logcat
  (с бэктрейсом).
* **Рендер** — Vulkan, а если его нет или драйвер не подошёл — OpenGL ES 3 (wgpu выбирает
  сам; что выбрано — строка `renderer: …` в логе). Рендеру нужна HDR-цель `Rgba16Float`
  с MSAA ×4; на GLES это требует `EXT_color_buffer_float` (есть почти на всех телефонах
  с GLES 3.2) — без неё игра пишет понятную ошибку в лог и закрывается.
* **Звук** — cpal через AAudio (`libaaudio.so`, Android 8.0+); C++-библиотека NDK не
  нужна звуку, только C++-мосту.
* **C++** — статические модули движка (`noise`, `builtin_gameplay`) собираются clang-ом
  NDK и работают; `libc++_shared.so` лежит в APK. Динамические плагины
  (`add_cpp_plugins`) на Android не используйте: в APK нет `.so` плагина.

### 9.6. Ограничения

* Один ABI за сборку по умолчанию — `arm64-v8a` (все телефоны последних лет). Несколько:
  `--abi arm64-v8a,x86_64` (x86_64 — эмулятор на ПК); нужна цель Rust для каждого.
* Навигационная панель остаётся видимой: полностью «погружённый» режим включается
  только вызовом Java API из кода, а Java-кода в APK нет.
* Пока игра свёрнута, поток звука открыт и играет тишину (микшер на паузе).
* Геймпады на Android не поддерживаются (gilrs не умеет Android).
* `ENGINE_FRAMES`, `ENGINE_SCREENSHOT` не действуют (переменных окружения у приложения нет).
* Google Play требует формат AAB — `piska-export` делает только APK (раздача напрямую,
  itch.io, RuStore, F-Droid, свои устройства).

### 9.7. Что проверено

На машине разработки движка (Windows 11, NDK 28.2, build-tools 36.0.0, платформа API 37,
Rust 1.97) собран `games/template`, отладочная сборка, `arm64-v8a`:

* `cargo ndk -t arm64-v8a check -p template` — без ошибок (до этого Android-код движка
  ни разу не компилировался; исправлять пришлось только новое);
* `piska-export android --game template` → `template.apk`, **27.2 МиБ** (из них
  `libtemplate.so` 19.2 МиБ после `--strip-debug`, `libc++_shared.so` 9.2 МиБ, пак 19 КиБ;
  release-сборка будет заметно меньше — её здесь не собирали ради места на диске);
* `aapt2 dump badging`: `org.piska.template`, versionCode 1, min SDK 26, target SDK 37,
  `NativeActivity`, `application-isGame`, `uses-gl-es 0x30000`, `native-code: arm64-v8a`;
* `apksigner verify`: подписи v2 и v3 верны; `zipalign -c -P 16 4`: всё выровнено,
  `.so` — по 16 КиБ;
* `llvm-readelf`: `NEEDED` = `liblog`, `libandroid`, `libc++_shared` (в APK), `libdl`,
  `libaaudio`, `libm`, `libc` — всё есть в Android 8.0+; экспортированы
  `ANativeActivity_onCreate` и `android_main`; сегменты выровнены по 16 КиБ.
  Vulkan и EGL библиотека не требует (wgpu подгружает их при запуске).

**Не проверено: APK ни разу не запускался** — на машине нет ни устройства, ни эмулятора.
Поведение на устройстве (жизненный цикл, касания, звук через AAudio, выбор Vulkan / GLES,
клавиатура, сохранения) написано по документации android-activity / winit / cpal и
прочитано в коде, но не наблюдалось. Первая проверка на телефоне:
`adb install -r … && adb logcat -s piska RustStdoutStderr`.

---

## 10. Экспорт для Linux

```bash
cargo run -p piska-export -- linux --game template --release --tar
```

Результат — `dist/<игра>/linux/`:

| файл | что |
|---|---|
| `<пакет>` | программа (ELF x86_64) |
| `assets/` | ассеты рядом, как у Windows (`engine::assets_dir!()` находит их возле программы) |
| `icon.png` | значок 256×256 (`icon` из `[package.metadata.piska]`, иначе `assets/icon.png`, иначе значок движка) |
| `<игра>.desktop` | ярлык для меню приложений (шаблон: `@DIR@` заменяет скрипт ниже) |
| `install-desktop-entry.sh` | добавить игру в меню пользователя (`--remove` — убрать) |
| `<игра>-linux-x86_64.tar.gz` | с `--tar`: всё это в каталоге `<игра>/`, с битом «исполняемый» — это и раздают |

Игрок: распаковать, запустить `./<пакет>` (по желанию — `./install-desktop-entry.sh`).

### 10.1. Сборка на Linux (основной путь)

Обычный `cargo build`. Пакеты для **сборки**:

| дистрибутив | команда |
|---|---|
| Debian / Ubuntu | `sudo apt install build-essential pkg-config libasound2-dev libudev-dev` |
| Fedora | `sudo dnf install gcc gcc-c++ pkgconf-pkg-config alsa-lib-devel systemd-devel` |
| Arch | `sudo pacman -S base-devel pkgconf alsa-lib systemd-libs` |

Для **запуска** (у игрока): `libasound2` (ALSA; есть везде, где есть звук — PipeWire и
PulseAudio дают ALSA-совместимость), `libudev1` (systemd), драйвер Vulkan (Mesa:
`mesa-vulkan-drivers` в Debian/Ubuntu, `mesa-vulkan-drivers` в Fedora, `vulkan-radeon` /
`vulkan-intel` в Arch; у NVIDIA — драйвер NVIDIA) или OpenGL ES 3 — wgpu берёт что есть;
X11 или Wayland (winit подгружает их библиотеки сам при запуске). Программа требует
glibc не новее той, что была на машине сборки — экспорт пишет точную версию
(`requires glibc 2.xx or newer`).

**Совместимость со старыми дистрибутивами.** Собранное на свежем дистрибутиве (Ubuntu
24.04 — glibc 2.39) не запустится на Debian 11 / Ubuntu 20.04. Два пути:

* собрать в Debian 11 (glibc 2.31) — так делает CI (§11, контейнер `debian:bullseye`);
* собрать через zig (§10.2) и на Linux: `--glibc 2.31` (или `--zig <путь>`) включает тот же
  путь, что на Windows.

### 10.2. Сборка на Windows / macOS: zig

Rust компилирует под `x86_64-unknown-linux-gnu` где угодно, но для C++-моста нужен
компилятор под Linux, а для линковки — glibc, ALSA и udev, которых на Windows нет.
`piska-export` решает это одним инструментом — **zig** (компилятор C/C++ на базе clang
со встроенными заголовками и заглушками glibc любой версии, libc++ и lld):

| что | откуда |
|---|---|
| zig 0.13+ (проверено с 0.17.1) | https://ziglang.org/download/ — архив для своей системы (≈ 80 МБ распакованный); в PATH не обязательно |
| цель Rust | `rustup target add x86_64-unknown-linux-gnu` |

Где ищется zig: `--zig <путь к zig.exe>`, затем переменная `PISKA_ZIG`, затем `zig` в
PATH. Нет нигде — экспорт останавливается и пишет, где его взять.

```bash
cargo run -p piska-export -- linux --game template --tar --zig C:/tools/zig/zig.exe
PISKA_ZIG=C:/tools/zig/zig.exe cargo run -p piska-export -- linux --game template --tar --glibc 2.28
```

Как это устроено (cargo-zigbuild и pkg-config **не нужны**):

* `piska-export` кладёт в `<target>/piska-linux/bin/` жёсткие ссылки на себя с именами
  `piska-zig-cc`, `piska-zig-cxx`, `piska-zig-ar`, `piska-zig-pkg-config` и передаёт их
  cargo как линковщик и `CC` / `CXX` / `AR` цели `x86_64-unknown-linux-gnu`. Запущенный
  под таким именем, он вызывает `zig cc` / `zig c++` / `zig ar` с
  `-target x86_64-linux-gnu.<glibc>` (по умолчанию 2.31) и убирает аргументы, которые zig
  задаёт сам (`--target`, `-fuse-ld=lld`, `-B…` от rustc; `-lgcc_s` заменяется
  на libunwind zig). Стандартная библиотека C++ — libc++ из zig, статически.
* **ALSA и udev.** Звук (cpal → `alsa-sys`) и геймпады (gilrs → `libudev-sys`) линкуются с
  `libasound.so.2` и `libudev.so.1`. Их не скачивают: zig собирает **заглушки** —
  `.so` с нужным `SONAME` и пустыми функциями по списку, взятому из объявлений в
  исходниках `alsa-sys` и `libudev-sys` (≈ 1800 и 80 функций), а поддельный
  `pkg-config` говорит `build.rs` этих крейтов, где они. В программе остаются только
  записи `NEEDED`; на машине игрока загружаются настоящие библиотеки дистрибутива
  (ссылки без версий связываются с версией по умолчанию — как при обычной сборке).
* Кэш zig — в `<target>/piska-linux/zig-cache/`, заглушки — в `<target>/piska-linux/stubs-<glibc>/`.

Запустить программу на Windows нельзя — проверяется только файл (ниже).

### 10.3. Проверка результата

После сборки экспорт читает ELF сам (без внешних инструментов) и пишет в отчёт:
архитектуру (должна быть x86_64), загрузчик (`/lib64/ld-linux-x86-64.so.2`), список
`NEEDED` (предупреждение, если там библиотека не из обычного дистрибутива) и
наибольшую нужную версию glibc (предупреждение, если новее 2.31).

### 10.4. Права на исполнение

На Linux бит «исполняемый» ставится сам. Если собирали на Windows, у файла в каталоге его
нет — раздавайте `.tar.gz` (`--tar`): в архиве права записаны (`0755` у программы и скрипта).

### 10.5. Ограничения

* Только x86_64 (ARM64-Linux — тот же путь с другой целью, не сделан).
* С Windows — только через zig (§10.2). Программа, собранная так на машине разработки
  движка, проверена только статически (§10.6) — запущена не была.
* Геймпады — через udev (gilrs): нужен доступ к `/dev/input/event*` (обычно есть у
  пользователя за компьютером).

### 10.6. Что проверено

На машине разработки движка (Windows 11, zig 0.17.1, Rust 1.97) `games/template`
собран кросс-сборкой, отладочная сборка:
`piska-export linux --game template --tar --zig <путь к zig.exe>`.

* Линковка: `libasound.so.2` — заглушка на 1426 функций, `libudev.so.1` — на 82;
  C++-мост скомпилирован `zig c++` (libc++ статически).
* `llvm-readelf`: ELF64 x86-64, PIE, загрузчик `/lib64/ld-linux-x86-64.so.2`;
  `NEEDED` = `libudev.so.1`, `libasound.so.2`, `libm.so.6`, `libc.so.6`,
  `ld-linux-x86-64.so.2`, `libpthread.so.0`, `libdl.so.2` (ни `libstdc++`, ни `libgcc_s`);
  наибольшая версия символов — **GLIBC_2.30** (≤ 2.31: Debian 11, Ubuntu 20.04 и новее).
* Размер: программа 112 МиБ (отладочная сборка с номерами строк), `.tar.gz` 25 МиБ;
  release-сборку здесь не собирали ради места на диске.
* `.tar.gz`: 22 записи, у программы и скрипта права `0755`.

**Не проверено: программа ни разу не запускалась** — на машине нет Linux (ни WSL, ни
виртуальной машины). Что заглушки ALSA/udev правильно заменяются настоящими библиотеками
при запуске, следует из того, как работает динамический загрузчик, но не наблюдалось.
Первая проверка на Linux: распаковать архив и запустить `./template`.

---

## 11. Сборка в GitHub Actions

`.github/workflows/build.yml` — на каждый push / pull request:

| задача | где | что делает |
|---|---|---|
| `test` | `ubuntu-latest`, `windows-latest` | `cargo build --workspace --all-targets`, `cargo test --workspace` (на Linux — Mesa lavapipe для тестов рендера без видеокарты, `ENGINE_AUDIO=off`) |
| `export-linux` | контейнер `debian:bullseye` (glibc 2.31) | `piska-export linux --release --tar` → артефакт `template-linux-x86_64` |
| `export-windows` | `windows-latest` | `piska-export windows --release` → артефакт |
| `export-web` | `ubuntu-latest` | `piska-export web --release` → артефакт |
| `export-android` | `ubuntu-latest` (SDK, NDK, Java уже есть на образе) | cargo-ndk, `piska-export android --release` → APK (отладочный ключ раннера) |

Это документированный способ получить родную, проверенную тестами Linux-сборку. Для
подписи своим ключом в CI: хранилище — секрет в base64, пароль — секрет в
`PISKA_KEYSTORE_PASSWORD`, `--keystore` / `--key-alias` в шаге экспорта. Workflow
написан, но на машине разработки движка не запускался.

---

## 12. Иконка

Значок приложения нарисован один раз: `tools/gen_icon.py` (Python + Pillow) пишет в
`assets/branding/` исходник `icon.svg`, PNG 16…1024 px, многоразмерный `icon.ico` и
фавикон. Размеры 16, 24 и 32 px рисуются с привязкой краёв к пикселям, поэтому не «мылятся».
Перерисовать: `python tools/gen_icon.py` (результат побайтово воспроизводим).

Игра без настроек получает значок движка на всех платформах. Свой значок — один PNG
(лучше 512×512 и больше, квадратный, с прозрачными углами, если они нужны). Положите его
в `games/<игра>/assets/icon.png` — или другой путь в `assets/` и укажите его в
`[package.metadata.piska]`:

```toml
icon = "sprites/my_icon.png"
```

Порядок выбора: `icon` из настроек → `assets/icon.png` → значок движка. Что получается:

| платформа | где виден значок | как задаётся |
|---|---|---|
| окно (Windows, Linux/X11) | заголовок, панель задач, Alt-Tab | `engine::game_main!` берёт `assets/icon.png` из каталога ассетов; иначе значок движка. Явно: `WindowConfig::titled("…").with_icon_png(include_bytes!("../assets/icon.png"))` |
| `.exe` на Windows | проводник, закреплённый ярлык, панель задач | одна строка в `build.rs` игры (ниже) |
| Linux | `icon.png` 256×256 + `.desktop` | `piska-export linux` |
| Android | `@mipmap/ic_launcher` 192×192 | `piska-export android` |
| браузер | вкладка | `favicon.png` (64×64) и `<link rel="icon">` в `index.html` |

Окно на macOS, Wayland, в браузере и на Android значок окна не показывает (winit его там
не поддерживает) — на этих платформах значок задаёт бандл, пакет или страница.

### 12.1. Значок в `.exe` (Windows)

Исполняемый файл получает значок как ресурс Windows. Для этого игре нужен `build.rs` и
крейт `engine-build` (он не требует ни `rc.exe`, ни Windows SDK: ресурс пишется готовым
COFF-объектом и передаётся линкеру; на Linux, Android и в браузере ничего не делает, так что
кросс-сборки не ломаются):

```toml
# Cargo.toml игры
[build-dependencies]
engine-build = { path = "../../crates/engine-build" }
```

```rust
// build.rs игры
fn main() {
    engine_build::windows_icon();
}
```

`games/template` уже так устроен — копируйте его. Значок берётся из `PISKA_ICON` (его
выставляет `piska-export windows`, когда в `[package.metadata.piska]` задан `icon`), иначе из
`assets/icon.png` игры, иначе это значок движка. Из PNG собирается `.ico` с размерами
16, 24, 32, 48, 64, 128 и 256. Свой `.ico` — `engine_build::windows_icon_file("assets/app.ico")`.

`piska-export windows` после сборки читает таблицу ресурсов готового exe и печатает
`exe icon: 7 images (…)`; если значка нет, пишет, что добавить в `build.rs`. Если значок
задан в `[package.metadata.piska]`, экспорт кладёт его и в `assets/icon.png` рядом с exe,
чтобы окно игры показывало тот же значок.

Если значок в проводнике не обновился после пересборки — это кэш значков Windows: подождите,
перезапустите проводник или переименуйте файл.
