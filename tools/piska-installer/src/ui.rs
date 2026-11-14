//! Окно установщика: слева шаги мастера и язык, справа страница, снизу «Назад / Далее».
//! Всё оформление — токены и виджеты редактора и хаба (`ui-guidelines.md`); числа размеров
//! здесь только для раскладки страниц.

use std::time::Duration;

use egui::{pos2, vec2, Align, Align2, Color32, Id, Layout, Margin, Rect, Response, RichText, Sense, Stroke, Ui};
use engine_editor::kit::i18n::{brand, tr_ctx_s, tr_s, trf, trn, Language};
use engine_editor::kit::icons::{self, Icon};
use engine_editor::kit::controls::{self, Kind};
use engine_editor::kit::theme;
use engine_editor::kit::window as kit;
use piska_hub::widgets::{self, Tone};

use crate::app::{Action, BrowseTarget, Dialog, Installer, Mode, Outcome, Page, Presence, Preset, Source, StepState};
use crate::diag::RowStatus;
use crate::protocol::{human_bytes, LogLevel, PlanStep};
use crate::runner::Purpose;

/// Ширина левой колонки.
const NAV_WIDTH: f32 = 232.0;
/// Поля страницы слева и справа.
const PAD: f32 = 28.0;
/// Нижняя полоса с кнопками.
const BAR: f32 = 52.0;
/// Колонка подписей на странице Options.
const LABEL: f32 = 176.0;
/// Высота строки таблиц.
const ROW: f32 = 30.0;

/// Текст «не удалось запустить» (PowerShell / bash не нашлись или не запустились).
pub fn start_failed_text(err: &str) -> String {
    if cfg!(windows) {
        trf!("Windows PowerShell could not be started: {error}", error = err)
    } else {
        trf!("bash could not be started: {error}", error = err)
    }
}

/// Название компонента.
pub fn component_name(id: &str) -> &'static str {
    match id {
        "core" => tr_s("Core"),
        "cpp" => tr_s("C++ plugins"),
        "web" => tr_s("Web export"),
        "android" => tr_s("Android export"),
        "linux-cross" => tr_s("Linux export"),
        "engine" => tr_s("Engine and tools"),
        "config" => tr_s("Configuration"),
        _ => tr_s("Other"),
    }
}

/// Зачем компонент.
pub fn component_purpose(id: &str) -> &'static str {
    match id {
        "core" => {
            if cfg!(windows) {
                tr_s("Rust, Visual Studio C++ build tools and Git: needed to build the engine and every game")
            } else {
                tr_s("Rust, the system compiler and libraries, Git: needed to build the engine and every game")
            }
        }
        "cpp" => tr_s("CMake and Ninja for hot-reloadable C++ gameplay plugins"),
        "web" => tr_s("The WebAssembly target: export games to the browser"),
        "android" => tr_s("JDK, Android SDK and NDK, cargo-ndk: build Android packages (APK)"),
        "linux-cross" => tr_s("zig and the Linux target: build Linux versions of games on this computer"),
        "engine" => tr_s("Engine sources; the hub, editor and modeler are built; shortcuts"),
        _ => "",
    }
}

/// Название шага (скрипты пишут по-английски; известные шаги переводятся по id).
pub fn step_title(id: &str, script: &str) -> String {
    let tail = |prefix: &str| script.strip_prefix(prefix).unwrap_or(script).trim().to_string();
    match id {
        "core.git" => "Git".to_string(),
        "core.buildtools" => tr_s("Visual Studio Build Tools (C++ workload and Windows SDK)").to_string(),
        "core.rustup" => tr_s("Rust (rustup, stable toolchain)").to_string(),
        "core.rust-components" => tr_s("Rust components (clippy, rustfmt)").to_string(),
        "core.vulkan" => tr_s("Vulkan runtime (GPU driver)").to_string(),
        "core.packages" => tr_s("System packages (compiler, libraries, Vulkan)").to_string(),
        "cpp.cmake" => "CMake".to_string(),
        "cpp.ninja" => "Ninja".to_string(),
        "cpp.packages" => tr_s("CMake and Ninja (system packages)").to_string(),
        "android.jdk" => script.to_string(),
        "android.cmdline-tools" => tr_s("Android command-line tools (sdkmanager)").to_string(),
        "android.licenses" => tr_s("Android SDK licences").to_string(),
        "android.packages" => tr_s("Android SDK packages").to_string(),
        "android.cargo-ndk" => script.to_string(),
        "android.env" => tr_s("Environment variables for Android").to_string(),
        "linux-cross.zig" => script.to_string(),
        "linux-cross.env" => tr_s("Environment variable PISKA_ZIG").to_string(),
        "engine.source" => tr_s("Engine sources").to_string(),
        "engine.build" => tr_s("Build the hub, editor, export tool and modeler").to_string(),
        "engine.shortcuts" => tr_s("Start menu shortcuts").to_string(),
        "engine.desktop" => tr_s("Application menu entries").to_string(),
        "engine.assoc" => tr_s("File associations").to_string(),
        "engine.uninstaller" => tr_s("Uninstall script").to_string(),
        "config.preferences" => tr_s("Editor preferences: tool paths").to_string(),
        "config.cargo" => tr_s("Cargo configuration: shared build folder").to_string(),
        _ if id.ends_with(".target") || id.contains(".target.") => trf!("Rust target {target}", target = tail("Rust target")),
        _ => script.to_string(),
    }
}

/// Почему шагу нужны права администратора.
pub fn elevation_text(id: &str, script: &str) -> String {
    match id {
        "core.buildtools" => tr_s("Visual Studio Build Tools install for all users: Windows shows one administrator (UAC) prompt.").to_string(),
        "core.git" | "cpp.cmake" | "android.jdk" if cfg!(windows) => tr_s("This installer may ask Windows for administrator rights (one UAC prompt).").to_string(),
        "core.packages" | "cpp.packages" | "android.jdk" => {
            tr_s("The package manager needs administrator rights: pkexec asks for the password in its own window, or you run the shown command in a terminal.").to_string()
        }
        _ => script.to_string(),
    }
}

/// Подпись вида строки плана.
fn line_kind(kind: &str) -> String {
    match kind {
        "run" => tr_s("Run").into(),
        "download" => tr_s("Download").into(),
        "verify" => tr_s("Verify").into(),
        "cache" => tr_s("Cache").into(),
        "extract" => tr_s("Extract").into(),
        "env" => tr_s("Variable").into(),
        "file" => tr_s("File").into(),
        "prefs" => tr_s("Setting").into(),
        "registry" => tr_s("Registry").into(),
        "shortcut" => tr_s("Shortcut").into(),
        "copy" => tr_s("Copy").into(),
        "note" => tr_s("Note").into(),
        "block" => tr_s("Block").into(),
        other => other.to_string(),
    }
}

/// Действие удаления по-человечески.
fn uninstall_action(action: &str) -> String {
    match action {
        "delete-file" => tr_s("Delete the file").into(),
        "delete-dir" => tr_s("Delete the folder").into(),
        "restore-env" => tr_s("Restore the environment variable").into(),
        "remove-path" => tr_s("Remove from PATH").into(),
        "restore-assoc" => tr_s("Restore the file association").into(),
        "restore-prefs" => tr_s("Restore the editor preferences").into(),
        "remove-cargo-block" | "remove-block" => tr_s("Remove the added block").into(),
        "cargo-uninstall" => tr_s("Uninstall the cargo tool").into(),
        "rustup-target-remove" => tr_s("Remove the Rust target").into(),
        "rustup-self-uninstall" => tr_s("Uninstall Rust (rustup)").into(),
        "winget-uninstall" => tr_s("Uninstall the program (winget)").into(),
        "note-packages" => tr_s("System packages stay installed").into(),
        other => other.to_string(),
    }
}

fn page_title(page: Page, mode: Mode) -> &'static str {
    match page {
        Page::Welcome => tr_s("Welcome"),
        Page::Components => tr_s("Components"),
        Page::Options => tr_s("Options"),
        Page::Licences => tr_s("Licences"),
        Page::Review => tr_s("Review"),
        Page::Installing => match mode {
            Mode::Uninstall => tr_s("Uninstalling"),
            _ => tr_s("Installing"),
        },
        Page::Finish => tr_s("Finish"),
        Page::Uninstall => tr_s("Uninstall"),
    }
}

fn presence_text(p: &Presence) -> (String, Color32) {
    match p {
        Presence::Checking => (tr_s("Checking…").into(), theme::TEXT_MUTED),
        Presence::Installed { version: Some(v) } => (trf!("Installed · {version}", version = v), theme::OK),
        Presence::Installed { version: None } => (tr_s("Installed").into(), theme::OK),
        Presence::Partial { missing, total } => (trf!("Incomplete: {missing} of {total} steps", missing = missing, total = total), theme::WARN),
        Presence::Outdated { version: Some(v) } => (trf!("Outdated · {version}", version = v), theme::WARN),
        Presence::Outdated { version: None } => (tr_s("Outdated").into(), theme::WARN),
        Presence::Missing => (tr_s("Not installed").into(), theme::TEXT_MUTED),
        Presence::Unknown => (tr_s("Unknown").into(), theme::TEXT_DISABLED),
    }
}

/// «1 мин 05 с» / «12 s».
fn duration_text(d: Duration) -> String {
    // Вверх: «0 с» не показывается.
    let s = d.as_secs_f64().ceil() as u64;
    if s >= 60 {
        trf!("{m} min {s} s", m = s / 60, s = format!("{:02}", s % 60))
    } else {
        trf!("{s} s", s = s)
    }
}

impl Installer {
    /// Нарисовать установщик в контексте egui (окно или снимок).
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.hits.clear();
        if self.apply_style(ctx) {
            ctx.request_repaint();
            return;
        }
        egui::SidePanel::left("installer-nav")
            .exact_width(NAV_WIDTH)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(Margin { left: 14.0, right: 14.0, top: 16.0, bottom: 14.0 }))
            .show(ctx, |ui| self.nav(ui));
        egui::TopBottomPanel::bottom("installer-bar")
            .exact_height(BAR)
            .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(Margin::symmetric(PAD - 8.0, 0.0)))
            .show(ctx, |ui| self.bottom_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(Margin { left: PAD, right: PAD, top: 22.0, bottom: 12.0 }))
            .show(ctx, |ui| {
                self.page_header(ui);
                match self.page {
                    Page::Welcome => self.welcome_page(ui),
                    Page::Components => self.components_page(ui),
                    Page::Options => self.options_page(ui),
                    Page::Licences => self.licences_page(ui),
                    Page::Review => self.review_page(ui),
                    Page::Installing => self.installing_page(ui),
                    Page::Finish => self.finish_page(ui),
                    Page::Uninstall => self.uninstall_page(ui),
                }
            });
        self.dialogs(ctx);
        self.keys(ctx);
        self.splash_ui(ctx);
        for action in self.take_actions() {
            self.apply(action);
        }
        let busy = self.detect.as_ref().is_some_and(|d| d.running())
            || self.plan.as_ref().is_some_and(|d| d.running())
            || self.uninstall_plan.as_ref().is_some_and(|d| d.running())
            || self.run.as_ref().is_some_and(|r| r.is_running())
            || self.doctor.is_none()
            || self.toast.is_some();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Клавиатура: Enter — главная кнопка страницы («Далее», «Установить»), Esc — «Назад».
    fn keys(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        let (enter, escape) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if enter && self.can_advance() && !matches!(self.page, Page::Finish) {
            self.push(Action::Next);
        } else if escape && !matches!(self.page, Page::Installing | Page::Finish) {
            self.push(Action::Back);
        }
    }

    /// Тема и масштаб; `true` — шрифты только что поставлены (кадр рисовать рано).
    fn apply_style(&mut self, ctx: &egui::Context) -> bool {
        let zoom = self.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
        }
        let key = Id::new("installer-style");
        let applied: Option<bool> = ctx.data(|d| d.get_temp(key));
        if applied.is_none() {
            theme::apply(ctx, true);
            theme::set_density(ctx, theme::Density::Comfortable);
            ctx.data_mut(|d| d.insert_temp(key, true));
            return true;
        }
        false
    }

    fn record(&mut self, name: impl Into<String>, response: &Response) {
        self.hits.insert(name.into(), response.rect);
    }

    /// Кнопка с именем (для тестов). `true` — нажата. `large` — кнопка нижней строки
    /// (не уже [`theme::DIALOG_BUTTON_MIN_W`]); высота у всех одна.
    #[allow(clippy::too_many_arguments)]
    fn push_button(&mut self, ui: &mut Ui, name: &str, icon: Option<Icon>, label: &str, primary: bool, enabled: bool, large: bool) -> bool {
        let kind = if primary { Kind::Primary } else { Kind::Normal };
        self.push_button_kind(ui, name, icon, label, kind, enabled, large)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_button_kind(&mut self, ui: &mut Ui, name: &str, icon: Option<Icon>, label: &str, kind: Kind, enabled: bool, large: bool) -> bool {
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                let id = Id::new(("installer-button", name));
                let min_w = if large { theme::DIALOG_BUTTON_MIN_W } else { 0.0 };
                controls::button_with_id(ui, Some(id), kind, icon, label, min_w)
            })
            .inner;
        self.record(name, &response);
        enabled && response.clicked()
    }

    /// Флажок с подписью ([`controls::check_with_id`]); `locked` — отмечен и не снимается.
    fn check(&mut self, ui: &mut Ui, name: &str, on: bool, label: &str, enabled: bool, locked: bool) -> bool {
        let response = controls::check_with_id(ui, Id::new(("installer-check", name)), on, label, enabled, locked);
        self.record(name, &response);
        enabled && !locked && response.clicked()
    }

    /// Переключатель-кружок с подписью ([`controls::radio_with_id`]). `true` — нажат.
    fn choice(&mut self, ui: &mut Ui, name: &str, on: bool, label: &str, enabled: bool) -> bool {
        let response = controls::radio_with_id(ui, Id::new(("installer-radio", name)), on, label, enabled);
        self.record(name, &response);
        enabled && response.clicked()
    }

    // -----------------------------------------------------------------------
    // Рамка окна
    // -----------------------------------------------------------------------

    fn nav(&mut self, ui: &mut Ui) {
        // Знак и название.
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            kit::paint_engine_mark(ui.painter(), rect, 1.0);
            ui.add_space(theme::SP_2);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(brand::ENGINE_NAME).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT_STRONG));
                ui.label(RichText::new(trf!("Setup {version}", version = env!("CARGO_PKG_VERSION"))).font(theme::caption()).color(theme::TEXT_MUTED));
            });
        });
        ui.add_space(theme::SP_12);
        let pages = self.pages();
        let current = pages.iter().position(|p| *p == self.page).unwrap_or(0);
        let locked = matches!(self.page, Page::Installing | Page::Finish) || self.run.as_ref().is_some_and(|r| r.is_running());
        for (i, page) in pages.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
            let response = ui.interact(rect, Id::new(("installer-nav", i)), Sense::click());
            let selected = i == current;
            let done = i < current;
            let clickable = done && !locked;
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, theme::R_S + 1.0, theme::mix(theme::BG_HEADER, Color32::WHITE, 0.07));
                painter.rect_filled(Rect::from_min_size(pos2(rect.left(), rect.top() + 7.0), vec2(2.0, rect.height() - 14.0)), 1.0, theme::accent());
            } else if response.hovered() && clickable {
                painter.rect_filled(rect, theme::R_S + 1.0, theme::mix(theme::BG_HEADER, Color32::WHITE, 0.04));
            }
            let dot = pos2(rect.left() + 20.0, rect.center().y);
            if done {
                painter.circle_filled(dot, 9.0, theme::mix(theme::BG_HEADER, theme::OK, 0.25));
                icons::paint(painter, Rect::from_center_size(dot, vec2(11.0, 11.0)), Icon::Check, theme::OK);
            } else {
                let (fill, ink) = if selected { (theme::accent(), Color32::WHITE) } else { (theme::BG_FIELD, theme::TEXT_DISABLED) };
                painter.circle_filled(dot, 9.0, fill);
                painter.text(dot, Align2::CENTER_CENTER, (i + 1).to_string(), theme::strong(10.5), ink);
            }
            let color = if selected { theme::TEXT_STRONG } else if done { theme::TEXT } else { theme::TEXT_MUTED };
            painter.text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, page_title(*page, self.mode), theme::body(), color);
            self.record(format!("nav-{i}"), &response);
            if clickable && response.clicked() {
                self.push(Action::GoTo(*page));
            }
        }
        // Низ колонки: язык и режим.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if self.dry_run {
                ui.horizontal(|ui| {
                    widgets::paint_badge_inline(ui, tr_s("DRY RUN"), Tone::Warn);
                });
                ui.label(RichText::new(tr_s("Nothing on this computer is changed: the scripts only show what they would do.")).font(theme::caption()).color(theme::TEXT_MUTED));
                ui.add_space(theme::SP_4);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (language, name) in [(Language::English, "English"), (Language::Russian, "Русский")] {
                    let selected = self.language == language;
                    let id = format!("lang-{}", language.code());
                    if self.push_button(ui, &id, None, name, selected, true, false) && !selected {
                        self.push(Action::SetLanguage(language));
                    }
                }
            });
            ui.add_space(theme::SP_1);
            widgets::section_label(ui, tr_s("Language"));
        });
    }

    fn page_header(&mut self, ui: &mut Ui) {
        let (title, subtitle) = match self.page {
            Page::Welcome => (trf!("{engine} Setup", engine = brand::ENGINE_NAME), tr_s("Installs and configures everything needed to build games with the engine.").to_string()),
            Page::Components => (tr_s("Choose components").into(), tr_s("What to install. Dependencies are added automatically; what is already on this computer is skipped.").into()),
            Page::Options => (tr_s("Options").into(), tr_s("Where to install, where the engine sources come from and what to set up.").into()),
            Page::Licences => (tr_s("Licences").into(), tr_s("Third-party tools that need your acceptance. Nothing is accepted for you.").into()),
            Page::Review => (tr_s("Review the plan").into(), tr_s("Exactly what will be run, downloaded and changed. Nothing has been changed yet.").into()),
            Page::Installing => match (self.mode, self.run.as_ref().map(|r| r.options.dry_run)) {
                (Mode::Uninstall, _) => (tr_s("Uninstalling").into(), tr_s("Removing what this installer added.").into()),
                (_, Some(true)) => (tr_s("Dry run").into(), tr_s("The scripts show what they would do; nothing is changed.").into()),
                _ => (tr_s("Installing").into(), tr_s("You can keep using the computer. Cancel stops safely; finished steps are kept.").into()),
            },
            Page::Finish => (self.finish_title(), self.finish_subtitle()),
            Page::Uninstall => (tr_s("Uninstall").into(), tr_s("Only what this installer added is removed. Programs that were here before are not touched.").into()),
        };
        ui.label(RichText::new(title).font(theme::strong(18.0)).color(theme::TEXT_STRONG));
        ui.add_space(theme::SP_1);
        ui.label(RichText::new(subtitle).font(theme::body()).color(theme::TEXT_MUTED));
        ui.add_space(theme::SP_6);
        widgets::separator(ui);
        ui.add_space(theme::SP_6);
    }

    fn bottom_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            match self.page {
                Page::Installing => {
                    let running = self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Running);
                    let cancelling = self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Cancelling);
                    if running || cancelling {
                        let label = if cancelling { tr_s("Stopping…") } else { tr_s("Cancel") };
                        if self.push_button(ui, "cancel", Some(Icon::Stop), label, false, running, true) {
                            self.push(Action::Cancel);
                        }
                    } else if self.run.as_ref().is_some_and(|r| r.outcome == Outcome::Succeeded) {
                        if self.push_button(ui, "next", None, tr_s("Next"), true, true, true) {
                            self.push(Action::Next);
                        }
                    } else if self.push_button(ui, "close", None, tr_s("Close"), false, true, true) {
                        self.push(Action::Close);
                    }
                }
                Page::Finish => {
                    if self.push_button(ui, "close", None, tr_s("Close"), true, true, true) {
                        self.push(Action::Close);
                    }
                }
                page => {
                    let (label, icon) = match page {
                        Page::Review if self.dry_run => (tr_s("Run Dry Run"), Some(Icon::Play)),
                        Page::Review if self.mode == Mode::Repair => (tr_s("Repair"), Some(Icon::Build)),
                        Page::Review => (tr_s("Install"), Some(Icon::ArrowDown)),
                        Page::Uninstall => (tr_s("Uninstall"), Some(Icon::Trash)),
                        _ => (tr_s("Next"), None),
                    };
                    let name = if matches!(page, Page::Review) { "install" } else if page == Page::Uninstall { "uninstall" } else { "next" };
                    let enabled = self.can_advance();
                    let kind = if page == Page::Uninstall { Kind::Danger } else { Kind::Primary };
                    if self.push_button_kind(ui, name, icon, label, kind, enabled, true) {
                        self.push(Action::Next);
                    }
                    let first = self.pages().first() == Some(&page);
                    if !first && self.push_button(ui, "back", None, tr_ctx_s("installer", "Back"), false, true, true) {
                        self.push(Action::Back);
                    }
                    if !enabled {
                        if let Some(why) = self.blocked_reason() {
                            ui.add_space(theme::SP_4);
                            ui.label(RichText::new(why).font(theme::caption()).color(theme::WARN));
                        }
                    }
                }
            }
            // Слева: сообщение.
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if let Some((text, _)) = &self.toast {
                    icons::inline_small(ui, Icon::Check, theme::OK);
                    ui.label(RichText::new(text.clone()).font(theme::caption()).color(theme::TEXT));
                } else if matches!(self.page, Page::Components | Page::Options | Page::Review) {
                    self.disk_summary(ui);
                }
            });
        });
    }

    /// Почему «Далее» недоступна.
    fn blocked_reason(&self) -> Option<String> {
        match self.page {
            Page::Welcome if self.backend_error.is_some() => Some(tr_s("The installer scripts could not be prepared.").into()),
            Page::Components if self.selected.is_empty() => Some(tr_s("Choose at least one component.").into()),
            Page::Options if self.install_dir.trim().is_empty() => Some(tr_s("Choose the install folder.").into()),
            Page::Options if !self.source_ok() => Some(tr_s("The engine folder is not an engine checkout.").into()),
            Page::Licences if self.detect.as_ref().is_some_and(|d| d.running()) => Some(tr_s("Still checking this computer…").into()),
            Page::Licences => Some(tr_s("Accept every licence to continue.").into()),
            Page::Review if self.plan.as_ref().is_some_and(|p| p.running()) => Some(tr_s("Preparing the plan…").into()),
            Page::Review if !self.licences_accepted() => Some(tr_s("Accept the licences first.").into()),
            Page::Review => Some(tr_s("The plan could not be prepared.").into()),
            Page::Uninstall if self.uninstall_plan.as_ref().is_some_and(|p| p.running()) => Some(tr_s("Reading the install manifest…").into()),
            Page::Uninstall => Some(tr_s("Nothing to remove.").into()),
            _ => None,
        }
    }

    fn disk_summary(&self, ui: &mut Ui) {
        let need = self.required_bytes();
        let (text, color) = match self.free_bytes() {
            Some(free) => (
                trf!("Needs about {need} · {free} free", need = human_bytes(need), free = human_bytes(free)),
                if self.low_disk() { theme::WARN } else { theme::TEXT_MUTED },
            ),
            None => (trf!("Needs about {need}", need = human_bytes(need)), theme::TEXT_MUTED),
        };
        let icon = if self.low_disk() { Icon::Warn } else { Icon::Save };
        icons::inline_small(ui, icon, color);
        ui.label(RichText::new(text).font(theme::caption()).color(color));
    }

    // -----------------------------------------------------------------------
    // Страницы
    // -----------------------------------------------------------------------

    fn welcome_page(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let left = (width * 0.56).min(520.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left);
                widgets::section_label(ui, tr_s("What happens"));
                ui.add_space(theme::SP_3);
                let items = [
                    (Icon::Search, tr_s("Checks what is already installed. This only reads and starts right away.")),
                    (Icon::List, tr_s("You choose the components; what they depend on is added for you.")),
                    (Icon::Eye, tr_s("You see the exact plan: every command, download, checksum and change.")),
                    (Icon::ArrowDown, tr_s("Only what is missing is installed. You can stop at any time and continue later.")),
                ];
                for (icon, text) in items {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
                        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), icon, theme::TEXT_MUTED);
                        ui.add(egui::Label::new(RichText::new(text).color(theme::TEXT)).wrap());
                    });
                    ui.add_space(theme::SP_2);
                }
                ui.add_space(theme::SP_6);
                if let Some(existing) = self.existing.clone() {
                    widgets::section_label(ui, tr_s("Existing installation"));
                    ui.add_space(theme::SP_2);
                    let comps = existing.components.iter().map(|c| component_name(c)).collect::<Vec<_>>().join(", ");
                    ui.label(RichText::new(trf!("Found: {components}", components = if comps.is_empty() { "-".to_string() } else { comps })).font(theme::caption()).color(theme::TEXT_MUTED));
                    ui.add_space(theme::SP_3);
                    for (mode, name, title, hint) in [
                        (Mode::Modify, "mode-modify", tr_s("Modify"), tr_s("Add components to the installation.")),
                        (Mode::Repair, "mode-repair", tr_s("Repair"), tr_s("Run the steps that failed or whose result is missing again.")),
                        (Mode::Uninstall, "mode-uninstall", tr_s("Uninstall"), tr_s("Remove what this installer added (listed before anything is removed).")),
                    ] {
                        if self.choice(ui, name, self.mode == mode, title, true) {
                            self.push(Action::SetMode(mode));
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(theme::SP_12);
                            ui.label(RichText::new(hint).font(theme::caption()).color(theme::TEXT_MUTED));
                        });
                        ui.add_space(theme::SP_2);
                    }
                    ui.add_space(theme::SP_4);
                }
                widgets::section_label(ui, tr_s("Licence of the engine"));
                ui.add_space(theme::SP_2);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(trf!("{engine} is free software: {licence}.", engine = brand::ENGINE_NAME, licence = env!("CARGO_PKG_LICENSE"))).color(theme::TEXT));
                });
                ui.horizontal(|ui| {
                    if self.push_button(ui, "licence-mit", Some(Icon::External), "MIT", false, true, false) {
                        self.push(Action::OpenUrl("https://opensource.org/license/mit".into()));
                    }
                    if self.push_button(ui, "licence-apache", Some(Icon::External), "Apache-2.0", false, true, false) {
                        self.push(Action::OpenUrl("https://www.apache.org/licenses/LICENSE-2.0".into()));
                    }
                });
            });
            ui.add_space(theme::SP_12);
            let right = ui.available_width();
            ui.allocate_ui_with_layout(vec2(right, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                // Колонка не шире остатка страницы: длинные значения переносятся, а не уходят за край.
                ui.set_width(right);
                ui.set_max_width(right);
                self.this_computer(ui);
            });
        });
    }

    /// Правая колонка Welcome: что найдено на этой машине.
    fn this_computer(&mut self, ui: &mut Ui) {
        widgets::section_label(ui, tr_s("This computer"));
        ui.add_space(theme::SP_3);
        if let Some(err) = &self.backend_error {
            widgets::note(ui, Icon::Error, Tone::Error, &trf!("The installer scripts could not be prepared: {error}", error = err));
            return;
        }
        let hello = self.detect.as_ref().and_then(|d| d.hello.clone());
        let os = if cfg!(windows) {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else {
            "Linux"
        };
        let rows: Vec<(String, String)> = vec![
            (tr_s("System").into(), os.to_string()),
            (tr_s("Script shell").into(), hello.as_ref().map(|h| h.shell.clone()).unwrap_or_else(|| "-".into())),
            (
                tr_s("Free space").into(),
                self.free_bytes().map(human_bytes).unwrap_or_else(|| "-".into()),
            ),
            (
                tr_s("Administrator").into(),
                match hello.as_ref() {
                    Some(h) if h.is_admin => tr_s("yes").into(),
                    Some(_) => tr_s("no").into(),
                    None => "-".into(),
                },
            ),
        ];
        let width = ui.available_width();
        egui::Grid::new("installer-computer").num_columns(2).spacing(vec2(14.0, 6.0)).max_col_width((width - 130.0).max(80.0)).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(RichText::new(k).color(theme::TEXT_MUTED));
                ui.add(egui::Label::new(RichText::new(v).color(theme::TEXT)).truncate());
                ui.end_row();
            }
        });
        if hello.as_ref().is_some_and(|h| !h.is_admin) {
            caption(ui, tr_s("Administrator rights are asked only for the steps that need them."));
        }
        ui.add_space(theme::SP_6);
        match &self.detect {
            Some(d) if d.running() => {
                kit::busy_indicator(ui, tr_s("Checking what is already installed…"), None);
            }
            Some(d) if d.error.is_some() => {
                widgets::note(ui, Icon::Warn, Tone::Warn, &trf!("The check did not finish: {error}", error = d.error.clone().unwrap_or_default()));
            }
            Some(d) => {
                let done = d.steps.iter().filter(|s| s.skip).count();
                let text = trf!("{done} of {total} steps are already done on this computer.", done = done, total = d.steps.len());
                widgets::note(ui, Icon::Check, Tone::Ok, &text);
                ui.add_space(theme::SP_4);
                for c in self.packages.components.clone() {
                    let (text, color) = presence_text(&self.presence(&c.id));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(component_name(&c.id)).color(theme::TEXT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(text).font(theme::caption()).color(color));
                        });
                    });
                }
            }
            None => {}
        }
    }

    fn components_page(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (preset, name, label) in [
                (Preset::Minimal, "preset-minimal", tr_s("Minimal")),
                (Preset::Typical, "preset-typical", tr_s("Typical")),
                (Preset::Everything, "preset-everything", tr_s("Everything")),
            ] {
                if self.push_button(ui, name, None, label, self.preset == preset, true, false) {
                    self.push(Action::Preset(preset));
                }
            }
            if self.preset == Preset::Custom {
                ui.add_space(theme::SP_3);
                ui.label(RichText::new(tr_s("Custom selection")).font(theme::caption()).color(theme::TEXT_MUTED));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(trf!("Download about {download} · on disk about {disk}", download = human_bytes(self.download_bytes()), disk = human_bytes(self.required_bytes())))
                        .font(theme::caption())
                        .color(theme::TEXT_MUTED),
                );
            });
        });
        ui.add_space(theme::SP_4);
        let width = ui.available_width();
        let (w_check, w_name, w_size, w_status) = (30.0, 150.0, 74.0, 170.0);
        let w_purpose = (width - w_check - w_name - 2.0 * w_size - w_status).max(140.0);
        let columns = [w_check, w_name, w_purpose, w_size, w_size, w_status];
        table_header(ui, &columns, &["", tr_s("Component"), tr_s("What it is for"), tr_s("Download"), tr_s("On disk"), tr_s("On this computer")], &[false, false, false, true, true, false]);
        let resolved = self.resolved();
        for c in self.packages.components.clone() {
            let (rect, _) = ui.allocate_exact_size(vec2(width, ROW + 4.0), Sense::hover());
            let row = ui.interact(rect, Id::new(("installer-comp-row", &c.id)), Sense::click());
            let on = resolved.contains(&c.id);
            let auto = self.auto_selected(&c.id);
            let locked = !auto.is_empty();
            if row.hovered() && !locked {
                ui.painter().rect_filled(rect, theme::R_S, theme::hover_fill());
            }
            let mut x = rect.left();
            let y = rect.center().y;
            let box_rect = Rect::from_center_size(pos2(x + 12.0, y), vec2(theme::ICON, theme::ICON));
            paint_checkbox(ui.painter(), box_rect, on, !locked, row.hovered() && !locked);
            x += columns[0];
            let painter = ui.painter();
            painter.text(pos2(x, y), Align2::LEFT_CENTER, component_name(&c.id), theme::body(), theme::TEXT);
            x += columns[1];
            let purpose = if locked {
                trf!("Needed by: {list}", list = auto.iter().map(|a| component_name(a)).collect::<Vec<_>>().join(", "))
            } else {
                component_purpose(&c.id).to_string()
            };
            widgets::paint_elided(painter, pos2(x, y), &purpose, theme::body(), if locked { theme::INFO } else { theme::TEXT_MUTED }, columns[2] - 12.0, Align2::LEFT_CENTER);
            x += columns[2];
            painter.text(pos2(x + columns[3] - 12.0, y), Align2::RIGHT_CENTER, human_bytes(c.download), theme::mono(), theme::TEXT_MUTED);
            x += columns[3];
            painter.text(pos2(x + columns[4] - 12.0, y), Align2::RIGHT_CENTER, human_bytes(c.disk), theme::mono(), theme::TEXT_MUTED);
            x += columns[4];
            let presence = self.presence(&c.id);
            let (text, color) = presence_text(&presence);
            painter.circle_filled(pos2(x + 5.0, y), 3.5, color);
            widgets::paint_elided(painter, pos2(x + 14.0, y), &text, theme::body(), if color == theme::TEXT_MUTED { theme::TEXT_MUTED } else { theme::TEXT }, columns[5] - 14.0, Align2::LEFT_CENTER);
            painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, theme::LINE));
            self.hits.insert(format!("comp-{}", c.id), rect);
            if row.clicked() && !locked {
                self.push(Action::Toggle(c.id.clone()));
            }
            if locked {
                row.on_hover_text(tr_s("Selected automatically: a selected component needs it."));
            } else {
                row.on_hover_text(component_purpose(&c.id));
            }
        }
        ui.add_space(theme::SP_6);
        // Пояснения о зависимостях.
        let auto: Vec<(String, Vec<String>)> = self.packages.components.iter().map(|c| (c.id.clone(), self.auto_selected(&c.id))).filter(|(_, by)| !by.is_empty()).collect();
        for (id, by) in auto {
            let text = trf!(
                "{component} is selected automatically: {list} cannot be installed without it.",
                component = component_name(&id),
                list = by.iter().map(|a| component_name(a)).collect::<Vec<_>>().join(", ")
            );
            widgets::note(ui, Icon::Info, Tone::Neutral, &text);
            ui.add_space(theme::SP_3);
        }
        if self.mode == Mode::Modify {
            widgets::note(ui, Icon::Info, Tone::Neutral, tr_s("Modify adds components. To remove what was installed, choose Uninstall on the first page."));
            ui.add_space(theme::SP_3);
        }
        if self.low_disk() {
            let text = trf!(
                "Not enough free space: about {need} is needed, {free} is free. Free some space or choose another install folder on the next page.",
                need = human_bytes(self.required_bytes()),
                free = human_bytes(self.free_bytes().unwrap_or(0))
            );
            widgets::note(ui, Icon::Warn, Tone::Warn, &text);
            self.hits.insert("disk-warning".into(), ui.min_rect());
        }
    }

    fn options_page(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("installer-options").auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width();
            let field = (width - LABEL - 110.0).clamp(160.0, 560.0);
            // Каталог установки.
            self.option_row(ui, tr_s("Install folder"), |me, ui| {
                ui.horizontal(|ui| {
                    widgets::text_field(ui, Id::new("installer-install-dir"), &mut me.install_dir, tr_s("Folder"), field);
                    let r = widgets::button_sized(ui, Id::new("installer-browse-install"), tr_s("Browse…"), 92.0);
                    me.record("browse-install", &r);
                    if r.clicked() {
                        me.push(Action::Browse(BrowseTarget::InstallDir));
                    }
                });
                caption(ui, tr_s("Tools without their own installer (zig, Android command-line tools), the download cache and the engine go here."));
            });
            // Исходники.
            let engine = self.resolved().iter().any(|c| c == "engine");
            self.option_row(ui, tr_s("Engine sources"), |me, ui| {
                if !engine {
                    caption(ui, tr_s("Used only by the Engine and tools component, which is not selected."));
                }
                if me.choice(ui, "source-existing", me.source == Source::Existing, tr_s("Use an existing folder"), engine) {
                    me.push(Action::SetSource(Source::Existing));
                }
                ui.horizontal(|ui| {
                    ui.add_space(theme::SP_12);
                    ui.add_enabled_ui(engine && me.source == Source::Existing, |ui| {
                        widgets::text_field(ui, Id::new("installer-source-dir"), &mut me.source_dir, tr_s("Engine folder"), field - 22.0);
                        let r = widgets::button_sized(ui, Id::new("installer-browse-source"), tr_s("Browse…"), 92.0);
                        me.record("browse-source", &r);
                        if r.clicked() {
                            me.push(Action::Browse(BrowseTarget::SourceDir));
                        }
                    });
                });
                if engine && me.source == Source::Existing {
                    ui.horizontal(|ui| {
                        ui.add_space(theme::SP_12);
                        let here = me.engine_root.as_ref().is_some_and(|r| piska_hub::paths::same(r, std::path::Path::new(me.source_dir.trim())));
                        if !crate::app::is_checkout(std::path::Path::new(me.source_dir.trim())) {
                            ui.label(RichText::new(tr_s("Not an engine checkout: Cargo.toml and tools/piska-export are missing.")).font(theme::caption()).color(theme::WARN));
                        } else if here {
                            caption(ui, tr_s("The copy of the engine this installer was started from."));
                        } else {
                            caption(ui, tr_s("An engine checkout: it is built where it is."));
                        }
                    });
                }
                ui.add_space(theme::SP_2);
                if me.choice(ui, "source-download", me.source == Source::Download, tr_s("Download from the repository"), engine) {
                    me.push(Action::SetSource(Source::Download));
                }
                ui.horizontal(|ui| {
                    ui.add_space(theme::SP_12);
                    ui.label(RichText::new(me.packages.repo.clone()).font(theme::mono()).color(if engine { theme::TEXT_MUTED } else { theme::TEXT_DISABLED }));
                });
                if engine && me.source == Source::Download {
                    ui.add_space(theme::SP_2);
                    ui.horizontal(|ui| {
                        ui.add_space(theme::SP_12);
                        let w = field + 92.0 - 16.0;
                        widgets::note_width(
                            ui,
                            Icon::Lock,
                            Tone::Warn,
                            tr_s("The repository is private right now: downloading needs access to it. Sign in to GitHub first (Git Credential Manager, or gh auth login), or use an existing folder."),
                            w,
                        );
                    });
                    me.hits.insert("private-repo-note".into(), ui.min_rect());
                }
            });
            // Ярлыки.
            self.option_row(ui, tr_ctx_s("installer", "Shortcuts"), |me, ui| {
                let menu = if cfg!(windows) { tr_s("Start menu") } else { tr_s("Application menu") };
                if me.check(ui, "start-menu", me.start_menu, menu, engine, false) {
                    me.start_menu = !me.start_menu;
                }
                if me.check(ui, "desktop", me.desktop, tr_s("Desktop"), engine, false) {
                    me.desktop = !me.desktop;
                }
            });
            // Ассоциации.
            self.option_row(ui, tr_s("File associations"), |me, ui| {
                let text = trf!(
                    "Open .pmodel, .scene and .scenario files with the engine, and .{ext} projects with the hub",
                    ext = brand::PROJECT_EXTENSION
                );
                if me.check(ui, "file-assoc", me.file_associations, &text, engine, false) {
                    me.file_associations = !me.file_associations;
                }
                caption(ui, &trf!("A .{ext} file opens the Import page of {hub}. Only for your user account.", ext = brand::PROJECT_EXTENSION, hub = brand::HUB_NAME));
            });
            // Настройки редактора.
            self.option_row(ui, tr_s("Editor preferences"), |me, ui| {
                if me.check(ui, "write-prefs", me.write_prefs, tr_s("Write the detected tool paths into the editor preferences"), true, false) {
                    me.write_prefs = !me.write_prefs;
                }
                caption(ui, tr_s("Existing values are kept; a backup copy is made first."));
            });
            // Переменные окружения.
            let vars = self.environment_lines();
            self.option_row(ui, tr_s("Environment variables"), |_, ui| {
                if vars.is_empty() {
                    caption(ui, tr_s("None for the selected components."));
                }
                for v in &vars {
                    ui.label(RichText::new(v).font(theme::mono()).color(theme::TEXT));
                }
            });
            ui.add_space(theme::SP_3);
            // Дополнительно.
            let id = Id::new("installer-advanced");
            let open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            let header = ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_MUTED);
                ui.label(RichText::new(tr_s("Advanced")).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
            });
            let toggle = ui.interact(header.response.rect, Id::new("installer-advanced-toggle"), Sense::click());
            self.record("advanced", &toggle);
            if toggle.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(id, !open));
            }
            if open {
                ui.add_space(theme::SP_3);
                self.option_row(ui, tr_s("Dry run"), |me, ui| {
                    if me.check(ui, "dry-run", me.dry_run, tr_s("Only show and log the plan; change nothing"), !me.forced_dry_run, me.forced_dry_run) {
                        me.dry_run = !me.dry_run;
                    }
                });
                self.option_row(ui, tr_s("Download cache"), |me, ui| {
                    ui.horizontal(|ui| {
                        widgets::text_field(ui, Id::new("installer-cache"), &mut me.cache, tr_s("<install folder>/cache"), field);
                        let r = widgets::button_sized(ui, Id::new("installer-browse-cache"), tr_s("Browse…"), 92.0);
                        if r.clicked() {
                            me.push(Action::Browse(BrowseTarget::Cache));
                        }
                    });
                    caption(ui, tr_s("Files already there that pass their checksum are used without downloading (offline installs)."));
                });
                self.option_row(ui, tr_s("Proxy"), |me, ui| {
                    widgets::text_field(ui, Id::new("installer-proxy"), &mut me.proxy, "http://proxy.example:3128", field);
                    caption(ui, tr_s("Passed to the scripts as HTTPS_PROXY; empty — the system settings."));
                });
            }
        });
    }

    /// Строка страницы Options: подпись слева, содержимое справа.
    fn option_row(&mut self, ui: &mut Ui, label: &str, add: impl FnOnce(&mut Self, &mut Ui)) {
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(LABEL, 30.0), Sense::hover());
            ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::body(), theme::TEXT_MUTED);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                add(self, ui);
            });
        });
        ui.add_space(theme::SP_4);
    }

    /// Переменные окружения, которые поставят выбранные компоненты (из проверки).
    fn environment_lines(&self) -> Vec<String> {
        let resolved = self.resolved();
        let Some(detect) = &self.detect else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for step in detect.steps.iter().filter(|s| resolved.contains(&s.component)) {
            for line in step.lines.iter().filter(|l| l.kind == "env") {
                let text = line.text.trim_start_matches("export ").to_string();
                if !out.contains(&text) {
                    out.push(text);
                }
            }
        }
        out
    }

    fn licences_page(&mut self, ui: &mut Ui) {
        if self.detect.as_ref().is_some_and(|d| d.running()) {
            kit::busy_indicator(ui, tr_s("Checking which licences apply…"), None);
            ui.add_space(theme::SP_4);
        }
        let needed = self.licences_needed();
        if needed.is_empty() && !self.detect.as_ref().is_some_and(|d| d.running()) {
            theme::empty_state(ui, Icon::Check, tr_s("No third-party licence needs your acceptance"), tr_s("The selected components use only open-source tools, or the tools are already installed."));
            return;
        }
        widgets::note(ui, Icon::Info, Tone::Neutral, tr_s("Each licence needs its own tick. The scripts accept a licence only when you tick it here."));
        ui.add_space(theme::SP_6);
        let steps: Vec<PlanStep> = self.detect.as_ref().map(|d| d.steps.clone()).unwrap_or_default();
        for licence in needed {
            let key = Installer::licence_key(&licence);
            let on = self.accepted.contains(&key);
            let name = licence_name(&licence);
            if self.check(ui, &format!("licence-{key}"), on, &trf!("I accept the licence terms of {name}", name = name), true, false) {
                self.push(Action::AcceptLicence(key.clone(), !on));
            }
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_12);
                let by: Vec<String> = steps.iter().filter(|s| s.licence.as_ref().is_some_and(|l| l.id == licence.id)).map(|s| component_name(&s.component).to_string()).collect();
                ui.label(RichText::new(trf!("Needed by: {list}", list = by.join(", "))).font(theme::caption()).color(theme::TEXT_MUTED));
                ui.add_space(theme::SP_4);
                if self.push_button(ui, &format!("licence-link-{key}"), Some(Icon::External), tr_s("Read the licence"), false, true, false) {
                    self.push(Action::OpenUrl(licence.url.clone()));
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_12);
                ui.label(RichText::new(licence.url.clone()).font(theme::mono()).color(theme::TEXT_DISABLED));
            });
            ui.add_space(theme::SP_6);
        }
    }

    fn review_page(&mut self, ui: &mut Ui) {
        let Some((running, error, steps)) = self.plan.as_ref().map(|p| (p.running(), p.error.clone(), p.steps.clone())) else {
            return;
        };
        if running {
            kit::busy_indicator(ui, tr_s("Preparing the exact plan (a dry run of the installer script)…"), None);
            ui.add_space(theme::SP_4);
        }
        if let Some(err) = error {
            widgets::note(ui, Icon::Error, Tone::Error, &trf!("The plan could not be prepared: {error}", error = err));
            ui.add_space(theme::SP_3);
            if self.push_button(ui, "refresh-plan", Some(Icon::Refresh), tr_s("Try Again"), false, true, false) {
                self.push(Action::RefreshPlan);
            }
            return;
        }
        let total = steps.len();
        let todo = steps.iter().filter(|s| !s.skip).count();
        let download: u64 = steps.iter().filter(|s| !s.skip).map(|s| s.download_mb).sum();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(trf!("{total} steps: {todo} to run, {done} already done", total = total, todo = todo, done = total - todo))
                    .font(theme::strong(theme::FONT_TITLE))
                    .color(theme::TEXT),
            );
            if download > 0 {
                ui.label(RichText::new(trf!("· downloads with pinned sizes: {size}", size = human_bytes(download * 1024 * 1024))).font(theme::caption()).color(theme::TEXT_MUTED));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let ready = !steps.is_empty();
                if self.push_button(ui, "save-plan", Some(Icon::Save), tr_s("Save Plan…"), false, ready, false) {
                    self.push(Action::Browse(BrowseTarget::PlanFolder));
                }
                if self.push_button(ui, "copy-plan", Some(Icon::Copy), tr_s("Copy Plan"), false, ready, false) {
                    self.push(Action::CopyPlan);
                }
            });
        });
        ui.add_space(theme::SP_4);
        if !self.licences_accepted() {
            widgets::note(ui, Icon::Warn, Tone::Warn, tr_s("Some licences are not accepted yet: go back to the Licences page."));
            ui.add_space(theme::SP_3);
        }
        if self.low_disk() {
            widgets::note(ui, Icon::Warn, Tone::Warn, &trf!("Not enough free space: about {need} is needed.", need = human_bytes(self.required_bytes())));
            ui.add_space(theme::SP_3);
        }
        let elevated = steps.iter().filter(|s| !s.skip && s.elevation.is_some()).count();
        if elevated > 0 {
            let text = trn!(
                "{n} step asks for administrator rights; the reason is shown at the step. Your password never passes through this installer.",
                "{n} steps ask for administrator rights; the reason is shown at each step. Your password never passes through this installer.",
                elevated as u64
            );
            widgets::note(ui, Icon::Lock, Tone::Neutral, &text);
            ui.add_space(theme::SP_3);
        }
        egui::ScrollArea::vertical().id_salt("installer-plan").auto_shrink([false, false]).show(ui, |ui| {
            for step in &steps {
                self.plan_step(ui, step);
            }
        });
    }

    fn plan_step(&mut self, ui: &mut Ui, step: &PlanStep) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
        let painter = ui.painter();
        let y = rect.center().y;
        let (icon, color) = if step.skip { (Icon::Check, theme::OK) } else { (Icon::ArrowDown, theme::accent()) };
        icons::paint(painter, Rect::from_center_size(pos2(rect.left() + 8.0, y), vec2(13.0, 13.0)), icon, color);
        painter.text(pos2(rect.left() + 22.0, y), Align2::LEFT_CENTER, format!("{}/{}", step.n, step.total), theme::mono(), theme::TEXT_DISABLED);
        let title = step_title(&step.id, &step.title);
        let title_rect = painter.text(pos2(rect.left() + 66.0, y), Align2::LEFT_CENTER, &title, theme::strong(theme::FONT_TITLE), if step.skip { theme::TEXT_MUTED } else { theme::TEXT });
        let mut left = title_rect.right() + 10.0;
        left = widgets::paint_badge(painter, left, y, component_name(&step.component), Tone::Neutral).right() + 6.0;
        if !step.skip {
            if step.elevation.is_some() {
                left = widgets::paint_badge(painter, left, y, tr_s("Administrator"), Tone::Warn).right() + 6.0;
            }
            if step.licence.is_some() {
                left = widgets::paint_badge(painter, left, y, tr_s("Licence"), Tone::Accent).right() + 6.0;
            }
        }
        let _ = left;
        if step.skip {
            let why = match (step.reason.as_deref(), &step.found) {
                (Some("requested"), _) => tr_s("skipped on request").to_string(),
                (_, Some(found)) => trf!("already there: {found}", found = found),
                _ => tr_s("done earlier").to_string(),
            };
            widgets::paint_elided(painter, pos2(rect.right(), y), &why, theme::mono(), theme::TEXT_MUTED, (rect.right() - left - 12.0).max(80.0), Align2::RIGHT_CENTER);
        } else if step.download_mb > 0 {
            painter.text(pos2(rect.right(), y), Align2::RIGHT_CENTER, human_bytes(step.download_mb * 1024 * 1024), theme::mono(), theme::TEXT_MUTED);
        }
        if !step.skip {
            if let Some(reason) = &step.elevation {
                detail_row(ui, Icon::Lock, theme::WARN, tr_s("Rights").to_string(), &elevation_text(&step.id, reason), false);
            }
            if let Some(l) = &step.licence {
                detail_row(ui, Icon::File, theme::INFO, tr_s("Licence").to_string(), &format!("{} — {}", licence_name(l), l.url), false);
            }
            for line in &step.lines {
                let mut text = line.text.clone();
                let mut chip: Option<(String, Tone)> = None;
                if line.kind == "download" {
                    if let Some(d) = step.downloads.iter().find(|d| line.text.starts_with(&d.url)) {
                        chip = Some(if d.pinned { (tr_s("sha256 pinned").to_string(), Tone::Ok) } else { (tr_s("sha256 checked against the publisher at install time").to_string(), Tone::Accent) });
                        text = d.url.clone();
                        if let Some(mb) = d.size_mb {
                            text = format!("{text}  ({})", human_bytes(mb * 1024 * 1024));
                        }
                    }
                }
                let row = detail_row(ui, line_icon(&line.kind), theme::TEXT_MUTED, line_kind(&line.kind), &text, true);
                if let Some((chip, tone)) = chip {
                    let painter = ui.painter();
                    let w = widgets::badge_width(painter, &chip);
                    widgets::paint_badge(painter, row.right() - w, row.center().y, &chip, tone);
                }
            }
        }
        ui.painter().hline(egui::Rangef::new(rect.left(), rect.left() + width), ui.min_rect().bottom() + 3.0, Stroke::new(1.0_f32, theme::LINE));
        ui.add_space(theme::SP_4);
    }

    fn installing_page(&mut self, ui: &mut Ui) {
        let Some(run) = &self.run else {
            return;
        };
        let total = run.steps.len().max(1);
        let finished = run.steps.iter().filter(|s| !matches!(s.state, StepState::Pending | StepState::Running)).count();
        let running = run.steps.iter().position(|s| s.state == StepState::Running);
        let outcome = run.outcome.clone();
        let header = match (&outcome, running) {
            (_, Some(i)) => trf!("Step {n} of {total} · {title}", n = i + 1, total = total, title = step_title(&run.steps[i].id, &run.steps[i].title)),
            (Outcome::Running, None) if run.steps.is_empty() => tr_s("Starting the installer script…").to_string(),
            _ => trf!("{done} of {total} steps", done = finished, total = total),
        };
        ui.label(RichText::new(header).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
        ui.add_space(theme::SP_2);
        let fraction = if run.purpose == Purpose::Uninstall && run.steps.is_empty() { None } else { Some(run.fraction()) };
        widgets::progress_bar(ui, if matches!(outcome, Outcome::Running) { fraction } else { Some(run.fraction()) }, matches!(outcome, Outcome::Failed(_)));
        ui.add_space(theme::SP_2);
        if let Some(current) = run.current.clone().filter(|_| run.is_running()) {
            let w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
            widgets::paint_elided(ui.painter(), rect.left_center(), &current, theme::mono(), theme::TEXT_MUTED, w, Align2::LEFT_CENTER);
        }
        // Скачивание.
        if let Some(p) = run.progress.clone().filter(|_| run.is_running()) {
            ui.add_space(theme::SP_3);
            let fraction = p.total.filter(|t| *t > 0).map(|t| (p.bytes as f32 / t as f32).min(1.0));
            let mut text = match p.total {
                Some(t) if p.estimated => trf!("{done} of about {total}", done = human_bytes(p.bytes), total = human_bytes(t)),
                Some(t) => trf!("{done} of {total}", done = human_bytes(p.bytes), total = human_bytes(t)),
                None => trf!("{done} downloaded", done = human_bytes(p.bytes)),
            };
            if let Some(speed) = p.speed.filter(|s| *s > 1.0) {
                text.push_str(&format!(" · {}/s", human_bytes(speed as u64)));
                if let Some(t) = p.total.filter(|t| *t > p.bytes) {
                    let left = Duration::from_secs_f64((t - p.bytes) as f64 / speed);
                    text.push_str(&format!(" · {}", trf!("about {time} left", time = duration_text(left))));
                }
            }
            ui.horizontal(|ui| {
                icons::inline_small(ui, Icon::ArrowDown, theme::TEXT_MUTED);
                ui.label(RichText::new(text).font(theme::caption()).color(theme::TEXT));
            });
            kit::paint_progress(ui.painter(), Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 3.0)), fraction, ui.input(|i| i.time), 1.0);
            ui.add_space(theme::SP_4);
            ui.label(RichText::new(p.url.clone()).font(theme::mono()).color(theme::TEXT_DISABLED));
        }
        if let Some(reason) = run.elevation.clone().filter(|_| run.is_running()) {
            ui.add_space(theme::SP_3);
            let current_id = running.map(|i| run.steps[i].id.clone()).unwrap_or_default();
            widgets::note(ui, Icon::Lock, Tone::Warn, &trf!("Administrator rights are requested now: {reason}", reason = elevation_text(&current_id, &reason)));
        }
        if let Some(quiet) = run.quiet().filter(|q| *q >= self.still_working()) {
            ui.add_space(theme::SP_3);
            widgets::note(
                ui,
                Icon::Clock,
                Tone::Neutral,
                &trf!(
                    "Still working… no output for {time}. Big installers are quiet for a long time; nothing is stopped unless you press Cancel.",
                    time = duration_text(quiet)
                ),
            );
            self.hits.insert("still-working".into(), ui.min_rect());
        }
        ui.add_space(theme::SP_4);
        match outcome {
            Outcome::Failed(f) => self.failure_box(ui, &f),
            Outcome::Cancelled => {
                widgets::note(ui, Icon::Info, Tone::Neutral, tr_s("Stopped. Finished steps are recorded: Resume continues where it stopped."));
                ui.add_space(theme::SP_4);
                if self.push_button(ui, "retry", Some(Icon::Play), tr_s("Resume"), true, true, false) {
                    self.push(Action::Retry);
                }
                ui.add_space(theme::SP_4);
            }
            Outcome::Reboot => {
                widgets::note(ui, Icon::Warn, Tone::Warn, tr_s("Windows needs to restart to finish this step. Restart, then run Setup again and choose Repair."));
                ui.add_space(theme::SP_4);
            }
            Outcome::Declined(why) => {
                let text = if why == "licence" { tr_s("Stopped: a licence was not accepted.") } else { tr_s("Stopped: administrator rights were not confirmed.") };
                widgets::note(ui, Icon::Warn, Tone::Warn, text);
                ui.add_space(theme::SP_4);
                if self.push_button(ui, "retry", Some(Icon::Refresh), tr_s("Try Again"), false, true, false) {
                    self.push(Action::Retry);
                }
                ui.add_space(theme::SP_4);
            }
            _ => {}
        }
        self.step_list_and_log(ui);
    }

    fn failure_box(&mut self, ui: &mut Ui, f: &crate::app::Failure) {
        let title = match &f.step {
            Some(id) => trf!("Step “{title}” failed", title = step_title(id, &f.title)),
            None => match f.exit {
                Some(code) => trf!("The installer script stopped unexpectedly (exit code {code})", code = code),
                None => tr_s("The installer script stopped unexpectedly").to_string(),
            },
        };
        widgets::note(ui, Icon::Error, Tone::Error, &title);
        self.hits.insert("failure".into(), ui.min_rect());
        ui.add_space(theme::SP_3);
        if f.elevation_refused {
            widgets::note(ui, Icon::Lock, Tone::Warn, tr_s("Administrator rights were refused. Retry and answer Yes in the Windows prompt, or install this tool by hand."));
            ui.add_space(theme::SP_3);
        }
        if let Some(command) = &f.manual_command {
            widgets::note(ui, Icon::Code, Tone::Warn, tr_s("This step needs administrator rights and pkexec is not available. Run this command in a terminal, then press Retry:"));
            ui.add_space(theme::SP_2);
            code_box(ui, std::slice::from_ref(command), 1);
            ui.add_space(theme::SP_2);
            if self.push_button(ui, "copy-command", Some(Icon::Copy), tr_s("Copy Command"), false, true, false) {
                self.push(Action::CopyCommand);
            }
            ui.add_space(theme::SP_3);
        }
        let mut lines: Vec<String> = Vec::new();
        if !f.error.is_empty() {
            lines.push(f.error.clone());
        }
        if let Some(run) = &self.run {
            for e in run.tail(12) {
                if !lines.contains(&e.text) {
                    lines.push(e.text.clone());
                }
            }
        }
        code_box(ui, &lines, 10);
        ui.add_space(theme::SP_4);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if self.push_button(ui, "retry", Some(Icon::Refresh), tr_s("Retry Step"), true, true, false) {
                self.push(Action::Retry);
            }
            if f.skippable && self.push_button(ui, "skip", Some(Icon::ChevronRight), tr_s("Skip Step"), false, true, false) {
                self.push(Action::SkipStep);
            }
            let has_log = self.log_path().is_some();
            if self.push_button(ui, "open-log", Some(Icon::File), tr_s("Open Log"), false, has_log, false) {
                self.push(Action::OpenLog);
            }
            if self.push_button(ui, "copy-error", Some(Icon::Copy), tr_s("Copy Error"), false, true, false) {
                self.push(Action::CopyError);
            }
        });
        ui.add_space(theme::SP_4);
    }

    fn step_list_and_log(&mut self, ui: &mut Ui) {
        let Some(run) = &self.run else {
            return;
        };
        let steps = run.steps.clone();
        let log: Vec<crate::app::LogEntry> = run.log.clone();
        let malformed = run.malformed;
        let id = Id::new("installer-log-open");
        let open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let list_height = if open { (ui.available_height() * 0.45).max(120.0) } else { (ui.available_height() - 34.0).max(80.0) };
        egui::ScrollArea::vertical().id_salt("installer-steps").max_height(list_height).auto_shrink([false, true]).show(ui, |ui| {
            for s in &steps {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                let y = rect.center().y;
                let painter = ui.painter();
                let (icon, color, word) = match s.state {
                    StepState::Pending => (Icon::More, theme::TEXT_DISABLED, tr_s("waiting")),
                    StepState::Running => (Icon::Play, theme::accent(), tr_s("running")),
                    StepState::Done => (Icon::Check, theme::OK, tr_s("done")),
                    StepState::Skipped => (Icon::ChevronRight, theme::TEXT_MUTED, tr_s("already there")),
                    StepState::Failed => (Icon::Error, theme::ERROR, tr_s("failed")),
                    StepState::Declined => (Icon::Warn, theme::WARN, tr_s("declined")),
                    StepState::Planned => (Icon::ArrowDown, theme::INFO, tr_s("would run")),
                    StepState::WouldSkip => (Icon::Check, theme::TEXT_MUTED, tr_s("would skip")),
                };
                if s.state == StepState::Running {
                    painter.rect_filled(rect, theme::R_S, theme::mix(theme::BG_PANEL, theme::accent(), 0.08));
                }
                icons::paint(painter, Rect::from_center_size(pos2(rect.left() + 10.0, y), vec2(12.0, 12.0)), icon, color);
                painter.text(pos2(rect.left() + 26.0, y), Align2::LEFT_CENTER, step_title(&s.id, &s.title), theme::body(), if s.state == StepState::Pending { theme::TEXT_MUTED } else { theme::TEXT });
                painter.text(pos2(rect.right() - 4.0, y), Align2::RIGHT_CENTER, word, theme::caption(), color);
            }
        });
        ui.add_space(theme::SP_3);
        let header = ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
            icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_MUTED);
            ui.label(RichText::new(trn!("Log ({n} line)", "Log ({n} lines)", log.len() as u64)).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
            if malformed > 0 {
                ui.label(RichText::new(trn!("{n} line was not an event and is shown as is", "{n} lines were not events and are shown as is", malformed as u64)).font(theme::caption()).color(theme::TEXT_MUTED));
            }
        });
        let toggle = ui.interact(header.response.rect, Id::new("installer-log-toggle"), Sense::click());
        self.record("log-toggle", &toggle);
        if toggle.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !open));
        }
        if open {
            egui::Frame::none().fill(theme::BG_INSET).stroke(Stroke::new(1.0_f32, theme::LINE_STRONG)).rounding(theme::R_S).inner_margin(Margin::same(6.0)).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("installer-log").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for e in log.iter().rev().take(600).rev() {
                        let color = match e.level {
                            LogLevel::Warn => theme::WARN,
                            LogLevel::Error => theme::ERROR,
                            LogLevel::Output => theme::TEXT_MUTED,
                            LogLevel::Info => theme::TEXT,
                        };
                        ui.label(RichText::new(&e.text).font(theme::mono()).color(color));
                    }
                });
            });
        }
    }

    fn finish_title(&self) -> String {
        let Some(run) = &self.run else {
            return tr_s("Finished").into();
        };
        match (run.purpose, run.options.dry_run) {
            (_, true) => tr_s("Dry run finished: nothing was changed").into(),
            (Purpose::Uninstall, _) => tr_s("Uninstall finished").into(),
            _ => trf!("{engine} is installed", engine = brand::ENGINE_NAME),
        }
    }

    fn finish_subtitle(&self) -> String {
        match &self.run {
            Some(run) if run.options.dry_run => tr_s("The plan above is what a real installation would do. Run Setup again without Dry run to install.").into(),
            Some(run) if run.purpose == Purpose::Uninstall => tr_s("What this installer added was removed.").into(),
            _ => tr_s("Open a new terminal so that PATH and the new environment variables take effect.").into(),
        }
    }

    fn finish_page(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("installer-finish").auto_shrink([false, false]).show(ui, |ui| {
            let (counts, components, log, repo, install_dir) = match &self.run {
                Some(run) => (
                    run.summary.as_ref().map(|s| s.steps.clone()).unwrap_or_default(),
                    run.options.components.iter().map(|c| component_name(c).to_string()).collect::<Vec<_>>().join(", "),
                    self.log_path(),
                    self.repo_dir(),
                    run.options.install_dir.clone().unwrap_or_default(),
                ),
                None => (Default::default(), String::new(), None, None, String::new()),
            };
            egui::Grid::new("installer-finish-summary").num_columns(2).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                let mut row = |k: &str, v: String, mono: bool| {
                    ui.label(RichText::new(k).color(theme::TEXT_MUTED));
                    let text = RichText::new(v).color(theme::TEXT);
                    ui.label(if mono { text.font(theme::mono()) } else { text });
                    ui.end_row();
                };
                if !components.is_empty() {
                    row(tr_s("Components"), components, false);
                }
                row(
                    tr_s("Steps"),
                    trf!("{done} done, {skipped} already there, {failed} failed", done = counts.done, skipped = counts.skipped, failed = counts.failed),
                    false,
                );
                if !install_dir.is_empty() {
                    row(tr_s("Install folder"), install_dir, true);
                }
                if let Some(repo) = &repo {
                    row(tr_s("Engine"), repo.display().to_string(), true);
                }
                row(tr_s("Log"), log.clone().unwrap_or_else(|| tr_s("none (dry run)").into()), log.is_some());
            });
            ui.add_space(theme::SP_6);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let hub = self.hub_exe();
                let label = trf!("Launch {hub}", hub = brand::HUB_NAME);
                let launch = self.push_button(ui, "launch-hub", Some(Icon::Play), &label, true, hub.is_some(), false);
                if hub.is_none() {
                    if let Some(r) = self.hit_rect("launch-hub") {
                        ui.interact(r, Id::new("installer-launch-tip"), Sense::hover()).on_hover_text(tr_s("The hub is not built yet: install the Engine and tools component."));
                    }
                }
                if launch {
                    self.push(Action::LaunchHub);
                }
                if self.push_button(ui, "open-docs", Some(Icon::External), tr_s("Open the Documentation"), false, true, false) {
                    self.push(Action::OpenDocs);
                }
                if self.push_button(ui, "show-log", Some(Icon::File), tr_s("Show the Log"), false, log.is_some(), false) {
                    self.push(Action::ShowLog);
                }
            });
            ui.add_space(theme::SP_8);
            widgets::section_label(ui, tr_s("Check of this computer"));
            ui.add_space(theme::SP_3);
            match self.doctor.clone() {
                None => {
                    kit::busy_indicator(ui, tr_s("Checking the installed tools…"), None);
                }
                Some(Err(err)) => widgets::note(ui, Icon::Warn, Tone::Warn, &err),
                Some(Ok(rows)) => self.doctor_table(ui, &rows),
            }
        });
    }

    fn doctor_table(&mut self, ui: &mut Ui, rows: &[crate::diag::Row]) {
        let width = ui.available_width();
        let (w_comp, w_status, w_version) = (110.0, 92.0, 100.0);
        let w_check = ((width - w_comp - w_status - w_version) * 0.36).max(130.0);
        let w_where = (width - w_comp - w_status - w_version - w_check).max(150.0);
        let columns = [w_comp, w_check, w_status, w_version, w_where];
        table_header(ui, &columns, &[tr_s("Component"), tr_s("Check"), tr_s("Status"), tr_s("Version"), tr_s("Where / what to do")], &[false; 5]);
        for r in rows {
            let failed = matches!(r.status, RowStatus::Old | RowStatus::Missing);
            let height = if failed && r.fix.is_some() { ROW + 16.0 } else { ROW };
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let painter = ui.painter();
            let y = rect.top() + ROW * 0.5;
            let mut x = rect.left();
            painter.text(pos2(x, y), Align2::LEFT_CENTER, component_name(&r.component), theme::body(), theme::TEXT_MUTED);
            x += columns[0];
            widgets::paint_elided(painter, pos2(x, y), &r.label, theme::mono(), theme::TEXT, columns[1] - 10.0, Align2::LEFT_CENTER);
            x += columns[1];
            let (word, color) = match r.status {
                RowStatus::Ok => (tr_s("found"), theme::OK),
                RowStatus::Old => (tr_s("too old"), theme::WARN),
                RowStatus::Missing => (tr_s("missing"), theme::ERROR),
                RowStatus::Info => (tr_s("note"), theme::TEXT_MUTED),
            };
            painter.circle_filled(pos2(x + 4.0, y), 3.5, color);
            painter.text(pos2(x + 12.0, y), Align2::LEFT_CENTER, word, theme::body(), color);
            x += columns[2];
            painter.text(pos2(x, y), Align2::LEFT_CENTER, r.version.clone().unwrap_or_else(|| "-".into()), theme::mono(), theme::TEXT_MUTED);
            x += columns[3];
            let place = match (&r.path, r.detail.is_empty()) {
                (Some(p), true) => p.clone(),
                (Some(p), false) => format!("{p}  ({})", r.detail),
                (None, _) => r.detail.clone(),
            };
            widgets::paint_elided(painter, pos2(x, y), &place, theme::mono(), theme::TEXT_MUTED, columns[4], Align2::LEFT_CENTER);
            if let (true, Some(fix)) = (failed, &r.fix) {
                widgets::paint_elided(painter, pos2(x, y + 16.0), &["→ ", fix.as_str()].concat(), theme::mono(), theme::WARN, columns[4], Align2::LEFT_CENTER);
            }
            painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, theme::LINE));
        }
    }

    fn uninstall_page(&mut self, ui: &mut Ui) {
        let Some(plan) = &self.uninstall_plan else {
            return;
        };
        if plan.running() {
            kit::busy_indicator(ui, tr_s("Reading the install manifest…"), None);
            return;
        }
        if let Some(err) = plan.error.clone() {
            widgets::note(ui, Icon::Error, Tone::Error, &trf!("The uninstall plan could not be prepared: {error}", error = err));
            return;
        }
        let items = plan.uninstall.clone().unwrap_or_default();
        if items.is_empty() {
            theme::empty_state(ui, Icon::Check, tr_s("Nothing to remove"), tr_s("There is no install manifest: this installer has not added anything here."));
            return;
        }
        if let Some(e) = &self.existing {
            ui.label(RichText::new(trf!("From the install manifest {path}", path = e.manifest.display())).font(theme::caption()).color(theme::TEXT_MUTED));
            ui.add_space(theme::SP_4);
        }
        let width = ui.available_width();
        let columns = [220.0, (width - 220.0 - 120.0).max(160.0), 120.0];
        table_header(ui, &columns, &[tr_s("Action"), tr_s("What"), tr_s("Remove")], &[false; 3]);
        egui::ScrollArea::vertical().id_salt("installer-uninstall").auto_shrink([false, false]).show(ui, |ui| {
            for (i, item) in items.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                let y = rect.center().y;
                let painter = ui.painter();
                painter.text(pos2(rect.left(), y), Align2::LEFT_CENTER, uninstall_action(&item.action), theme::body(), theme::TEXT);
                widgets::paint_elided(painter, pos2(rect.left() + columns[0], y), &item.target, theme::mono(), theme::TEXT_MUTED, columns[1] - 10.0, Align2::LEFT_CENTER);
                let x = rect.left() + columns[0] + columns[1];
                if item.ask {
                    let on = !self.keep.contains(&item.target);
                    let r = ui.interact(Rect::from_min_size(pos2(x, rect.top()), vec2(columns[2], ROW)), Id::new(("installer-keep", i)), Sense::click());
                    paint_checkbox(ui.painter(), Rect::from_center_size(pos2(x + 8.0, y), vec2(theme::ICON, theme::ICON)), on, true, r.hovered());
                    ui.painter().text(pos2(x + 22.0, y), Align2::LEFT_CENTER, if on { tr_s("remove") } else { tr_s("keep") }, theme::caption(), theme::TEXT_MUTED);
                    self.hits.insert(format!("keep-{i}"), r.rect);
                    if r.clicked() {
                        self.push(Action::ToggleKeep(item.target.clone()));
                    }
                } else {
                    ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, tr_s("removed"), theme::caption(), theme::TEXT_DISABLED);
                }
                ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, theme::LINE));
            }
        });
    }

    // -----------------------------------------------------------------------
    // Окна
    // -----------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut actions: Vec<Action> = Vec::new();
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let width = 520.0;
        let title = match &dialog {
            Dialog::Browse(..) => tr_s("Choose a Folder"),
            Dialog::ConfirmElevation(_) => tr_s("Administrator Rights"),
            Dialog::ConfirmCancel => tr_s("Stop the Installation?"),
            Dialog::ConfirmClose => tr_s("Close Setup?"),
            Dialog::Prompt(p) if p.kind == "licence" => tr_s("Licence"),
            Dialog::Prompt(_) => tr_s("Question"),
        };
        let is_close = matches!(dialog, Dialog::ConfirmClose);
        let response = kit::dialog("installer-dialog", title).width(width).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let mut buttons = |ui: &mut Ui, primary: &str, secondary: &str, danger: bool| -> Option<bool> {
                let mut result = None;
                kit::dialog_actions(ui, |ui| {
                    let p = if danger { theme::danger_button(ui, primary) } else { kit::primary_action(ui, primary) };
                    hits.push(("dialog-ok".into(), p.rect));
                    if p.clicked() {
                        result = Some(true);
                    }
                    let s = kit::action_button(ui, secondary);
                    hits.push(("dialog-cancel".into(), s.rect));
                    if s.clicked() {
                        result = Some(false);
                    }
                });
                result
            };
            match &mut dialog {
                Dialog::Browse(target, browser) => {
                    let hint = match target {
                        BrowseTarget::InstallDir => tr_s("Where to install. A new folder is created if needed."),
                        BrowseTarget::SourceDir => tr_s("A folder with the engine sources: Cargo.toml and tools/piska-export inside."),
                        BrowseTarget::Cache => tr_s("Where downloaded files are kept."),
                        BrowseTarget::PlanFolder => tr_s("The plan is saved there as install-plan.txt."),
                    };
                    ui.label(RichText::new(hint).color(theme::TEXT_MUTED));
                    folder_list(ui, browser, width);
                    match buttons(ui, tr_s("Choose"), tr_s("Cancel"), false) {
                        Some(true) => actions.push(Action::BrowseChoose),
                        Some(false) => keep = false,
                        None => {}
                    }
                }
                Dialog::ConfirmElevation(list) => {
                    ui.add(egui::Label::new(RichText::new(tr_s("These steps ask for administrator rights. Windows shows its own prompt for each; this installer never sees your password.")).color(theme::TEXT)).wrap());
                    for (id, title, reason) in list.iter() {
                        ui.horizontal_top(|ui| {
                            icons::inline(ui, Icon::Lock, theme::WARN);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(step_title(id, title)).color(theme::TEXT));
                                ui.add(egui::Label::new(RichText::new(elevation_text(id, reason)).font(theme::caption()).color(theme::TEXT_MUTED)).wrap());
                            });
                        });
                    }
                    match buttons(ui, tr_s("Continue"), tr_s("Cancel"), false) {
                        Some(true) => actions.push(Action::InstallConfirmed),
                        Some(false) => keep = false,
                        None => {}
                    }
                }
                Dialog::ConfirmCancel | Dialog::ConfirmClose => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(tr_s("The current step is stopped. Steps that finished stay installed and are recorded, so Setup can continue later.")).color(theme::TEXT),
                        )
                        .wrap(),
                    );
                    let close = is_close;
                    match buttons(ui, if close { tr_s("Stop and Close") } else { tr_s("Stop") }, tr_s("Keep Installing"), true) {
                        Some(true) => actions.push(if close { Action::CloseConfirmed } else { Action::CancelConfirmed }),
                        Some(false) => keep = false,
                        None => {}
                    }
                }
                Dialog::Prompt(p) => {
                    let text = prompt_text(p);
                    ui.add(egui::Label::new(RichText::new(text).color(theme::TEXT)).wrap());
                    if let Some(url) = p.url.clone() {
                        if ui.add(egui::Label::new(RichText::new(&url).font(theme::mono()).color(theme::accent())).sense(Sense::click())).clicked() {
                            actions.push(Action::OpenUrl(url));
                        }
                    }
                    let (yes, no) = if p.kind == "licence" { (tr_s("Accept"), tr_s("Decline")) } else { (tr_s("Yes"), tr_s("No")) };
                    if let Some(answer) = buttons(ui, yes, no, false) {
                        actions.push(Action::Answer(answer));
                    }
                }
            }
        });
        for (name, rect) in hits {
            self.hits.insert(name, rect);
        }
        if response.cancel {
            match dialog {
                Dialog::Prompt(_) => actions.push(Action::Answer(false)),
                _ => keep = false,
            }
        }
        // Ответ и «Выбрать» берут окно из `self.dialog` (и закрывают его сами).
        let needs_dialog = actions.iter().any(|a| matches!(a, Action::Answer(_) | Action::BrowseChoose));
        let closes = actions.iter().any(|a| matches!(a, Action::InstallConfirmed | Action::CancelConfirmed | Action::CloseConfirmed));
        if needs_dialog || (keep && !closes) {
            self.dialog = Some(dialog);
        }
        for a in actions {
            self.push(a);
        }
    }

    // -----------------------------------------------------------------------
    // Заставка
    // -----------------------------------------------------------------------

    /// Показывать заставку при первых кадрах (оконный запуск).
    pub fn enable_splash(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Id::new("installer-splash"), kit::LoadingScreen::default()));
    }

    fn splash_ui(&mut self, ctx: &egui::Context) {
        let id = Id::new("installer-splash");
        let Some(mut screen) = ctx.data(|d| d.get_temp::<kit::LoadingScreen>(id)) else {
            return;
        };
        if !screen.visible() {
            return;
        }
        // Заставка держится, пока скрипты извлекаются и запускается проверка (первые кадры).
        let step = (ctx.input(|i| i.time) < 0.4).then_some(tr_s("Preparing the installer scripts…"));
        screen.show(ctx, &kit::paint_engine_mark, &trf!("{engine} Setup", engine = brand::ENGINE_NAME), env!("CARGO_PKG_VERSION"), step);
        ctx.data_mut(|d| d.insert_temp(id, screen));
    }
}

/// Текст вопроса скрипта (известные — переводятся).
fn prompt_text(p: &crate::protocol::Prompt) -> String {
    if p.kind == "licence" {
        let name = p.name.clone().unwrap_or_default();
        return trf!("Do you accept the licence terms of {name}?", name = name.trim_start_matches("the "));
    }
    let subject = p.subject.clone().unwrap_or_default();
    match p.key.as_deref() {
        Some("prefs-overwrite") => trf!("The editor preferences already set {key}. Replace the value with the detected one?", key = subject),
        Some("cargo-config") => trf!("Add a [build] target-dir block to {file}? A backup is made first.", file = subject),
        Some("file-associations") => tr_s("Register the file associations for your user account?").into(),
        Some("elevation") => tr_s("The next step needs administrator rights. Continue?").into(),
        Some("uninstall-proceed") => tr_s("Remove what this installer added?").into(),
        Some("uninstall-item") => trf!("Remove {target}?", target = subject),
        _ => p.question.clone().unwrap_or_default(),
    }
}

fn licence_name(l: &crate::protocol::LicenceRef) -> String {
    match l.id.as_deref() {
        Some("vs-buildtools") => "Visual Studio Build Tools".into(),
        Some("android-sdk") => "Android SDK".into(),
        _ => l.name.trim_start_matches("the ").to_string(),
    }
}

fn line_icon(kind: &str) -> Icon {
    match kind {
        "run" => Icon::Code,
        "download" => Icon::ArrowDown,
        "verify" => Icon::Check,
        "cache" => Icon::Save,
        "extract" => Icon::FolderOpen,
        "env" => Icon::Environment,
        "file" | "copy" | "block" => Icon::File,
        "prefs" => Icon::Gear,
        "registry" => Icon::Gear,
        "shortcut" => Icon::External,
        _ => Icon::Info,
    }
}

/// Строка подробностей шага плана: иконка, подпись вида, текст. Возвращает её прямоугольник.
fn detail_row(ui: &mut Ui, icon: Icon, color: Color32, kind: String, text: &str, mono: bool) -> Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    icons::paint(painter, Rect::from_center_size(pos2(rect.left() + 74.0, y), vec2(11.0, 11.0)), icon, color);
    painter.text(pos2(rect.left() + 86.0, y), Align2::LEFT_CENTER, kind, theme::caption(), theme::TEXT_MUTED);
    let font = if mono { theme::mono() } else { theme::caption() };
    let r = widgets::paint_elided(painter, pos2(rect.left() + 176.0, y), text, font, theme::TEXT, width - 176.0 - 8.0, Align2::LEFT_CENTER);
    if r.width() >= width - 176.0 - 10.0 {
        ui.interact(rect, Id::new(("installer-detail", text)), Sense::hover()).on_hover_text(text);
    }
    rect
}

fn caption(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::caption()).color(theme::TEXT_MUTED)).wrap());
}

fn paint_checkbox(painter: &egui::Painter, rect: Rect, on: bool, enabled: bool, hovered: bool) {
    controls::paint_check_box(painter, rect, on, enabled, hovered);
}

/// Заголовок таблицы: подписи колонок мелким текстом и линия.
fn table_header(ui: &mut Ui, columns: &[f32], labels: &[&str], right: &[bool]) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let painter = ui.painter();
    let mut x = rect.left();
    for ((w, label), right) in columns.iter().zip(labels).zip(right) {
        let (pos, align) = if *right { (pos2(x + w - 12.0, rect.center().y), Align2::RIGHT_CENTER) } else { (pos2(x, rect.center().y), Align2::LEFT_CENTER) };
        painter.text(pos, align, label.to_uppercase(), theme::strong(theme::FONT_SECTION), theme::TEXT_MUTED);
        x += w;
    }
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, theme::LINE_SOFT));
}

/// Моноширинный блок строк (ошибка, команда).
fn code_box(ui: &mut Ui, lines: &[String], max: usize) {
    egui::Frame::none().fill(theme::BG_INSET).stroke(Stroke::new(1.0_f32, theme::LINE_STRONG)).rounding(theme::R_S).inner_margin(Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let start = lines.len().saturating_sub(max);
        for line in &lines[start..] {
            ui.add(egui::Label::new(RichText::new(line).font(theme::mono()).color(theme::TEXT)).wrap());
        }
    });
}

/// Обзор каталогов хаба ([`piska_hub::FolderBrowser`]): путь, «вверх», подкаталоги.
fn folder_list(ui: &mut Ui, browser: &mut piska_hub::FolderBrowser, width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if widgets::icon_button(ui, Id::new("installer-browse-up"), Icon::ChevronUp, tr_s("Up one level")).clicked() {
            browser.up();
        }
        let w = ui.available_width();
        widgets::text_field(ui, Id::new("installer-browse-path"), &mut browser.path, tr_s("Folder path"), w);
    });
    browser.refresh();
    let entries = browser.entries.clone();
    let mut enter = None;
    let mut pick = None;
    egui::Frame::none().fill(theme::BG_INSET).stroke(Stroke::new(1.0_f32, theme::LINE)).rounding(theme::R_S).inner_margin(Margin::same(4.0)).show(ui, |ui| {
        ui.set_width(width - 10.0);
        egui::ScrollArea::vertical().id_salt("installer-browse-list").max_height(240.0).min_scrolled_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if entries.is_empty() {
                ui.label(RichText::new(tr_s("No subfolders")).color(theme::TEXT_DISABLED));
            }
            for (i, (name, path, _)) in entries.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(width - 18.0, 24.0), Sense::hover());
                let response = ui.interact(rect, Id::new(("installer-browse-entry", i)), Sense::click());
                let chosen = browser.path.trim() == path.display().to_string();
                theme::list_row(ui.painter(), rect, chosen, true, response.hovered());
                icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(14.0, 14.0)), Icon::Folder, theme::FOLDER);
                ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
                if response.double_clicked() {
                    enter = Some(path.clone());
                } else if response.clicked() {
                    pick = Some(path.clone());
                }
            }
        });
    });
    ui.label(RichText::new(tr_s("Click a folder to choose it, double-click to open it.")).font(theme::caption()).color(theme::TEXT_DISABLED));
    if let Some(dir) = enter {
        browser.enter(&dir);
    } else if let Some(dir) = pick {
        browser.path = dir.display().to_string();
        browser.listed = Some(dir);
    }
}
