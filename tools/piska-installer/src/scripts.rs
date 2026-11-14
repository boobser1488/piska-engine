//! Скрипты установки, вшитые в программу: установщик не повторяет их логику, а запускает их
//! (`install.ps1 -Json` на Windows, `bash install.sh --json` на Linux и macOS) и читает
//! события. При запуске скрипты пишутся в личный временный каталог и читаются обратно для
//! проверки; каталог удаляется, когда установщик закрывается.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine_editor::kit::i18n::{brand, tr_s, trf};

pub const INSTALL_PS1: &str = include_str!("../../../install/install.ps1");
pub const INSTALL_SH: &str = include_str!("../../../install/install.sh");
pub const PACKAGES_TOML: &str = include_str!("../../../install/packages.toml");

/// Какой скрипт и каким интерпретатором.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// `install.ps1` (Windows PowerShell 5.1 или PowerShell 7).
    PowerShell,
    /// `install.sh` (bash 3.2+).
    Bash,
}

impl Shell {
    pub fn native() -> Shell {
        if cfg!(windows) {
            Shell::PowerShell
        } else {
            Shell::Bash
        }
    }
}

/// Скрипты во временном каталоге (удаляется при `drop`).
#[derive(Debug)]
pub struct Extracted {
    pub dir: PathBuf,
}

impl Extracted {
    pub fn ps1(&self) -> PathBuf {
        self.dir.join("install.ps1")
    }

    pub fn sh(&self) -> PathBuf {
        self.dir.join("install.sh")
    }

    pub fn script(&self, shell: Shell) -> PathBuf {
        match shell {
            Shell::PowerShell => self.ps1(),
            Shell::Bash => self.sh(),
        }
    }
}

impl Drop for Extracted {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Записать скрипты в новый каталог внутри `parent` и проверить, что записано именно то.
pub fn extract_to(parent: &Path) -> Result<Extracted, String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let name = format!("{}-installer-{}-{}-{}", brand::CONFIG_DIR_NAME, std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst), stamp);
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).map_err(|e| trf!("cannot create {path}: {error}", path = dir.display(), error = e))?;
    let extracted = Extracted { dir };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&extracted.dir, std::fs::Permissions::from_mode(0o700));
    }
    for (file, text) in [("install.ps1", INSTALL_PS1), ("install.sh", INSTALL_SH), ("packages.toml", PACKAGES_TOML)] {
        let path = extracted.dir.join(file);
        std::fs::write(&path, text.as_bytes()).map_err(|e| trf!("cannot write {path}: {error}", path = path.display(), error = e))?;
        let back = std::fs::read(&path).map_err(|e| trf!("cannot read back {path}: {error}", path = path.display(), error = e))?;
        if back != text.as_bytes() {
            return Err(trf!("{path} was not written correctly (the file read back differs): check the disk and the antivirus", path = path.display()));
        }
    }
    Ok(extracted)
}

/// В системный временный каталог.
pub fn extract() -> Result<Extracted, String> {
    extract_to(&std::env::temp_dir())
}

/// Параметры запуска скрипта — то, что выбрано в мастере.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptOptions {
    pub components: Vec<String>,
    pub dry_run: bool,
    /// Только тесты: план «как на чистой машине».
    pub assume_clean: bool,
    pub install_dir: Option<String>,
    /// Готовый каталог с исходниками движка (вместо клонирования).
    pub source_dir: Option<String>,
    pub cache: Option<String>,
    /// Принятые лицензии по id (`vs-buildtools`, `android-sdk`).
    pub accept_licences: Vec<String>,
    /// Ответ «да» на «Continue?» перед шагом с правами администратора (запрос Windows всё равно появится).
    pub confirm_elevation: bool,
    pub skip_steps: Vec<String>,
    pub resume: bool,
    pub repair: bool,
    pub no_prefs: bool,
    pub no_shortcuts: bool,
    pub desktop_shortcut: bool,
    pub file_associations: bool,
    pub uninstall: bool,
    /// Без вопросов (режим без окна).
    pub yes: bool,
    /// Linux / macOS: `pkexec` или `none` (команду выполняет человек в терминале).
    pub elevate: Option<String>,
}

impl ScriptOptions {
    /// Аргументы скрипта (без пути к нему).
    pub fn args(&self, shell: Shell) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let ps = shell == Shell::PowerShell;
        let flag = |on: bool, ps_name: &str, sh_name: &str, out: &mut Vec<String>| {
            if on {
                out.push(if ps { ps_name } else { sh_name }.to_string());
            }
        };
        flag(true, "-Json", "--json", &mut out);
        if self.uninstall {
            flag(true, "-Uninstall", "--uninstall", &mut out);
        } else if !self.components.is_empty() {
            out.push(if ps { "-Components" } else { "--components" }.into());
            out.push(self.components.join(","));
        }
        flag(self.dry_run || self.assume_clean, "-DryRun", "--dry-run", &mut out);
        flag(self.assume_clean, "-AssumeClean", "--assume-clean", &mut out);
        flag(self.resume, "-Resume", "--resume", &mut out);
        flag(self.repair, "-Repair", "--repair", &mut out);
        flag(self.yes, "-Yes", "--yes", &mut out);
        let value = |ps_name: &str, sh_name: &str, v: &Option<String>, out: &mut Vec<String>| {
            if let Some(v) = v.as_ref().filter(|v| !v.trim().is_empty()) {
                out.push(if ps { ps_name } else { sh_name }.to_string());
                out.push(v.trim().to_string());
            }
        };
        value("-InstallDir", "--prefix", &self.install_dir, &mut out);
        value("-SourceDir", "--source", &self.source_dir, &mut out);
        value("-Cache", "--cache", &self.cache, &mut out);
        let list = |v: &[String]| if v.is_empty() { None } else { Some(v.join(",")) };
        value("-AcceptLicense", "--accept-license", &list(&self.accept_licences), &mut out);
        value("-SkipSteps", "--skip-steps", &list(&self.skip_steps), &mut out);
        flag(self.confirm_elevation, "-ConfirmElevation", "--confirm-elevation", &mut out);
        flag(self.no_prefs, "-NoPrefs", "--no-prefs", &mut out);
        flag(self.no_shortcuts, "-NoShortcuts", "--no-shortcuts", &mut out);
        flag(self.desktop_shortcut, "-DesktopShortcut", "--desktop-shortcut", &mut out);
        flag(self.file_associations, "-FileAssociations", "--file-associations", &mut out);
        if !ps {
            value("", "--elevate", &self.elevate, &mut out);
        }
        out
    }
}

/// Программа, которая выполнит скрипт.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreter {
    pub program: PathBuf,
    /// Аргументы до аргументов скрипта (включая путь к нему).
    pub leading: Vec<String>,
    pub shell: Shell,
}

fn in_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// Windows: `powershell.exe` 5.1 (есть всегда), иначе `pwsh` 7; `prefer_pwsh` — наоборот.
/// Политика выполнения меняется только для этого процесса (`-ExecutionPolicy Bypass`).
/// Linux / macOS: `bash`.
pub fn interpreter(script: &Path, shell: Shell, prefer_pwsh: bool) -> Result<Interpreter, String> {
    match shell {
        Shell::PowerShell => {
            let system = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .map(|root| root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"))
                .filter(|p| p.is_file());
            let pwsh = in_path(&["pwsh.exe", "pwsh"]);
            let program = if prefer_pwsh { pwsh.or(system) } else { system.or(pwsh) }
                .or_else(|| in_path(&["powershell.exe", "powershell"]))
                .ok_or_else(|| tr_s("neither Windows PowerShell (powershell.exe) nor PowerShell 7 (pwsh) was found").to_string())?;
            Ok(Interpreter {
                program,
                leading: ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]
                    .iter()
                    .map(|s| s.to_string())
                    .chain(std::iter::once(script.display().to_string()))
                    .collect(),
                shell,
            })
        }
        Shell::Bash => {
            let program = [PathBuf::from("/bin/bash"), PathBuf::from("/usr/bin/bash")]
                .into_iter()
                .find(|p| p.is_file())
                .or_else(|| in_path(&["bash", "bash.exe"]))
                .ok_or_else(|| tr_s("bash was not found (the installer needs bash 3.2 or newer)").to_string())?;
            Ok(Interpreter { program, leading: vec![script.display().to_string()], shell })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_extracted_verified_and_removed() {
        let parent = std::env::temp_dir().join(format!("piska-installer-extract-{}", std::process::id()));
        std::fs::create_dir_all(&parent).unwrap();
        let dir;
        {
            let e = extract_to(&parent).unwrap();
            dir = e.dir.clone();
            assert_eq!(std::fs::read_to_string(e.ps1()).unwrap(), INSTALL_PS1);
            assert_eq!(std::fs::read_to_string(e.sh()).unwrap(), INSTALL_SH);
            assert_eq!(std::fs::read_to_string(e.dir.join("packages.toml")).unwrap(), PACKAGES_TOML);
            assert!(dir.starts_with(&parent));
        }
        assert!(!dir.exists(), "the temp folder is removed");
        std::fs::remove_dir_all(parent).unwrap();
        // Windows PowerShell 5.1 читает файл без BOM как ANSI: скрипт обязан быть ASCII.
        assert!(INSTALL_PS1.is_ascii() && INSTALL_SH.is_ascii());
        assert!(!INSTALL_SH.contains('\r'), "install.sh has LF line endings");
    }

    #[test]
    fn options_become_script_arguments() {
        let o = ScriptOptions {
            components: vec!["core".into(), "web".into()],
            dry_run: true,
            install_dir: Some("D:\\Piska".into()),
            source_dir: Some("C:\\src\\engine".into()),
            accept_licences: vec!["vs-buildtools".into(), "android-sdk".into()],
            confirm_elevation: true,
            skip_steps: vec!["core.vulkan".into()],
            no_prefs: true,
            file_associations: true,
            ..ScriptOptions::default()
        };
        assert_eq!(
            o.args(Shell::PowerShell).join(" "),
            "-Json -Components core,web -DryRun -InstallDir D:\\Piska -SourceDir C:\\src\\engine -AcceptLicense vs-buildtools,android-sdk -SkipSteps core.vulkan -ConfirmElevation -NoPrefs -FileAssociations"
        );
        let sh = ScriptOptions { elevate: Some("pkexec".into()), ..o.clone() };
        assert_eq!(
            sh.args(Shell::Bash).join(" "),
            "--json --components core,web --dry-run --prefix D:\\Piska --source C:\\src\\engine --accept-license vs-buildtools,android-sdk --skip-steps core.vulkan --confirm-elevation --no-prefs --file-associations --elevate pkexec"
        );
        let u = ScriptOptions { uninstall: true, dry_run: true, components: vec!["core".into()], ..ScriptOptions::default() };
        assert_eq!(u.args(Shell::PowerShell), ["-Json", "-Uninstall", "-DryRun"]);
    }
}
