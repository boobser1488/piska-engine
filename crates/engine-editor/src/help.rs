//! Меню Tools и Help: Piska Modeler, сценарии, «Getting Started» (с чего начать — карточки
//! с кнопкой первого шага) и шпаргалка Keyboard Shortcuts, собранная из реестра команд.

use crate::i18n::{n_, tr_s, trf};
use egui::vec2;

use crate::icons::{self, Icon};
use crate::shortcuts::COMMANDS;
use crate::state::{Action, Editor};
use crate::theme::{self, item};

/// Открытые окна справки.
#[derive(Default)]
pub(crate) struct HelpState {
    pub getting_started: bool,
    /// Piska Modeler не нашёлся: окно с объяснением, где его взять.
    pub modeler_missing: bool,
}

impl Editor {
    /// Окно Help → Getting Started.
    pub fn show_getting_started(&mut self, open: bool) {
        self.help.getting_started = open;
    }

    pub fn is_getting_started_open(&self) -> bool {
        self.help.getting_started
    }

    /// Окно Help → Keyboard Shortcuts.
    pub fn show_shortcuts_help(&mut self, open: bool) {
        self.shortcuts_help_open = open;
    }

    pub fn is_shortcuts_help_open(&self) -> bool {
        self.shortcuts_help_open
    }

    /// Показано ли окно «Piska Modeler не найден».
    pub fn is_modeler_missing_shown(&self) -> bool {
        self.help.modeler_missing
    }

    /// Запустить Piska Modeler (с файлом модели или пустым). Нет программы — окно с
    /// объяснением вместо молчания.
    pub(crate) fn launch_modeler(&mut self, file: Option<&std::path::Path>) {
        let args: Vec<std::ffi::OsString> = file.map(|f| f.as_os_str().to_owned()).into_iter().collect();
        self.launch_modeler_with(&args);
    }

    /// «Edit a Copy in Modeler»: новая `.pmodel` рядом с чужим мешем, меш — опора в ней;
    /// модель сохраняется сразу и появляется в окне Project, когда окно редактора снова
    /// получает фокус (ассеты перечитываются).
    pub(crate) fn edit_copy_in_modeler(&mut self, source: &std::path::Path) -> Result<(), crate::EditorError> {
        if crate::modeler_link::modeler_executable().is_none() {
            self.help.modeler_missing = true;
            return Ok(());
        }
        let target = crate::modeler_link::copy_target(source);
        let name = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::write(&target, crate::modeler_link::empty_model(&name))
            .map_err(|e| crate::EditorError::Other(trf!("cannot create `{p0}`: {e}", p0 = target.display(), e = e)))?;
        self.launch_modeler_with(&crate::modeler_link::copy_args(&target, source));
        Ok(())
    }

    /// Пункт «… in Modeler» для файла: подпись и доступность (`None` — файл не модель).
    pub fn modeler_menu_entry(path: &std::path::Path) -> Option<(&'static str, bool)> {
        let entry = crate::modeler_link::ModelerEntry::for_path(path)?;
        Some((entry.label(), crate::modeler_link::modeler_executable().is_some()))
    }

    /// Файл модели, которую рисует MeshRenderer объекта (`.pmodel`, `.obj`, `.glb`…).
    pub fn object_model_file(&self, id: engine_core::prelude::GameObjectId) -> Option<std::path::PathBuf> {
        let world = &self.app.world;
        let renderer = world.scene.get_component::<engine_render::MeshRenderer>(id)?;
        if renderer.mesh.is_none() {
            return None;
        }
        let path = world.resources.get::<engine_assets::AssetDatabase>()?.path_of(renderer.mesh.guid())?;
        crate::modeler_link::ModelerEntry::for_path(&path).map(|_| path)
    }

    fn launch_modeler_with(&mut self, args: &[std::ffi::OsString]) {
        let Some(exe) = crate::modeler_link::modeler_executable() else {
            self.help.modeler_missing = true;
            return;
        };
        let mut command = engine_build::host::command(&exe);
        command.args(args);
        match command.spawn() {
            Ok(_) => log::info!("started Piska Modeler"),
            Err(err) => log::error!("cannot start `{}`: {err}", exe.display()),
        }
    }

    /// Файл документации `docs/api/<name>` из рабочей области движка, если он есть.
    pub(crate) fn doc_file(&self, name: &str) -> Option<std::path::PathBuf> {
        let mut roots: Vec<std::path::PathBuf> = self.config.workspace.iter().cloned().collect();
        if let Ok(exe) = std::env::current_exe() {
            roots.extend(exe.ancestors().skip(1).take(5).map(|p| p.to_path_buf()));
        }
        if let Ok(cwd) = std::env::current_dir() {
            roots.extend(cwd.ancestors().take(3).map(|p| p.to_path_buf()));
        }
        roots.into_iter().map(|root| root.join("docs").join("api").join(name)).find(|p| p.is_file())
    }

    /// Меню Tools: редактор моделей и язык сценариев — то, что раньше было не найти.
    pub(crate) fn tools_menu(&mut self, ui: &mut egui::Ui) {
        let mounted = self.assets_root().is_some();
        let no_assets = "No assets directory is mounted: call `engine::mount_assets` before starting the editor";
        theme::menu_section(ui, tr_s("3D models"));
        if item(tr_s("Model Editor (Piska Modeler)")).icon(Icon::Cube).show(ui) {
            self.launch_modeler(None);
        }
        if item(tr_s("New Model…")).icon(Icon::Plus).enabled_if(mounted, no_assets).show(ui) {
            self.actions.push(Action::CreateModel { folder: self.project.folder.clone() });
        }
        theme::menu_section(ui, tr_s("Scenario language"));
        if item(tr_s("New Scenario Script…")).icon(Icon::Script).show(ui) {
            self.open_scenario_dialog("", self.selection);
        }
        let doc = self.doc_file("scenario.md");
        if item(tr_s("Scenario Documentation"))
            .icon(Icon::Text)
            .enabled_if(doc.is_some(), "docs/api/scenario.md is not next to this editor")
            .show(ui)
        {
            if let Some(doc) = doc {
                self.actions.push(Action::OpenCode(doc));
            }
        }
        theme::menu_section(ui, tr_s("Scripts"));
        if item(tr_s("New Rust Script…")).icon(Icon::Script).show(ui) {
            self.open_script_dialog(true, String::new(), self.selection);
        }
        if item(tr_s("New C++ Script…"))
            .icon(Icon::Script)
            .enabled_if(self.config.script_dir.is_some(), "Start the editor with `EditorConfig::cpp_scripts(dir)` to create C++ scripts")
            .show(ui)
        {
            self.open_script_dialog(false, String::new(), self.selection);
        }
        // Tools → Check Build Tools… (build_tools.rs).
        self.build_tools_menu_item(ui);
    }

    /// Меню Help.
    pub(crate) fn help_menu(&mut self, ui: &mut egui::Ui) {
        if item(tr_s("Getting Started")).icon(Icon::Info).show(ui) {
            self.help.getting_started = true;
        }
        if item(tr_s("Keyboard Shortcuts")).icon(Icon::Text).shortcut(&self.shortcut_text("shortcuts-help")).show(ui) {
            self.shortcuts_help_open = true;
        }
        let doc = self.doc_file("editor.md");
        if item(tr_s("Editor Manual"))
            .icon(Icon::File)
            .enabled_if(doc.is_some(), "docs/api/editor.md is not next to this editor")
            .show(ui)
        {
            if let Some(doc) = doc {
                self.actions.push(Action::OpenCode(doc));
            }
        }
        // Журнал программы без консоли (запуск не из терминала): где он лежит — в подсказке.
        theme::menu_separator(ui);
        let logs = engine_build::host::log_dir(crate::i18n::brand::CONFIG_DIR_NAME);
        let tip = logs.as_ref().map_or(String::new(), |dir| dir.display().to_string());
        if item(tr_s("Show Log Folder"))
            .icon(Icon::FolderOpen)
            .enabled_if(logs.is_some(), tr_s("The log folder is not known on this system"))
            .show(ui)
        {
            if let Some(dir) = logs {
                let _ = std::fs::create_dir_all(&dir);
                crate::state::reveal(&dir);
            }
        }
        if !tip.is_empty() {
            ui.label(egui::RichText::new(tip).font(theme::caption()).color(theme::TEXT_MUTED));
        }
    }

    /// Окна справки (вызывается из `Editor::ui`).
    pub(crate) fn help_windows(&mut self, ctx: &egui::Context) {
        self.shortcuts_window(ctx);
        self.getting_started_window(ctx);
        self.modeler_missing_window(ctx);
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_help_open {
            return;
        }
        let mut open = true;
        crate::window_kit::tool_window(egui::Id::new("editor_shortcuts_help"), tr_s("Keyboard Shortcuts"))
            .default_size(vec2(560.0, 520.0))
            .show(ctx, &mut open, |ui| {
                ui.label(
                    egui::RichText::new(
                        tr_s("Current bindings. Change them in Preferences → Shortcuts. In the Scene view: right mouse \
                         button + W A S D / Q E flies, Alt + left button orbits, middle button pans, Ctrl / Shift + \
                         click adds to the selection, dragging over empty space draws a selection box."),
                    )
                    .color(theme::TEXT_MUTED),
                );
                ui.add_space(theme::SP_S);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut category = "";
                    for def in COMMANDS {
                        if def.category != category {
                            category = def.category;
                            ui.add_space(theme::SP_S);
                            theme::section_label(ui, tr_s(category));
                        }
                        let binding = self.shortcut_text(def.id);
                        ui.horizontal(|ui| {
                            let width = ui.available_width();
                            let key_width = (width * 0.35).clamp(110.0, 200.0);
                            ui.add_sized(
                                vec2(width - key_width - theme::SP_M, theme::ROW),
                                egui::Label::new(egui::RichText::new(tr_s(def.title)).color(theme::TEXT)).truncate(),
                            );
                            let text = if binding.is_empty() { "—".to_string() } else { binding };
                            ui.add_sized(
                                vec2(key_width, theme::ROW),
                                egui::Label::new(egui::RichText::new(text).font(theme::mono()).color(theme::TEXT_MUTED)),
                            );
                        });
                    }
                });
            });
        if !open {
            self.shortcuts_help_open = false;
        }
    }

    fn getting_started_window(&mut self, ctx: &egui::Context) {
        if !self.help.getting_started {
            return;
        }
        let mut open = true;
        let mut run: Option<&'static str> = None;
        crate::window_kit::tool_window(egui::Id::new("editor_getting_started"), tr_s("Getting Started"))
            .default_size(vec2(620.0, 560.0))
            .show(ctx, &mut open, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (key, icon, title, text, button) in CARDS {
                        if card(ui, *icon, title, text, button) {
                            run = Some(key);
                        }
                    }
                });
            });
        if !open {
            self.help.getting_started = false;
        }
        if let Some(key) = run {
            self.run_getting_started(key);
        }
    }

    /// Первый шаг карточки Getting Started.
    pub fn run_getting_started(&mut self, key: &str) {
        match key {
            "rust" => self.open_script_dialog(true, String::new(), self.selection),
            "cpp" => self.open_script_dialog(false, String::new(), self.selection),
            "scenario" => self.open_scenario_dialog("", self.selection),
            "model" => {
                if self.assets_root().is_some() {
                    self.actions.push(Action::CreateModel { folder: self.project.folder.clone() });
                } else {
                    self.launch_modeler(None);
                }
            }
            "terrain" => {
                let id = self.create_terrain();
                self.set_scene_3d(true);
                self.actions.push(Action::Select(Some(id)));
                self.set_terrain_tool(Some(engine_render::BrushTool::Raise));
            }
            "material" => self.actions.push(Action::CreateMaterial { folder: self.project.folder.clone() }),
            "build" => self.actions.push(Action::ShowBuildDialog(true)),
            _ => return,
        }
        self.help.getting_started = false;
    }

    fn modeler_missing_window(&mut self, ctx: &egui::Context) {
        if !self.help.modeler_missing {
            return;
        }
        let mut close = false;
        let response = crate::window_kit::dialog(egui::Id::new("modeler_missing"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(420.0);
            theme::dialog_title(ui, Icon::Cube, theme::TEXT_MUTED, tr_s("Piska Modeler is not installed here"));
            ui.label(
                egui::RichText::new(
                    tr_s("The model editor is a separate program, `piska-modeler` (its own workspace in `modeler/`), that has to sit next to the editor \
                     program. Build it once from the engine folder:"),
                )
                .color(theme::TEXT_MUTED),
            );
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new("cargo build --manifest-path modeler/Cargo.toml -p piska-modeler").font(theme::mono()).color(theme::TEXT));
            ui.add_space(theme::SP_S);
            ui.label(
                egui::RichText::new(tr_s("or point the PISKA_MODELER environment variable at the program, then try again."))
                    .color(theme::TEXT_MUTED),
            );
            crate::window_kit::dialog_actions(ui, |ui| {
                if theme::primary_button(ui, tr_s("OK")).clicked() {
                    close = true;
                }
            });
        });
        if close || response.should_close() {
            self.help.modeler_missing = false;
        }
    }
}

/// Карточки Getting Started: ключ первого шага, иконка, заголовок, текст, кнопка.
const CARDS: &[(&str, Icon, &str, &str, &str)] = &[
    ("rust", Icon::Script, n_("Write a script in Rust"), n_("A component is a Rust struct with `component!`; the editor rebuilds and restarts itself with your code."), n_("New Rust Script…")),
    ("cpp", Icon::Script, n_("Write a script in C++"), n_("C++ components are compiled into a plugin that reloads while the editor runs."), n_("New C++ Script…")),
    ("scenario", Icon::Script, n_("Write a Scenario script"), n_("Scenario is the engine's small language for cutscenes and level logic: no build, saving restarts it."), n_("New Scenario Script…")),
    ("model", Icon::Cube, n_("Model an object"), n_("Piska Modeler builds low-poly models from boxes and paints them. Models live in Assets as .pmodel."), n_("New Model…")),
    ("terrain", Icon::Environment, n_("Sculpt a terrain"), n_("Adds a terrain to the scene, switches to 3D and picks the Raise brush: drag over the ground."), n_("Create Terrain")),
    ("material", Icon::Sphere, n_("Make a material"), n_("A .material asset with colour, textures, metallic and roughness; drop it onto an object."), n_("New Material")),
    ("build", Icon::Build, n_("Build the game"), n_("Export a web, Windows or Android build of this game from File → Build…."), n_("Open Build…")),
];

/// Карточка: иконка, заголовок, пояснение, кнопка первого шага справа. `true` — нажали.
fn card(ui: &mut egui::Ui, icon: Icon, title: &'static str, text: &'static str, button: &'static str) -> bool {
    let mut clicked = false;
    egui::Frame::none()
        .fill(theme::BG_RAISED)
        .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
        .rounding(theme::R_M)
        .inner_margin(egui::Margin::same(theme::SP_M + 2.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::inline(ui, icon, theme::TEXT_MUTED);
                ui.vertical(|ui| {
                    let width = (ui.available_width() - 150.0).max(120.0);
                    ui.set_max_width(width);
                    ui.label(egui::RichText::new(tr_s(title)).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
                    ui.label(egui::RichText::new(tr_s(text)).color(theme::TEXT_MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = theme::button(ui, tr_s(button)).clicked();
                });
            });
        });
    ui.add_space(theme::SP_S);
    clicked
}
