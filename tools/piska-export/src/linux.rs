//! Экспорт для Linux (x86_64): исполняемый файл + `assets/` рядом (как у Windows),
//! значок, `.desktop` и скрипт, добавляющий игру в меню; `--tar` — ещё и
//! `<game>-linux-x86_64.tar.gz` (права на исполнение сохраняются и при сборке на Windows).
//!
//! Как собирается:
//! * **на Linux x86_64** — обычный `cargo build` (нужны пакеты разработки ALSA и udev);
//! * **на Windows / macOS** (или на Linux с `--glibc`) — `cargo build --target
//!   x86_64-unknown-linux-gnu`, где компилятор C/C++ и линковщик — zig
//!   (`-target x86_64-linux-gnu.<glibc>`, по умолчанию glibc 2.31), а ALSA и udev
//!   подставлены заглушками для линковки (см. [`crate::zigcc`]). Нужен только zig.
//!
//! Результат проверяется: ELF x86_64, загрузчик, `NEEDED`, наибольшая нужная версия glibc.

use std::path::{Path, PathBuf};

use crate::archive::{self, TarFile};
use crate::project::{copy_dir, fresh_dir, Project};
use crate::{cargo, elf, icon, step, zigcc, Error, Options, Platform, Report, Result};

pub const TARGET: &str = "x86_64-unknown-linux-gnu";
/// glibc по умолчанию при кросс-сборке: 2.31 (Debian 11, Ubuntu 20.04) — программа
/// запускается на всех живых дистрибутивах.
pub const DEFAULT_GLIBC: &str = "2.31";
const INTERPRETER: &str = "/lib64/ld-linux-x86-64.so.2";

/// Библиотеки, которые есть в любом настольном дистрибутиве (с установленным звуком).
pub const DISTRO_LIBS: &[&str] = &[
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "libgcc_s.so.1",
    "libstdc++.so.6",
    "ld-linux-x86-64.so.2",
    "libasound.so.2",
    "libudev.so.1",
];

/// Чем собирать.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Build {
    /// `cargo build` на самом Linux x86_64.
    Native,
    /// `cargo build --target x86_64-unknown-linux-gnu` с zig вместо компилятора и линковщика.
    Zig { zig: PathBuf, glibc: String },
}

/// Что известно о машине (для тестов подставляется).
pub struct Host<'a> {
    pub linux_x86_64: bool,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub zig_in_path: &'a dyn Fn() -> bool,
}

impl Host<'_> {
    pub fn current() -> Host<'static> {
        Host {
            linux_x86_64: cfg!(all(target_os = "linux", target_arch = "x86_64")),
            env: &|name| std::env::var(name).ok().filter(|v| !v.is_empty()),
            zig_in_path: &zigcc::zig_in_path,
        }
    }
}

/// Проверить версию glibc вида `2.31`.
fn valid_glibc(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Выбрать способ сборки или объяснить, чего не хватает. На Linux без `--glibc` и
/// `--zig` — обычная сборка; иначе нужен zig (`--zig`, `PISKA_ZIG`, PATH).
pub fn plan(host: &Host, glibc: Option<&str>, zig: Option<&Path>) -> Result<Build> {
    if let Some(version) = glibc {
        if !valid_glibc(version) {
            return Err(Error(format!("`--glibc {version}` is not a glibc version like 2.31")));
        }
    }
    if host.linux_x86_64 && glibc.is_none() && zig.is_none() {
        return Ok(Build::Native);
    }
    let zig = zigcc::find_zig(zig, host.env, host.zig_in_path)?;
    Ok(Build::Zig {
        zig,
        glibc: glibc.unwrap_or(DEFAULT_GLIBC).to_string(),
    })
}

/// `.desktop`; `@DIR@` заменяет скрипт установки на каталог игры.
pub fn desktop_entry(title: &str, exe: &str) -> String {
    let title: String = title.chars().filter(|c| !c.is_control()).collect();
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={title}\nExec=\"@DIR@/{exe}\"\nPath=@DIR@\n\
         Icon=@DIR@/icon.png\nTerminal=false\nCategories=Game;\n"
    )
}

/// Скрипт: добавить игру в меню приложений пользователя (`--remove` — убрать).
pub fn install_script(game: &str, exe: &str) -> String {
    format!(
        r#"#!/bin/sh
# Adds the game to the application menu of the current user; --remove takes it out.
set -e
dir=$(cd "$(dirname "$0")" && pwd)
apps="${{XDG_DATA_HOME:-$HOME/.local/share}}/applications"
entry="$apps/piska-{game}.desktop"
if [ "$1" = "--remove" ]; then
    rm -f "$entry"
    echo "removed $entry"
    exit 0
fi
mkdir -p "$apps"
chmod +x "$dir/{exe}"
sed "s|@DIR@|$dir|g" "$dir/{game}.desktop" > "$entry"
echo "added $entry"
"#
    )
}

/// Итог проверки ELF: замечания для отчёта (ошибка — если файл не для Linux x86_64).
pub fn check_binary(info: &elf::ElfInfo) -> Result<Vec<String>> {
    if info.machine != "x86_64" {
        return Err(Error(format!("the program is built for `{}`, not x86_64", info.machine)));
    }
    let mut notes = Vec::new();
    match info.interpreter.as_deref() {
        Some(INTERPRETER) => {}
        Some(other) => notes.push(format!("warning: unusual dynamic loader `{other}` (expected {INTERPRETER})")),
        None => notes.push("note: no dynamic loader (static program)".to_string()),
    }
    let unusual: Vec<&str> = info
        .needed
        .iter()
        .map(String::as_str)
        .filter(|lib| !DISTRO_LIBS.contains(lib))
        .collect();
    notes.push(format!("needs: {}", info.needed.join(", ")));
    if !unusual.is_empty() {
        notes.push(format!(
            "warning: {} may be missing on players' machines",
            unusual.join(", ")
        ));
    }
    if let Some((major, minor)) = info.max_glibc() {
        let note = format!("requires glibc {major}.{minor} or newer");
        if (major, minor) > (2, 31) {
            notes.push(format!(
                "{note}: older distributions (Debian 11, Ubuntu 20.04) cannot run it; \
                 build with `--glibc {DEFAULT_GLIBC}` (zig) to support them"
            ));
        } else {
            notes.push(format!("{note} (Debian 11 / Ubuntu 20.04 and every newer distribution)"));
        }
    }
    Ok(notes)
}

fn exe_name(project: &Project) -> String {
    project.package.clone()
}

pub fn export(project: &Project, options: &Options) -> Result<Report> {
    const STEPS: u32 = 4;
    let out = project.out_dir(options.out.as_deref(), Platform::Linux);

    step(1, STEPS, "checking the toolchain");
    let build = plan(&Host::current(), options.glibc.as_deref(), options.zig.as_deref())?;
    if matches!(build, Build::Zig { .. }) && !cargo::has_target(TARGET) {
        return Err(Error(format!("the Rust target `{TARGET}` is not installed. Run `rustup target add {TARGET}`")));
    }

    let profile = if options.release { "release" } else { "dev" };
    let profile_dir = if profile == "dev" { "debug" } else { profile };
    let target_dir = cargo::target_dir(&project.root)?;
    let mut notes = Vec::new();
    let binary = match &build {
        Build::Native => {
            step(2, STEPS, &format!("compiling `{}` for Linux x86_64", project.package));
            cargo::run(&project.root, &["build", "-p", &project.package, "--bins", "--profile", profile], &[])?;
            target_dir.join(profile_dir).join(exe_name(project))
        }
        Build::Zig { zig, glibc } => {
            let version = zigcc::zig_version(zig)?;
            println!("piska-export: zig {version} ({}), glibc {glibc}", zig.display());
            let work = target_dir.join("piska-linux");
            let wrappers = work.join("bin");
            let stubs = work.join(format!("stubs-{glibc}"));
            let cache = work.join("zig-cache");
            zigcc::install_wrappers(&wrappers)?;
            let built = zigcc::build_stubs(&project.root, zig, glibc, &stubs, &cache)?;
            for (soname, count) in &built {
                println!("piska-export:   link stub {soname} ({count} functions)");
            }
            notes.push(format!(
                "cross-built with zig {version} for glibc {glibc}; {} linked against generated stubs (the real distro libraries are loaded at run time)",
                built.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" and ")
            ));
            step(2, STEPS, &format!("compiling `{}` for Linux x86_64 (zig)", project.package));
            let env = zigcc::cargo_env(zig, glibc, &wrappers, &stubs, &cache);
            let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            cargo::run(
                &project.root,
                &["build", "-p", &project.package, "--bins", "--profile", profile, "--target", TARGET],
                &env_refs,
            )?;
            target_dir.join(TARGET).join(profile_dir).join(exe_name(project))
        }
    };
    if !binary.is_file() {
        return Err(Error(format!(
            "`{}` is missing: the game needs `src/main.rs` calling `{}::main()`",
            binary.display(),
            project.lib_name
        )));
    }

    step(3, STEPS, "checking the program and copying files");
    let info = elf::parse_file(&binary)?;
    notes.extend(check_binary(&info)?);
    fresh_dir(&out)?;
    let exe = exe_name(project);
    let exe_path = out.join(&exe);
    if exe_path.is_file() {
        std::fs::remove_file(&exe_path)?;
    }
    let exe_size = std::fs::copy(&binary, &exe_path)?;
    set_executable(&exe_path)?;
    project.pack_assets()?;
    let mut files = vec![(exe.clone(), exe_size)];
    if project.assets_dir().is_dir() {
        let size = copy_dir(&project.assets_dir(), &out.join("assets"))?;
        files.push(("assets/".to_string(), size));
    }
    let icon_png = icon::app_icon(project, 256)?;
    std::fs::write(out.join("icon.png"), &icon_png)?;
    let desktop_name = format!("{}.desktop", project.game);
    let desktop = desktop_entry(&project.title, &exe);
    std::fs::write(out.join(&desktop_name), &desktop)?;
    let script_name = "install-desktop-entry.sh";
    let script = install_script(&project.game, &exe);
    std::fs::write(out.join(script_name), &script)?;
    set_executable(&out.join(script_name))?;
    for name in ["icon.png", desktop_name.as_str(), script_name] {
        files.push((name.to_string(), std::fs::metadata(out.join(name))?.len()));
    }

    if options.tar {
        step(4, STEPS, "writing the .tar.gz");
        let tar_name = format!("{}-linux-x86_64.tar.gz", project.game);
        let mut entries = vec![
            TarFile { path: format!("{}/{exe}", project.game), bytes: std::fs::read(&binary)?, executable: true },
            TarFile { path: format!("{}/icon.png", project.game), bytes: icon_png, executable: false },
            TarFile { path: format!("{}/{desktop_name}", project.game), bytes: desktop.into_bytes(), executable: false },
            TarFile { path: format!("{}/{script_name}", project.game), bytes: script.into_bytes(), executable: true },
        ];
        collect_tar_files(&out.join("assets"), &format!("{}/assets", project.game), &mut entries)?;
        let bytes = archive::tar_gz(&entries)?;
        std::fs::write(out.join(&tar_name), &bytes)?;
        files.push((tar_name, bytes.len() as u64));
    } else {
        step(4, STEPS, "done (add --tar for a .tar.gz)");
    }

    notes.push(format!(
        "on Linux: run ./{exe} in {} (./install-desktop-entry.sh adds it to the application menu)",
        out.display()
    ));
    if !cfg!(target_os = "linux") {
        notes.push("built on another system: never run here; copy the .tar.gz to Linux (it keeps the executable bit)".to_string());
    }
    Ok(Report { out_dir: out, files, notes })
}

fn collect_tar_files(dir: &Path, prefix: &str, out: &mut Vec<TarFile>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_tar_files(&path, &format!("{prefix}/{name}"), out)?;
        } else {
            out.push(TarFile { path: format!("{prefix}/{name}"), bytes: std::fs::read(&path)?, executable: false });
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    // На Windows бита «исполняемый» нет: его хранит .tar.gz (--tar).
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host<'a>(linux: bool, zig_in_path: bool, env: &'a dyn Fn(&str) -> Option<String>) -> Host<'a> {
        let in_path: &'a dyn Fn() -> bool = if zig_in_path { &|| true } else { &|| false };
        Host { linux_x86_64: linux, env, zig_in_path: in_path }
    }

    #[test]
    fn linux_host_builds_natively_unless_asked_for_zig() {
        let no_env = |_: &str| None;
        assert_eq!(plan(&host(true, false, &no_env), None, None).unwrap(), Build::Native);
        assert_eq!(
            plan(&host(true, true, &no_env), Some("2.28"), None).unwrap(),
            Build::Zig { zig: PathBuf::from("zig"), glibc: "2.28".into() }
        );
        assert!(plan(&host(true, true, &no_env), Some("latest"), None).unwrap_err().0.contains("not a glibc version"));
    }

    #[test]
    fn other_systems_need_zig_and_say_where_to_get_it() {
        let no_env = |_: &str| None;
        let err = plan(&host(false, false, &no_env), None, None).unwrap_err().0;
        assert!(err.contains("https://ziglang.org/download/") && err.contains("--zig"), "{err}");
        assert!(err.contains("build.yml"), "points to the CI route: {err}");
        assert_eq!(
            plan(&host(false, true, &no_env), None, None).unwrap(),
            Build::Zig { zig: PathBuf::from("zig"), glibc: "2.31".into() }
        );
        let me = std::env::current_exe().unwrap();
        assert_eq!(
            plan(&host(false, false, &no_env), None, Some(&me)).unwrap(),
            Build::Zig { zig: me, glibc: "2.31".into() }
        );
    }

    #[test]
    fn desktop_entry_and_script_fit_together() {
        let desktop = desktop_entry("Star\nfall", "template");
        assert!(desktop.starts_with("[Desktop Entry]\n"));
        assert!(desktop.contains("Name=Starfall\n"));
        assert!(desktop.contains("Exec=\"@DIR@/template\"\n"));
        assert!(desktop.contains("Icon=@DIR@/icon.png\n"));
        assert!(desktop.contains("Categories=Game;"));
        let script = install_script("template", "template");
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(r#"sed "s|@DIR@|$dir|g" "$dir/template.desktop""#));
        assert!(script.contains("${XDG_DATA_HOME:-$HOME/.local/share}/applications"));
    }

    #[test]
    fn binary_check_reports_loader_libraries_and_glibc() {
        let info = elf::parse(&elf::tests::sample_elf()).unwrap();
        let notes = check_binary(&info).unwrap();
        assert!(notes.iter().any(|n| n == "needs: libc.so.6, libm.so.6"), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("glibc 2.29") && n.contains("every newer")), "{notes:?}");
        assert!(!notes.iter().any(|n| n.starts_with("warning")), "{notes:?}");

        let mut newer = info;
        newer.version_needs.push(("libc.so.6".into(), "GLIBC_2.34".into()));
        newer.needed.push("libfoo.so.1".into());
        let notes = check_binary(&newer).unwrap();
        assert!(notes.iter().any(|n| n.contains("glibc 2.34") && n.contains("--glibc 2.31")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("libfoo.so.1 may be missing")), "{notes:?}");

        newer.machine = "aarch64".into();
        assert!(check_binary(&newer).is_err());
    }
}
