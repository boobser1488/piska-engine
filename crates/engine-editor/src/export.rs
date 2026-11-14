//! Окно File → Build…: экспорт игры через `piska-export` (веб, Windows, Android, Linux).
//!
//! Экспорт идёт той же фоновой машиной, что и Build (Ctrl+B): вывод — в Console,
//! статус — в строке окна. После удачной веб-сборки окно предлагает открыть игру в
//! браузере (локальный сервер `piska-export serve`).

use crate::i18n::{tr_s, trf};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

use egui::{vec2, RichText};

use crate::code::{BuildCommand, BuildStatus};
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::{theme, EditorError};

/// Платформа экспорта.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPlatform {
    /// Браузер: WebAssembly + WebGPU (или WebGL2).
    Web,
    /// Windows: `.exe` и каталог `assets/` рядом.
    Windows,
    /// Android: подписанный APK (нужны Android SDK, NDK, Java, cargo-ndk).
    Android,
    /// Linux x86_64: программа, `assets/`, `.desktop`, `.tar.gz`.
    Linux,
}

/// Можно ли собрать Linux-версию на этой машине: сам Linux или есть zig для
/// кросс-сборки — в `PISKA_ZIG` или в PATH (проверяется один раз).
fn linux_buildable() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        let runs = |program: &str, args: &[&str]| {
            engine_build::host::command(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        let zig = std::env::var("PISKA_ZIG").ok().filter(|z| !z.is_empty()).unwrap_or_else(|| "zig".to_string());
        cfg!(target_os = "linux") || runs(&zig, &["version"])
    })
}

impl ExportPlatform {
    /// Платформы, которые этот компьютер умеет собрать: Web и Android — везде
    /// (чего не хватает из Android SDK, скажет сам экспорт), Windows — на Windows,
    /// Linux — на Linux или с zig (`PISKA_ZIG` / PATH).
    pub fn available() -> Vec<ExportPlatform> {
        let mut platforms = vec![ExportPlatform::Web];
        if cfg!(windows) {
            platforms.push(ExportPlatform::Windows);
        }
        platforms.push(ExportPlatform::Android);
        if linux_buildable() {
            platforms.push(ExportPlatform::Linux);
        }
        platforms
    }

    /// Имя подкоманды `piska-export` и каталога в `dist/<game>/`.
    pub fn command(self) -> &'static str {
        match self {
            ExportPlatform::Web => "web",
            ExportPlatform::Windows => "windows",
            ExportPlatform::Android => "android",
            ExportPlatform::Linux => "linux",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ExportPlatform::Web => tr_s("Web"),
            ExportPlatform::Windows => tr_s("Windows"),
            ExportPlatform::Android => tr_s("Android"),
            ExportPlatform::Linux => tr_s("Linux"),
        }
    }
}

/// Что и куда собрать.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub platform: ExportPlatform,
    /// Оптимизированная сборка (дольше собирается, меньше и быстрее).
    pub release: bool,
    /// Каталог результата: относительно рабочего пространства или абсолютный.
    /// Пусто — `dist/<game>/<platform>`.
    pub out_dir: String,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            platform: ExportPlatform::Web,
            release: false,
            out_dir: String::new(),
        }
    }
}

/// Порт локального сервера для «Run in Browser».
pub const SERVE_PORT: u16 = 8080;

#[derive(Default)]
pub(crate) struct ExportState {
    pub open: bool,
    pub settings: ExportSettings,
    /// Последний запущенный экспорт: платформа и каталог результата.
    pub last: Option<(ExportPlatform, PathBuf)>,
    /// Работающий `piska-export serve`.
    server: Option<(Child, PathBuf)>,
    /// Открытое системное окно «Папка» для каталога результата.
    browse: Option<engine_build::file_dialog::Dialog>,
}

impl Drop for ExportState {
    fn drop(&mut self) {
        if let Some((mut child, _)) = self.server.take() {
            let _ = child.kill();
        }
    }
}

impl Editor {
    /// Рабочее пространство движка (там `Cargo.toml` и `games/`).
    fn workspace(&self) -> PathBuf {
        self.config
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Каталог экспорта по умолчанию (Preferences → Build & Tools → Export folder).
    fn export_root(&self) -> PathBuf {
        let folder = self.prefs.export_folder.trim();
        let folder = if folder.is_empty() { "dist" } else { folder };
        if Path::new(folder).is_absolute() {
            PathBuf::from(folder)
        } else {
            self.workspace().join(folder)
        }
    }

    /// Каталог результата для настроек: заданный или `<каталог экспорта>/<game>/<platform>`.
    pub fn export_out_dir(&self, settings: &ExportSettings) -> Option<PathBuf> {
        let game = self.config.game_crate.as_ref()?;
        let out = settings.out_dir.trim();
        Some(if out.is_empty() {
            self.export_root().join(game).join(settings.platform.command())
        } else if Path::new(out).is_absolute() {
            PathBuf::from(out)
        } else {
            self.workspace().join(out)
        })
    }

    /// Команда экспорта, которую запустит окно Build: `cargo run -p piska-export -- …`.
    pub fn export_command(&self, settings: &ExportSettings) -> Result<BuildCommand, EditorError> {
        let game = self.config.game_crate.as_ref().ok_or_else(|| {
            EditorError::Other(
                "the editor does not know which game crate to export; start it with \
                 `EditorConfig::default().game_crate(\"<name>\")` (the crate in games/<name>)"
                    .to_string(),
            )
        })?;
        if !ExportPlatform::available().contains(&settings.platform) {
            return Err(EditorError::Other(match settings.platform {
                ExportPlatform::Linux => {
                    tr_s("Linux builds need a Linux machine, or zig for cross-compiling (PISKA_ZIG or PATH) \
                     (see docs/api/export.md)")
                        .to_string()
                }
                other => trf!("{} builds can only be made on {}", other.label(), other.label()),
            }));
        }
        let out = self.export_out_dir(settings).expect("game crate is set");
        let mut args = vec![
            "run".to_string(),
            "-q".to_string(),
            "-p".to_string(),
            "piska-export".to_string(),
            "--".to_string(),
            settings.platform.command().to_string(),
            "--game".to_string(),
            game.clone(),
            "--out".to_string(),
            out.display().to_string(),
        ];
        if settings.release {
            args.push("--release".to_string());
        }
        if settings.platform == ExportPlatform::Linux {
            // Архив с правами на исполнение — то, что раздают игрокам.
            args.push("--tar".to_string());
        }
        Ok(self.with_tools(
            BuildCommand::new(trf!("Export {}", settings.platform.label()), "cargo")
                .args(args)
                .dir(self.workspace()),
        ))
    }

    pub(crate) fn start_export(&mut self, settings: ExportSettings) -> Result<(), EditorError> {
        if self.build_status() == BuildStatus::Running {
            return Err(EditorError::Other("a build is already running; wait for it to finish".to_string()));
        }
        let command = self.export_command(&settings)?;
        let out = self.export_out_dir(&settings).expect("checked by export_command");
        // Работающий `piska-export serve` держит исполняемый файл инструмента: cargo не
        // смог бы его пересобрать. Новая сборка всё равно заменит раздаваемые файлы.
        if let Some((mut child, _)) = self.export.server.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        log::info!(target: "build", "exporting `{}` for {} into {}", self.config.game_crate.as_deref().unwrap_or(""), settings.platform.label(), out.display());
        self.export.last = Some((settings.platform, out));
        self.export.settings = settings;
        self.code.start_build(vec![command]);
        Ok(())
    }

    /// Открыть каталог последнего экспорта в файловом менеджере.
    pub(crate) fn reveal_export(&self) -> Result<(), EditorError> {
        let (_, dir) = self.export.last.as_ref().ok_or_else(|| EditorError::Other("nothing has been exported yet".to_string()))?;
        crate::state::reveal(dir);
        Ok(())
    }

    /// Раздать последнюю веб-сборку локальным сервером и открыть её в браузере.
    pub(crate) fn run_in_browser(&mut self) -> Result<(), EditorError> {
        let dir = match &self.export.last {
            Some((ExportPlatform::Web, dir)) if dir.join("index.html").is_file() => dir.clone(),
            _ => return Err(EditorError::Other(tr_s("export the game for the Web first").to_string())),
        };
        let url = format!("http://127.0.0.1:{SERVE_PORT}/");
        // Сервер уже раздаёт этот каталог — достаточно открыть страницу.
        if let Some((child, served)) = &mut self.export.server {
            if *served == dir && matches!(child.try_wait(), Ok(None)) {
                open_url(&url);
                return Ok(());
            }
            let _ = child.kill();
            self.export.server = None;
        }
        let child = engine_build::host::command("cargo")
            .args(["run", "-q", "-p", "piska-export", "--", "serve"])
            .arg(&dir)
            .args(["--port", &SERVE_PORT.to_string(), "--open"])
            .current_dir(self.workspace())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| EditorError::Other(trf!("cannot start `piska-export serve`: {e}. Is cargo on PATH?", e = e)))?;
        log::info!(target: "build", "serving {} at {url}", dir.display());
        self.export.server = Some((child, dir));
        Ok(())
    }

    /// Открыто ли окно Build.
    pub fn export_dialog_open(&self) -> bool {
        self.export.open
    }

    /// Настройки окна Build (последние выбранные).
    pub fn export_settings(&self) -> &ExportSettings {
        &self.export.settings
    }

    /// Последняя строка `piska-export: [n/m] …` из лога — шаг идущего экспорта.
    pub(crate) fn export_step() -> Option<String> {
        engine_core::logging::records()
            .iter()
            .rev()
            .take(400)
            .find_map(|r| r.message.strip_prefix("piska-export: [").map(|rest| format!("[{rest}")))
    }

    /// Окно File → Build….
    pub(crate) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export.open {
            return;
        }
        let mut open = true;
        let status = self.build_status();
        let running = status == BuildStatus::Running;
        let game = self.config.game_crate.clone();
        let export_folder = match self.prefs.export_folder.trim() {
            "" => "dist".to_string(),
            folder => folder.trim_end_matches(['/', '\\']).to_string(),
        };
        // Ответ системного окна «Папка» (кнопка «Browse…» у каталога результата).
        if let Some(outcome) = self.export.browse.as_ref().and_then(|d| d.poll()) {
            self.export.browse = None;
            if let engine_build::file_dialog::Outcome::Chosen(path) = outcome {
                self.export.settings.out_dir = path.display().to_string();
            }
        } else if self.export.browse.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let browse_start = self.workspace().join(&export_folder);
        let can_browse = engine_build::file_dialog::available() && self.export.browse.is_none();
        let mut browse = false;
        let mut settings = self.export.settings.clone();
        let mut actions: Vec<ExportAction> = Vec::new();
        crate::window_kit::tool_window(egui::Id::new("editor_export"), tr_s("Build"))
            .resizable(false)
            .default_size(vec2(480.0, 360.0))
            .show(ctx, &mut open, |ui| {
                ui.set_width(480.0);
                let Some(game) = game.as_deref() else {
                    theme::empty_state(
                        ui,
                        Icon::Build,
                        tr_s("No game crate"),
                        "Start the editor with EditorConfig::default().game_crate(\"<name>\") \
                         for a game in games/<name> to export it.",
                    );
                    return;
                };
                section(ui, tr_s("Target"));
                egui::Grid::new("export_target")
                    .num_columns(2)
                    .spacing(vec2(theme::SP_L, theme::SP_M))
                    .min_col_width(120.0)
                    .show(ui, |ui| {
                        muted(ui, tr_s("Game"));
                        ui.label(RichText::new(format!("games/{game}")).font(theme::mono()).color(theme::TEXT));
                        ui.end_row();
                        muted(ui, tr_s("Platform"));
                        let options: Vec<(ExportPlatform, &str, &str)> = ExportPlatform::available()
                            .into_iter()
                            .map(|p| {
                                let tip = match p {
                                    ExportPlatform::Web => tr_s("WebAssembly page for browsers (WebGPU, WebGL2 fallback)"),
                                    ExportPlatform::Windows => tr_s("Windows program: .exe with the assets folder next to it"),
                                    ExportPlatform::Android => {
                                        tr_s("Signed APK for phones and tablets (Android 8.0+); needs the Android SDK, NDK, Java and cargo-ndk")
                                    }
                                    ExportPlatform::Linux => {
                                        tr_s("Linux x86_64 program with assets, .desktop entry and a .tar.gz")
                                    }
                                };
                                (p, p.label(), tip)
                            })
                            .collect();
                        ui.add_enabled_ui(!running, |ui| theme::segmented(ui, &mut settings.platform, &options));
                        ui.end_row();
                        muted(ui, tr_s("Configuration"));
                        ui.add_enabled_ui(!running, |ui| {
                            theme::segmented(
                                ui,
                                &mut settings.release,
                                &[
                                    (false, tr_s("Debug"), tr_s("Faster to build; larger and slower")),
                                    (true, tr_s("Release"), tr_s("Optimized: smaller and faster, takes longer to build")),
                                ],
                            )
                        });
                        ui.end_row();
                    });
                ui.add_space(theme::SP_L);
                section(ui, tr_s("Output"));
                let default_out = format!("{export_folder}/{game}/{}", settings.platform.command());
                ui.horizontal(|ui| {
                    // Поле на всю ширину, справа — «Browse…» (системное окно выбора папки).
                    let button_w = 92.0;
                    let field_w = ui.available_width() - button_w - ui.spacing().item_spacing.x;
                    ui.add_enabled_ui(!running, |ui| crate::controls::text_field(ui, &mut settings.out_dir, &default_out, field_w));
                    let button = ui
                        .add_enabled_ui(!running && can_browse, |ui| crate::controls::button_sized(ui, crate::controls::Kind::Normal, None, tr_s("Browse…"), button_w))
                        .inner;
                    if button.clicked() {
                        browse = true;
                    }
                });
                ui.add_space(theme::SP_XS);
                ui.label(
                    RichText::new(tr_s("Relative to the workspace, or an absolute path. Empty — the default above."))
                        .size(theme::FONT_CAPTION)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(theme::SP_L);

                // Итог последнего экспорта (или идущий шаг).
                let last = self.export.last.clone();
                let (icon, color, text) = match (&status, &last) {
                    (BuildStatus::Running, _) => (
                        Icon::Build,
                        theme::WARN,
                        Self::export_step().unwrap_or_else(|| tr_s("Starting the export tool…").to_string()),
                    ),
                    (BuildStatus::Succeeded { seconds }, Some((platform, dir))) => (
                        Icon::Check,
                        theme::OK,
                        trf!("{p0} build ready in {seconds} s: {p1}", p0 = platform.label(), p1 = dir.display(), seconds = format!("{:.0}", seconds)),
                    ),
                    (BuildStatus::Failed { .. }, Some(_)) => {
                        (Icon::Error, theme::ERROR, tr_s("Export failed — see the Console for the compiler output").to_string())
                    }
                    _ => (Icon::Info, theme::TEXT_MUTED, tr_s("Ready to build").to_string()),
                };
                ui.horizontal(|ui| {
                    icons::inline(ui, icon, color);
                    ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                });
                ui.add_space(theme::SP_L);

                let succeeded = matches!(status, BuildStatus::Succeeded { .. });
                // Общая строка кнопок: Build справа, Close левее; действия с результатом — слева.
                crate::window_kit::dialog_actions(ui, |ui| {
                    let build = ui.add_enabled_ui(!running, |ui| theme::primary_button(ui, tr_s("Build"))).inner;
                    if build.clicked() {
                        actions.push(ExportAction::Build);
                    }
                    if theme::secondary_button(ui, tr_s("Close")).clicked() {
                        actions.push(ExportAction::Close);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if let (true, Some((platform, _))) = (succeeded, &last) {
                            if theme::icon_text_button(ui, Icon::FolderOpen, tr_s("Reveal in File Manager")).clicked() {
                                actions.push(ExportAction::Reveal);
                            }
                            if *platform == ExportPlatform::Web
                                && theme::icon_text_button(ui, Icon::Globe, tr_s("Run in Browser")).clicked()
                            {
                                actions.push(ExportAction::RunInBrowser);
                            }
                        }
                    });
                });
            });
        self.export.settings = settings.clone();
        if browse {
            let request = engine_build::file_dialog::Request::new(engine_build::file_dialog::Kind::Folder, tr_s("Choose the Output Folder"))
                .folder(&browse_start);
            self.export.browse = Some(engine_build::file_dialog::Dialog::spawn(request));
        }
        if !open {
            self.export.open = false;
        }
        for action in actions {
            self.actions.push(match action {
                ExportAction::Build => crate::state::Action::Export(settings.clone()),
                ExportAction::Reveal => crate::state::Action::RevealExport,
                ExportAction::RunInBrowser => crate::state::Action::RunInBrowser,
                ExportAction::Close => crate::state::Action::ShowBuildDialog(false),
            });
        }
    }
}

enum ExportAction {
    Build,
    Reveal,
    RunInBrowser,
    Close,
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
    ui.add_space(theme::SP_S);
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0_f32, theme::LINE));
    ui.add_space(theme::SP_M);
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_MUTED));
}

fn open_url(url: &str) {
    let result = if cfg!(windows) {
        engine_build::host::command("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        engine_build::host::command("open").arg(url).spawn()
    } else {
        engine_build::host::command("xdg-open").arg(url).spawn()
    };
    if let Err(err) = result {
        log::warn!("open {url} in a browser ({err})");
    }
}
