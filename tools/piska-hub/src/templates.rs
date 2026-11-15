//! Новый проект из шаблона: проверка имени и места, список файлов, запись на диск.
//!
//! Проект — крейт игры, как `games/template`: библиотека с `build()`, программа игры,
//! программа редактора, ассеты с `.meta`, стартовая сцена, тест без окна и `README.md`.
//! В `games/` движка он становится членом рабочего пространства; в другом месте —
//! отдельным рабочим пространством с путём к этой копии движка.

use engine_editor::kit::i18n::{tr_s};
use std::path::{Path, PathBuf};

use crate::paths;

/// Шаблон нового проекта.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Empty2D,
    Empty3D,
    /// «Звездопад» — копия `games/template`.
    Starfall,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::Empty2D, Template::Empty3D, Template::Starfall];

    pub fn title(self) -> &'static str {
        match self {
            Template::Empty2D => tr_s("Empty 2D"),
            Template::Empty3D => tr_s("Empty 3D"),
            Template::Starfall => tr_s("Starfall sample"),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Template::Empty2D => {
                tr_s("Orthographic camera and a sprite turned by a Scenario script. Opens at once, nothing to compile.")
            }
            Template::Empty3D => {
                tr_s("Camera, sun, sky, ground grid and a cube spun by a Scenario script. Opens at once, nothing to compile.")
            }
            Template::Starfall => {
                tr_s("A complete small 2D game: sprites, prefabs, 2D physics, UI, sound and a saved high score.")
            }
        }
    }

    /// Короткая строка о составе: что внутри.
    pub fn contents(self) -> &'static str {
        match self {
            Template::Empty2D => tr_s("1 scene · 1 sprite · 1 Scenario script"),
            Template::Empty3D => tr_s("1 scene · 1 Scenario script"),
            Template::Starfall => tr_s("1 scene · 1 prefab · 3 sprites · 1 sound · 3 scripts"),
        }
    }

    pub fn dimension(self) -> &'static str {
        match self {
            Template::Empty3D => "3d",
            _ => "2d",
        }
    }
}

/// Что создать.
#[derive(Clone, Debug, PartialEq)]
pub struct NewProject {
    /// Имя пакета; каталог называется так же.
    pub name: String,
    /// Каталог, в котором появится каталог проекта.
    pub location: PathBuf,
    pub template: Template,
    /// Каталог `cpp/` с примером компонента на C++ (статически в крейт игры).
    pub cpp: bool,
}

/// Чем плохо имя проекта.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum NameError {
    #[error("Enter a name")]
    Empty,
    #[error("The name is longer than 64 characters")]
    TooLong,
    #[error("The name must start with a lowercase letter, not `{0}`")]
    BadStart(char),
    #[error("Use lowercase letters, digits, `_` and `-` only; `{0}` is not allowed")]
    BadChar(char),
    #[error("`{0}` is a Rust keyword; the crate could not be imported. Try `{0}_game`")]
    Keyword(String),
    #[error("`{0}` is reserved by Cargo or the engine; choose another name")]
    Reserved(String),
}

/// Почему проект не создаётся.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CreateError {
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("Choose a location")]
    NoLocation,
    #[error("`{0}` already exists and is not empty; choose another name or location")]
    Exists(PathBuf),
    #[error(
        "`{0}` is inside the engine checkout. Projects there must go to its games/ folder \
         (they become workspace members); otherwise choose a folder outside the engine"
    )]
    InsideEngine(PathBuf),
    #[error("A package named `{0}` already exists in the engine workspace; choose another name")]
    PackageTaken(String),
    #[error("The Starfall template is read from `{0}`, which is missing or incomplete")]
    TemplateMissing(PathBuf),
    #[error("Cannot write `{path}`: {reason}")]
    Io { path: PathBuf, reason: String },
}

/// Ключевые слова Rust: имя библиотеки не может с ними совпадать.
const KEYWORDS: [&str; 51] = [
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn", "abstract", "become",
    "box", "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// Имена, которые заняты Cargo, стандартной библиотекой или движком.
const RESERVED: [&str; 22] = [
    "std", "core", "alloc", "proc_macro", "proc-macro", "test", "build", "deps", "examples", "incremental",
    "engine", "engine-core", "engine-assets", "engine-render", "engine-physics", "engine-audio", "engine-ui",
    "engine-editor", "engine-scenario", "piska-export", "piska-hub", "template",
];

/// Проверить имя проекта как имя пакета Cargo (и библиотеки Rust).
pub fn validate_name(name: &str) -> Result<(), NameError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(NameError::Empty);
    };
    if name.chars().count() > 64 {
        return Err(NameError::TooLong);
    }
    if !first.is_ascii_lowercase() {
        return Err(NameError::BadStart(first));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-'))
    {
        return Err(NameError::BadChar(bad));
    }
    let lib = name.replace('-', "_");
    if KEYWORDS.contains(&lib.as_str()) {
        return Err(NameError::Keyword(name.to_string()));
    }
    if RESERVED.contains(&name) || name.starts_with("engine-") || name.starts_with("engine_") {
        return Err(NameError::Reserved(name.to_string()));
    }
    Ok(())
}

/// `my_game` → `My Game` — заголовок окна и списка.
pub fn title_of(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Что будет записано.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Каталог проекта.
    pub dir: PathBuf,
    pub package: String,
    /// Внутри `games/` движка (член рабочего пространства).
    pub in_workspace: bool,
    /// Файлы: путь относительно каталога проекта (через `/`) и содержимое.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Plan {
    /// Пути файлов — для предпросмотра в мастере и тестов.
    pub fn file_names(&self) -> Vec<&str> {
        self.files.iter().map(|(path, _)| path.as_str()).collect()
    }
}

/// Проверить имя и место и составить список файлов. На диск ничего не пишется.
pub fn plan(new: &NewProject, engine_root: &Path) -> Result<Plan, CreateError> {
    validate_name(&new.name)?;
    if new.location.as_os_str().is_empty() {
        return Err(CreateError::NoLocation);
    }
    let location = paths::clean(&new.location);
    let games = engine_root.join("games");
    let in_workspace = paths::same(&location, &games);
    if !in_workspace && paths::is_within(&location, engine_root) {
        return Err(CreateError::InsideEngine(location));
    }
    let dir = location.join(&new.name);
    let not_empty = std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some());
    if dir.is_file() || not_empty {
        return Err(CreateError::Exists(dir));
    }
    if in_workspace && workspace_packages(engine_root).iter().any(|p| p == &new.name) {
        return Err(CreateError::PackageTaken(new.name.clone()));
    }

    let names = Names::new(&new.name);
    let engine_path = if in_workspace {
        "../../crates".to_string()
    } else {
        format!("{}/crates", engine_root.display().to_string().replace('\\', "/"))
    };
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut text = |path: &str, body: String| files.push((path.to_string(), body.into_bytes()));
    text("Cargo.toml", cargo_toml(&names, new, in_workspace, &engine_path));
    text("README.md", readme(&names, new, in_workspace));
    text("src/main.rs", main_rs(&names));
    text(&format!("src/bin/{}-editor.rs", names.package), editor_rs(&names));
    text("src/scripts/mod.rs", SCRIPTS_MOD.to_string());
    if !in_workspace {
        text(".gitignore", "/target\n".to_string());
    }
    if new.cpp {
        text("build.rs", BUILD_RS.to_string());
        text("cpp/Wobble.cpp", WOBBLE_CPP.to_string());
    }

    match new.template {
        Template::Empty2D | Template::Empty3D => {
            let three = new.template == Template::Empty3D;
            files.push(("src/lib.rs".to_string(), empty_lib(&names, three, new.cpp).into_bytes()));
            files.push(("tests/smoke.rs".to_string(), empty_smoke(&names, new.cpp).into_bytes()));
            let scene_guid = new_guid();
            files.push(("assets/scenes/main.scene.meta".to_string(), meta(&scene_guid, tr_s("Scene")).into_bytes()));
            // Поведение демо — сценарий Scenario, а не код на Rust: проект открывается
            // общим редактором сразу, без компиляции (docs/api/hub.md).
            let script_guid = new_guid();
            let script = if three { SPIN_3D } else { SPIN_2D };
            files.push(("assets/scenarios/spin.scenario".to_string(), script.to_string().into_bytes()));
            files.push(("assets/scenarios/spin.scenario.meta".to_string(), meta(&script_guid, tr_s("Scenario")).into_bytes()));
            if three {
                files.push(("assets/scenes/main.scene".to_string(), SCENE_3D.replace("{SCRIPT}", &script_guid).into_bytes()));
            } else {
                let sprite_guid = new_guid();
                files.push(("assets/sprites/square.png".to_string(), square_png()));
                files.push(("assets/sprites/square.png.meta".to_string(), meta(&sprite_guid, tr_s("Texture")).into_bytes()));
                files.push((
                    "assets/scenes/main.scene".to_string(),
                    SCENE_2D.replace("{SPRITE}", &sprite_guid).replace("{SCRIPT}", &script_guid).into_bytes(),
                ));
            }
        }
        Template::Starfall => starfall_files(&names, new.cpp, engine_root, &mut files)?,
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Plan {
        dir,
        package: new.name.clone(),
        in_workspace,
        files,
    })
}

/// Записать проект на диск. Отдельный проект получает копию `Cargo.lock` движка —
/// те же версии зависимостей, что уже проверены.
pub fn create(plan: &Plan, engine_root: &Path) -> Result<(), CreateError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |e: std::io::Error| CreateError::Io {
            path,
            reason: e.to_string(),
        }
    };
    let not_empty = std::fs::read_dir(&plan.dir).is_ok_and(|mut entries| entries.next().is_some());
    if not_empty {
        return Err(CreateError::Exists(plan.dir.clone()));
    }
    for (relative, bytes) in &plan.files {
        let path = plan.dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::write(&path, bytes).map_err(io(&path))?;
    }
    if !plan.in_workspace {
        let lock = engine_root.join("Cargo.lock");
        if lock.is_file() {
            let target = plan.dir.join("Cargo.lock");
            std::fs::copy(&lock, &target).map_err(io(&target))?;
        }
    }
    Ok(())
}

/// Имена пакетов рабочего пространства движка (`games/*` и `crates/*`, `tools/*`).
fn workspace_packages(engine_root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for group in ["games", "crates", "tools"] {
        let Ok(entries) = std::fs::read_dir(engine_root.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path().join("Cargo.toml")) else {
                continue;
            };
            if let Some(name) = text
                .parse::<toml::Table>()
                .ok()
                .and_then(|t| t.get("package")?.get("name")?.as_str().map(str::to_string))
            {
                names.push(name);
            }
        }
    }
    names
}

struct Names {
    package: String,
    lib: String,
    title: String,
}

impl Names {
    fn new(name: &str) -> Self {
        Self {
            package: name.to_string(),
            lib: name.replace('-', "_"),
            title: title_of(name),
        }
    }
}

/// Новый GUID ассета: 32 шестнадцатеричные цифры.
pub fn new_guid() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut parts = [0u64; 2];
    for part in &mut parts {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        hasher.write_u32(std::process::id());
        *part = hasher.finish();
    }
    format!("{:016x}{:016x}", parts[0], parts[1])
}

fn meta(guid: &str, importer: &str) -> String {
    format!("(\n    guid: \"{guid}\",\n    importer: \"{importer}\",\n)\n")
}

/// Белый квадрат 32×32: спрайт, который красится полем `color`.
fn square_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 255, 255, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encoding a PNG in memory does not fail");
    bytes
}

fn cargo_toml(names: &Names, new: &NewProject, in_workspace: bool, crates: &str) -> String {
    let package = &names.package;
    let mut text = String::new();
    text.push_str("[package]\n");
    text.push_str(&format!("name = \"{package}\"\n"));
    text.push_str(&format!("description = \"{}: игра на Piska Engine.\"\n", names.title));
    if in_workspace {
        text.push_str("version.workspace = true\nedition.workspace = true\nlicense.workspace = true\n");
    } else {
        text.push_str("version = \"0.1.0\"\nedition = \"2021\"\n");
    }
    text.push_str("publish = false\n");
    if new.cpp {
        text.push_str("build = \"build.rs\"\n");
    }
    text.push_str(&format!(
        "\n[package.metadata.piska]\ntitle = \"{}\"\n# Значок в Piska Hub и в списке проектов: \"2d\" или \"3d\".\ndimension = \"{}\"\n",
        names.title,
        new.template.dimension()
    ));
    text.push_str("\n[dependencies]\n# Редактор в игру не входит: экспорт собирает только рантайм.\n");
    // `full` — весь рантайм; сузить набор можно по docs/design/engine-features.md.
    text.push_str(
        "# `full` — весь рантайм движка. Чтобы игра весила меньше, оставьте только нужные возможности\n\
         # (physics2d, physics3d, audio, ui, cpp, scenario, gamepad, ...): docs/design/engine-features.md.\n",
    );
    text.push_str(&format!(
        "engine = {{ path = \"{crates}/engine\", default-features = false, features = [\"full\"] }}\n"
    ));
    text.push_str(if in_workspace { "log.workspace = true\n" } else { "log = \"0.4\"\n" });
    if new.cpp {
        text.push_str(&format!(
            "\n[build-dependencies]\n# Компоненты на C++ из cpp/ линкуются в крейт статически (build.rs).\nengine-cpp-build = {{ path = \"{crates}/engine-cpp-build\" }}\n"
        ));
    }
    text.push_str(&format!(
        "\n[features]\n# Программа редактора: `cargo run -p {package} --features editor --bin {package}-editor`.\n\
         # В экспорт и обычную сборку игры редактор не входит.\neditor = [\"engine/editor\"]\n\n\
         [[bin]]\nname = \"{package}-editor\"\nrequired-features = [\"editor\"]\n"
    ));
    if !in_workspace {
        text.push_str(
            "\n# Отдельный проект: своё рабочее пространство, движок подключён путём.\n[workspace]\n\n\
             # Как в движке: зависимости оптимизированы (игра играбельна и в отладке), свой код\n\
             # собирается быстро.\n[profile.dev]\nopt-level = 1\ndebug = \"line-tables-only\"\n\n\
             [profile.dev.package.\"*\"]\nopt-level = 3\n",
        );
    }
    text
}

fn main_rs(names: &Names) -> String {
    format!(
        "//! Программа игры для десктопа: `cargo run -p {package}`.\n\
         //! В браузере и на Android точку входа даёт `engine::game_main!` в `lib.rs`.\n\n\
         // На Windows — без окна консоли и в отладочной сборке (запущенная из терминала игра\n\
         // подключается к его консоли сама: `engine::init_logger`).\n\
         #![windows_subsystem = \"windows\"]\n\n\
         fn main() -> Result<(), engine::EngineError> {{\n    {lib}::main()\n}}\n",
        package = names.package,
        lib = names.lib
    )
}

fn editor_rs(names: &Names) -> String {
    format!(
        "//! Игра в редакторе: `cargo run -p {package} --features editor --bin {package}-editor`\n\
         //! (или кнопка Open в Piska Hub).\n//!\n\
         //! Скрипты на Rust — в `src/scripts/` (Assets → New Rust Script…). Build (Ctrl+B)\n\
         //! пересобирает эту программу и перезапускает редактор с новым кодом.\n\n\
         // Без окна консоли рядом с редактором (вывод — в консоль терминала или в журнал).\n\
         #![windows_subsystem = \"windows\"]\n\n\
         engine::editor_main!({lib}::build, |config| config.open_scene(\"scenes/main.scene\"));\n",
        package = names.package,
        lib = names.lib
    )
}

const SCRIPTS_MOD: &str = "//! Скрипты на Rust этой игры: каталог, в котором их создаёт редактор (New Rust Script).
//!
//! ЭТОТ ФАЙЛ ПИШЕТ РЕДАКТОР — не правьте его руками: он перезаписывается, когда в
//! каталоге создают, переименовывают или удаляют скрипт, и перед каждой сборкой из
//! редактора. В нём `mod` для каждого файла каталога и `register`, который
//! регистрирует все компоненты, объявленные в них через `component!`.
//!
//! Игра вызывает один раз: `scripts::register(&mut app);`

/// Зарегистрировать компоненты всех скриптов каталога.
pub fn register(app: &mut engine::prelude::App) {
    let _ = app;
}
";

const BUILD_RS: &str = "//! Компоненты на C++: все `cpp/*.cpp` собираются и линкуются в крейт статически.
//! На wasm32 (веб) C++ не собирается — см. docs/api/cpp.md §17.

fn main() {
    engine_cpp_build::compile(\"cpp\");
}
";

const WOBBLE_CPP: &str = "// Пример компонента на C++: покачивает объект вверх-вниз.
// Повесьте его на объект в редакторе (Add Component → Wobble). Новые файлы *.cpp в этом
// каталоге собираются вместе с игрой (build.rs); API — docs/api/cpp.md.
#include \"engine_component.hpp\"

#include <cmath>

struct Wobble : engine::Component {
    float amplitude = 0.25f;  // высота качания
    float speed = 2.0f;       // радиан в секунду

    float time = 0.0f;
    float base_y = 0.0f;

    void on_start(engine::Object self) override { base_y = self.position().y; }

    void on_update(engine::Object self, float dt) override {
        time += dt;
        engine::Vec3 p = self.position();
        p.y = base_y + std::sin(time * speed) * amplitude;
        self.set_position(p);
    }
};
ENGINE_FIELDS(Wobble,
              ENGINE_FIELD(Wobble, amplitude, ENGINE_FIELD_F32),
              ENGINE_FIELD(Wobble, speed, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Wobble);
";

fn empty_lib(names: &Names, _three: bool, cpp: bool) -> String {
    let cpp_line = if cpp {
        "    // Компоненты на C++ из cpp/ (build.rs → engine-cpp-build).\n    setup: |app| engine::add_static_cpp(app, engine::cpp_scripts!()),\n"
    } else {
        ""
    };
    format!(
        "//! {title} — игра на Piska Engine.\n\
         //!\n\
         //! `engine::game!` объявляет игру: имя, стартовую сцену, компоненты на Rust. Из этого\n\
         //! получаются `build()` (её открывает `src/bin/{package}-editor.rs` в редакторе) и\n\
         //! программа для десктопа, браузера и Android. Устройство крейта игры —\n\
         //! docs/api/export.md, компоненты и сцены — docs/api/gameplay.md.\n\
         //!\n\
         //! Пока в проекте нет своего кода на Rust (скрипты в `src/scripts/`) или C++, Piska Hub\n\
         //! открывает его общим редактором без компиляции; поведение демо — сценарий\n\
         //! `assets/scenarios/spin.scenario`. Первый скрипт на Rust (Assets → New Rust Script…)\n\
         //! соберёт программу редактора этого проекта.\n\n\
         /// Скрипты на Rust, созданные в редакторе (New Rust Script).\n\
         pub mod scripts;\n\n\
         // Игра: имя (заголовок окна, сохранения), стартовая сцена и свои компоненты на Rust\n\
         // (`components: [Player, Enemy]`; скрипты из `src/scripts/` регистрируются сами).\n\
         // Компоненту в этом файле нужна строка `use engine::prelude::*;`.\n\
         engine::game! {{\n\
         \x20   name: \"{title}\",\n\
         \x20   scene: \"scenes/main.scene\",\n\
         \x20   components: [],\n\
         {cpp_line}\
         }}\n",
        title = names.title,
        package = names.package,
    )
}

fn empty_smoke(names: &Names, cpp: bool) -> String {
    let cpp_check = if cpp {
        "    assert!(app.world.registry.create(\"Wobble\").is_ok(), \"the C++ component from cpp/ is registered\");\n"
    } else {
        ""
    };
    format!(
        "//! Проверка без окна: игра собирается из своих ассетов, сцена загружается,\n\
         //! сценарий `scenarios/spin.scenario` поворачивает игрока. `cargo test -p {package}`.\n\n\
         #[test]\n\
         fn the_game_starts_and_the_rotator_turns_the_player() {{\n\
         \x20   std::env::set_var(\"ENGINE_AUDIO\", \"off\");\n\
         \x20   let mut app = {lib}::build().expect(\"the game builds from its assets directory\");\n\
         {cpp_check}\
         \x20   let player = app.world.scene.find_by_name(\"Player\").expect(\"the scene has a Player\");\n\
         \x20   let before = app.world.scene.get(player).unwrap().transform.rotation;\n\
         \x20   app.run_headless(60, 1.0 / 60.0);\n\
         \x20   let after = app.world.scene.get(player).unwrap().transform.rotation;\n\
         \x20   assert!(before.angle_between(after) > 0.5, \"one second at 45°/s of the Scenario script turns the player\");\n\
         }}\n",
        package = names.package,
        lib = names.lib,
    )
}

const SCENE_2D: &str = "// Стартовая сцена: камера, земля и вращающийся квадрат (сценарий scenarios/spin.scenario).
(
    name: \"Main\",
    objects: [
        (
            name: \"Main Camera\",
            tag: \"MainCamera\",
            components: [
                (kind: \"Camera\", fields: {
                    \"ortho_size\": Float(5.0),
                    \"clear_color\": Color(0.08, 0.09, 0.11, 1.0),
                }),
            ],
        ),
        (
            name: \"Ground\",
            transform: (position: (0.0, -3.5, 0.0)),
            components: [
                (kind: \"SpriteRenderer\", fields: {
                    \"texture\": Asset(\"{SPRITE}\"),
                    \"color\": Color(0.2, 0.22, 0.26, 1.0),
                    \"size\": Vec2(40.0, 1.0),
                }),
            ],
        ),
        (
            name: \"Player\",
            tag: \"Player\",
            components: [
                (kind: \"SpriteRenderer\", fields: {
                    \"texture\": Asset(\"{SPRITE}\"),
                    \"color\": Color(0.3, 0.55, 1.0, 1.0),
                    \"size\": Vec2(1.5, 1.5),
                    \"order\": Int(1),
                }),
                (kind: \"ScenarioRunner\", fields: { \"script\": Asset(\"{SCRIPT}\") }),
            ],
        ),
    ],
)
";

/// Демо 2D: сценарий поворачивает свой объект.
const SPIN_2D: &str = "-- Демо стартовой сцены: объект с этим сценарием (ScenarioRunner) вращается.
-- Поменяйте скорость и сохраните файл во время Play — сценарий перезапустится сам.
-- Язык: docs/api/scenario.md.
const speed = 45          -- градусов в секунду
const obj = self          -- объект, на котором висит сценарий
let angle = 0
while true do
    angle = angle + speed * dt()
    obj.rotation = vec3(0, 0, angle)
    wait_frame()
end
";

/// Демо 3D: сценарий вращает куб вокруг вертикальной оси.
const SPIN_3D: &str = "-- Демо стартовой сцены: куб с этим сценарием (ScenarioRunner) вращается вокруг оси Y.
-- Поменяйте скорость и сохраните файл во время Play — сценарий перезапустится сам.
-- Язык: docs/api/scenario.md.
const speed = 45          -- градусов в секунду
const obj = self          -- объект, на котором висит сценарий
let angle = 0
while true do
    angle = angle + speed * dt()
    obj.rotation = vec3(0, angle, 0)
    wait_frame()
end
";

const SCENE_3D: &str = "// Стартовая сцена: камера, солнце, небо, земля с сеткой и вращающийся куб
// (сценарий scenarios/spin.scenario).
(
    name: \"Main\",
    objects: [
        (
            name: \"Main Camera\",
            tag: \"MainCamera\",
            transform: (position: (0.0, 2.6, 7.5), rotation: (-14.0, 0.0, 0.0)),
            components: [
                (kind: \"Camera\", fields: {
                    \"projection\": Enum(\"Perspective\"),
                    \"fov_deg\": Float(50.0),
                    \"background\": Enum(\"Sky\"),
                }),
            ],
        ),
        (
            name: \"Sun\",
            transform: (rotation: (-50.0, 35.0, 0.0)),
            components: [
                (kind: \"Light\", fields: { \"intensity\": Float(2.4) }),
            ],
        ),
        (
            name: \"Ground\",
            transform: (scale: (40.0, 1.0, 40.0)),
            components: [
                (kind: \"MeshRenderer\", fields: {
                    \"shape\": Enum(\"Plane\"),
                    \"color\": Color(0.32, 0.34, 0.36, 1.0),
                    \"roughness\": Float(0.9),
                    \"grid\": Bool(true),
                    \"cast_shadows\": Bool(false),
                }),
            ],
        ),
        (
            name: \"Player\",
            tag: \"Player\",
            transform: (position: (0.0, 1.0, 0.0), scale: (1.4, 1.4, 1.4)),
            components: [
                (kind: \"MeshRenderer\", fields: {
                    \"color\": Color(0.3, 0.55, 1.0, 1.0),
                    \"roughness\": Float(0.4),
                }),
                (kind: \"ScenarioRunner\", fields: { \"script\": Asset(\"{SCRIPT}\") }),
            ],
        ),
    ],
)
";

/// «Звездопад»: ассеты, код и тесты `games/template` под новым именем.
fn starfall_files(names: &Names, cpp: bool, engine_root: &Path, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), CreateError> {
    let template = engine_root.join("games").join("template");
    let missing = || CreateError::TemplateMissing(template.clone());
    // Файлы шаблона бывают с CRLF: приводим к LF, чтобы правки по образцу находились.
    let read = |relative: &str| {
        std::fs::read_to_string(template.join(relative))
            .map(|text| text.replace("\r\n", "\n"))
            .map_err(|_| missing())
    };
    let rename = |text: &str| {
        text.replace("template::", &format!("{}::", names.lib))
            .replace("\"Piska Starfall\"", &format!("\"{}\"", names.title))
            // Программы проекта в тестах (tests/subsystem.rs): `template`, `template-editor`.
            .replace("CARGO_BIN_EXE_template", &format!("CARGO_BIN_EXE_{}", names.package))
    };

    let mut lib = rename(&read("src/lib.rs")?);
    if cpp {
        let anchor = "    components: [GameManager],\n"; // i18n-skip: строка кода шаблона, не текст интерфейса
        if !lib.contains(anchor) {
            return Err(missing());
        }
        lib = lib.replacen(
            anchor,
            &format!(
                "{anchor}    // Компоненты на C++ из cpp/ (build.rs → engine-cpp-build).\n    setup: |app| engine::add_static_cpp(app, engine::cpp_scripts!()),\n"
            ),
            1,
        );
    }
    files.push(("src/lib.rs".to_string(), lib.into_bytes()));
    if let Ok(scripts) = std::fs::read_dir(template.join("src").join("scripts")) {
        for entry in scripts.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name != "mod.rs" && name.ends_with(".rs") {
                files.push((format!("src/scripts/{name}"), rename(&read(&format!("src/scripts/{name}"))?).into_bytes()));
            }
        }
    }
    // Тесты шаблона и свой короткий.
    if let Ok(tests) = std::fs::read_dir(template.join("tests")) {
        for entry in tests.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".rs") {
                files.push((format!("tests/{name}"), rename(&read(&format!("tests/{name}"))?).into_bytes()));
            }
        }
    }
    files.push((
        "tests/smoke.rs".to_string(),
        format!(
            "//! Проверка без окна: игра собирается из своих ассетов и идёт. `cargo test -p {package}`.\n\n\
             use engine::prelude::*;\n\n\
             #[test]\n\
             fn the_game_starts_and_stars_fall() {{\n\
             \x20   std::env::set_var(\"ENGINE_AUDIO\", \"off\");\n\
             \x20   let mut app = {lib}::build_with(SaveData::in_memory()).expect(\"the game builds from its assets directory\");\n\
             {cpp_check}\
             \x20   app.run_headless(150, 1.0 / 60.0);\n\
             \x20   assert!(app.world.scene.find_by_name(\"Player\").is_some());\n\
             \x20   assert!(!app.world.scene.find_all_by_tag(\"Star\").is_empty(), \"stars spawn from the prefab\");\n\
             }}\n",
            package = names.package,
            lib = names.lib,
            cpp_check = if cpp {
                "    assert!(app.world.registry.create(\"Wobble\").is_ok(), \"the C++ component from cpp/ is registered\");\n"
            } else {
                ""
            },
        )
        .into_bytes(),
    ));
    // Ассеты как есть, с теми же `.meta` (GUID — внутри проекта, сцены ссылаются на них).
    let assets = template.join("assets");
    if !assets.join("scenes").join("main.scene").is_file() {
        return Err(missing());
    }
    copy_tree(&assets, "assets", files).map_err(|_| missing())?;
    Ok(())
}

fn copy_tree(dir: &Path, prefix: &str, files: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let relative = format!("{prefix}/{name}");
        if path.is_dir() {
            copy_tree(&path, &relative, files)?;
        } else {
            files.push((relative, std::fs::read(&path)?));
        }
    }
    Ok(())
}

fn readme(names: &Names, new: &NewProject, in_workspace: bool) -> String {
    let package = &names.package;
    let place = if in_workspace {
        "Проект лежит в `games/` движка и собирается в его рабочем пространстве.".to_string()
    } else {
        // i18n-skip: текст файла README (язык документации проекта — русский)
        "Отдельный проект: свой `[workspace]` и путь к копии движка в `Cargo.toml`. \
         Первая сборка компилирует движок целиком (10–15 минут и несколько ГБ на диске)."
            .to_string()
    };
    let cpp = if new.cpp {
        // i18n-skip: текст файла README
        "\n* `cpp/` — компоненты на C++ (пример `Wobble.cpp`), собираются `build.rs` вместе с игрой (docs/api/cpp.md §17)."
    } else {
        ""
    };
    format!(
        "# {title}\n\n\
         Игра на Piska Engine, создана в Piska Hub из шаблона «{template}». {place}\n\n\
         ## Запуск\n\n\
         | что | команда |\n|---|---|\n\
         | открыть в редакторе | Piska Hub → Open, или `cargo run -p {package} --features editor --bin {package}-editor` |\n\
         | запустить игру | `cargo run -p {package}` |\n\
         | тесты без окна | `cargo test -p {package}` |\n\n\
         ## Что где\n\n\
         * `src/lib.rs` — `engine::game!` (имя, стартовая сцена, компоненты) и компоненты на Rust.\n\
         * `src/scripts/` — скрипты на Rust из редактора (Assets → New Rust Script…); `mod.rs` пишет редактор.\n\
         * `src/bin/{package}-editor.rs` — программа редактора этой игры.\n\
         * `assets/` — сцены, спрайты, звуки; рядом с каждым файлом `.meta` с GUID (храните их в git).\n\
         * `tests/` — тесты без окна (`app.run_headless`).{cpp}\n\n\
         Экспорт в браузер, Windows, Android и Linux — File → Build… в редакторе или `piska-export`\n\
         (docs/api/export.md).\n",
        title = names.title,
        template = new.template.title(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_cargo_rules() {
        assert_eq!(validate_name("my_game"), Ok(()));
        assert_eq!(validate_name("space-shooter2"), Ok(()));
        assert_eq!(validate_name(""), Err(NameError::Empty));
        assert_eq!(validate_name("2fast"), Err(NameError::BadStart('2')));
        assert_eq!(validate_name("MyGame"), Err(NameError::BadStart('M')));
        assert_eq!(validate_name("my game"), Err(NameError::BadChar(' ')));
        assert_eq!(validate_name("игра"), Err(NameError::BadStart('и')));
        assert_eq!(validate_name("type"), Err(NameError::Keyword("type".into())));
        assert_eq!(validate_name("engine"), Err(NameError::Reserved("engine".into())));
        assert_eq!(validate_name("engine-x"), Err(NameError::Reserved("engine-x".into())));
        assert_eq!(validate_name(&"a".repeat(65)), Err(NameError::TooLong));
        assert!(NameError::Keyword("type".into()).to_string().contains("type_game"));
        assert_eq!(title_of("my_cool-game"), "My Cool Game");
    }

    #[test]
    fn guids_are_unique_hex() {
        let a = new_guid();
        let b = new_guid();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    fn root() -> PathBuf {
        paths::find_engine_root().unwrap()
    }

    #[test]
    fn an_empty_2d_project_has_the_whole_crate() {
        let root = root();
        let outside = std::env::temp_dir().join(format!("piska-hub-plan-{}", std::process::id()));
        let plan = plan(
            &NewProject {
                name: "my_game".into(),
                location: outside.clone(),
                template: Template::Empty2D,
                cpp: false,
            },
            &root,
        )
        .unwrap();
        assert!(!plan.in_workspace);
        assert_eq!(
            plan.file_names(),
            [
                ".gitignore",
                "Cargo.toml",
                "README.md",
                "assets/scenarios/spin.scenario",
                "assets/scenarios/spin.scenario.meta",
                "assets/scenes/main.scene",
                "assets/scenes/main.scene.meta",
                "assets/sprites/square.png",
                "assets/sprites/square.png.meta",
                "src/bin/my_game-editor.rs",
                "src/lib.rs",
                "src/main.rs",
                "src/scripts/mod.rs",
                "tests/smoke.rs",
            ]
        );
        let file = |name: &str| {
            String::from_utf8(plan.files.iter().find(|(path, _)| path == name).unwrap().1.clone()).unwrap()
        };
        let manifest: toml::Table = file("Cargo.toml").parse().unwrap();
        assert!(manifest.contains_key("workspace"), "a standalone project is its own workspace");
        let engine = manifest["dependencies"]["engine"]["path"].as_str().unwrap();
        assert!(Path::new(engine).join("Cargo.toml").is_file(), "{engine} points at this engine");
        assert_eq!(manifest["bin"][0]["name"].as_str(), Some("my_game-editor"));
        // Сцена ссылается на спрайт по GUID из его .meta.
        let meta = file("assets/sprites/square.png.meta");
        let guid = meta.split('"').nth(1).unwrap();
        assert!(file("assets/scenes/main.scene").contains(&format!("Asset(\"{guid}\")")));
        assert!(file("src/bin/my_game-editor.rs").contains("engine::editor_main!(my_game::build"));
        assert!(file("tests/smoke.rs").contains("my_game::build()"));
        assert!(image::load_from_memory(&plan.files.iter().find(|(p, _)| p == "assets/sprites/square.png").unwrap().1).is_ok());
    }

    #[test]
    fn workspace_projects_use_relative_paths_and_cpp_adds_a_build_script() {
        let root = root();
        let plan = plan(
            &NewProject {
                name: "hub_plan_probe".into(),
                location: root.join("games"),
                template: Template::Empty3D,
                cpp: true,
            },
            &root,
        )
        .unwrap();
        assert!(plan.in_workspace);
        assert!(plan.file_names().contains(&"build.rs"));
        assert!(plan.file_names().contains(&"cpp/Wobble.cpp"));
        assert!(!plan.file_names().contains(&".gitignore"));
        let manifest = String::from_utf8(plan.files.iter().find(|(p, _)| p == "Cargo.toml").unwrap().1.clone()).unwrap();
        assert!(manifest.contains("path = \"../../crates/engine\""));
        assert!(manifest.contains("engine-cpp-build = { path = \"../../crates/engine-cpp-build\" }"));
        assert!(!manifest.contains("[workspace]"));
        let lib = String::from_utf8(plan.files.iter().find(|(p, _)| p == "src/lib.rs").unwrap().1.clone()).unwrap();
        assert!(lib.contains("add_static_cpp") && !lib.contains("component!"), "the empty templates need no Rust component");
        let scene = String::from_utf8(plan.files.iter().find(|(p, _)| p == "assets/scenes/main.scene").unwrap().1.clone()).unwrap();
        assert!(scene.contains("ScenarioRunner"));
        assert!(!plan.dir.exists(), "planning writes nothing");
    }

    #[test]
    fn bad_places_and_taken_names_are_refused() {
        let root = root();
        let new = |name: &str, location: PathBuf| NewProject {
            name: name.into(),
            location,
            template: Template::Empty2D,
            cpp: false,
        };
        let inside = plan(&new("x_game", root.join("crates")), &root).unwrap_err();
        assert!(matches!(inside, CreateError::InsideEngine(_)));
        assert!(matches!(plan(&new("swarm", root.join("games")), &root), Err(CreateError::PackageTaken(_)) | Err(CreateError::Exists(_))));
        assert!(matches!(plan(&new("Bad", root.join("games")), &root), Err(CreateError::Name(_))));
        assert_eq!(plan(&new("ok", PathBuf::new()), &root).unwrap_err(), CreateError::NoLocation);
        let taken = std::env::temp_dir().join(format!("piska-hub-taken-{}", std::process::id()));
        std::fs::create_dir_all(taken.join("busy")).unwrap();
        std::fs::write(taken.join("busy").join("file.txt"), "x").unwrap();
        assert!(matches!(plan(&new("busy", taken.clone()), &root), Err(CreateError::Exists(_))));
        std::fs::remove_dir_all(&taken).unwrap();
    }

    #[test]
    fn starfall_is_the_template_under_a_new_name() {
        let root = root();
        let dir = std::env::temp_dir().join(format!("piska-hub-starfall-{}", std::process::id()));
        let plan = plan(
            &NewProject {
                name: "star_catcher".into(),
                location: dir.clone(),
                template: Template::Starfall,
                cpp: true,
            },
            &root,
        )
        .unwrap();
        let names = plan.file_names();
        for wanted in ["assets/prefabs/star.prefab", "assets/sprites/star.png.meta", "tests/headless.rs", "tests/smoke.rs"] {
            assert!(names.contains(&wanted), "{wanted} in {names:?}");
        }
        let text = |name: &str| String::from_utf8(plan.files.iter().find(|(p, _)| p == name).unwrap().1.clone()).unwrap();
        assert!(text("tests/headless.rs").contains("star_catcher::"));
        assert!(!text("tests/headless.rs").contains("template::"));
        assert!(text("src/lib.rs").contains("\"Star Catcher\""));
        assert!(text("src/lib.rs").contains("add_static_cpp"));
        // На диск — и только в пустой каталог.
        create(&plan, &root).unwrap();
        assert!(dir.join("star_catcher").join("Cargo.lock").is_file(), "a standalone project gets the engine's lock file");
        let info = crate::project::inspect(&plan.dir).unwrap();
        assert_eq!(info.editor_bin.as_deref(), Some("star_catcher-editor"));
        assert!(info.has_cpp && info.standalone);
        assert_eq!(info.dimension, crate::project::Dimension::TwoD);
        assert!(matches!(create(&plan, &root), Err(CreateError::Exists(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
