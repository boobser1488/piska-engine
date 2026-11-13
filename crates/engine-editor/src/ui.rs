//! Кадр интерфейса: меню, панель инструментов, раскладка окон, диалоги,
//! окно Preferences, горячие клавиши и ввод для игры в режиме Play.

use egui::{pos2, vec2, Color32, Key as EguiKey, Rect, Sense, Stroke};
use engine_core::input::{Key, MouseButton, TouchPhase};
use engine_core::logging::Level;
use engine_core::prelude::Vec2;
use engine_render::{LightKind, MeshShape};

use crate::catalog;
use crate::dock::{LayoutAction, LayoutPreset, Panel};
use crate::icons::{self, Icon};
use crate::prefs::UI_SCALES;
use crate::state::{Action, Dialog, Editor, Mode, Tool};
use crate::theme::{self, item};
use crate::console;

/// Сколько ждать до следующего кадра, когда в режиме Edit ничего не происходит:
/// подсистемы (горячая перезагрузка плагинов) всё равно должны «тикать».
pub(crate) const IDLE_REPAINT: std::time::Duration = std::time::Duration::from_millis(250);

/// Что тема уже применила к этому контексту: акцент и шрифты.
#[derive(Clone, Copy, PartialEq)]
struct Themed {
    accent: usize,
}

impl Editor {
    /// Построить интерфейс редактора за один проход egui. Не требует GPU, поэтому
    /// тесты гоняют его с `egui::Context::default()` и синтетическим вводом.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if self.apply_theme(ctx) {
            // Шрифты подключаются только со следующего кадра: этот пропускаем.
            ctx.request_repaint();
            return;
        }
        let new_records = self.console.poll();
        // Error Pause (Console): новая ошибка в режиме Play — пауза.
        if new_records && self.console.take_new_errors() && self.console.error_pause && self.mode == Mode::Playing {
            self.actions.push(Action::Pause);
        }
        self.shortcuts(ctx);
        self.autosave_tick(ctx.input(|i| i.time));

        egui::TopBottomPanel::top("editor_top")
            .exact_height(theme::MENU_BAR + theme::MAIN_TOOLBAR)
            .frame(egui::Frame::none().fill(theme::BG_HEADER))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.menu_bar(ui);
                self.toolbar(ui);
            });

        egui::TopBottomPanel::bottom("editor_status")
            .exact_height(theme::STATUS_BAR)
            .frame(egui::Frame::none().fill(theme::BG_APP))
            .show_separator_line(false)
            .show(ctx, |ui| self.status_bar(ui));

        // Все окна — вкладки дока между панелью инструментов и строкой состояния.
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG_APP))
            .show(ctx, |ui| self.dock_ui(ui));

        self.dialogs(ctx);
        self.save_layout_dialog(ctx);
        crate::rust_scripts::script_file_dialog(self, ctx);
        self.preferences_window(ctx);
        self.project_settings_window(ctx);
        self.export_window(ctx);
        self.apply_queued();

        // Никто ничего не тянет и не печатает — правка закончена, это шаг Undo.
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
            self.settle();
        }

        // Когда рисовать следующий кадр. В режиме Edit без ввода — редко.
        if self.wants_continuous_redraw() || new_records || !self.pending_scripts().is_empty() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(IDLE_REPAINT);
        }
    }

    /// Тема, масштаб и акцент из настроек; Ctrl +/− меняет масштаб и в настройках.
    /// `true` — тему применили впервые (шрифты заработают со следующего кадра).
    fn apply_theme(&mut self, ctx: &egui::Context) -> bool {
        let key = egui::Id::new("piska_theme_applied");
        let wanted = Themed { accent: self.prefs.accent };
        let applied = ctx.data(|data| data.get_temp::<Option<Themed>>(key)).flatten();
        if applied != Some(wanted) {
            let accent = theme::ACCENTS
                .get(self.prefs.accent)
                .map_or(theme::ACCENTS[0].1, |(_, color)| *color);
            theme::set_accent(accent);
            theme::apply(ctx, applied.is_none());
            ctx.data_mut(|data| data.insert_temp(key, Some(wanted)));
            ctx.set_zoom_factor(self.prefs.ui_scale);
        }
        // Масштаб меняют и Preferences, и встроенные Ctrl + / Ctrl − / Ctrl 0.
        let scale_key = egui::Id::new("piska_ui_scale");
        let last = ctx.data(|data| data.get_temp::<f32>(scale_key));
        let zoom = ctx.zoom_factor();
        match last {
            Some(last) if (last - self.prefs.ui_scale).abs() > 1e-3 => ctx.set_zoom_factor(self.prefs.ui_scale),
            Some(last) if (last - zoom).abs() > 1e-3 => self.prefs.ui_scale = zoom,
            None => ctx.set_zoom_factor(self.prefs.ui_scale),
            _ => {}
        }
        ctx.data_mut(|data| data.insert_temp(scale_key, self.prefs.ui_scale));
        applied.is_none()
    }

    pub(crate) fn open_save_as(&mut self) {
        self.dialog = Some(Dialog::SaveAs {
            path: self
                .scene_path
                .clone()
                .unwrap_or_else(|| self.suggested_scene_path()),
            then: None,
        });
    }

    fn open_new_script(&mut self) {
        self.open_script_dialog(false, String::new(), self.selection);
    }

    /// Горячие клавиши: все команды — в реестре `shortcuts.rs` (Preferences → Shortcuts).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        self.dispatch_shortcuts(ctx);
    }

    // ------------------------------------------------------------------
    // Меню
    // ------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::MENU_BAR), Sense::hover());
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(theme::SP_S + 2.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut ui;
        logo(ui);
        ui.add_space(theme::SP_S);
        egui::menu::bar(ui, |ui| {
            ui.spacing_mut().button_padding = vec2(theme::SP_M, 2.0);
            ui.spacing_mut().interact_size.y = theme::MENU_BAR - 2.0;
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.menu_button("File", |ui| {
                theme::menu_body(ui);
                self.file_menu(ui);
            });
            ui.menu_button("Edit", |ui| {
                theme::menu_body(ui);
                self.edit_menu(ui);
            });
            ui.menu_button("GameObject", |ui| {
                theme::menu_body(ui);
                self.gameobject_menu(ui, None);
            });
            ui.menu_button("View", |ui| {
                theme::menu_body(ui);
                self.view_menu(ui);
            });
            ui.menu_button("Assets", |ui| {
                theme::menu_body(ui);
                self.assets_menu(ui);
            });
            ui.menu_button("Window", |ui| {
                theme::menu_body(ui);
                self.window_menu(ui);
            });
        });
    }

    /// Меню Window: окна (открыть / показать), раскладки.
    fn window_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        theme::menu_section(ui, "Windows");
        for panel in Panel::ALL {
            let open = self.is_panel_open(panel);
            if item(panel.title())
                .icon(panel.icon())
                .shortcut(&k.text(crate::shortcuts::window_command(panel)))
                .checked(open)
                .show(ui)
            {
                self.actions.push(Action::Layout(LayoutAction::Focus(panel)));
            }
        }
        theme::menu_separator(ui);
        let saved = self.saved_layouts();
        theme::submenu(ui, Icon::Tiles, "Layouts", |ui| {
            for preset in LayoutPreset::ALL {
                if item(preset.name()).show(ui) {
                    self.actions.push(Action::Layout(LayoutAction::Preset(preset)));
                }
            }
            if !saved.is_empty() {
                theme::menu_separator(ui);
                for name in &saved {
                    if item(name).icon(Icon::Tiles).show(ui) {
                        self.actions.push(Action::Layout(LayoutAction::Load(name.clone())));
                    }
                }
            }
            theme::menu_separator(ui);
            if item("Save Layout…").icon(Icon::Save).show(ui) {
                self.dock.save_dialog = Some(String::new());
            }
            if saved.is_empty() {
                item("Delete Layout")
                    .icon(Icon::Trash)
                    .enabled_if(false, "No saved layouts yet: save one with Save Layout…")
                    .show(ui);
            } else {
                theme::submenu(ui, Icon::Trash, "Delete Layout", |ui| {
                    for name in &saved {
                        if item(name).show(ui) {
                            self.actions.push(Action::Layout(LayoutAction::Delete(name.clone())));
                        }
                    }
                });
            }
            theme::menu_separator(ui);
            if item("Reset Layout").icon(Icon::Reset).show(ui) {
                self.actions.push(Action::Layout(LayoutAction::Reset));
            }
        });
        if let Some(panel) = self.maximized_panel() {
            theme::menu_separator(ui);
            if item(&format!("Restore {}", panel.title())).icon(Icon::Collapse).shortcut(&k.text("maximize-panel")).show(ui) {
                self.actions.push(Action::Layout(LayoutAction::ToggleMaximize(panel)));
            }
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let editing = self.mode == Mode::Edit;
        let k = self.prefs.shortcuts.clone();
        let stop_first = k.tip("Stop the game first", "play");
        if item("New Scene").icon(Icon::Scene).shortcut(&k.text("new-scene")).show(ui) {
            self.actions.push(Action::NewScene);
        }
        let scenes: Vec<String> = self
            .assets
            .iter()
            .filter(|asset| asset.importer == "Scene")
            .map(|asset| asset.path.clone())
            .collect();
        if scenes.is_empty() {
            item("Open Scene")
                .icon(Icon::FolderOpen)
                .enabled_if(false, "There are no .scene files in the assets directory yet")
                .show(ui);
        } else {
            theme::submenu(ui, Icon::FolderOpen, "Open Scene", |ui| {
                for scene in &scenes {
                    let current = self.scene_path.as_deref() == Some(scene.as_str());
                    if item(scene).icon(Icon::Scene).checked(current).show(ui) {
                        self.actions.push(Action::OpenScene(scene.clone()));
                    }
                }
            });
        }
        theme::menu_separator(ui);
        if item("Save")
            .icon(Icon::Save)
            .shortcut(&k.text("save"))
            .enabled_if(editing, &stop_first)
            .show(ui)
        {
            self.actions.push(Action::Save);
        }
        if item("Save As…")
            .shortcut(&k.text("save-as"))
            .enabled_if(editing, &stop_first)
            .show(ui)
        {
            self.open_save_as();
        }
        theme::menu_separator(ui);
        if item("Build…")
            .icon(Icon::Build)
            .enabled_if(
                self.config.game_crate.is_some(),
                "Start the editor with EditorConfig::game_crate(\"<name>\") to export a game from games/<name>",
            )
            .show(ui)
        {
            self.actions.push(Action::ShowBuildDialog(true));
        }
        theme::menu_separator(ui);
        if item("Project Hub…").icon(Icon::Folder).show(ui) {
            self.actions.push(Action::ProjectHub);
        }
        let recent = crate::hub_link::recent_projects();
        if !recent.is_empty() {
            theme::submenu(ui, Icon::FolderOpen, "Open Recent", |ui| {
                for (name, dir) in &recent {
                    if item(name).icon(Icon::Folder).show(ui) {
                        self.actions.push(Action::OpenProject(dir.clone()));
                    }
                }
            });
        }
        theme::menu_separator(ui);
        if item("Preferences…").icon(Icon::Gear).shortcut(&k.text("preferences")).show(ui) {
            self.preferences_open = true;
        }
        theme::menu_separator(ui);
        if item("Quit").icon(Icon::Close).shortcut(&k.text("quit")).show(ui) {
            self.actions.push(Action::Quit);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        let editing = self.mode == Mode::Edit;
        let selection = self.selection;
        let history_reason = if editing { "Nothing to undo" } else { "Undo is not available in Play mode" };
        if item("Undo")
            .icon(Icon::Undo)
            .shortcut(&k.text("undo"))
            .enabled_if(self.can_undo(), history_reason)
            .show(ui)
        {
            self.actions.push(Action::Undo);
        }
        let redo_reason = if editing { "Nothing to redo" } else { "Redo is not available in Play mode" };
        if item("Redo")
            .icon(Icon::Redo)
            .shortcut(&k.text("redo"))
            .enabled_if(self.can_redo(), redo_reason)
            .show(ui)
        {
            self.actions.push(Action::Redo);
        }
        theme::menu_separator(ui);
        let nothing = "Select an object first";
        if item("Duplicate")
            .icon(Icon::Copy)
            .shortcut(&k.text("duplicate"))
            .enabled_if(selection.is_some(), nothing)
            .show(ui)
        {
            self.actions.extend(selection.map(Action::Duplicate));
        }
        if item("Rename")
            .icon(Icon::Text)
            .shortcut(&k.text("rename"))
            .enabled_if(selection.is_some(), nothing)
            .show(ui)
        {
            let target = selection.and_then(|id| self.app.world.scene.get(id).map(|o| (id, o.name.clone())));
            if let Some((id, name)) = target {
                self.hierarchy.begin_rename(id, &name);
            }
        }
        if item("Delete")
            .icon(Icon::Trash)
            .shortcut(&k.text("delete"))
            .enabled_if(selection.is_some(), nothing)
            .show(ui)
        {
            self.actions.extend(selection.map(Action::Delete));
        }
        theme::menu_separator(ui);
        if item("Frame Selected").icon(Icon::Search).shortcut(&k.text("frame-selected")).show(ui) {
            self.actions.push(Action::FrameSelected);
        }
        if item("Select None")
            .shortcut(&k.text("select-none"))
            .enabled_if(selection.is_some(), "Nothing is selected")
            .show(ui)
        {
            self.actions.push(Action::Select(None));
        }
        theme::menu_separator(ui);
        let (play_label, play_icon) = if editing { ("Play", Icon::Play) } else { ("Stop", Icon::Stop) };
        if item(play_label).icon(play_icon).shortcut(&k.text("play")).show(ui) {
            self.actions.push(if editing { Action::Play } else { Action::Stop });
        }
        if item(if self.mode == Mode::Paused { "Resume" } else { "Pause" })
            .icon(Icon::Pause)
            .shortcut(&k.text("pause"))
            .enabled_if(!editing, "The game is not running")
            .show(ui)
        {
            self.actions.push(Action::Pause);
        }
        if item("Step").icon(Icon::Step).shortcut(&k.text("step")).show(ui) {
            self.actions.push(Action::Step);
        }
        theme::menu_separator(ui);
        if item("Project Settings…").icon(Icon::Gear).shortcut(&k.text("project-settings")).show(ui) {
            self.script_order.open = true;
        }
    }

    /// Меню создания объектов. `parent` — для контекстного меню строки иерархии.
    pub(crate) fn gameobject_menu(&mut self, ui: &mut egui::Ui, parent: Option<engine_core::prelude::GameObjectId>) {
        let k = self.prefs.shortcuts.clone();
        let child = parent.is_some();
        if item(if child { "Create Empty Child" } else { "Create Empty" })
            .icon(Icon::Object)
            .shortcut(&k.text(if child { "create-empty-child" } else { "create-empty" }))
            .show(ui)
        {
            self.actions.push(Action::CreateEmpty { parent });
        }
        if !child
            && item("Create Empty Child")
                .icon(Icon::Object)
                .shortcut(&k.text("create-empty-child"))
                .enabled_if(self.selection.is_some(), "Select the parent object first")
                .show(ui)
        {
            self.actions.push(Action::CreateEmpty { parent: self.selection });
        }
        theme::menu_separator(ui);
        theme::submenu(ui, Icon::Cube, "3D Object", |ui| {
            for (shape, icon) in [
                (MeshShape::Cube, Icon::Cube),
                (MeshShape::Sphere, Icon::Sphere),
                (MeshShape::Cylinder, Icon::Cylinder),
                (MeshShape::Plane, Icon::Plane),
            ] {
                if item(&format!("{shape:?}")).icon(icon).show(ui) {
                    self.actions.push(Action::CreateMesh(shape));
                }
            }
            crate::terrain_tools::menu_items(self, ui);
        });
        let available: Vec<&catalog::Preset> = catalog::PRESETS
            .iter()
            .filter(|preset| preset.available(&self.app.world.registry))
            .collect();
        theme::submenu(ui, Icon::Sprite, "2D", |ui| {
            if item(if child { "Sprite (child)" } else { "Sprite" }).icon(Icon::Sprite).show(ui) {
                self.actions.push(Action::CreateSprite { parent });
            }
            for preset in available.iter().filter(|p| p.menu() == "2D") {
                if item(preset.label).icon(preset.icon).show(ui) {
                    self.actions.push(Action::Create { preset: preset.id.to_string(), parent });
                }
            }
        });
        theme::submenu(ui, Icon::Bulb, "Light", |ui| {
            if item("Directional Light").icon(Icon::Sun).show(ui) {
                self.actions.push(Action::CreateLight(LightKind::Directional));
            }
            if item("Point Light").icon(Icon::Bulb).show(ui) {
                self.actions.push(Action::CreateLight(LightKind::Point));
            }
        });
        for (menu, icon) in catalog::PRESET_MENUS.iter().filter(|(menu, _)| *menu != "2D") {
            let entries: Vec<&&catalog::Preset> = available.iter().filter(|p| p.menu() == *menu).collect();
            if entries.is_empty() {
                continue; // крейт с этими компонентами не подключён — пункта нет
            }
            theme::submenu(ui, *icon, menu, |ui| {
                for preset in entries {
                    if item(preset.label).icon(preset.icon).show(ui) {
                        self.actions.push(Action::Create { preset: preset.id.to_string(), parent });
                    }
                }
            });
        }
        if !child {
            crate::terrain_tools::snap_menu_item(self, ui);
        }
        if !child && item("Camera").icon(Icon::Camera).show(ui) {
            self.actions.push(Action::CreateCamera);
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        theme::menu_section(ui, "Scene view");
        if item("3D View").icon(Icon::Cube).shortcut(&k.text("view-3d")).checked(self.scene_3d).show(ui) {
            let enabled = !self.scene_3d;
            self.set_scene_3d(enabled);
        }
        if item("Grid").icon(Icon::Grid).shortcut(&k.text("view-grid")).checked(self.show_grid).show(ui) {
            self.show_grid = !self.show_grid;
        }
        if item("Gizmos").icon(Icon::Gizmo).shortcut(&k.text("view-gizmos")).checked(self.show_gizmos).show(ui) {
            self.show_gizmos = !self.show_gizmos;
        }
        if item("Snap to Grid").icon(Icon::Magnet).shortcut(&k.text("toggle-snap")).checked(self.snap).show(ui) {
            self.snap = !self.snap;
        }
        theme::menu_separator(ui);
        theme::submenu(ui, Icon::Search, "UI Scale", |ui| {
            for percent in UI_SCALES {
                let scale = percent as f32 / 100.0;
                let chosen = (self.prefs.ui_scale - scale).abs() < 0.02;
                let shortcut = if percent == 100 { "Ctrl+0" } else { "" };
                if item(&format!("{percent}%")).shortcut(shortcut).checked(chosen).show(ui) {
                    self.prefs.ui_scale = scale;
                }
            }
        });
    }

    fn assets_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        let mounted = self.assets_root().is_some();
        let no_assets = "No assets directory is mounted: call `engine::mount_assets` before starting the editor";
        if item("Refresh")
            .icon(Icon::Refresh)
            .shortcut(&k.text("refresh"))
            .enabled_if(mounted, no_assets)
            .show(ui)
        {
            self.actions.push(Action::RefreshAssets);
        }
        if item("New Folder…").icon(Icon::Folder).enabled_if(mounted, no_assets).show(ui) {
            self.dialog = Some(Dialog::NewFolder {
                parent: self.project.folder.clone(),
                name: String::new(),
            });
        }
        if item("Import New Asset…").icon(Icon::ArrowDown).enabled_if(mounted, no_assets).show(ui) {
            self.show_import_browser();
        }
        if item("Reveal in File Manager")
            .icon(Icon::External)
            .enabled_if(mounted, no_assets)
            .show(ui)
        {
            self.actions.push(Action::RevealAsset(self.project.folder.clone()));
        }
        if item("New Model (Piska Modeler)").icon(Icon::Cube).enabled_if(mounted, no_assets).show(ui) {
            self.actions.push(Action::CreateModel { folder: self.project.folder.clone() });
        }
        theme::menu_separator(ui);
        let scripts = self.config.script_dir.is_some();
        if item("New Rust Script…").icon(Icon::Script).show(ui) {
            self.open_script_dialog(true, String::new(), self.selection);
        }
        if item("New C++ Script…")
            .icon(Icon::Script)
            .enabled_if(scripts, "Start the editor with `EditorConfig::cpp_scripts(dir)` to create scripts")
            .show(ui)
        {
            self.open_new_script();
        }
        if item("New Scenario Script…").icon(Icon::Script).show(ui) {
            self.open_scenario_dialog("", self.selection);
        }
        let buildable = self.can_build();
        if item("Build Code")
            .icon(Icon::Build)
            .shortcut(&k.text("build-code"))
            .enabled_if(buildable, "No build commands: add them with `EditorConfig::build_command`")
            .show(ui)
        {
            self.actions.push(Action::Build);
        }
    }

    // ------------------------------------------------------------------
    // Панель инструментов
    // ------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::MAIN_TOOLBAR), Sense::hover());
        let playing = self.mode != Mode::Edit;
        // Режим Play: панель слегка тонируется акцентом и снизу — полоса акцента, как в Unity.
        if playing {
            ui.painter().rect_filled(bar, 0.0, theme::play_tint(self.prefs.play_tint));
        }
        ui.painter()
            .hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let line = if playing { theme::accent() } else { theme::LINE };
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - if playing { 1.0 } else { 0.5 },
            Stroke::new(if playing { 2.0_f32 } else { 1.0_f32 }, line),
        );
        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(theme::SP_S + 2.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        left.spacing_mut().item_spacing.x = 1.0;
        self.toolbar_left(&mut left);

        // Play / Pause / Step — по центру окна.
        let transport = Rect::from_center_size(bar.center(), vec2(3.0 * 30.0 + 2.0, theme::MAIN_TOOLBAR));
        let mut center = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(transport)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.transport(&mut center);

        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(theme::SP_M, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 1.0;
        if icons::button(&mut right, Icon::Gear, self.preferences_open, None, &self.prefs.shortcuts.tip("Preferences", "preferences")).clicked() {
            self.preferences_open = !self.preferences_open;
        }
        if icons::button(&mut right, Icon::Folder, false, None, "Project Hub: open or create another project").clicked() {
            self.actions.push(Action::ProjectHub);
        }
        theme::toolbar_separator(&mut right);
        self.layout_selector(&mut right);
        theme::toolbar_separator(&mut right);
        self.stats_readout(&mut right);
    }

    fn toolbar_left(&mut self, ui: &mut egui::Ui) {
        group(ui, |ui| {
            for (tool, icon, tooltip, command) in [
                (Tool::Move, Icon::Move, "Move tool", "tool-move"),
                (Tool::Rotate, Icon::Rotate, "Rotate tool", "tool-rotate"),
                (Tool::Scale, Icon::Scale, "Scale tool", "tool-scale"),
            ] {
                let tooltip = self.prefs.shortcuts.tip(tooltip, command);
                if icons::button(ui, icon, self.tool == tool, None, &tooltip).clicked() {
                    self.tool = tool;
                }
            }
        });
        theme::toolbar_separator(ui);
        group(ui, |ui| {
            let (icon, tooltip) = if self.gizmo_local {
                (Icon::Local, "Move along the object's own axes (click for world axes)")
            } else {
                (Icon::Globe, "Move along world axes (click for the object's own axes)")
            };
            if icons::button(ui, icon, false, None, tooltip).clicked() {
                self.gizmo_local = !self.gizmo_local;
            }
            let snap_tip = format!(
                "Snap: {} m steps, {}° turns, ×{} scale (Preferences → Scene View). Hold Ctrl to invert",
                self.prefs.snap_move, self.prefs.snap_rotate, self.prefs.snap_scale
            );
            if icons::button(ui, Icon::Magnet, self.snap, None, &snap_tip)
            .clicked()
            {
                self.snap = !self.snap;
            }
        });
        theme::toolbar_separator(ui);
        let mut three_d = self.scene_3d;
        if theme::segmented(
            ui,
            &mut three_d,
            &[(false, "2D", "Flat XY view of the scene"), (true, "3D", "Perspective view with an orbit camera")],
        ) {
            self.set_scene_3d(three_d);
        }
        theme::toolbar_separator(ui);
        group(ui, |ui| {
            if icons::button(ui, Icon::Grid, self.show_grid, None, "Show the grid").clicked() {
                self.show_grid = !self.show_grid;
            }
            if icons::button(ui, Icon::Gizmo, self.show_gizmos, None, "Show object icons, colliders and light directions")
                .clicked()
            {
                self.show_gizmos = !self.show_gizmos;
            }
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        let playing = self.mode != Mode::Edit;
        group(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            // Play: в режиме игры кнопка «нажата» (акцент), значок — Stop.
            let (rect, response) = ui.allocate_exact_size(vec2(30.0, theme::CONTROL_H + 2.0), Sense::click());
            let fill = if playing {
                theme::accent()
            } else if response.hovered() {
                theme::BG_HOVER
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, theme::R_S, fill);
            let (icon, color, tooltip) = if playing {
                (Icon::Stop, theme::TEXT_STRONG, self.prefs.shortcuts.tip("Stop and restore the scene", "play"))
            } else {
                (Icon::Play, theme::TEXT, self.prefs.shortcuts.tip("Play", "play"))
            };
            icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(13.0, 13.0)), icon, color);
            if response.on_hover_text(tooltip).clicked() {
                self.actions.push(if playing { Action::Stop } else { Action::Play });
            }
            ui.add_enabled_ui(playing, |ui| {
                let tooltip = self.prefs.shortcuts.tip(if self.mode == Mode::Paused { "Resume" } else { "Pause" }, "pause");
                if icons::button(ui, Icon::Pause, self.mode == Mode::Paused, None, &tooltip).clicked() {
                    self.actions.push(Action::Pause);
                }
            });
            if icons::button(ui, Icon::Step, false, None, &self.prefs.shortcuts.tip("Advance one frame and pause", "step")).clicked() {
                self.actions.push(Action::Step);
            }
        });
    }

    fn stats_readout(&mut self, ui: &mut egui::Ui) {
        let fps = self.profiler.fps();
        let text = if !self.wants_continuous_redraw() {
            format!("idle  {:>5.2} ms", self.profiler.recent_work_ms())
        } else if fps > 0.0 {
            format!("{fps:>4.0} fps  {:>5.2} ms", self.profiler.recent_frame_ms())
        } else {
            "-- fps".to_string()
        };
        let color = if self.mode == Mode::Playing { theme::TEXT } else { theme::TEXT_MUTED };
        let galley = ui.painter().layout_no_wrap(text, theme::mono(), color);
        let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(theme::SP_M * 2.0, 8.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
        }
        ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
        let redraw = if self.wants_continuous_redraw() {
            "continuously"
        } else {
            "only when something changes"
        };
        if response
            .on_hover_text(format!(
                "Editor frame rate. The window redraws {redraw}. Click to open the Profiler"
            ))
            .clicked()
        {
            self.actions.push(Action::Layout(LayoutAction::Focus(Panel::Profiler)));
        }
    }

    /// Выбор раскладки окон справа на панели инструментов (как Layout в Unity).
    fn layout_selector(&mut self, ui: &mut egui::Ui) {
        let saved = self.saved_layouts();
        let popup = egui::Id::new("toolbar_layouts");
        let galley = ui.painter().layout_no_wrap("Layout".to_string(), theme::body(), theme::TEXT);
        let size = vec2(galley.size().x + theme::SP_M * 2.0 + theme::ICON_S, theme::CONTROL_H + 2.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let open = ui.memory(|m| m.is_popup_open(popup));
        if response.hovered() || open {
            ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
        }
        ui.painter().galley(
            pos2(rect.left() + theme::SP_M, rect.center().y - galley.size().y * 0.5),
            galley,
            theme::TEXT,
        );
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - theme::SP_M - 2.0, rect.center().y + 1.0), vec2(9.0, 9.0)),
            Icon::ChevronDown,
            theme::TEXT_MUTED,
        );
        let response = response.on_hover_text("Window layouts");
        if response.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup::popup_below_widget(ui, popup, &response, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            theme::menu_body(ui);
            for preset in LayoutPreset::ALL {
                if item(preset.name()).show(ui) {
                    self.actions.push(Action::Layout(LayoutAction::Preset(preset)));
                }
            }
            if !saved.is_empty() {
                theme::menu_separator(ui);
                for name in &saved {
                    if item(name).icon(Icon::Tiles).show(ui) {
                        self.actions.push(Action::Layout(LayoutAction::Load(name.clone())));
                    }
                }
            }
            theme::menu_separator(ui);
            if item("Save Layout…").icon(Icon::Save).show(ui) {
                self.dock.save_dialog = Some(String::new());
            }
            if item("Reset Layout").icon(Icon::Reset).show(ui) {
                self.actions.push(Action::Layout(LayoutAction::Reset));
            }
        });
    }

    // ------------------------------------------------------------------
    // Строка состояния и нижняя панель
    // ------------------------------------------------------------------

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(theme::SP_M, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let ui = &mut ui;
        ui.spacing_mut().item_spacing.x = theme::SP_S + 2.0;
        let small = |text: String, color: Color32| egui::RichText::new(text).size(theme::FONT_CAPTION).color(color);

        // Справа налево: режим, FPS в Play, выделение, сцена, фоновая задача.
        let (mode, mode_color) = match self.mode {
            Mode::Edit => ("Edit", theme::TEXT_DISABLED),
            Mode::Playing => ("Playing", theme::mix(theme::accent(), Color32::WHITE, 0.45)),
            Mode::Paused => ("Paused", theme::WARN),
        };
        ui.label(small(mode.to_string(), mode_color));
        if self.mode != Mode::Edit {
            let fps = self.profiler.fps();
            ui.label(egui::RichText::new(format!("{fps:.0} fps")).font(theme::mono()).color(theme::TEXT_MUTED));
        }
        status_divider(ui);
        if let Some(object) = self.selection.and_then(|id| self.app.world.scene.get(id)) {
            let (icon, _) = catalog::object_icon(object);
            ui.label(small(object.name.clone(), theme::TEXT));
            icons::inline_small(ui, icon, theme::TEXT_MUTED);
            status_divider(ui);
        }
        let file = self
            .scene_path
            .as_deref()
            .map_or("not saved yet".to_string(), |path| format!("Assets/{path}"));
        ui.label(small(file, theme::TEXT_DISABLED));
        let scene = format!("{}{}", self.app.world.scene.name, if self.dirty { "*" } else { "" });
        ui.label(small(scene, theme::TEXT_MUTED)).on_hover_text(if self.dirty {
            self.prefs.shortcuts.tip("The scene has unsaved changes", "save")
        } else {
            "The scene is saved".to_string()
        });
        if self.build_status() == crate::code::BuildStatus::Running {
            status_divider(ui);
            let step = Self::export_step().unwrap_or_else(|| "Building…".to_string());
            ui.allocate_ui(vec2(220.0, theme::STATUS_BAR), |ui| {
                theme::progress_bar(ui, None, &step);
            });
        }

        // Слева — последнее сообщение консоли со значком уровня; щелчок открывает Console.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let Some(record) = self.console.last() else {
                return;
            };
            let (icon, color) = console::level_style(record.level);
            let text_color = match record.level {
                Level::Error | Level::Warn => theme::TEXT,
                _ => theme::TEXT_MUTED,
            };
            icons::inline_small(ui, icon, color);
            let first_line = record.message.lines().next().unwrap_or_default().to_string();
            let response = ui.add(
                egui::Label::new(egui::RichText::new(first_line).size(theme::FONT_CAPTION).color(text_color))
                    .truncate()
                    .sense(Sense::click()),
            );
            if response.on_hover_text("Open the Console").clicked() {
                self.actions.push(Action::Layout(LayoutAction::Focus(Panel::Console)));
            }
        });
    }

    // ------------------------------------------------------------------
    // Диалоги
    // ------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        enum Outcome {
            None,
            Cancel,
            Confirm,
            Discard,
        }
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let mut outcome = Outcome::None;
        let scene_name = self.app.world.scene.name.clone();

        let frame = theme::dialog_frame(&ctx.style());
        let response = egui::Modal::new(egui::Id::new("editor_dialog"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(400.0);
                let enter = ui.input(|i| i.key_pressed(EguiKey::Enter));
                let field = |ui: &mut egui::Ui, text: &mut String, hint: &str| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(text)
                            .hint_text(hint)
                            .min_size(vec2(0.0, theme::FIELD_H + 2.0))
                            .vertical_align(egui::Align::Center)
                            .desired_width(f32::INFINITY),
                    );
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    edit.lost_focus() && enter
                };
                let (title, confirm, danger) = match dialog {
                    Dialog::Unsaved { .. } => {
                        dialog_title(ui, Icon::Warn, theme::WARN, "Save changes before closing?");
                        dialog_text(ui, &format!("Scene “{scene_name}” has changes that are not saved. If you don't save, they are lost."));
                        ("Save", "Save", false)
                    }
                    Dialog::NewScript { name, .. } => {
                        let rust = &mut self.script_rust;
                        dialog_title(ui, Icon::Script, theme::TEXT_MUTED, if *rust { "New Rust Script" } else { "New C++ Script" });
                        crate::rust_scripts::language_switch(ui, rust);
                        dialog_text(ui, &crate::rust_scripts::script_dialog_text(*rust, &self.config));
                        if field(ui, name, "EnemyPatrol") {
                            outcome = Outcome::Confirm;
                        }
                        ("", "Create and Build", false)
                    }
                    Dialog::NewScenario { name, folder, .. } => {
                        dialog_title(ui, Icon::Script, theme::TEXT_MUTED, "New Scenario Script");
                        let place = if folder.is_empty() { crate::scenarios::SCENARIO_DIR } else { folder.as_str() };
                        dialog_text(
                            ui,
                            &format!(
                                "Creates “Assets/{place}/<name>.scenario” from a short template and opens it. \
                                 Scenarios need no build: saving the file restarts them, even in Play."
                            ),
                        );
                        if field(ui, name, "intro") {
                            outcome = Outcome::Confirm;
                        }
                        ("", "Create", false)
                    }
                    Dialog::SaveAs { path, .. } => {
                        dialog_title(ui, Icon::Save, theme::TEXT_MUTED, "Save Scene As");
                        dialog_text(ui, "Path inside the assets directory. `.scene` is added if missing.");
                        if field(ui, path, "scenes/level.scene") {
                            outcome = Outcome::Confirm;
                        }
                        ("", "Save", false)
                    }
                    Dialog::NewFolder { parent, name } => {
                        dialog_title(ui, Icon::Folder, theme::TEXT_MUTED, "New Folder");
                        let place = if parent.is_empty() { "Assets".to_string() } else { format!("Assets/{parent}") };
                        dialog_text(ui, &format!("Created inside “{place}”."));
                        if field(ui, name, "Folder name") {
                            outcome = Outcome::Confirm;
                        }
                        ("", "Create", false)
                    }
                };
                ui.add_space(theme::SP_L);
                ui.horizontal(|ui| {
                    if !title.is_empty() && theme::secondary_button(ui, "Don't Save").clicked() {
                        outcome = Outcome::Discard;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirmed = if danger {
                            theme::danger_button(ui, confirm)
                        } else {
                            theme::primary_button(ui, confirm)
                        };
                        if confirmed.clicked() {
                            outcome = Outcome::Confirm;
                        }
                        if theme::secondary_button(ui, "Cancel").clicked() {
                            outcome = Outcome::Cancel;
                        }
                    });
                });
            });
        if response.should_close() && matches!(outcome, Outcome::None) {
            outcome = Outcome::Cancel;
        }

        match (outcome, self.dialog.take()) {
            (Outcome::None, dialog) => self.dialog = dialog,
            (Outcome::Cancel, _) | (_, None) => {}
            (Outcome::Discard, Some(Dialog::Unsaved { then })) => {
                self.dirty = false;
                self.apply(*then);
            }
            (Outcome::Confirm, Some(Dialog::Unsaved { then })) => {
                if self.scene_path.is_some() {
                    self.apply(Action::Save);
                    if !self.dirty {
                        self.apply(*then);
                    }
                } else {
                    // Имени файла ещё нет — сначала спросим его.
                    self.dialog = Some(Dialog::SaveAs {
                        path: self.suggested_scene_path(),
                        then: Some(then),
                    });
                }
            }
            (Outcome::Confirm, Some(Dialog::SaveAs { path, then })) => {
                self.apply(Action::SaveAs(path));
                if let (false, Some(then)) = (self.dirty, then) {
                    self.apply(*then);
                }
            }
            (Outcome::Confirm, Some(Dialog::NewScript { name, object })) => {
                self.apply(if self.script_rust {
                    Action::NewRustScript { name, attach_to: object }
                } else {
                    Action::NewScript { name, attach_to: object }
                });
            }
            (Outcome::Confirm, Some(Dialog::NewScenario { name, folder, object })) => {
                self.apply(Action::NewScenario { name, folder, attach_to: object });
            }
            (Outcome::Confirm, Some(Dialog::NewFolder { parent, name })) => {
                let name = name.trim();
                if !name.is_empty() {
                    let path = if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") };
                    self.apply(Action::CreateFolder(path));
                }
            }
            (Outcome::Discard, Some(dialog)) => self.dialog = Some(dialog),
        }
    }

    /// В режиме Play клавиатура, мышь, колесо, текст и касания над видом Game
    /// принадлежат игре; размер вида Game — размер её «экрана».
    ///
    /// Вызывается в начале кадра, *до* [`Editor::advance`]: игра видит нажатие в том же
    /// кадре, в котором оно пришло от окна, а не кадром позже.
    pub fn forward_game_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        if self.mode != Mode::Playing {
            return;
        }
        // Состояние на конец прошлого кадра: печатает ли пользователь в поле редактора.
        let typing = ctx.wants_keyboard_input();
        // Окно Game (оно может стоять рядом со Scene): мышь — только над ним.
        let rect = self.dock.game.rect;
        let over_game = self.is_panel_visible(crate::dock::Panel::Game);
        let pixels_per_point = ctx.pixels_per_point();
        let pointer = ctx.pointer_latest_pos();
        let input = &mut self.app.world.input;
        let [width, height] = self.dock.game.pixels;
        input.set_screen_size(Vec2::new(width as f32, height as f32));
        let to_game = |position: egui::Pos2| {
            let local = (position - rect.min) * pixels_per_point;
            Vec2::new(local.x, local.y)
        };

        for event in &raw.events {
            match event {
                egui::Event::WindowFocused(false) => input.release_all(),
                egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    modifiers,
                    ..
                } => {
                    // Отпускание доставляем всегда, чтобы клавиша не «залипла».
                    if *pressed && (typing || modifiers.command) {
                        continue;
                    }
                    if let Some(key) = map_key(*key) {
                        input.set_key(key, *pressed);
                    }
                }
                egui::Event::Text(text) if over_game && !typing => input.add_text(text),
                egui::Event::PointerMoved(position) if over_game && rect.contains(*position) => {
                    input.set_mouse_position(to_game(*position));
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    let button = match button {
                        egui::PointerButton::Primary => MouseButton::Left,
                        egui::PointerButton::Secondary => MouseButton::Right,
                        egui::PointerButton::Middle => MouseButton::Middle,
                        _ => continue,
                    };
                    // Отпускание — всегда (кнопку могли отпустить за краем вида).
                    if !*pressed || (over_game && rect.contains(*pos)) {
                        if *pressed {
                            input.set_mouse_position(to_game(*pos));
                        }
                        input.set_mouse_button(button, *pressed);
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. }
                    if over_game && pointer.is_some_and(|p| rect.contains(p)) =>
                {
                    // Игра считает прокрутку в «строках», как Unity.
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => *delta,
                        egui::MouseWheelUnit::Point => *delta / 40.0,
                        egui::MouseWheelUnit::Page => *delta * 10.0,
                    };
                    input.add_scroll(Vec2::new(lines.x, lines.y));
                }
                egui::Event::Touch { id, phase, pos, .. } => {
                    let phase = match phase {
                        egui::TouchPhase::Start => TouchPhase::Began,
                        egui::TouchPhase::Move => TouchPhase::Moved,
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => TouchPhase::Ended,
                    };
                    if phase == TouchPhase::Ended || (over_game && rect.contains(*pos)) {
                        input.set_touch(id.0, to_game(*pos), phase);
                    }
                }
                _ => {}
            }
        }
        if !typing {
            input.set_key(Key::LShift, raw.modifiers.shift);
            input.set_key(Key::LAlt, raw.modifiers.alt);
        }
    }
}

/// Знак движка в начале строки меню.
fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
    paint_mark(ui.painter(), rect);
}

/// Значок движка (`assets/branding/icon.svg`, `tools/gen_icon.py`): белая «P» и янтарная
/// искра на синей плитке. Те же доли, что у файла значка, поэтому узнаётся везде.
fn paint_mark(painter: &egui::Painter, rect: egui::Rect) {
    let tile = Color32::from_rgb(47, 84, 235);
    let at = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    let block = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(at(x0, y0), at(x1, y1));
    let r = rect.width();
    painter.rect_filled(rect, 0.21 * r, tile);
    painter.rect_filled(block(0.46, 0.19, 0.52, 0.57), 0.0, Color32::WHITE); // верх чаши
    painter.rect_filled(block(0.29, 0.19, 0.46, 0.81), 0.0, Color32::WHITE); // ножка
    painter.circle_filled(at(0.52, 0.38), 0.19 * r, Color32::WHITE); // чаша
    painter.rect_filled(block(0.46, 0.31, 0.53, 0.45), 0.0, tile); // просвет чаши
    painter.circle_filled(at(0.53, 0.38), 0.07 * r, tile);
    painter.rect_filled(block(0.58, 0.64, 0.72, 0.78), 0.0, Color32::from_rgb(255, 176, 32));
}

/// Группа кнопок: стоят встык, без подложки (группы разделяет `toolbar_separator`).
fn group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        add(ui);
    });
}

fn dialog_title(ui: &mut egui::Ui, icon: Icon, color: Color32, title: &str) {
    theme::dialog_title(ui, icon, color, title);
}

/// Тонкая вертикальная черта между полями строки состояния.
fn status_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, theme::STATUS_BAR - 8.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, theme::LINE_SOFT));
}

fn dialog_text(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
    ui.add_space(theme::SP_M);
}

pub(crate) fn map_key(key: EguiKey) -> Option<Key> {
    Some(match key {
        EguiKey::A => Key::A,
        EguiKey::B => Key::B,
        EguiKey::C => Key::C,
        EguiKey::D => Key::D,
        EguiKey::E => Key::E,
        EguiKey::F => Key::F,
        EguiKey::G => Key::G,
        EguiKey::H => Key::H,
        EguiKey::I => Key::I,
        EguiKey::J => Key::J,
        EguiKey::K => Key::K,
        EguiKey::L => Key::L,
        EguiKey::M => Key::M,
        EguiKey::N => Key::N,
        EguiKey::O => Key::O,
        EguiKey::P => Key::P,
        EguiKey::Q => Key::Q,
        EguiKey::R => Key::R,
        EguiKey::S => Key::S,
        EguiKey::T => Key::T,
        EguiKey::U => Key::U,
        EguiKey::V => Key::V,
        EguiKey::W => Key::W,
        EguiKey::X => Key::X,
        EguiKey::Y => Key::Y,
        EguiKey::Z => Key::Z,
        EguiKey::Num0 => Key::Digit0,
        EguiKey::Num1 => Key::Digit1,
        EguiKey::Num2 => Key::Digit2,
        EguiKey::Num3 => Key::Digit3,
        EguiKey::Num4 => Key::Digit4,
        EguiKey::Num5 => Key::Digit5,
        EguiKey::Num6 => Key::Digit6,
        EguiKey::Num7 => Key::Digit7,
        EguiKey::Num8 => Key::Digit8,
        EguiKey::Num9 => Key::Digit9,
        EguiKey::ArrowLeft => Key::Left,
        EguiKey::ArrowRight => Key::Right,
        EguiKey::ArrowUp => Key::Up,
        EguiKey::ArrowDown => Key::Down,
        EguiKey::Space => Key::Space,
        EguiKey::Enter => Key::Enter,
        EguiKey::Escape => Key::Escape,
        EguiKey::Tab => Key::Tab,
        EguiKey::Backspace => Key::Backspace,
        EguiKey::Delete => Key::Delete,
        EguiKey::Insert => Key::Insert,
        EguiKey::Home => Key::Home,
        EguiKey::End => Key::End,
        EguiKey::PageUp => Key::PageUp,
        EguiKey::PageDown => Key::PageDown,
        EguiKey::F1 => Key::F1,
        EguiKey::F2 => Key::F2,
        EguiKey::F3 => Key::F3,
        EguiKey::F4 => Key::F4,
        EguiKey::F5 => Key::F5,
        EguiKey::F6 => Key::F6,
        EguiKey::F7 => Key::F7,
        EguiKey::F8 => Key::F8,
        EguiKey::F9 => Key::F9,
        EguiKey::F10 => Key::F10,
        EguiKey::F11 => Key::F11,
        EguiKey::F12 => Key::F12,
        _ => return None,
    })
}
