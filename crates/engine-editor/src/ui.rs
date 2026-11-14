//! Кадр интерфейса: меню, панель инструментов, раскладка окон, диалоги,
//! окно Preferences, горячие клавиши и ввод для игры в режиме Play.

use crate::i18n::{tr_ctx_s, tr_s, trf};
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
        self.themed_frames += 1;
        self.restore_window_places(ctx);
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
        self.scene_picker_window(ctx);
        self.undo_history_window(ctx);
        self.new_scene_window(ctx);
        self.save_layout_dialog(ctx);
        crate::rust_scripts::script_file_dialog(self, ctx);
        self.preferences_window(ctx);
        self.project_settings_window(ctx);
        self.export_window(ctx);
        self.project_file_window(ctx);
        self.help_windows(ctx);
        self.build_tools_window(ctx);
        self.splash_ui(ctx);
        self.apply_queued();
        crate::editor_audio::apply(self);
        self.remember_window_places(ctx);

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
        // Полёт по виду Scene кончается, как только отпустили правую кнопку.
        if !ctx.input(|i| i.pointer.secondary_down()) {
            self.viewport.flying = false;
        }
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
            ui.menu_button(tr_s("File"), |ui| {
                theme::menu_body(ui);
                self.file_menu(ui);
            });
            ui.menu_button(tr_ctx_s("menu", "Edit"), |ui| {
                theme::menu_body(ui);
                self.edit_menu(ui);
            });
            ui.menu_button(tr_s("GameObject"), |ui| {
                theme::menu_body(ui);
                self.gameobject_menu(ui, None);
            });
            ui.menu_button(tr_s("View"), |ui| {
                theme::menu_body(ui);
                self.view_menu(ui);
            });
            ui.menu_button(tr_s("Assets"), |ui| {
                theme::menu_body(ui);
                self.assets_menu(ui);
            });
            ui.menu_button(tr_s("Tools"), |ui| {
                theme::menu_body(ui);
                self.tools_menu(ui);
            });
            ui.menu_button(tr_s("Window"), |ui| {
                theme::menu_body(ui);
                self.window_menu(ui);
            });
            ui.menu_button(tr_s("Help"), |ui| {
                theme::menu_body(ui);
                self.help_menu(ui);
            });
        });
    }

    /// Меню Window: окна (открыть / показать), раскладки.
    fn window_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        theme::menu_section(ui, tr_ctx_s("menu", "Windows"));
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
        theme::submenu(ui, Icon::Tiles, tr_s("Layouts"), |ui| {
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
            if item(tr_s("Save Layout…")).icon(Icon::Save).show(ui) {
                self.dock.save_dialog = Some(String::new());
            }
            if saved.is_empty() {
                item(tr_s("Delete Layout"))
                    .icon(Icon::Trash)
                    .enabled_if(false, tr_s("No saved layouts yet: save one with Save Layout…"))
                    .show(ui);
            } else {
                theme::submenu(ui, Icon::Trash, tr_s("Delete Layout"), |ui| {
                    for name in &saved {
                        if item(name).show(ui) {
                            self.actions.push(Action::Layout(LayoutAction::Delete(name.clone())));
                        }
                    }
                });
            }
            theme::menu_separator(ui);
            if item(tr_s("Reset Layout")).icon(Icon::Reset).show(ui) {
                self.actions.push(Action::Layout(LayoutAction::Reset));
            }
        });
        if let Some(panel) = self.maximized_panel() {
            theme::menu_separator(ui);
            if item(&trf!("Restore {}", panel.title())).icon(Icon::Collapse).shortcut(&k.text("maximize-panel")).show(ui) {
                self.actions.push(Action::Layout(LayoutAction::ToggleMaximize(panel)));
            }
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let editing = self.mode == Mode::Edit;
        let k = self.prefs.shortcuts.clone();
        let stop_first = k.tip(tr_s("Stop the game first"), "play");
        if item(tr_s("New Scene…")).icon(Icon::Scene).shortcut(&k.text("new-scene")).show(ui) {
            self.show_new_scene_dialog(true);
        }
        // Open Scene… (окно со списком и поиском) и Open Recent Scene ▸ — scene_picker.rs.
        self.scene_menu_items(ui);
        theme::menu_separator(ui);
        if item(tr_s("Save"))
            .icon(Icon::Save)
            .shortcut(&k.text("save"))
            .enabled_if(editing, &stop_first)
            .show(ui)
        {
            self.actions.push(Action::Save);
        }
        if item(tr_s("Save As…"))
            .shortcut(&k.text("save-as"))
            .enabled_if(editing, &stop_first)
            .show(ui)
        {
            self.open_save_as();
        }
        theme::menu_separator(ui);
        if item(tr_s("Build…"))
            .icon(Icon::Build)
            .enabled_if(
                self.config.game_crate.is_some(),
                "Start the editor with EditorConfig::game_crate(\"<name>\") to export a game from games/<name>",
            )
            .show(ui)
        {
            self.actions.push(Action::ShowBuildDialog(true));
        }
        self.project_file_menu_item(ui);
        theme::menu_separator(ui);
        if item(tr_s("Project Hub…")).icon(Icon::Folder).show(ui) {
            self.actions.push(Action::ProjectHub);
        }
        let recent = crate::hub_link::recent_projects();
        if !recent.is_empty() {
            theme::submenu(ui, Icon::FolderOpen, tr_s("Open Recent"), |ui| {
                for (name, dir) in &recent {
                    if item(name).icon(Icon::Folder).show(ui) {
                        self.actions.push(Action::OpenProject(dir.clone()));
                    }
                }
            });
        }
        theme::menu_separator(ui);
        if item(tr_s("Preferences…")).icon(Icon::Gear).shortcut(&k.text("preferences")).show(ui) {
            self.preferences_open = true;
        }
        theme::menu_separator(ui);
        if item(tr_s("Quit")).icon(Icon::Close).shortcut(&k.text("quit")).show(ui) {
            self.actions.push(Action::Quit);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        let editing = self.mode == Mode::Edit;
        let selection = self.selection;
        let history_reason = if editing { tr_s("Nothing to undo") } else { tr_s("Undo is not available in Play mode") };
        if item(tr_s("Undo"))
            .icon(Icon::Undo)
            .shortcut(&k.text("undo"))
            .enabled_if(self.can_undo(), history_reason)
            .show(ui)
        {
            self.actions.push(Action::Undo);
        }
        let redo_reason = if editing { tr_s("Nothing to redo") } else { tr_s("Redo is not available in Play mode") };
        if item(tr_s("Redo"))
            .icon(Icon::Redo)
            .shortcut(&k.text("redo"))
            .enabled_if(self.can_redo(), redo_reason)
            .show(ui)
        {
            self.actions.push(Action::Redo);
        }
        if item(tr_s("Undo History")).icon(Icon::Clock).checked(self.undo_history_open).show(ui) {
            self.undo_history_open = !self.undo_history_open;
        }
        theme::menu_separator(ui);
        let nothing = tr_s("Select an object first");
        if item(tr_s("Duplicate"))
            .icon(Icon::Copy)
            .shortcut(&k.text("duplicate"))
            .enabled_if(selection.is_some(), nothing)
            .show(ui)
        {
            self.actions.extend(selection.map(Action::Duplicate));
        }
        if item(tr_s("Rename"))
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
        if item(tr_s("Delete"))
            .icon(Icon::Trash)
            .shortcut(&k.text("delete"))
            .enabled_if(selection.is_some(), nothing)
            .show(ui)
        {
            self.actions.extend(selection.map(Action::Delete));
        }
        theme::menu_separator(ui);
        if item(tr_s("Frame Selected")).icon(Icon::Search).shortcut(&k.text("frame-selected")).show(ui) {
            self.actions.push(Action::FrameSelected);
        }
        if item(tr_s("Select None"))
            .shortcut(&k.text("select-none"))
            .enabled_if(selection.is_some(), tr_s("Nothing is selected"))
            .show(ui)
        {
            self.actions.push(Action::Select(None));
        }
        theme::menu_separator(ui);
        let (play_label, play_icon) = if editing { (tr_s("Play"), Icon::Play) } else { (tr_s("Stop"), Icon::Stop) };
        if item(play_label).icon(play_icon).shortcut(&k.text("play")).show(ui) {
            self.actions.push(if editing { Action::Play } else { Action::Stop });
        }
        if item(if self.mode == Mode::Paused { tr_s("Resume") } else { tr_s("Pause") })
            .icon(Icon::Pause)
            .shortcut(&k.text("pause"))
            .enabled_if(!editing, tr_s("The game is not running"))
            .show(ui)
        {
            self.actions.push(Action::Pause);
        }
        if item(tr_s("Step")).icon(Icon::Step).shortcut(&k.text("step")).show(ui) {
            self.actions.push(Action::Step);
        }
        theme::menu_separator(ui);
        if item(tr_s("Project Settings…")).icon(Icon::Gear).shortcut(&k.text("project-settings")).show(ui) {
            self.script_order.open = true;
        }
    }

    /// Меню создания объектов. `parent` — для контекстного меню строки иерархии.
    pub(crate) fn gameobject_menu(&mut self, ui: &mut egui::Ui, parent: Option<engine_core::prelude::GameObjectId>) {
        let k = self.prefs.shortcuts.clone();
        let child = parent.is_some();
        if item(if child { tr_s("Create Empty Child") } else { tr_s("Create Empty") })
            .icon(Icon::Object)
            .shortcut(&k.text(if child { "create-empty-child" } else { "create-empty" }))
            .show(ui)
        {
            self.actions.push(Action::CreateEmpty { parent });
        }
        if !child
            && item(tr_s("Create Empty Child"))
                .icon(Icon::Object)
                .shortcut(&k.text("create-empty-child"))
                .enabled_if(self.selection.is_some(), tr_s("Select the parent object first"))
                .show(ui)
        {
            self.actions.push(Action::CreateEmpty { parent: self.selection });
        }
        theme::menu_separator(ui);
        theme::submenu(ui, Icon::Cube, tr_s("3D Object"), |ui| {
            for (shape, icon, label) in [
                (MeshShape::Cube, Icon::Cube, tr_s("Cube")),
                (MeshShape::Sphere, Icon::Sphere, tr_s("Sphere")),
                (MeshShape::Cylinder, Icon::Cylinder, tr_s("Cylinder")),
                (MeshShape::Plane, Icon::Plane, tr_s("Plane")),
            ] {
                if item(label).icon(icon).show(ui) {
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
            if item(if child { tr_s("Sprite (child)") } else { tr_s("Sprite") }).icon(Icon::Sprite).show(ui) {
                self.actions.push(Action::CreateSprite { parent });
            }
            for preset in available.iter().filter(|p| p.menu() == "2D") {
                if item(tr_s(preset.label)).icon(preset.icon).show(ui) {
                    self.actions.push(Action::Create { preset: preset.id.to_string(), parent });
                }
            }
        });
        theme::submenu(ui, Icon::Bulb, tr_s("Light"), |ui| {
            if item(tr_s("Directional Light")).icon(Icon::Sun).show(ui) {
                self.actions.push(Action::CreateLight(LightKind::Directional));
            }
            if item(tr_s("Point Light")).icon(Icon::Bulb).show(ui) {
                self.actions.push(Action::CreateLight(LightKind::Point));
            }
        });
        for (menu, icon) in catalog::PRESET_MENUS.iter().filter(|(menu, _)| *menu != "2D") {
            let entries: Vec<&&catalog::Preset> = available.iter().filter(|p| p.menu() == *menu).collect();
            if entries.is_empty() {
                continue; // крейт с этими компонентами не подключён — пункта нет
            }
            theme::submenu(ui, *icon, tr_s(menu), |ui| {
                for preset in entries {
                    if item(tr_s(preset.label)).icon(preset.icon).show(ui) {
                        self.actions.push(Action::Create { preset: preset.id.to_string(), parent });
                    }
                }
            });
        }
        if !child {
            crate::terrain_tools::snap_menu_item(self, ui);
        }
        if !child && item(tr_s("Camera")).icon(Icon::Camera).show(ui) {
            self.actions.push(Action::CreateCamera);
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        theme::menu_section(ui, tr_s("Scene view"));
        if item(tr_s("3D View")).icon(Icon::Cube).shortcut(&k.text("view-3d")).checked(self.scene_3d).show(ui) {
            let enabled = !self.scene_3d;
            self.set_scene_3d(enabled);
        }
        if item(tr_s("Grid")).icon(Icon::Grid).shortcut(&k.text("view-grid")).checked(self.show_grid).show(ui) {
            self.show_grid = !self.show_grid;
        }
        if item(tr_s("Gizmos")).icon(Icon::Gizmo).shortcut(&k.text("view-gizmos")).checked(self.show_gizmos).show(ui) {
            self.show_gizmos = !self.show_gizmos;
        }
        if item(tr_s("Snap to Grid")).icon(Icon::Magnet).shortcut(&k.text("toggle-snap")).checked(self.snap).show(ui) {
            self.snap = !self.snap;
        }
        theme::menu_separator(ui);
        theme::submenu(ui, Icon::Search, tr_s("UI Scale"), |ui| {
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
        if item(tr_s("Refresh"))
            .icon(Icon::Refresh)
            .shortcut(&k.text("refresh"))
            .enabled_if(mounted, no_assets)
            .show(ui)
        {
            self.actions.push(Action::RefreshAssets);
        }
        if item(tr_s("New Folder…")).icon(Icon::Folder).enabled_if(mounted, no_assets).show(ui) {
            self.dialog = Some(Dialog::NewFolder {
                parent: self.project.folder.clone(),
                name: String::new(),
            });
        }
        if item(tr_s("Import New Asset…")).icon(Icon::ArrowDown).enabled_if(mounted, no_assets).show(ui) {
            self.show_import_browser();
        }
        if item(tr_s("Reveal in File Manager"))
            .icon(Icon::External)
            .enabled_if(mounted, no_assets)
            .show(ui)
        {
            self.actions.push(Action::RevealAsset(self.project.folder.clone()));
        }
        if item(tr_s("New Model (Piska Modeler)")).icon(Icon::Cube).enabled_if(mounted, no_assets).show(ui) {
            self.actions.push(Action::CreateModel { folder: self.project.folder.clone() });
        }
        theme::menu_separator(ui);
        let scripts = self.config.script_dir.is_some();
        if item(tr_s("New Rust Script…")).icon(Icon::Script).show(ui) {
            self.open_script_dialog(true, String::new(), self.selection);
        }
        if item(tr_s("New C++ Script…"))
            .icon(Icon::Script)
            .enabled_if(scripts, "Start the editor with `EditorConfig::cpp_scripts(dir)` to create scripts")
            .show(ui)
        {
            self.open_new_script();
        }
        if item(tr_s("New Scenario Script…")).icon(Icon::Script).show(ui) {
            self.open_scenario_dialog("", self.selection);
        }
        let buildable = self.can_build();
        if item(tr_s("Build Code"))
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
        // Ширина группы — по её содержимому прошлого кадра: группа ровно по центру окна.
        let width = if self.chrome.transport.width() > 1.0 { self.chrome.transport.width() } else { 3.0 * 26.0 + 2.0 };
        let transport = Rect::from_center_size(bar.center(), vec2(width, theme::MAIN_TOOLBAR));
        let mut center = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(transport)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.transport(&mut center);
        self.chrome.toolbar = bar;
        self.chrome.transport = center.min_rect();

        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(theme::SP_M, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 1.0;
        if icons::button(&mut right, Icon::Gear, self.preferences_open, None, &self.prefs.shortcuts.tip(tr_s("Preferences"), "preferences")).clicked() {
            self.preferences_open = !self.preferences_open;
        }
        if icons::button(&mut right, Icon::Folder, false, None, tr_s("Project Hub: open or create another project")).clicked() {
            self.actions.push(Action::ProjectHub);
        }
        theme::toolbar_separator(&mut right);
        self.layout_selector(&mut right);
    }

    /// Левая часть главной панели — по задачам, слева направо: инструмент → его
    /// параметры (точка вращения, оси, привязка). Параметры вида (2D / 3D, сетка, значки,
    /// туман) — на полосе самого вида Scene, а не здесь.
    fn toolbar_left(&mut self, ui: &mut egui::Ui) {
        group(ui, |ui| {
            for (tool, icon, tooltip, command) in [
                (Tool::Move, Icon::Move, tr_s("Move tool"), "tool-move"),
                (Tool::Rotate, Icon::Rotate, tr_s("Rotate tool"), "tool-rotate"),
                (Tool::Scale, Icon::Scale, tr_s("Scale tool"), "tool-scale"),
            ] {
                let tooltip = self.prefs.shortcuts.tip(tooltip, command);
                if icons::button(ui, icon, self.tool == tool, None, &tooltip).clicked() {
                    self.tool = tool;
                }
            }
        });
        theme::toolbar_separator(ui);
        // Подписанные переключатели вместо значков, смысл которых надо угадывать.
        let pivot_tip = self.prefs.shortcuts.tip(
            tr_s("Where the gizmo rotates and scales the selection: the active object's pivot or the centre of the selection"),
            "toggle-pivot",
        );
        let mut center = self.pivot_center;
        if theme::segmented(ui, &mut center, &[(false, tr_s("Pivot"), pivot_tip.as_str()), (true, tr_s("Center"), pivot_tip.as_str())]) {
            self.pivot_center = center;
        }
        ui.add_space(theme::SP_S);
        let space_tip = self.prefs.shortcuts.tip(tr_s("Gizmo axes: world (Global) or the object's own (Local)"), "toggle-local");
        let mut local = self.gizmo_local;
        if theme::segmented(ui, &mut local, &[(false, tr_s("Global"), space_tip.as_str()), (true, tr_s("Local"), space_tip.as_str())]) {
            self.gizmo_local = local;
        }
        ui.add_space(theme::SP_S);
        let snap_tip = trf!(
            "Snap: {} m steps, {}° turns, ×{} scale (Preferences → Scene View). Hold Ctrl to invert",
            self.prefs.snap_move, self.prefs.snap_rotate, self.prefs.snap_scale
        );
        let mut snap = self.snap;
        theme::toggle_button(ui, &mut snap, Icon::Magnet, tr_s("Snap"), &snap_tip);
        self.snap = snap;
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        let playing = self.mode != Mode::Edit;
        group(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            // Play: в режиме игры кнопка «нажата» (акцент), значок — Stop.
            let (rect, response) = ui.allocate_exact_size(vec2(theme::CONTROL_H + theme::SP_3, theme::CONTROL_H), Sense::click());
            let fill = if playing {
                theme::accent()
            } else if response.hovered() {
                theme::BG_HOVER
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, theme::R_S, fill);
            let (icon, color, tooltip) = if playing {
                (Icon::Stop, theme::TEXT_STRONG, self.prefs.shortcuts.tip(tr_s("Stop and restore the scene"), "play"))
            } else {
                (Icon::Play, theme::TEXT, self.prefs.shortcuts.tip(tr_s("Play"), "play"))
            };
            icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(13.0, 13.0)), icon, color);
            if response.on_hover_text(tooltip).clicked() {
                self.actions.push(if playing { Action::Stop } else { Action::Play });
            }
            ui.add_enabled_ui(playing, |ui| {
                let tooltip = self.prefs.shortcuts.tip(if self.mode == Mode::Paused { tr_s("Resume") } else { tr_s("Pause") }, "pause");
                if icons::button(ui, Icon::Pause, self.mode == Mode::Paused, None, &tooltip).clicked() {
                    self.actions.push(Action::Pause);
                }
            });
            if icons::button(ui, Icon::Step, false, None, &self.prefs.shortcuts.tip(tr_s("Advance one frame and pause"), "step")).clicked() {
                self.actions.push(Action::Step);
            }
        });
    }

    fn stats_readout(&mut self, ui: &mut egui::Ui) {
        let fps = self.profiler.fps();
        let text = if !self.wants_continuous_redraw() {
            trf!("idle  {} ms", format!("{:>5.2}", self.profiler.recent_work_ms()))
        } else if fps > 0.0 {
            trf!("{fps} fps  {p0} ms", p0 = format!("{:>5.2}", self.profiler.recent_frame_ms()), fps = format!("{:>4.0}", fps))
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
            .on_hover_text(trf!(
                "Editor frame rate. The window redraws {redraw}. Click to open the Profiler"
            , redraw = redraw))
            .clicked()
        {
            self.actions.push(Action::Layout(LayoutAction::Focus(Panel::Profiler)));
        }
    }

    /// Выбор раскладки окон справа на панели инструментов (как Layout в Unity).
    fn layout_selector(&mut self, ui: &mut egui::Ui) {
        let saved = self.saved_layouts();
        let popup = egui::Id::new("toolbar_layouts");
        let galley = ui.painter().layout_no_wrap(tr_s("Layout").to_string(), theme::body(), theme::TEXT);
        let size = vec2(galley.size().x + theme::SP_M * 2.0 + theme::ICON_S, theme::CONTROL_H);
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
        let response = response.on_hover_text(tr_s("Window layouts"));
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
            if item(tr_s("Save Layout…")).icon(Icon::Save).show(ui) {
                self.dock.save_dialog = Some(String::new());
            }
            if item(tr_s("Reset Layout")).icon(Icon::Reset).show(ui) {
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
            Mode::Edit => (tr_ctx_s("state", "Edit"), theme::TEXT_DISABLED),
            Mode::Playing => (tr_s("Playing"), theme::mix(theme::accent(), Color32::WHITE, 0.45)),
            Mode::Paused => (tr_s("Paused"), theme::WARN),
        };
        ui.label(small(mode.to_string(), mode_color));
        // У кого ввод в Play: у игры (окно Game в фокусе) или у редактора.
        if self.mode == Mode::Playing {
            let (text, tip) = if self.game_has_input_focus() {
                (tr_s("Game input"), tr_s("Keys, mouse and gamepads go to the game. Shift+Esc or a click on another window gives them back to the editor."))
            } else {
                (tr_s("Editor input"), tr_s("The game gets no input now. Click the Game view to give it keys, mouse and gamepads."))
            };
            ui.label(small(text.to_string(), theme::TEXT_MUTED)).on_hover_text(tip);
        }
        // Частота кадров редактора (щелчок — Profiler): с главной панели переехала сюда.
        self.stats_readout(ui);
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
            .map_or(tr_s("not saved yet").to_string(), |path| format!("Assets/{path}"));
        ui.label(small(file, theme::TEXT_DISABLED));
        let scene = format!("{}{}", self.app.world.scene.name, if self.dirty { "*" } else { "" });
        ui.label(small(scene, theme::TEXT_MUTED)).on_hover_text(if self.dirty {
            self.prefs.shortcuts.tip(tr_s("The scene has unsaved changes"), "save")
        } else {
            tr_s("The scene is saved").to_string()
        });
        if self.build_status() == crate::code::BuildStatus::Running {
            status_divider(ui);
            let step = Self::export_step().unwrap_or_else(|| tr_s("Building…").to_string());
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
            if response.on_hover_text(tr_s("Open the Console")).clicked() {
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

        let response = crate::window_kit::dialog(egui::Id::new("editor_dialog"), "")
            .width(0.0)
            .show(ctx, |ui| {
                ui.set_width(400.0);
                let enter = ui.input(|i| i.key_pressed(EguiKey::Enter));
                let field = |ui: &mut egui::Ui, text: &mut String, hint: &str| {
                    let edit = crate::controls::text_field(ui, text, hint, f32::INFINITY);
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    edit.lost_focus() && enter
                };
                let (title, confirm, danger) = match dialog {
                    Dialog::Unsaved { .. } => {
                        dialog_title(ui, Icon::Warn, theme::WARN, tr_s("Save changes before closing?"));
                        dialog_text(ui, &trf!("Scene “{scene_name}” has changes that are not saved. If you don't save, they are lost.", scene_name = scene_name));
                        (tr_s("Save"), tr_s("Save"), false)
                    }
                    Dialog::NewScript { name, .. } => {
                        let rust = &mut self.script_rust;
                        dialog_title(ui, Icon::Script, theme::TEXT_MUTED, if *rust { tr_s("New Rust Script") } else { tr_s("New C++ Script") });
                        crate::rust_scripts::language_switch(ui, rust);
                        dialog_text(ui, &crate::rust_scripts::script_dialog_text(*rust, &self.config));
                        if field(ui, name, "EnemyPatrol") {
                            outcome = Outcome::Confirm;
                        }
                        ("", tr_s("Create and Build"), false)
                    }
                    Dialog::NewScenario { name, folder, .. } => {
                        dialog_title(ui, Icon::Script, theme::TEXT_MUTED, tr_s("New Scenario Script"));
                        let place = if folder.is_empty() { crate::scenarios::SCENARIO_DIR } else { folder.as_str() };
                        dialog_text(
                            ui,
                            &trf!(
                                "Creates “Assets/{place}/<name>.scenario” from a short template and opens it. \
                                 Scenarios need no build: saving the file restarts them, even in Play."
                            , place = place),
                        );
                        if field(ui, name, "intro") {
                            outcome = Outcome::Confirm;
                        }
                        ("", tr_s("Create"), false)
                    }
                    Dialog::SaveAs { path, .. } => {
                        dialog_title(ui, Icon::Save, theme::TEXT_MUTED, tr_s("Save Scene As"));
                        dialog_text(ui, tr_s("Path inside the assets directory. `.scene` is added if missing."));
                        if field(ui, path, "scenes/level.scene") {
                            outcome = Outcome::Confirm;
                        }
                        ("", tr_s("Save"), false)
                    }
                    Dialog::NewFolder { parent, name } => {
                        dialog_title(ui, Icon::Folder, theme::TEXT_MUTED, "New Folder");
                        let place = if parent.is_empty() { tr_s("Assets").to_string() } else { format!("Assets/{parent}") };
                        dialog_text(ui, &trf!("Created inside “{place}”.", place = place));
                        if field(ui, name, tr_s("Folder name")) {
                            outcome = Outcome::Confirm;
                        }
                        ("", tr_s("Create"), false)
                    }
                };
                // Общая строка кнопок: главная справа, Cancel левее, «Don't Save» — у левого края.
                crate::window_kit::dialog_actions(ui, |ui| {
                    let confirmed = if danger {
                        theme::danger_button(ui, confirm)
                    } else {
                        crate::window_kit::primary_action(ui, confirm)
                    };
                    if confirmed.clicked() {
                        outcome = Outcome::Confirm;
                    }
                    if crate::window_kit::action_button(ui, tr_s("Cancel")).clicked() {
                        outcome = Outcome::Cancel;
                    }
                    if !title.is_empty() {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if crate::window_kit::action_button(ui, tr_s("Don't Save")).clicked() {
                                outcome = Outcome::Discard;
                            }
                        });
                    }
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
    ///
    /// Как в Unity, ввод получает игра, только пока фокус у окна Game (по нему щёлкнули,
    /// или Play сам дал ему фокус — Preferences → Game View → Focus Game View On Play).
    /// Щелчок по другому окну, Shift+Esc, потеря фокуса окном редактора, Pause и Stop
    /// отдают ввод редактору: всё нажатое отпускается, геймпады замолкают
    /// ([`engine_core::input::Input::set_gamepads_suspended`]) — ничего не «залипает».
    pub fn forward_game_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        self.route_game_input(ctx, raw);
        self.sync_cursor_capture(ctx);
    }

    /// Отпустить всё нажатое у игры, но не её просьбу о захвате курсора: вернув фокус
    /// виду Game (щелчком), игрок снова получает захваченную мышь.
    fn release_game_input(&mut self) {
        let input = &mut self.app.world.input;
        let requested = input.cursor_capture_requested();
        input.release_all();
        input.set_cursor_captured(requested);
    }

    /// Захват курсора (`Input::set_cursor_captured` — просьба игры): даётся, только пока идёт
    /// Play, ввод у игры и вид Game не закрыт меню, всплывающим окном или диалогом.
    /// Итог — в `Input::set_cursor_capture_state`; окно редактора (runner.rs) запирает и
    /// прячет курсор системы по `take_cursor_request`.
    fn sync_cursor_capture(&mut self, ctx: &egui::Context) {
        let covered = ctx.memory(|m| m.any_popup_open())
            || self.dialog.is_some()
            || self.scene_picker.open
            || self.scene_picker.new_scene
            || self.dock.save_dialog.is_some()
            || self.project.modal_open()
            || self.preferences_open
            || self.settings_modal_open();
        let input = &mut self.app.world.input;
        let want = self.mode == Mode::Playing && self.dock.game.input_focus && input.cursor_capture_requested() && !covered;
        if want != input.is_cursor_captured() {
            if want {
                // Движение указателя до захвата (щелчок, который его дал) — не поворот взгляда.
                let stale = input.mouse_delta();
                input.add_mouse_delta(-stale);
            }
            input.set_cursor_capture_state(want);
            self.dock.game.cursor_request = Some(want);
        }
    }

    /// Курсор захвачен для игры: чем отпустить (подсказка у вида Game).
    pub fn game_cursor_captured(&self) -> bool {
        self.mode == Mode::Playing && self.app.world.input.is_cursor_captured()
    }

    /// Сырое движение мыши от системы (`DeviceEvent::MouseMotion`): игре — только пока
    /// курсор захвачен (иначе движение приходит позицией курсора).
    pub fn add_raw_mouse_motion(&mut self, dx: f32, dy: f32) {
        if self.game_cursor_captured() {
            self.app.world.input.add_mouse_delta(Vec2::new(dx, dy));
        }
    }

    /// Что сделать с курсором системы: `Some(true)` — запереть и спрятать, `Some(false)` —
    /// вернуть. Забирает окно редактора после кадра.
    pub(crate) fn take_cursor_request(&mut self) -> Option<bool> {
        self.dock.game.cursor_request.take()
    }

    /// Середина вида Game (точки egui): туда окно возвращает запертый курсор, если система
    /// умеет только «держать в окне» (Windows), чтобы щелчки не уходили в другие окна.
    pub(crate) fn game_view_center(&self) -> egui::Pos2 {
        self.dock.game.rect.center()
    }

    fn route_game_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        for event in &raw.events {
            if let egui::Event::WindowFocused(focused) = event {
                self.dock.game.window_focused = *focused;
            }
        }
        let rect = self.dock.game.rect;
        let game_layer = self.dock.game.layer;
        let visible = self.is_panel_visible(crate::dock::Panel::Game);
        // Точка над видом Game, и сверху не лежит меню, всплывающее или плавающее окно.
        let on_game = |position: egui::Pos2| {
            visible && rect.contains(position) && ctx.layer_id_at(position).is_none_or(|layer| layer == game_layer)
        };
        if self.mode == Mode::Playing {
            // Щелчок по виду Game сразу отдаёт ему фокус: этот же щелчок уже игровой.
            let grab = raw.events.iter().any(|event| {
                matches!(event, egui::Event::PointerButton { pos, pressed: true, .. } if on_game(*pos))
                    || matches!(event, egui::Event::Touch { pos, phase: egui::TouchPhase::Start, .. } if on_game(*pos))
            });
            if grab {
                self.dock.game.released = false;
                if self.focused_panel() != Some(crate::dock::Panel::Game) {
                    self.focus_panel(crate::dock::Panel::Game);
                }
            }
        }
        // Фокус вернулся на Game другим путём (вкладка, Ctrl+2) — Shift+Esc больше не действует.
        let panel = self.focused_panel();
        if panel == Some(crate::dock::Panel::Game) && self.dock.game.last_panel != panel {
            self.dock.game.released = false;
        }
        self.dock.game.last_panel = panel;
        let focus = self.game_input_focus_now();
        if focus != self.dock.game.input_focus {
            self.dock.game.input_focus = focus;
            if !focus {
                self.release_game_input();
            }
        }
        self.app.world.input.set_gamepads_suspended(!focus);
        if !focus || self.mode != Mode::Playing {
            return;
        }
        // Состояние на конец прошлого кадра: печатает ли пользователь в поле редактора.
        let typing = ctx.wants_keyboard_input();
        // Shift+Esc — вернуть клавиатуру и мышь редактору (Esc без Shift остаётся игре).
        let release = raw.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: EguiKey::Escape, pressed: true, modifiers, .. } if modifiers.shift)
        });
        if release {
            self.dock.game.released = true;
            self.dock.game.input_focus = false;
            self.release_game_input();
            self.app.world.input.set_gamepads_suspended(true);
            return;
        }
        let over_game = visible;
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
                egui::Event::PointerMoved(position) if on_game(*position) => {
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
                    if !*pressed || on_game(*pos) {
                        if *pressed {
                            input.set_mouse_position(to_game(*pos));
                        }
                        input.set_mouse_button(button, *pressed);
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. }
                    if pointer.is_some_and(on_game) =>
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
                    if phase == TouchPhase::Ended || on_game(*pos) {
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

    /// Ввод сейчас у игры: идёт Play (не пауза), фокус у видимого окна Game, окно
    /// редактора в фокусе системы и ввод не отдан редактору Shift+Esc.
    fn game_input_focus_now(&self) -> bool {
        self.mode == Mode::Playing
            && self.dock.game.window_focused
            && !self.dock.game.released
            && self.focused_panel() == Some(crate::dock::Panel::Game)
            && self.is_panel_visible(crate::dock::Panel::Game)
    }

    /// Клавиатура, мышь, касания и геймпады идут игре (вид Game в фокусе, режим Play).
    /// В остальное время игра видит «ничего не нажато», а редактор — свои клавиши.
    pub fn game_has_input_focus(&self) -> bool {
        self.dock.game.input_focus
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
    paint_mark_faded(painter, rect, 1.0);
}

/// Значок движка с прозрачностью `alpha` (заставка растворяется вместе с ним).
pub(crate) fn paint_mark_faded(painter: &egui::Painter, rect: egui::Rect, alpha: f32) {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let tile = fade(Color32::from_rgb(47, 84, 235));
    let at = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    let block = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(at(x0, y0), at(x1, y1));
    let r = rect.width();
    painter.rect_filled(rect, 0.21 * r, tile);
    let white = fade(Color32::WHITE);
    painter.rect_filled(block(0.46, 0.19, 0.52, 0.57), 0.0, white); // верх чаши
    painter.rect_filled(block(0.29, 0.19, 0.46, 0.81), 0.0, white); // ножка
    painter.circle_filled(at(0.52, 0.38), 0.19 * r, white); // чаша
    painter.rect_filled(block(0.46, 0.31, 0.53, 0.45), 0.0, tile); // просвет чаши
    painter.circle_filled(at(0.53, 0.38), 0.07 * r, tile);
    painter.rect_filled(block(0.58, 0.64, 0.72, 0.78), 0.0, fade(Color32::from_rgb(255, 176, 32)));
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
