//! Командная строка установщика и режим без окна (`--unattended`, для CI): скрипт
//! запускается с `-Json -Yes`, его события идут прямо в stdout, код выхода — его.
//!
//! Коды выхода (те же у окна, по итогу последнего прогона):
//!
//! | код | что |
//! |---|---|
//! | 0 | готово (или ничего не запускалось) |
//! | 1 | шаг не удался |
//! | 2 | неверные аргументы |
//! | 3 | нужна перезагрузка, потом продолжить (`Repair`) |
//! | 4 | лицензия или права администратора не приняты |
//! | 5 | установку остановили (Cancel / закрыли окно) |
//! | 6 | скрипты не удалось подготовить или запустить (нет PowerShell / bash, нет места) |

use std::path::PathBuf;
use std::process::Stdio;

use engine_editor::kit::i18n::{brand, trf, Language};

use crate::app::{find_checkout, is_checkout, Outcome};
use crate::packages::Packages;
use crate::scripts::{self, ScriptOptions, Shell};

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_BAD_ARGS: i32 = 2;
pub const EXIT_REBOOT: i32 = 3;
pub const EXIT_DECLINED: i32 = 4;
pub const EXIT_CANCELLED: i32 = 5;
pub const EXIT_CANNOT_START: i32 = 6;

/// Разобранная командная строка.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub help: bool,
    pub version: bool,
    pub dry_run: bool,
    pub components: Option<Vec<String>>,
    pub install_dir: Option<String>,
    pub source: Option<String>,
    pub lang: Option<String>,
    pub unattended: bool,
    pub uninstall: bool,
    pub accept_licences: Vec<String>,
    pub accept_all_licences: bool,
    pub prefer_pwsh: bool,
}

/// Справка (`--help`).
pub fn usage() -> String {
    let name = brand::ENGINE_NAME;
    format!(
        "{name} Setup {version}\n\n\
         Graphical installer of {name}: a wizard over install/install.ps1 (Windows) and install/install.sh\n\
         (Linux, macOS). It checks what is installed, shows the exact plan and installs what is missing.\n\n\
         Usage: piska-installer [options]\n\n\
         \x20 --dry-run               show and log the plan, change nothing (in the window and with --unattended)\n\
         \x20 --components a,b        preselect components: core, cpp, web, android, linux-cross, engine, all\n\
         \x20 --install-dir DIR       install folder\n\
         \x20 --source DIR            an existing engine checkout to use instead of downloading the sources\n\
         \x20 --lang en|ru            interface language (default: the system language)\n\
         \x20 --unattended            no window: run the script and print its JSON events (one per line) for CI\n\
         \x20 --accept-license ID     with --unattended: accept a licence (vs-buildtools, android-sdk; repeat or a,b)\n\
         \x20 --accept-licenses       with --unattended: accept every licence\n\
         \x20 --uninstall             start on the Uninstall page (with --unattended: remove what the manifest lists)\n\
         \x20 --pwsh                  Windows: run the script in PowerShell 7 instead of Windows PowerShell 5.1\n\
         \x20 --help, --version\n\n\
         Exit codes: 0 done, 1 a step failed, 2 bad arguments, 3 restart needed, 4 licence or rights declined,\n\
         5 stopped by the user, 6 the scripts could not be prepared or started.\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

/// Разобрать аргументы (без имени программы).
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().filter(|v| !v.starts_with("--")).ok_or_else(|| trf!("{name} needs a value", name = name));
        match arg.as_str() {
            "--help" | "-h" | "/?" => out.help = true,
            "--version" | "-V" => out.version = true,
            "--dry-run" => out.dry_run = true,
            "--components" => {
                let list: Vec<String> = value("--components")?.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
                let known = Packages::embedded();
                let mut expanded = Vec::new();
                for c in list {
                    if c == "all" {
                        expanded.extend(known.components.iter().map(|k| k.id.clone()));
                    } else if known.component(&c).is_some() {
                        expanded.push(c);
                    } else {
                        let list = known.components.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join(", ");
                        return Err(trf!("unknown component `{name}`; known: {list}, all", name = c, list = list));
                    }
                }
                out.components = Some(expanded);
            }
            "--install-dir" => out.install_dir = Some(value("--install-dir")?),
            "--source" => out.source = Some(value("--source")?),
            "--lang" => {
                let lang = value("--lang")?;
                if Language::from_code(&lang).is_none() {
                    return Err(trf!("unknown language `{lang}` (en, ru)", lang = lang));
                }
                out.lang = Some(lang);
            }
            "--unattended" => out.unattended = true,
            "--uninstall" => out.uninstall = true,
            "--accept-license" | "--accept-licence" => {
                out.accept_licences.extend(value("--accept-license")?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
            }
            "--accept-licenses" | "--accept-licences" => out.accept_all_licences = true,
            "--pwsh" => out.prefer_pwsh = true,
            other => return Err(trf!("unknown argument `{arg}` (see --help)", arg = other)),
        }
    }
    if let Some(source) = &out.source {
        if !is_checkout(std::path::Path::new(source)) {
            return Err(trf!("--source {path}: not an engine checkout (Cargo.toml and tools/piska-export are missing)", path = source));
        }
    }
    Ok(out)
}

/// Код выхода по исходу прогона.
pub fn exit_code_of(outcome: Option<&Outcome>) -> i32 {
    match outcome {
        None | Some(Outcome::Succeeded) => EXIT_OK,
        Some(Outcome::Failed(_)) => EXIT_FAILED,
        Some(Outcome::Reboot) => EXIT_REBOOT,
        Some(Outcome::Declined(_)) => EXIT_DECLINED,
        Some(Outcome::Cancelled | Outcome::Cancelling | Outcome::Running) => EXIT_CANCELLED,
    }
}

/// Режим без окна: скрипт с `-Json -Yes`, его stdout и stderr — прямо в наши.
pub fn unattended(args: &Args) -> i32 {
    let packages = Packages::embedded();
    let components = args.components.clone().unwrap_or_else(|| vec!["core".into()]);
    let components = packages.resolve(&components);
    let source = args.source.clone().or_else(|| find_checkout().map(|p| p.display().to_string()));
    let options = ScriptOptions {
        components: if args.uninstall { Vec::new() } else { components.clone() },
        dry_run: args.dry_run,
        install_dir: args.install_dir.clone(),
        source_dir: source.filter(|_| components.iter().any(|c| c == "engine")),
        accept_licences: args.accept_licences.clone(),
        yes: true,
        uninstall: args.uninstall,
        elevate: (Shell::native() == Shell::Bash).then(|| "sudo".to_string()),
        ..ScriptOptions::default()
    };
    let extracted = match scripts::extract() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("piska-installer: {err}");
            return EXIT_CANNOT_START;
        }
    };
    let shell = Shell::native();
    let interpreter = match scripts::interpreter(&extracted.script(shell), shell, args.prefer_pwsh) {
        Ok(i) => i,
        Err(err) => {
            eprintln!("piska-installer: {err}");
            return EXIT_CANNOT_START;
        }
    };
    let mut script_args = options.args(shell);
    if args.accept_all_licences {
        script_args.push(if shell == Shell::PowerShell { "-AcceptLicenses" } else { "--accept-licenses" }.into());
    }
    let status = engine_build::host::command(&interpreter.program)
        .args(&interpreter.leading)
        .args(&script_args)
        .env_remove("PISKA_INSTALL_NOMAIN")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    drop(extracted);
    match status {
        Ok(s) => s.code().unwrap_or(EXIT_FAILED),
        Err(err) => {
            eprintln!("piska-installer: {}", crate::ui::start_failed_text(&err.to_string()));
            EXIT_CANNOT_START
        }
    }
}

/// Каталог, из которого запущен установщик (для `--source` по умолчанию).
pub fn checkout() -> Option<PathBuf> {
    find_checkout()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn arguments_are_parsed_and_checked() {
        let a = parse(&s(&["--dry-run", "--components", "core,web", "--lang", "ru", "--unattended", "--accept-license", "vs-buildtools,android-sdk"])).unwrap();
        assert!(a.dry_run && a.unattended);
        assert_eq!(a.components, Some(s(&["core", "web"])));
        assert_eq!(a.accept_licences, s(&["vs-buildtools", "android-sdk"]));
        assert_eq!(parse(&s(&["--components", "all"])).unwrap().components.unwrap().len(), 6);
        assert!(parse(&s(&["--components", "nope"])).unwrap_err().contains("unknown component"));
        assert!(parse(&s(&["--bogus"])).unwrap_err().contains("unknown argument"));
        assert!(parse(&s(&["--lang", "xx"])).is_err());
        assert!(parse(&s(&["--install-dir"])).is_err());
        assert!(parse(&s(&["--source", "/definitely/not/here"])).is_err());
        assert!(usage().contains("--unattended") && usage().contains(brand::ENGINE_NAME));
        assert_eq!(exit_code_of(Some(&Outcome::Cancelled)), EXIT_CANCELLED);
    }
}
