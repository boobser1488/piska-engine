//! `piska-export doctor` — диагностика окружения: всё ли стоит для сборки движка и
//! экспорта (Windows, Linux, Android, веб). Только читает: запускает `--version`
//! у найденных программ и смотрит каталоги, ничего не ставит и не меняет.
//!
//! ```text
//! piska-export doctor [--json] [--components core,cpp,web,android,linux-cross] [--strict]
//! ```
//!
//! Минимальные версии и имена пакетов берутся из `install/packages.toml` — того же файла,
//! что читают установщики (он вшит в программу). Для каждой найденной проблемы выводится
//! точная команда установщика, которая её чинит. Код выхода: 1, если не хватает чего-то
//! из выбранных компонентов (по умолчанию — `core`; с `--strict` — любого).
//!
//! Все обращения к системе идут через [`Sys`], поэтому тесты подставляют поддельное
//! окружение (каталоги-пустышки и заготовленные ответы `--version`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// Данные установщиков (одни и те же для скриптов, doctor, документации и тестов).
const PACKAGES: &str = include_str!("../../../install/packages.toml");

/// Компоненты в порядке показа.
pub const COMPONENTS: [&str; 5] = ["core", "cpp", "web", "android", "linux-cross"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

/// Окружение, в котором ищутся инструменты.
pub struct Sys {
    pub os: Os,
    pub var: Box<dyn Fn(&str) -> Option<String>>,
    /// Каталоги из PATH.
    pub path: Vec<PathBuf>,
    /// Запустить программу, вернуть склеенный вывод (stdout + stderr), если код выхода 0.
    pub run: Box<dyn Fn(&Path, &[&str]) -> Option<String>>,
}

impl Sys {
    pub fn real() -> Sys {
        Sys {
            os: Os::current(),
            var: Box::new(|name| std::env::var(name).ok().filter(|v| !v.is_empty())),
            path: std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
            run: Box::new(|program, args| {
                let output = Command::new(program).args(args).stdin(Stdio::null()).output().ok()?;
                if !output.status.success() {
                    return None;
                }
                Some(format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
            }),
        }
    }

    fn var(&self, name: &str) -> Option<String> {
        (self.var)(name)
    }

    fn home(&self) -> Option<PathBuf> {
        self.var("USERPROFILE").or_else(|| self.var("HOME")).map(PathBuf::from)
    }

    fn run(&self, program: &Path, args: &[&str]) -> Option<String> {
        (self.run)(program, args)
    }

    /// Программа `name` в `extra` и затем в PATH (на Windows пробуются `.exe`, `.cmd`, `.bat`).
    fn find(&self, name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
        let names: Vec<String> = if self.os == Os::Windows {
            vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat")]
        } else {
            vec![name.to_string()]
        };
        extra.iter().chain(self.path.iter()).find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
    }

    fn cargo_bin(&self) -> Vec<PathBuf> {
        let base = self.var("CARGO_HOME").map(PathBuf::from).or_else(|| self.home().map(|h| h.join(".cargo")));
        base.map(|b| vec![b.join("bin")]).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Найдено, но старее минимума.
    Old,
    Missing,
    /// Необязательное / справочное.
    Info,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Old => "OLD",
            Status::Missing => "MISSING",
            Status::Info => "info",
        }
    }

    fn failed(self) -> bool {
        matches!(self, Status::Old | Status::Missing)
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub component: &'static str,
    pub id: &'static str,
    pub label: String,
    pub status: Status,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: String,
    /// Команда установщика, которая это чинит.
    pub fix: Option<String>,
}

/// Минимумы из `install/packages.toml`.
#[derive(Clone, Debug)]
pub struct Data {
    pub min_rust: String,
    pub min_cmake: String,
    pub min_ninja: String,
    pub min_git: String,
    pub min_java: String,
    pub min_zig: String,
    pub min_ndk_major: u32,
    pub min_android_api: u32,
    pub vs_component: String,
    pub web_target: String,
    pub android_targets: Vec<String>,
    pub linux_target: String,
    pub android_platform: String,
    pub build_tools: String,
    pub ndk: String,
    pub cargo_ndk: String,
}

impl Data {
    pub fn load() -> Result<Data> {
        Self::parse(PACKAGES)
    }

    pub fn parse(text: &str) -> Result<Data> {
        let doc: toml::Value = text.parse().map_err(|e| Error(format!("install/packages.toml: {e}")))?;
        let get = |section: &str, key: &str| -> Result<String> {
            doc.get(section)
                .and_then(|s| s.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| Error(format!("install/packages.toml: no `{section}.{key}`")))
        };
        let list = |section: &str, key: &str| -> Vec<String> {
            doc.get(section)
                .and_then(|s| s.get(key))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        let num = |key: &str| -> Result<u32> {
            get("min", key)?.parse().map_err(|_| Error(format!("install/packages.toml: min.{key} is not a number")))
        };
        Ok(Data {
            min_rust: get("min", "rust")?,
            min_cmake: get("min", "cmake")?,
            min_ninja: get("min", "ninja")?,
            min_git: get("min", "git")?,
            min_java: get("min", "java")?,
            min_zig: get("min", "zig")?,
            min_ndk_major: num("ndk_major")?,
            min_android_api: num("android_api")?,
            vs_component: get("winget", "vs_required_component")?,
            web_target: get("rust", "web_target")?,
            android_targets: list("rust", "android_targets"),
            linux_target: get("rust", "linux_target")?,
            android_platform: get("android", "platform")?,
            build_tools: get("android", "build_tools")?,
            ndk: get("android", "ndk")?,
            cargo_ndk: get("android", "cargo_ndk")?,
        })
    }
}

// ---------------------------------------------------------------------------
// Версии
// ---------------------------------------------------------------------------

/// Первая версия вида `1.2` / `1.2.3` в тексте.
pub fn first_version(text: &str) -> Option<Vec<u32>> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.')) {
            let mut parts = Vec::new();
            let mut j = i;
            loop {
                let start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                parts.push(text[start..j].parse::<u32>().ok()?);
                if j + 1 < bytes.len() && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                    j += 1;
                } else {
                    break;
                }
            }
            if parts.len() >= 2 {
                return Some(parts);
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// Версия как в минимуме: `1.86` → `[1, 86]`.
pub fn parse_min(text: &str) -> Vec<u32> {
    text.split('.').filter_map(|p| p.trim().parse().ok()).collect()
}

/// `found >= min` (недостающие части считаются нулём).
pub fn at_least(found: &[u32], min: &[u32]) -> bool {
    for i in 0..found.len().max(min.len()) {
        let (a, b) = (found.get(i).copied().unwrap_or(0), min.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    true
}

pub fn show(version: &[u32]) -> String {
    version.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

/// Старшая версия Java из вывода `java -version`: `"17.0.2"` → 17, `"1.8.0_402"` → 8, `"22"` → 22.
pub fn java_major(text: &str) -> Option<u32> {
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-', '+']).filter_map(|p| p.parse::<u32>().ok());
    let first = parts.next()?;
    if first == 1 {
        parts.next()
    } else {
        Some(first)
    }
}

// ---------------------------------------------------------------------------
// Проверки
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    sys: &'a Sys,
    data: &'a Data,
}

impl Ctx<'_> {
    /// Команда установщика, которая ставит компонент.
    fn fix(&self, component: &str) -> String {
        match self.sys.os {
            Os::Windows => format!("powershell -ExecutionPolicy Bypass -File install\\install.ps1 -Components {component}"),
            _ => format!("bash install/install.sh --components {component}"),
        }
    }

    fn make(&self, component: &'static str, id: &'static str, label: &str) -> Check {
        Check {
            component,
            id,
            label: label.to_string(),
            status: Status::Missing,
            version: None,
            path: None,
            detail: String::new(),
            fix: None,
        }
    }

    fn missing(&self, mut c: Check, detail: &str) -> Check {
        c.status = Status::Missing;
        c.detail = detail.to_string();
        c.fix = Some(self.fix(c.component));
        c
    }

    /// Программа с версией: найти, запустить, сравнить с минимумом.
    fn program(&self, component: &'static str, id: &'static str, label: &str, tool: &str, extra: &[PathBuf], args: &[&str], min: &str) -> Check {
        let c = self.make(component, id, label);
        let Some(path) = self.sys.find(tool, extra) else {
            return self.missing(c, &format!("`{tool}` is not in PATH"));
        };
        self.with_version(c, path, args, min)
    }

    fn with_version(&self, mut c: Check, path: PathBuf, args: &[&str], min: &str) -> Check {
        c.path = Some(path.display().to_string());
        let Some(output) = self.sys.run(&path, args) else {
            c.status = Status::Missing;
            c.detail = format!("`{}` did not run", path.display());
            c.fix = Some(self.fix(c.component));
            return c;
        };
        match first_version(&output) {
            Some(v) => {
                c.version = Some(show(&v));
                if at_least(&v, &parse_min(min)) {
                    c.status = Status::Ok;
                } else {
                    c.status = Status::Old;
                    c.detail = format!("{} found, {min} or newer is needed", show(&v));
                    c.fix = Some(self.fix(c.component));
                }
            }
            None => {
                c.status = Status::Ok;
                c.detail = "runs (version not recognised)".to_string();
            }
        }
        c
    }

    fn ok(&self, mut c: Check, version: Option<String>, path: Option<String>) -> Check {
        c.status = Status::Ok;
        c.version = version;
        c.path = path;
        c
    }

    /// Установлена ли цель Rust: каталог `<sysroot>/lib/rustlib/<цель>`.
    fn rust_target(&self, component: &'static str, id: &'static str, target: &str, rustc: Option<&PathBuf>) -> Check {
        let c = self.make(component, id, &format!("Rust target {target}"));
        let Some(rustc) = rustc else {
            return self.missing(c, "rustc was not found (install `core` first)");
        };
        let sysroot = self.sys.run(rustc, &["--print", "sysroot"]).map(|s| PathBuf::from(s.lines().next().unwrap_or("").trim()));
        match sysroot {
            Some(root) if root.join("lib").join("rustlib").join(target).is_dir() => self.ok(c, None, Some(root.display().to_string())),
            Some(_) => self.missing(c, &format!("run `rustup target add {target}` (the installer does it)")),
            None => self.missing(c, "`rustc --print sysroot` failed"),
        }
    }

    fn vswhere(&self) -> Option<PathBuf> {
        let base = self.sys.var("ProgramFiles(x86)").or_else(|| self.sys.var("ProgramFiles"))?;
        let p = PathBuf::from(base).join("Microsoft Visual Studio").join("Installer").join("vswhere.exe");
        p.is_file().then_some(p)
    }

    fn visual_studio(&self) -> Option<String> {
        let vswhere = self.vswhere()?;
        let out = self.sys.run(
            &vswhere,
            &["-latest", "-products", "*", "-requires", &self.data.vs_component, "-property", "installationPath"],
        )?;
        out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
    }

    fn cxx(&self) -> Option<PathBuf> {
        ["c++", "g++", "clang++"].iter().find_map(|n| self.sys.find(n, &[]))
    }

    fn vulkan(&self) -> Check {
        let c = self.make("core", "vulkan", "Vulkan runtime (GPU driver)");
        match self.sys.os {
            Os::Windows => {
                let dir = self.sys.var("SystemRoot").or_else(|| self.sys.var("windir")).unwrap_or_else(|| "C:\\Windows".to_string());
                let dll = PathBuf::from(dir).join("System32").join("vulkan-1.dll");
                if dll.is_file() {
                    self.ok(c, None, Some(dll.display().to_string()))
                } else {
                    let mut c = self.missing(c, "no vulkan-1.dll: install the current driver from your GPU vendor (NVIDIA / AMD / Intel); the engine also runs on DX12");
                    c.status = Status::Info;
                    c
                }
            }
            Os::Macos => {
                let mut c = c;
                c.status = Status::Info;
                c.detail = "macOS uses Metal; nothing to install".to_string();
                c
            }
            Os::Linux => {
                let dirs = ["/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib", "/lib/x86_64-linux-gnu", "/lib64"];
                match dirs.iter().map(|d| Path::new(d).join("libvulkan.so.1")).find(|p| p.exists()) {
                    Some(p) => self.ok(c, None, Some(p.display().to_string())),
                    None => {
                        let mut c = self.missing(c, "libvulkan.so.1 not found: install the Vulkan loader and a driver (Mesa or NVIDIA)");
                        c.status = Status::Info;
                        c
                    }
                }
            }
        }
    }

    fn pkg_config(&self, id: &'static str, lib: &str, label: &str) -> Check {
        let c = self.make("core", id, label);
        let Some(pc) = self.sys.find("pkg-config", &[]) else {
            return self.missing(c, "pkg-config is not installed");
        };
        match self.sys.run(&pc, &["--modversion", lib]) {
            Some(v) => self.ok(c, first_version(&v).map(|v| show(&v)), Some(pc.display().to_string())),
            None => self.missing(c, &format!("`pkg-config --modversion {lib}` failed: install the development package")),
        }
    }

    // --- компоненты ---

    fn core(&self, rustc: &mut Option<PathBuf>) -> Vec<Check> {
        let cargo_bin = self.sys.cargo_bin();
        let mut out = Vec::new();
        let rust = self.program("core", "rustc", "rustc", "rustc", &cargo_bin, &["--version"], &self.data.min_rust);
        *rustc = rust.path.as_ref().filter(|_| rust.status != Status::Missing).map(PathBuf::from);
        out.push(rust);
        out.push(self.program("core", "cargo", "cargo", "cargo", &cargo_bin, &["--version"], &self.data.min_rust));
        let mut rustup = self.program("core", "rustup", "rustup", "rustup", &cargo_bin, &["--version"], "0.0");
        if rustup.status.failed() {
            rustup.status = Status::Info;
            rustup.detail = "not required to build, but the installer uses it to add targets".to_string();
        }
        out.push(rustup);
        match self.sys.os {
            Os::Windows => {
                let c = self.make("core", "msvc", "Visual Studio C++ tools + Windows SDK");
                out.push(match self.visual_studio() {
                    Some(dir) => self.ok(c, None, Some(dir)),
                    None => self.missing(c, "no Visual Studio / Build Tools with the C++ workload (needed by the MSVC Rust host)"),
                });
            }
            Os::Macos => {
                let c = self.make("core", "cc", "Xcode Command Line Tools");
                let xcode = self.sys.find("xcode-select", &[]);
                out.push(match xcode.and_then(|x| self.sys.run(&x, &["-p"])) {
                    Some(p) => self.ok(c, None, Some(p.trim().to_string())),
                    None => self.missing(c, "run `xcode-select --install`"),
                });
            }
            Os::Linux => {
                let c = self.make("core", "cc", "C compiler + linker (cc)");
                out.push(match ["cc", "gcc", "clang"].iter().find_map(|n| self.sys.find(n, &[])) {
                    Some(p) => self.ok(c, None, Some(p.display().to_string())),
                    None => self.missing(c, "no C compiler: install the build packages"),
                });
            }
        }
        out.push(self.program("core", "git", "git", "git", &[], &["--version"], &self.data.min_git));
        out.push(self.vulkan());
        if self.sys.os == Os::Linux {
            out.push(self.pkg_config("alsa", "alsa", "ALSA development files (sound)"));
            out.push(self.pkg_config("libudev", "libudev", "libudev development files (gamepads)"));
        }
        out
    }

    fn cpp(&self) -> Vec<Check> {
        let mut out = vec![
            self.program("cpp", "cmake", "CMake", "cmake", &[], &["--version"], &self.data.min_cmake),
            self.program("cpp", "ninja", "Ninja", "ninja", &[], &["--version"], &self.data.min_ninja),
        ];
        let c = self.make("cpp", "cxx", "C++ compiler");
        out.push(match self.cxx() {
            Some(p) => self.with_version(c, p, &["--version"], "0.0"),
            None => match (self.sys.os, self.visual_studio()) {
                (Os::Windows, Some(dir)) => self.ok(c, Some("MSVC".to_string()), Some(dir)),
                _ => self.missing(c, "no c++ / g++ / clang++ in PATH and no Visual Studio C++ tools"),
            },
        });
        out
    }

    fn web(&self, rustc: Option<&PathBuf>) -> Vec<Check> {
        vec![self.rust_target("web", "wasm", &self.data.web_target, rustc)]
    }

    fn sdk_root(&self) -> Option<PathBuf> {
        let mut places: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().filter_map(|v| self.sys.var(v)).map(PathBuf::from).collect();
        if let Some(local) = self.sys.var("LOCALAPPDATA") {
            places.push(PathBuf::from(local).join("Android").join("Sdk"));
        }
        if let Some(home) = self.sys.home() {
            places.push(home.join("Android").join("Sdk"));
            places.push(home.join("Library").join("Android").join("sdk"));
        }
        places.into_iter().find(|p| p.join("platforms").is_dir() || p.join("build-tools").is_dir() || p.join("platform-tools").is_dir())
    }

    fn android(&self, rustc: Option<&PathBuf>) -> Vec<Check> {
        let mut out = Vec::new();
        // Java: JAVA_HOME/bin/java, затем PATH.
        let c = self.make("android", "java", "JDK (Java)");
        let java_home = self.sys.var("JAVA_HOME").map(PathBuf::from).map(|h| h.join("bin"));
        let java = self.sys.find("java", &java_home.into_iter().collect::<Vec<_>>());
        out.push(match java {
            None => self.missing(c, "java was not found (set JAVA_HOME or install a JDK)"),
            Some(path) => match self.sys.run(&path, &["-version"]).as_deref().and_then(java_major) {
                Some(major) => {
                    let mut c = self.ok(c, Some(major.to_string()), Some(path.display().to_string()));
                    let min: u32 = self.data.min_java.parse().unwrap_or(11);
                    if major < min {
                        c.status = Status::Old;
                        c.detail = format!("JDK {major} found, {min} or newer is needed");
                        c.fix = Some(self.fix("android"));
                    }
                    c
                }
                None => self.missing(c, &format!("`{}` did not report a version", path.display())),
            },
        });
        let sdk = self.sdk_root();
        let c = self.make("android", "sdk", "Android SDK");
        let Some(sdk) = sdk else {
            out.push(self.missing(c, "no SDK in ANDROID_HOME / ANDROID_SDK_ROOT or the usual folders"));
            let rest = [("platform-tools", "Android platform-tools"), ("platform", "Android platform (android.jar)"), ("build-tools", "Android build-tools"), ("ndk", "Android NDK")];
            for (id, label) in rest {
                let c = self.make("android", id, label);
                out.push(self.missing(c, "needs the Android SDK"));
            }
            out.push(self.cargo_ndk());
            out.push(self.rust_target("android", "rust-android", &self.data.android_targets.first().cloned().unwrap_or_default(), rustc));
            return out;
        };
        out.push(self.ok(c, None, Some(sdk.display().to_string())));
        let c = self.make("android", "platform-tools", "Android platform-tools (adb)");
        let adb = sdk.join("platform-tools").join(if self.sys.os == Os::Windows { "adb.exe" } else { "adb" });
        out.push(if adb.is_file() { self.ok(c, None, Some(adb.display().to_string())) } else { self.missing(c, "no platform-tools in the SDK") });

        // Платформа: самая новая `platforms/android-NN`.
        let c = self.make("android", "platform", "Android platform (android.jar)");
        let api = list_dir(&sdk.join("platforms"))
            .into_iter()
            .filter(|n| sdk.join("platforms").join(n).join("android.jar").is_file())
            .filter_map(|n| n.strip_prefix("android-").and_then(|v| v.split('.').next().and_then(|a| a.parse::<u32>().ok())))
            .max();
        out.push(match api {
            Some(api) if api >= self.data.min_android_api => self.ok(c, Some(format!("API {api}")), Some(sdk.join("platforms").display().to_string())),
            Some(api) => {
                let mut c = self.missing(c, &format!("API {api} found, {} or newer is needed", self.data.min_android_api));
                c.status = Status::Old;
                c.version = Some(format!("API {api}"));
                c
            }
            None => self.missing(c, &format!("no platform (expected `{}`)", self.data.android_platform)),
        });

        // build-tools с aapt2, zipalign, apksigner.
        let c = self.make("android", "build-tools", "Android build-tools");
        let tools = list_dir(&sdk.join("build-tools"))
            .into_iter()
            .filter(|v| {
                let dir = sdk.join("build-tools").join(v);
                let has = |name: &str| ["", ".exe", ".bat"].iter().any(|e| dir.join(format!("{name}{e}")).is_file());
                has("aapt2") && has("zipalign") && has("apksigner")
            })
            .max_by_key(|v| first_version(v).unwrap_or_default());
        out.push(match tools {
            Some(v) => self.ok(c, Some(v), Some(sdk.join("build-tools").display().to_string())),
            None => self.missing(c, &format!("no build-tools with aapt2, zipalign and apksigner (expected {})", self.data.build_tools)),
        });

        // NDK: ANDROID_NDK_HOME или самая новая `ndk/<версия>`.
        let c = self.make("android", "ndk", "Android NDK");
        let ndk = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]
            .iter()
            .filter_map(|v| self.sys.var(v))
            .map(PathBuf::from)
            .find(|p| p.is_dir())
            .or_else(|| {
                let mut versions = list_dir(&sdk.join("ndk"));
                versions.sort_by_key(|v| first_version(&format!("{v}.0")).unwrap_or_default());
                versions.pop().map(|v| sdk.join("ndk").join(v))
            });
        out.push(match ndk {
            None => self.missing(c, &format!("no NDK in `{}` (expected {})", sdk.join("ndk").display(), self.data.ndk)),
            Some(dir) => {
                let revision = std::fs::read_to_string(dir.join("source.properties"))
                    .ok()
                    .and_then(|t| t.lines().find_map(|l| l.strip_prefix("Pkg.Revision").map(|v| v.trim_start_matches([' ', '=']).trim().to_string())));
                match revision {
                    Some(rev) => {
                        let major = rev.split('.').next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0);
                        let mut c = self.ok(c, Some(rev), Some(dir.display().to_string()));
                        if major < self.data.min_ndk_major {
                            c.status = Status::Old;
                            c.detail = format!("NDK r{major} found, r{} or newer is needed", self.data.min_ndk_major);
                            c.fix = Some(self.fix("android"));
                        }
                        c
                    }
                    None => self.missing(c, &format!("`{}` is not an NDK (no source.properties)", dir.display())),
                }
            }
        });
        out.push(self.cargo_ndk());
        for target in &self.data.android_targets {
            out.push(self.rust_target("android", "rust-android", target, rustc));
        }
        out
    }

    fn cargo_ndk(&self) -> Check {
        let c = self.make("android", "cargo-ndk", "cargo-ndk");
        let cargo_bin = self.sys.cargo_bin();
        match self.sys.find("cargo", &cargo_bin) {
            None => self.missing(c, "cargo was not found"),
            Some(cargo) => match self.sys.run(&cargo, &["ndk", "--version"]) {
                Some(out) => self.ok(c, first_version(&out).map(|v| show(&v)), Some(cargo.display().to_string())),
                None => self.missing(c, &format!("`cargo ndk` does not run: `cargo install cargo-ndk --version {} --locked`", self.data.cargo_ndk)),
            },
        }
    }

    fn linux_cross(&self, rustc: Option<&PathBuf>) -> Vec<Check> {
        let mut out = Vec::new();
        let c = self.make("linux-cross", "zig", "zig");
        let from_env = self.sys.var("PISKA_ZIG").map(PathBuf::from).filter(|p| p.is_file());
        let zig = from_env.or_else(|| self.sys.find("zig", &[]));
        out.push(match zig {
            Some(p) => {
                // `zig version` печатает просто `0.17.1`.
                let mut c = self.with_version(c, p, &["version"], &self.data.min_zig);
                if c.version.is_none() {
                    c.status = Status::Missing;
                }
                c
            }
            None => self.missing(c, "no zig in PISKA_ZIG or PATH (only needed to build for Linux from Windows / macOS)"),
        });
        let host_is_linux = self.sys.os == Os::Linux && cfg!(target_arch = "x86_64");
        let mut target = self.rust_target("linux-cross", "rust-linux", &self.data.linux_target, rustc);
        if host_is_linux && target.status.failed() {
            target.status = Status::Info;
            target.detail = "on a Linux x86_64 host the normal build needs no extra target".to_string();
            target.fix = None;
        }
        out.push(target);
        out
    }
}

fn list_dir(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default()
}

/// Все проверки выбранных компонентов (`None` — все).
pub fn run_checks(sys: &Sys, data: &Data, components: Option<&[String]>) -> Vec<Check> {
    let ctx = Ctx { sys, data };
    let wanted = |name: &str| components.is_none_or(|list| list.iter().any(|c| c == name));
    let mut rustc = None;
    // `core` нужен остальным (путь к rustc), поэтому считается всегда, а показывается по выбору.
    let core = ctx.core(&mut rustc);
    let mut out = Vec::new();
    if wanted("core") {
        out.extend(core);
    }
    if wanted("cpp") {
        out.extend(ctx.cpp());
    }
    if wanted("web") {
        out.extend(ctx.web(rustc.as_ref()));
    }
    if wanted("android") {
        out.extend(ctx.android(rustc.as_ref()));
    }
    if wanted("linux-cross") {
        out.extend(ctx.linux_cross(rustc.as_ref()));
    }
    out
}

/// Код выхода: 1, если провалена проверка обязательного компонента.
pub fn exit_code(checks: &[Check], components: Option<&[String]>, strict: bool) -> u8 {
    let required = |c: &Check| strict || components.is_some_and(|l| l.iter().any(|x| x == c.component)) || (components.is_none() && c.component == "core");
    u8::from(checks.iter().any(|c| c.status.failed() && required(c)))
}

pub fn to_json(sys: &Sys, checks: &[Check]) -> serde_json::Value {
    let os = match sys.os {
        Os::Windows => "windows",
        Os::Linux => "linux",
        Os::Macos => "macos",
    };
    let items: Vec<_> = checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "component": c.component,
                "id": c.id,
                "label": c.label,
                "status": c.status.word().to_lowercase(),
                "version": c.version,
                "path": c.path,
                "detail": c.detail,
                "fix": c.fix,
            })
        })
        .collect();
    let failed = checks.iter().filter(|c| c.status.failed()).count();
    serde_json::json!({ "os": os, "ok": failed == 0, "failed": failed, "checks": items })
}

/// Таблица для людей.
pub fn render_table(checks: &[Check]) -> String {
    let rows: Vec<[String; 5]> = checks
        .iter()
        .map(|c| {
            let tail = match (&c.path, c.detail.is_empty()) {
                (Some(p), true) => p.clone(),
                (Some(p), false) => format!("{p}  ({})", c.detail),
                (None, _) => c.detail.clone(),
            };
            [c.component.to_string(), c.label.clone(), c.status.word().to_string(), c.version.clone().unwrap_or_else(|| "-".to_string()), tail]
        })
        .collect();
    let head = ["component", "check", "status", "version", "where / what is wrong"];
    let mut width = head.map(str::len);
    for row in &rows {
        for (w, cell) in width.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: &[String; 5]| {
        let mut s = String::new();
        for (i, cell) in cells.iter().enumerate().take(4) {
            s.push_str(&format!("{cell:<w$}  ", w = width[i]));
        }
        s.push_str(&cells[4]);
        s.trim_end().to_string()
    };
    let mut out = String::new();
    out.push_str(&line(&head.map(str::to_string)));
    out.push('\n');
    out.push_str(&"-".repeat(width[0] + width[1] + width[2] + width[3] + 8 + 24));
    out.push('\n');
    for row in &rows {
        out.push_str(&line(row));
        out.push('\n');
    }
    let mut fixes: Vec<(&str, &str)> = Vec::new();
    for c in checks.iter().filter(|c| c.status.failed()) {
        if let Some(fix) = &c.fix {
            if !fixes.iter().any(|(_, f)| *f == fix.as_str()) {
                fixes.push((c.component, fix));
            }
        }
    }
    if fixes.is_empty() {
        out.push_str("\nnothing to fix.\n");
    } else {
        out.push_str("\nto fix:\n");
        for (component, fix) in fixes {
            out.push_str(&format!("  {component:<12} {fix}\n"));
        }
    }
    out
}

/// Точка входа подкоманды.
pub fn run(json: bool, components: Option<&str>, strict: bool) -> Result<u8> {
    let data = Data::load()?;
    let list: Option<Vec<String>> = components.map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    if let Some(list) = &list {
        if let Some(bad) = list.iter().find(|c| !COMPONENTS.contains(&c.as_str())) {
            return Err(Error(format!("unknown component `{bad}`; known: {}", COMPONENTS.join(", "))));
        }
    }
    let sys = Sys::real();
    let checks = run_checks(&sys, &data, list.as_deref());
    if json {
        println!("{}", serde_json::to_string_pretty(&to_json(&sys, &checks))?);
    } else {
        print!("{}", render_table(&checks));
    }
    Ok(exit_code(&checks, list.as_deref(), strict))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska-doctor-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    /// Поддельная система: Windows-имена, ответы по имени файла.
    fn fake(root: &Path, os: Os, answers: &[(&str, &str)], vars: &[(&str, String)]) -> Sys {
        let answers: HashMap<String, String> = answers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Sys {
            os,
            var: Box::new(move |n| vars.get(n).cloned()),
            path: vec![root.join("bin")],
            run: Box::new(move |program, args| {
                let name = program.file_stem()?.to_string_lossy().to_string();
                let key = format!("{name} {}", args.join(" "));
                answers.get(&key).or_else(|| answers.get(&name)).cloned()
            }),
        }
    }

    #[test]
    fn data_file_parses_and_has_every_minimum() {
        let data = Data::load().unwrap();
        assert!(!data.min_rust.is_empty() && data.min_ndk_major >= 23 && data.min_android_api >= 26);
        assert_eq!(data.web_target, "wasm32-unknown-unknown");
        assert!(data.android_targets.contains(&"aarch64-linux-android".to_string()));
    }

    #[test]
    fn versions_are_read_and_compared() {
        assert_eq!(first_version("rustc 1.97.1 (8bab26f4f 2026-07-14)"), Some(vec![1, 97, 1]));
        assert_eq!(first_version("cmake version 3.31.0\n\nCMake suite"), Some(vec![3, 31, 0]));
        assert_eq!(first_version("g++.exe (Rev8, Built by MSYS2 project) 15.2.0"), Some(vec![15, 2, 0]));
        assert_eq!(first_version("ninja 1.12.1"), Some(vec![1, 12, 1]));
        assert_eq!(first_version("no digits here"), None);
        assert!(at_least(&[1, 97, 1], &parse_min("1.86")));
        assert!(!at_least(&[1, 85, 9], &parse_min("1.86")));
        assert!(at_least(&[3, 20], &parse_min("3.20.0")));
        assert_eq!(java_major("openjdk version \"22.0.1\" 2024-04-16"), Some(22));
        assert_eq!(java_major("java version \"1.8.0_402\""), Some(8));
        assert_eq!(java_major("openjdk version \"17\" 2021-09-14"), Some(17));
    }

    #[test]
    fn empty_machine_reports_everything_missing_with_installer_commands() {
        let root = temp();
        let sys = fake(&root, Os::Windows, &[], &[("LOCALAPPDATA", root.join("local").display().to_string())]);
        let checks = run_checks(&sys, &Data::load().unwrap(), None);
        assert!(checks.iter().any(|c| c.id == "rustc" && c.status == Status::Missing));
        for c in checks.iter().filter(|c| c.status == Status::Missing) {
            let fix = c.fix.as_deref().unwrap_or_default();
            assert!(fix.contains("install.ps1") && fix.contains(c.component), "{}: {fix}", c.id);
        }
        assert_eq!(exit_code(&checks, None, false), 1);
        let table = render_table(&checks);
        assert!(table.contains("MISSING") && table.contains("to fix:"), "{table}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn complete_fake_android_setup_is_ok_and_old_ndk_is_flagged() {
        let root = temp();
        let sdk = root.join("sdk");
        touch(&sdk.join("platform-tools/adb.exe"));
        touch(&sdk.join("platforms/android-35/android.jar"));
        for tool in ["aapt2.exe", "zipalign.exe", "apksigner.bat"] {
            touch(&sdk.join("build-tools/36.0.0").join(tool));
        }
        touch(&sdk.join("build-tools/9.9.9/aapt2.exe")); // неполный набор не считается
        std::fs::create_dir_all(sdk.join("ndk/28.2.13676358")).unwrap();
        std::fs::write(sdk.join("ndk/28.2.13676358/source.properties"), "Pkg.Desc = Android NDK\nPkg.Revision = 28.2.13676358\n").unwrap();
        touch(&root.join("bin/java.exe"));
        touch(&root.join("bin/cargo.exe"));
        touch(&root.join("bin/rustc.exe"));
        let sysroot = root.join("sysroot");
        std::fs::create_dir_all(sysroot.join("lib/rustlib/aarch64-linux-android")).unwrap();
        let answers = [
            ("java -version", "openjdk version \"22.0.1\" 2024-04-16"),
            ("cargo ndk --version", "cargo-ndk 4.1.2"),
            ("rustc --print sysroot", sysroot.to_str().unwrap()),
            ("rustc --version", "rustc 1.97.1 (x 2026-07-14)"),
            ("cargo --version", "cargo 1.97.1"),
        ];
        let vars = [("ANDROID_HOME", sdk.display().to_string())];
        let sys = fake(&root, Os::Windows, &answers, &vars);
        let only = vec!["android".to_string()];
        let checks = run_checks(&sys, &Data::load().unwrap(), Some(&only));
        let by = |id: &str| checks.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no check {id}"));
        assert_eq!(by("java").version.as_deref(), Some("22"));
        assert_eq!(by("platform").version.as_deref(), Some("API 35"));
        assert_eq!(by("build-tools").version.as_deref(), Some("36.0.0"));
        assert_eq!(by("ndk").version.as_deref(), Some("28.2.13676358"));
        assert_eq!(by("cargo-ndk").version.as_deref(), Some("4.1.2"));
        assert!(checks.iter().all(|c| c.status == Status::Ok), "{}", render_table(&checks));
        assert_eq!(exit_code(&checks, Some(&only), false), 0);

        // NDK r21 — слишком старый.
        std::fs::write(sdk.join("ndk/28.2.13676358/source.properties"), "Pkg.Revision = 21.4.7075529\n").unwrap();
        let checks = run_checks(&sys, &Data::load().unwrap(), Some(&only));
        let ndk = checks.iter().find(|c| c.id == "ndk").unwrap();
        assert_eq!(ndk.status, Status::Old, "{ndk:?}");
        assert!(ndk.fix.is_some());
        assert_eq!(exit_code(&checks, Some(&only), false), 1);
        assert_eq!(exit_code(&checks, None, false), 0, "a broken optional component does not fail a default run");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn old_rust_is_reported_old_and_json_has_the_fields() {
        let root = temp();
        touch(&root.join("bin/rustc.exe"));
        let answers = [("rustc --version", "rustc 1.70.0 (x 2023-06-01)")];
        let sys = fake(&root, Os::Windows, &answers, &[]);
        let only = vec!["core".to_string()];
        let checks = run_checks(&sys, &Data::load().unwrap(), Some(&only));
        let rustc = checks.iter().find(|c| c.id == "rustc").unwrap();
        assert_eq!(rustc.status, Status::Old);
        let json = to_json(&sys, &checks);
        assert_eq!(json["os"], "windows");
        assert_eq!(json["ok"], false);
        let item = json["checks"].as_array().unwrap().iter().find(|c| c["id"] == "rustc").unwrap();
        assert_eq!(item["status"], "old");
        assert_eq!(item["version"], "1.70.0");
        assert!(item["fix"].as_str().unwrap().contains("-Components core"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_fix_commands_use_the_shell_installer() {
        let root = temp();
        let sys = fake(&root, Os::Linux, &[], &[]);
        let only = vec!["web".to_string()];
        let checks = run_checks(&sys, &Data::load().unwrap(), Some(&only));
        assert!(checks[0].fix.as_deref().unwrap().contains("install.sh --components web"), "{:?}", checks[0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_component_is_an_error() {
        assert!(run(false, Some("nope"), false).unwrap_err().0.contains("unknown component"));
    }
}
