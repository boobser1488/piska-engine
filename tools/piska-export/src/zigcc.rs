//! Кросс-сборка под Linux через zig без cargo-zigbuild.
//!
//! zig умеет быть C/C++-компилятором и линковщиком для `x86_64-linux-gnu` с glibc
//! любой версии (`-target x86_64-linux-gnu.2.31`) — ему не нужны ни sysroot, ни
//! библиотеки Linux на машине. `piska-export` сам становится «обёрткой»: рядом с
//! кэшем сборки появляются жёсткие ссылки на него с именами `piska-zig-cc`,
//! `piska-zig-cxx`, `piska-zig-ar`, `piska-zig-pkg-config`, и при запуске под таким
//! именем [`dispatch`] вызывает zig с нужной целью, убирая аргументы, которые
//! rustc и `cc` передают «обычному» компилятору (`--target=…`, `-fuse-ld=lld`,
//! `-B…`, `-lgcc_s`).
//!
//! Системные библиотеки, с которыми движок линкуется (ALSA для звука через cpal,
//! udev для геймпадов через gilrs), на Windows не установлены. Для линковки
//! zig собирает **заглушки**: `libasound.so` с `SONAME libasound.so.2` и
//! `libudev.so` с `SONAME libudev.so.1`, в которых есть все функции, объявленные в
//! `alsa-sys` / `libudev-sys`. В программу попадают только `NEEDED`-записи; на
//! машине игрока загружаются настоящие библиотеки дистрибутива. Поддельный
//! `pkg-config` сообщает `build.rs` этих крейтов, где лежат заглушки.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::{Error, Result};

/// Переменные окружения, которыми экспорт настраивает обёртки.
pub const ZIG_ENV: &str = "PISKA_ZIG";
const TARGET_ENV: &str = "PISKA_ZIG_TARGET";
const STUBS_ENV: &str = "PISKA_ZIG_STUBS";

/// Имена обёрток (без `.exe`).
pub const CC: &str = "piska-zig-cc";
pub const CXX: &str = "piska-zig-cxx";
pub const AR: &str = "piska-zig-ar";
pub const PKG_CONFIG: &str = "piska-zig-pkg-config";

/// Пакеты pkg-config, для которых есть заглушки: (пакет, библиотека, SONAME, крейт с объявлениями).
pub const STUB_LIBS: &[(&str, &str, &str, &str)] = &[
    ("alsa", "asound", "libasound.so.2", "alsa-sys"),
    ("libudev", "udev", "libudev.so.1", "libudev-sys"),
];

/// Найти zig: `--zig <путь>`, затем `PISKA_ZIG`, затем `zig` в PATH.
pub fn find_zig(option: Option<&Path>, env: &dyn Fn(&str) -> Option<String>, in_path: &dyn Fn() -> bool) -> Result<PathBuf> {
    if let Some(path) = option {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(Error(format!("--zig `{}`: no such file", path.display())))
        };
    }
    if let Some(path) = env(ZIG_ENV) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(Error(format!("{ZIG_ENV}=`{}`: no such file", path.display())))
        };
    }
    if in_path() {
        return Ok(PathBuf::from("zig"));
    }
    Err(Error(format!(
        "zig was not found: building for Linux from this system uses zig as the C/C++ compiler and \
         linker. Download zig 0.13 or newer from https://ziglang.org/download/ (the archive for your \
         system, ~80 MB unpacked), unpack it and either put the folder on PATH, set {ZIG_ENV} to \
         zig(.exe), or pass --zig <path to zig(.exe)>. Alternatively build on a Linux machine or with \
         the GitHub Actions workflow .github/workflows/build.yml"
    )))
}

pub fn zig_in_path() -> bool {
    Command::new("zig").arg("version").output().is_ok_and(|o| o.status.success())
}

/// Версия zig (`0.17.1`) — для отчёта и проверки, что это действительно zig.
pub fn zig_version(zig: &Path) -> Result<String> {
    let output = Command::new(zig)
        .arg("version")
        .output()
        .map_err(|e| Error(format!("cannot run `{}`: {e}", zig.display())))?;
    if !output.status.success() {
        return Err(Error(format!("`{} version` failed", zig.display())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ---------------------------------------------------------------------------
// Обёртки
// ---------------------------------------------------------------------------

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Положить обёртки (жёсткие ссылки на этот exe, иначе копии) в `dir`.
pub fn install_wrappers(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let me = std::env::current_exe()?;
    for name in [CC, CXX, AR, PKG_CONFIG] {
        let path = dir.join(exe_name(name));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        if std::fs::hard_link(&me, &path).is_err() {
            std::fs::copy(&me, &path)
                .map_err(|e| Error(format!("cannot create the zig wrapper `{}`: {e}", path.display())))?;
        }
    }
    Ok(())
}

pub fn wrapper(dir: &Path, name: &str) -> PathBuf {
    dir.join(exe_name(name))
}

/// Переменные окружения для `cargo build --target x86_64-unknown-linux-gnu`.
pub fn cargo_env(zig: &Path, glibc: &str, wrappers: &Path, stubs: &Path, cache: &Path) -> Vec<(String, String)> {
    let w = |name| wrapper(wrappers, name).display().to_string();
    vec![
        ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER".into(), w(CC)),
        ("CC_x86_64_unknown_linux_gnu".into(), w(CC)),
        ("CXX_x86_64_unknown_linux_gnu".into(), w(CXX)),
        ("AR_x86_64_unknown_linux_gnu".into(), w(AR)),
        // libc++ из zig (статически) вместо libstdc++, которой у zig нет.
        ("CXXSTDLIB_x86_64_unknown_linux_gnu".into(), "c++".into()),
        ("PKG_CONFIG_x86_64_unknown_linux_gnu".into(), w(PKG_CONFIG)),
        ("PKG_CONFIG_ALLOW_CROSS".into(), "1".into()),
        // Поддельному pkg-config не нужен, но `pkg-config`-крейт следит за этой
        // переменной: другой каталог заглушек — `build.rs` alsa-sys / libudev-sys
        // перезапустятся, а не возьмут старый ответ из кэша cargo.
        ("PKG_CONFIG_PATH_x86_64_unknown_linux_gnu".into(), stubs.display().to_string().replace('\\', "/")),
        (ZIG_ENV.into(), zig.display().to_string()),
        (TARGET_ENV.into(), format!("x86_64-linux-gnu.{glibc}")),
        (STUBS_ENV.into(), stubs.display().to_string()),
        // Кэш zig (libc++, заглушки glibc) — рядом с кэшем cargo, а не в профиле пользователя.
        ("ZIG_GLOBAL_CACHE_DIR".into(), cache.join("global").display().to_string()),
        ("ZIG_LOCAL_CACHE_DIR".into(), cache.join("local").display().to_string()),
    ]
}

/// Если exe запущен как обёртка — сделать её работу и вернуть код выхода.
pub fn dispatch() -> Option<ExitCode> {
    let exe = std::env::args_os().next()?;
    let stem = Path::new(&exe).file_stem()?.to_string_lossy().into_owned();
    let mode = [CC, CXX, AR, PKG_CONFIG].into_iter().find(|name| stem == *name)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match mode {
        PKG_CONFIG => pkg_config(&args),
        _ => run_zig(mode, &args),
    };
    Some(match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{stem}: {err}");
            ExitCode::FAILURE
        }
    })
}

fn env(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| Error(format!("{name} is not set (the wrapper is started by piska-export)")))
}

/// Развернуть `@файл` (файлы ответов rustc): по аргументу в строке, кавычки — по правилам GNU.
pub fn expand_response_files(args: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in args {
        match arg.strip_prefix('@') {
            Some(file) if Path::new(file).is_file() => {
                let text = std::fs::read_to_string(file)?;
                for line in text.lines() {
                    out.push(unquote(line));
                }
            }
            _ => out.push(arg.clone()),
        }
    }
    Ok(out)
}

fn unquote(line: &str) -> String {
    let inner = match line.strip_prefix('"').and_then(|l| l.strip_suffix('"')) {
        Some(inner) => inner,
        None => line,
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Аргументы для `zig cc` / `zig c++`: убрать то, что zig задаёт сам или не понимает.
pub fn filter_compiler_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            // Цель задаёт обёртка (`-target x86_64-linux-gnu.<glibc>`).
            "-target" | "--target" => skip_next = true,
            a if a.starts_with("--target=") => {}
            // rustc просит свой rust-lld; у zig свой lld.
            a if a.starts_with("-fuse-ld=") => {}
            a if a.starts_with("-B") => {}
            // libgcc_s у zig нет; раскрутка стека — его libunwind (статически).
            "-lgcc_s" => out.push("-lunwind".to_string()),
            "-lgcc" | "-lgcc_eh" => {}
            _ => out.push(arg.clone()),
        }
    }
    out
}

fn quote_for_response(arg: &str) -> String {
    format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
}

fn run_zig(mode: &str, args: &[String]) -> Result<ExitCode> {
    let zig = env(ZIG_ENV)?;
    let mut command = Command::new(&zig);
    if mode == AR {
        command.arg("ar").args(args);
    } else {
        let target = env(TARGET_ENV)?;
        let args = filter_compiler_args(&expand_response_files(args)?);
        command.arg(if mode == CXX { "c++" } else { "cc" });
        command.args(["-target", &target]);
        // Длинная команда линковки (тысячи объектов) не влезет в командную строку
        // Windows: передаём через свой файл ответов.
        let total: usize = args.iter().map(|a| a.len() + 3).sum();
        if total > 24_000 {
            let file = std::env::temp_dir().join(format!("piska-zig-{}.rsp", std::process::id()));
            let text: Vec<String> = args.iter().map(|a| quote_for_response(a)).collect();
            std::fs::write(&file, text.join("\n"))?;
            let mut at = OsString::from("@");
            at.push(file.as_os_str());
            command.arg(at);
        } else {
            command.args(&args);
        }
    }
    let status = command
        .status()
        .map_err(|e| Error(format!("cannot run zig `{zig}`: {e}")))?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8)
    })
}

/// Поддельный pkg-config: знает только пакеты из [`STUB_LIBS`] и отвечает путём к
/// заглушкам. Этого достаточно `pkg-config`-крейту в `build.rs` alsa-sys и libudev-sys.
pub fn pkg_config_answer(args: &[String], stubs: &str) -> std::result::Result<String, String> {
    let package = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or_else(|| "no package given".to_string())?;
    let (_, lib, _, _) = STUB_LIBS
        .iter()
        .find(|(name, ..)| name == package)
        .ok_or_else(|| format!("Package {package} was not found (piska-export has Linux stubs only for alsa and libudev)"))?;
    // Вывод pkg-config разбирается как командная строка: `\` — экранирование, пробел —
    // разделитель. Поэтому путь — с прямыми косыми, пробелы экранированы.
    let stubs = stubs.replace('\\', "/").replace(' ', "\\ ");
    let mut parts = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--modversion" => parts.push("1.0.0".to_string()),
            "--libs" => parts.push(format!("-L{stubs} -l{lib}")),
            _ => {}
        }
    }
    Ok(parts.join(" "))
}

fn pkg_config(args: &[String]) -> Result<ExitCode> {
    if args.iter().any(|a| a == "--version") {
        println!("0.29.2");
        return Ok(ExitCode::SUCCESS);
    }
    let stubs = env(STUBS_ENV)?;
    match pkg_config_answer(args, &stubs) {
        Ok(text) => {
            println!("{text}");
            Ok(ExitCode::SUCCESS)
        }
        Err(message) => {
            eprintln!("{message}");
            Ok(ExitCode::FAILURE)
        }
    }
}

// ---------------------------------------------------------------------------
// Заглушки библиотек
// ---------------------------------------------------------------------------

/// Имена C-функций, объявленных в исходниках крейта `-sys` (`pub fn name(` вне `impl`
/// — у методов первый параметр `self`).
pub fn declared_functions(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("pub fn ") {
        rest = &rest[at + 7..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let after = rest[name.len()..].trim_start();
        let params = after.strip_prefix('(').unwrap_or("");
        let is_method = params.trim_start().starts_with("&self")
            || params.trim_start().starts_with("&mut self")
            || params.trim_start().starts_with("self");
        if !name.is_empty() && after.starts_with('(') && !is_method && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// C-исходник заглушки: пустая функция на каждое имя.
pub fn stub_source(functions: &[String]) -> String {
    let mut text = String::from("/* Generated by piska-export: link-time stub, never loaded at run time. */\n");
    for name in functions {
        text.push_str(&format!("void {name}(void) {{}}\n"));
    }
    text
}

/// Каталог исходников пакета `name` из `cargo metadata` (с зависимостями).
fn package_dir(metadata: &serde_json::Value, name: &str) -> Option<PathBuf> {
    metadata["packages"]
        .as_array()?
        .iter()
        .filter(|p| p["name"] == name)
        .filter_map(|p| p["manifest_path"].as_str())
        .map(|m| Path::new(m).parent().map(Path::to_path_buf))
        .max_by_key(|d| d.as_ref().map(|d| d.display().to_string()))
        .flatten()
}

fn rust_sources(dir: &Path, out: &mut String) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push_str(&std::fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

/// Собрать заглушки всех [`STUB_LIBS`] в `dir`. Возвращает (SONAME, число функций).
pub fn build_stubs(root: &Path, zig: &Path, glibc: &str, dir: &Path, cache: &Path) -> Result<Vec<(String, usize)>> {
    std::fs::create_dir_all(dir)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--filter-platform", "x86_64-unknown-linux-gnu"])
        .current_dir(root)
        .output()
        .map_err(|e| Error(format!("cannot run `cargo metadata`: {e}")))?;
    if !output.status.success() {
        return Err(Error(format!("`cargo metadata` failed:\n{}", String::from_utf8_lossy(&output.stderr))));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut built = Vec::new();
    for (_, lib, soname, krate) in STUB_LIBS {
        let Some(source_dir) = package_dir(&metadata, krate) else {
            // Крейт не в графе зависимостей под Linux — библиотека не нужна.
            continue;
        };
        let mut source = String::new();
        rust_sources(&source_dir.join("src"), &mut source)?;
        let functions = declared_functions(&source);
        if functions.is_empty() {
            return Err(Error(format!("no functions found in `{}`", source_dir.display())));
        }
        let c_file = dir.join(format!("{lib}.c"));
        std::fs::write(&c_file, stub_source(&functions))?;
        let so = dir.join(format!("lib{lib}.so"));
        let status = Command::new(zig)
            .args(["cc", "-target", &format!("x86_64-linux-gnu.{glibc}"), "-shared", "-fPIC", "-nostdlib"])
            .arg(format!("-Wl,-soname,{soname}"))
            .arg("-o")
            .arg(&so)
            .arg(&c_file)
            .env("ZIG_GLOBAL_CACHE_DIR", cache.join("global"))
            .env("ZIG_LOCAL_CACHE_DIR", cache.join("local"))
            .status()
            .map_err(|e| Error(format!("cannot run zig: {e}")))?;
        if !status.success() {
            return Err(Error(format!("zig could not build the link stub for {soname}")));
        }
        built.push((soname.to_string(), functions.len()));
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zig_is_found_by_option_env_then_path() {
        let me = std::env::current_exe().unwrap();
        let no_env = |_: &str| None;
        assert_eq!(find_zig(Some(&me), &no_env, &|| false).unwrap(), me);
        assert!(find_zig(Some(Path::new("no/such/zig.exe")), &no_env, &|| true).is_err());
        let me_text = me.display().to_string();
        let env = move |name: &str| (name == ZIG_ENV).then(|| me_text.clone());
        assert_eq!(find_zig(None, &env, &|| false).unwrap(), me);
        assert_eq!(find_zig(None, &no_env, &|| true).unwrap(), PathBuf::from("zig"));
        let err = find_zig(None, &no_env, &|| false).unwrap_err().0;
        assert!(err.contains("https://ziglang.org/download/") && err.contains(ZIG_ENV) && err.contains("--zig"), "{err}");
    }

    #[test]
    fn compiler_args_drop_targets_and_rust_lld() {
        let args: Vec<String> = [
            "-m64", "--target=x86_64-unknown-linux-gnu", "-target", "x86_64-unknown-linux-gnu", "-fuse-ld=lld",
            "-B/rust/gcc-ld", "-lgcc_s", "-lc", "-o", "out", "-Wl,--as-needed",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(filter_compiler_args(&args), ["-m64", "-lunwind", "-lc", "-o", "out", "-Wl,--as-needed"]);
    }

    #[test]
    fn response_files_are_expanded_with_gnu_quoting() {
        let file = std::env::temp_dir().join(format!("piska-rsp-test-{}.rsp", std::process::id()));
        std::fs::write(&file, "-o\n\"C:\\\\dir with space\\\\out\"\n-lc\n").unwrap();
        let args = vec!["-m64".to_string(), format!("@{}", file.display())];
        assert_eq!(expand_response_files(&args).unwrap(), ["-m64", "-o", "C:\\dir with space\\out", "-lc"]);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn fake_pkg_config_points_at_stubs() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pkg_config_answer(&args(&["--libs", "--cflags", "alsa"]), "C:/stubs").unwrap(), "-LC:/stubs -lasound");
        assert_eq!(pkg_config_answer(&args(&["--modversion", "libudev"]), "C:/stubs").unwrap(), "1.0.0");
        assert!(pkg_config_answer(&args(&["--libs", "gtk+-3.0"]), "C:/stubs").unwrap_err().contains("gtk+-3.0"));
        assert_eq!(
            pkg_config_answer(&args(&["--libs", "alsa"]), r"C:\My Dir\stubs").unwrap(),
            r"-LC:/My\ Dir/stubs -lasound"
        );
    }

    #[test]
    fn stub_functions_come_from_extern_declarations_not_methods() {
        let source = r#"
            impl Bitfield { pub fn get_bit(&self, i: usize) -> bool { true } pub fn new(x: u8) -> Self { todo() } }
            extern "C" {
                pub fn snd_pcm_open(pcm: *mut *mut snd_pcm_t, name: *const c_char) -> c_int;
                pub fn snd_pcm_close (pcm: *mut snd_pcm_t) -> c_int;
                pub fn snd_pcm_open(pcm: *mut *mut snd_pcm_t) -> c_int;
            }
        "#;
        let names = declared_functions(source);
        assert_eq!(names, ["new", "snd_pcm_open", "snd_pcm_close"]);
        let c = stub_source(&names[1..]);
        assert!(c.contains("void snd_pcm_open(void) {}\n"));
    }
}
