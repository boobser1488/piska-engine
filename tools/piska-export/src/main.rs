//! `piska-export` — экспорт игры Piska Engine под платформу.
//!
//! ```text
//! cargo run -p piska-export -- web     --game template [--release] [--out DIR]
//! cargo run -p piska-export -- windows --game template [--release] [--out DIR]
//! cargo run -p piska-export -- android --game template [--release] [--abi arm64-v8a]
//! cargo run -p piska-export -- linux   --game template [--release] [--tar]
//! cargo run -p piska-export -- serve   [DIR] [--port 8080] [--open]
//! cargo run -p piska-export -- pack    --game template --out assets.pak
//! ```
//!
//! Игра — крейт `games/<name>/` рабочего пространства (см. `docs/api/export.md`).
//! Новая платформа — новый модуль с `fn export(&Project, &Options) -> Result<Report>`
//! и строка в [`Platform`].

mod android;
mod archive;
mod cargo;
mod elf;
mod icon;
mod linux;
mod project;
mod serve;
mod web;
mod windows;
mod zigcc;

use std::path::PathBuf;
use std::process::ExitCode;

use project::Project;

/// Куда экспортировать.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Web,
    Windows,
    Android,
    Linux,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Web => "web",
            Platform::Windows => "windows",
            Platform::Android => "android",
            Platform::Linux => "linux",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "web" => Some(Platform::Web),
            "windows" => Some(Platform::Windows),
            "android" => Some(Platform::Android),
            "linux" => Some(Platform::Linux),
            _ => None,
        }
    }
}

/// Параметры экспорта из командной строки.
#[derive(Clone, Debug)]
pub struct Options {
    pub game: String,
    pub release: bool,
    /// Каталог результата; по умолчанию `dist/<game>/<platform>`.
    pub out: Option<PathBuf>,
    /// Корень рабочего пространства; по умолчанию ищется вверх от текущего каталога.
    pub root: Option<PathBuf>,
    /// Android: ABI через запятую (по умолчанию `arm64-v8a`).
    pub abi: Option<String>,
    /// Android: своё хранилище ключей и псевдоним ключа (иначе — отладочный ключ).
    pub keystore: Option<PathBuf>,
    pub key_alias: Option<String>,
    /// Android: файл с паролем хранилища (иначе `PISKA_KEYSTORE_PASSWORD` или вопрос в терминале).
    pub keystore_pass_file: Option<PathBuf>,
    /// Linux: ещё и `<game>-linux-x86_64.tar.gz`.
    pub tar: bool,
    /// Web: без запасного WebGL2 (только WebGPU) — wasm меньше, но браузер без WebGPU игру не запустит.
    pub no_webgl: bool,
    /// Linux: версия glibc при сборке через zig (`2.31`).
    pub glibc: Option<String>,
    /// Linux: путь к zig (иначе `PISKA_ZIG`, затем PATH).
    pub zig: Option<PathBuf>,
}

/// Итог экспорта: что получилось и сколько весит.
pub struct Report {
    pub out_dir: PathBuf,
    pub files: Vec<(String, u64)>,
    pub notes: Vec<String>,
}

/// Ошибка экспорта — текст для человека: что не так и что сделать.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<E: std::error::Error> From<E> for Error {
    fn from(error: E) -> Self {
        Error(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Шаг экспорта: строка `[n/m] …` — её показывает окно Build редактора.
pub fn step(n: u32, total: u32, what: &str) {
    println!("piska-export: [{n}/{total}] {what}");
}

const USAGE: &str = "\
usage:
  piska-export web     --game <name> [--release] [--no-webgl] [--out <dir>] [--root <workspace>]
  piska-export windows --game <name> [--release] [--out <dir>] [--root <workspace>]
  piska-export android --game <name> [--release] [--out <dir>] [--abi arm64-v8a[,x86_64]]
                       [--keystore <file> --key-alias <alias> [--keystore-pass-file <file>]]
  piska-export linux   --game <name> [--release] [--out <dir>] [--tar] [--zig <zig.exe>] [--glibc 2.31]
  piska-export serve   [<dir>] [--port <port>] [--open]
  piska-export pack    --game <name> --out <file.pak> [--root <workspace>]
  piska-export list    [--json] [--root <workspace>]

The game is a crate in `games/<name>/` of the engine workspace (see docs/api/export.md).
Output goes to `dist/<name>/<platform>/` unless --out is given.";

fn main() -> ExitCode {
    // Запущен как обёртка zig (piska-zig-cc и т. п.) при сборке под Linux.
    if let Some(code) = zigcc::dispatch() {
        return code;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    positional: Vec<String>,
    game: Option<String>,
    release: bool,
    open: bool,
    out: Option<PathBuf>,
    root: Option<PathBuf>,
    port: Option<u16>,
    abi: Option<String>,
    keystore: Option<PathBuf>,
    key_alias: Option<String>,
    keystore_pass_file: Option<PathBuf>,
    tar: bool,
    no_webgl: bool,
    json: bool,
    glibc: Option<String>,
    zig: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args> {
    let mut parsed = Args {
        positional: Vec::new(),
        game: None,
        release: false,
        open: false,
        out: None,
        root: None,
        port: None,
        abi: None,
        keystore: None,
        key_alias: None,
        keystore_pass_file: None,
        tar: false,
        no_webgl: false,
        json: false,
        glibc: None,
        zig: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| Error(format!("`{name}` needs a value\n\n{USAGE}")))
        };
        match arg.as_str() {
            "--game" | "-g" => parsed.game = Some(value(arg)?),
            "--out" | "-o" => parsed.out = Some(PathBuf::from(value(arg)?)),
            "--root" => parsed.root = Some(PathBuf::from(value(arg)?)),
            "--port" | "-p" => {
                let text = value(arg)?;
                parsed.port = Some(text.parse().map_err(|_| Error(format!("`{text}` is not a port number")))?);
            }
            "--abi" => parsed.abi = Some(value(arg)?),
            "--keystore" => parsed.keystore = Some(PathBuf::from(value(arg)?)),
            "--key-alias" => parsed.key_alias = Some(value(arg)?),
            "--keystore-pass-file" => parsed.keystore_pass_file = Some(PathBuf::from(value(arg)?)),
            "--glibc" => parsed.glibc = Some(value(arg)?),
            "--zig" => parsed.zig = Some(PathBuf::from(value(arg)?)),
            "--tar" => parsed.tar = true,
            "--no-webgl" => parsed.no_webgl = true,
            "--json" => parsed.json = true,
            "--release" | "-r" => parsed.release = true,
            "--open" => parsed.open = true,
            "--help" | "-h" => return Err(Error(USAGE.to_string())),
            flag if flag.starts_with('-') => return Err(Error(format!("unknown option `{flag}`\n\n{USAGE}"))),
            other => parsed.positional.push(other.to_string()),
        }
    }
    Ok(parsed)
}

fn run(args: &[String]) -> Result<()> {
    let args = parse(args)?;
    let Some(command) = args.positional.first().cloned() else {
        return Err(Error(USAGE.to_string()));
    };
    match command.as_str() {
        "serve" => {
            let dir = args.positional.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            serve::serve(&dir, args.port.unwrap_or(8080), args.open)
        }
        "list" => {
            let root = project::workspace_root(args.root.as_deref())?;
            let games = project::list_games(&root)?;
            if args.json {
                // Для окна Build редактора: имя, пакет, заголовок и включённые возможности движка.
                let mut list = Vec::new();
                for game in &games {
                    list.push(match Project::open(Some(&root), game) {
                        Ok(project) => serde_json::json!({
                            "game": project.game,
                            "package": project.package,
                            "title": project.title,
                            "engine_default_features": project.engine.default_features,
                            "engine_features": project.engine.declared,
                            "engine_features_effective": project.engine.effective,
                        }),
                        Err(error) => serde_json::json!({ "game": game, "error": error.to_string() }),
                    });
                }
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                for game in games {
                    println!("{game}");
                }
            }
            Ok(())
        }
        "pack" => {
            let game = args.game.ok_or_else(|| Error(format!("`pack` needs --game <name>\n\n{USAGE}")))?;
            let out = args.out.ok_or_else(|| Error(format!("`pack` needs --out <file.pak>\n\n{USAGE}")))?;
            let project = Project::open(args.root.as_deref(), &game)?;
            let bytes = project.pack_assets()?;
            std::fs::write(&out, &bytes).map_err(|e| Error(format!("cannot write `{}`: {e}", out.display())))?;
            println!("piska-export: {} ({} KiB)", out.display(), bytes.len() / 1024);
            Ok(())
        }
        name => {
            let platform = Platform::parse(name).ok_or_else(|| Error(format!("unknown command `{name}`\n\n{USAGE}")))?;
            let game = args.game.ok_or_else(|| Error(format!("`{name}` needs --game <name>\n\n{USAGE}")))?;
            let options = Options {
                game,
                release: args.release,
                out: args.out,
                root: args.root,
                abi: args.abi,
                keystore: args.keystore,
                key_alias: args.key_alias,
                keystore_pass_file: args.keystore_pass_file,
                tar: args.tar,
                no_webgl: args.no_webgl,
                glibc: args.glibc,
                zig: args.zig,
            };
            let project = Project::open(options.root.as_deref(), &options.game)?;
            println!("piska-export: engine features of `{}`: {}", project.package, project.engine.summary());
            // Release без раскрутки стека: код меньше и быстрее. Раскрутка нужна только мосту C++
            // (`catch_unwind` вокруг сервисов, см. engine-cpp-bridge), поэтому играм с `cpp` её
            // оставляем. В вебе паника и так обрывает программу. Переменную уважает cargo.
            if options.release
                && platform != Platform::Web
                && !project.engine.enables("cpp")
                && std::env::var_os("CARGO_PROFILE_RELEASE_PANIC").is_none()
            {
                std::env::set_var("CARGO_PROFILE_RELEASE_PANIC", "abort");
                println!("piska-export: release profile: panic = \"abort\" (the game has no C++ bridge)");
            }
            let report = match platform {
                Platform::Web => web::export(&project, &options)?,
                Platform::Windows => windows::export(&project, &options)?,
                Platform::Android => android::export(&project, &options)?,
                Platform::Linux => linux::export(&project, &options)?,
            };
            print_report(platform, &report);
            Ok(())
        }
    }
}

fn print_report(platform: Platform, report: &Report) {
    println!("piska-export: {} build ready in {}", platform.name(), report.out_dir.display());
    for (name, size) in &report.files {
        println!("piska-export:   {name:<24} {}", human_size(*size));
    }
    for note in &report.notes {
        println!("piska-export: {note}");
    }
}

pub fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}
