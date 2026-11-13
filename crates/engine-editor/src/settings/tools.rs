//! Страница Preferences → Build & Tools: пути к инструментам сборки (cargo, Android SDK /
//! NDK, Java, zig, CMake, компилятор C++) с кнопками Detect (найти самому) и Validate
//! (проверить), и каталог экспорта по умолчанию. Пути уходят в команды сборки и экспорта
//! через [`Editor::with_tools`](crate::state::Editor) — пустое поле ничего не меняет.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use egui::{vec2, Rect, Sense};

use super::SettingsUi;
use crate::icons::{self, Icon};
use crate::prefs::Preferences;
use crate::state::Editor;
use crate::theme;

/// Какой инструмент.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Cargo,
    AndroidSdk,
    AndroidNdk,
    Java,
    Zig,
    Cmake,
    Cxx,
}

impl Tool {
    pub const ALL: [Tool; 7] = [Tool::Cargo, Tool::AndroidSdk, Tool::AndroidNdk, Tool::Java, Tool::Zig, Tool::Cmake, Tool::Cxx];

    /// Постоянное имя (идентификаторы кнопок, тесты).
    pub fn id(self) -> &'static str {
        match self {
            Tool::Cargo => "cargo",
            Tool::AndroidSdk => "android-sdk",
            Tool::AndroidNdk => "android-ndk",
            Tool::Java => "java",
            Tool::Zig => "zig",
            Tool::Cmake => "cmake",
            Tool::Cxx => "cxx",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tool::Cargo => "cargo",
            Tool::AndroidSdk => "Android SDK",
            Tool::AndroidNdk => "Android NDK",
            Tool::Java => "Java (JDK)",
            Tool::Zig => "zig",
            Tool::Cmake => "CMake",
            Tool::Cxx => "C++ compiler",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Tool::Cargo => "Program that builds Rust code and runs the export tool. Empty: cargo from PATH.",
            Tool::AndroidSdk => "Folder of the Android SDK (ANDROID_HOME) for Android exports.",
            Tool::AndroidNdk => "Folder of the Android NDK (ANDROID_NDK_HOME) for Android exports.",
            Tool::Java => "Folder of a JDK (JAVA_HOME) used to sign Android packages.",
            Tool::Zig => "zig program (PISKA_ZIG) for Linux builds made on Windows.",
            Tool::Cmake => "Program that runs `cmake` build commands of C++ plugins.",
            Tool::Cxx => "C++ compiler given to C++ plugin builds as CXX.",
        }
    }

    /// Папка (SDK, NDK, JDK), а не программа.
    fn is_folder(self) -> bool {
        matches!(self, Tool::AndroidSdk | Tool::AndroidNdk | Tool::Java)
    }

    pub(crate) fn value(self, prefs: &mut Preferences) -> &mut String {
        match self {
            Tool::Cargo => &mut prefs.tool_cargo,
            Tool::AndroidSdk => &mut prefs.tool_android_sdk,
            Tool::AndroidNdk => &mut prefs.tool_android_ndk,
            Tool::Java => &mut prefs.tool_java,
            Tool::Zig => &mut prefs.tool_zig,
            Tool::Cmake => &mut prefs.tool_cmake,
            Tool::Cxx => &mut prefs.tool_cxx,
        }
    }
}

/// Программа `name` в каталогах PATH (на Windows — с `.exe`).
fn on_path(name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(&file)).find(|p| p.is_file())
}

fn env_dir(names: &[&str]) -> Option<PathBuf> {
    names.iter().filter_map(std::env::var_os).map(PathBuf::from).find(|p| p.is_dir())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Android SDK в обычных местах установки.
fn detect_sdk() -> Option<PathBuf> {
    env_dir(&["ANDROID_HOME", "ANDROID_SDK_ROOT"]).or_else(|| {
        let mut places = Vec::new();
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            places.push(PathBuf::from(local).join("Android").join("Sdk"));
        }
        if let Some(home) = home() {
            places.push(home.join("Android").join("Sdk"));
            places.push(home.join("Library").join("Android").join("sdk"));
        }
        places.into_iter().find(|p| p.join("platform-tools").is_dir() || p.join("platforms").is_dir())
    })
}

/// Найти инструмент сам: переменные окружения, PATH, обычные места установки.
pub fn detect(tool: Tool) -> Option<PathBuf> {
    match tool {
        Tool::Cargo => on_path("cargo").or_else(|| {
            let base = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".cargo")))?;
            let file = base.join("bin").join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
            file.is_file().then_some(file)
        }),
        Tool::AndroidSdk => detect_sdk(),
        Tool::AndroidNdk => env_dir(&["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]).or_else(|| {
            // Самая новая версия в `<SDK>/ndk/<версия>`.
            let mut versions: Vec<PathBuf> = std::fs::read_dir(detect_sdk()?.join("ndk"))
                .ok()?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("source.properties").is_file())
                .collect();
            versions.sort();
            versions.pop()
        }),
        Tool::Java => env_dir(&["JAVA_HOME"]).or_else(|| {
            let studio = [
                PathBuf::from("C:/Program Files/Android/Android Studio/jbr"),
                PathBuf::from("/opt/android-studio/jbr"),
                PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home"),
            ];
            studio.into_iter().find(|p| p.join("bin").is_dir())
        }),
        Tool::Zig => std::env::var_os("PISKA_ZIG").map(PathBuf::from).filter(|p| p.is_file()).or_else(|| on_path("zig")),
        Tool::Cmake => on_path("cmake"),
        Tool::Cxx => ["c++", "g++", "clang++"].into_iter().find_map(on_path),
    }
}

/// Первая непустая строка вывода программы (`--version` пишут кто в stdout, кто в stderr).
fn first_line(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run `{}`: {e}", program.display()))?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
    if output.status.success() {
        Ok(if line.is_empty() { "runs".to_string() } else { line })
    } else {
        Err(format!("`{}` failed: {line}", program.display()))
    }
}

/// Проверить путь: программа запускается (и говорит версию), папка — та, что нужна.
/// Пустое значение проверяет то, что нашлось бы без него.
pub fn validate(tool: Tool, value: &str) -> Result<String, String> {
    let path = match value.trim() {
        "" => detect(tool).ok_or_else(|| format!("{} is not set and was not found: enter its path or install it", tool.label()))?,
        given => PathBuf::from(given),
    };
    if tool.is_folder() && !path.is_dir() {
        return Err(format!("folder `{}` does not exist", path.display()));
    }
    match tool {
        Tool::Cargo | Tool::Cmake | Tool::Cxx => first_line(&path, &["--version"]),
        Tool::Zig => first_line(&path, &["version"]).map(|v| format!("zig {v}")),
        Tool::Java => {
            let java = path.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
            if !java.is_file() {
                return Err(format!("`{}` has no bin/java: choose the JDK folder", path.display()));
            }
            first_line(&java, &["-version"])
        }
        Tool::AndroidSdk => {
            if path.join("platform-tools").is_dir() || path.join("platforms").is_dir() {
                Ok(format!("Android SDK at {}", path.display()))
            } else {
                Err(format!("`{}` has no platform-tools or platforms: choose the SDK folder", path.display()))
            }
        }
        Tool::AndroidNdk => match std::fs::read_to_string(path.join("source.properties")) {
            Ok(text) => Ok(text
                .lines()
                .find_map(|l| l.strip_prefix("Pkg.Revision").map(|v| format!("NDK {}", v.trim_start_matches([' ', '=']).trim())))
                .unwrap_or_else(|| "Android NDK".to_string())),
            Err(_) => Err(format!("`{}` has no source.properties: choose the NDK folder", path.display())),
        },
    }
}

impl Editor {
    /// Кнопка Detect инструмента на странице Build & Tools.
    pub fn tool_detect_id(tool: &str) -> egui::Id {
        egui::Id::new(("tool_detect", tool.to_string()))
    }

    /// Кнопка Validate инструмента на странице Build & Tools.
    pub fn tool_validate_id(tool: &str) -> egui::Id {
        egui::Id::new(("tool_validate", tool.to_string()))
    }

    /// Итог последней проверки инструмента (`tool` — `cargo`, `android-sdk`…): текст и
    /// «это ошибка».
    pub fn tool_check(&self, tool: &str) -> Option<(String, bool)> {
        self.settings.tool_checks.iter().find(|(id, _, _)| *id == tool).map(|(_, text, error)| (text.clone(), *error))
    }

    fn set_tool_check(&mut self, tool: Tool, result: Result<String, String>) {
        let (text, error) = match result {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        self.settings.tool_checks.retain(|(id, _, _)| *id != tool.id());
        self.settings.tool_checks.push((tool.id(), text.clone(), error));
        self.set_preferences_status(format!("{}: {text}", tool.label()), error);
    }
}

/// Build & Tools.
pub(super) fn build_tools(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    s.group("Tools");
    for tool in Tool::ALL {
        let check = editor.tool_check(tool.id());
        let value = tool.value(&mut editor.prefs);
        let row = s.row(tool.label(), tool.description(), !value.is_empty(), |ui| {
            let hint = match detect(tool) {
                Some(found) => found.display().to_string(),
                None => "not found".to_string(),
            };
            // Справа налево: значок проверки, Validate, Detect, а поле занимает остаток —
            // длинный путь прокручивается внутри поля, кнопки остаются в строке.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((text, error)) = &check {
                    let (rect, response) = ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
                    let (icon, color) = if *error { (Icon::Warn, theme::WARN) } else { (Icon::Check, theme::OK) };
                    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(theme::ICON_S, theme::ICON_S)), icon, color);
                    response.on_hover_text(text.as_str());
                }
                let validate_button = theme::button(ui, "Validate");
                let validate_clicked = ui
                    .interact(validate_button.rect, Editor::tool_validate_id(tool.id()), Sense::click())
                    .on_hover_text("Run the tool (or look into the folder) and show what was found")
                    .clicked();
                let detect_button = theme::button(ui, "Detect");
                let detect_clicked = ui
                    .interact(detect_button.rect, Editor::tool_detect_id(tool.id()), Sense::click())
                    .on_hover_text("Fill the path in from the environment, PATH or the usual install folders")
                    .clicked();
                ui.add_sized(
                    vec2(ui.available_width().max(80.0), theme::FIELD_H),
                    egui::TextEdit::singleline(value)
                        .font(theme::mono())
                        .hint_text(egui::RichText::new(hint).color(theme::TEXT_DISABLED))
                        .vertical_align(egui::Align::Center),
                );
                (detect_clicked, validate_clicked)
            })
            .inner
        });
        if row.reset {
            tool.value(&mut editor.prefs).clear();
        }
        if let Some((detect_clicked, validate_clicked)) = row.inner {
            if detect_clicked {
                match detect(tool) {
                    Some(found) => {
                        *tool.value(&mut editor.prefs) = found.display().to_string();
                        editor.set_tool_check(tool, Ok(format!("found {}", found.display())));
                    }
                    None => editor.set_tool_check(tool, Err(format!("{} was not found: enter its path", tool.label()))),
                }
            }
            if validate_clicked {
                let value = tool.value(&mut editor.prefs).clone();
                editor.set_tool_check(tool, validate(tool, &value));
            }
        }
    }
    s.note("Empty fields keep the usual lookup (PATH and environment variables); the faint text shows what would be used.");

    s.group("Export");
    let defaults = Preferences::default();
    s.text(
        "Export folder",
        "File → Build… puts games into <folder>/<game>/<platform>. Relative to the workspace, or absolute.",
        &mut editor.prefs.export_folder,
        &defaults.export_folder,
        "dist",
    );
}
