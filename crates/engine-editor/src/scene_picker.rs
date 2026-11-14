//! Выбор сцены: File → Open Scene… (Ctrl+O) — окно со списком всех `.scene` проекта и
//! поиском; File → Open Recent Scene (последние 10, по проекту); File → New Scene…
//! (Ctrl+N, 2D или 3D); переключатель сцен в строке сцены окна Hierarchy.
//!
//! Открытие идёт через [`Action::OpenScene`]: несохранённая сцена сначала спрашивает
//! Save / Don't Save / Cancel, идущая игра останавливается.

use crate::i18n::{tr_s, trf};
use std::time::SystemTime;

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::icons::{self, Icon};
use crate::state::{Action, Editor};
use crate::theme;

/// Сколько недавних сцен помнится на проект.
pub(crate) const RECENT_MAX: usize = 10;
/// Сколько проектов помнят недавние сцены.
const RECENT_PROJECTS: usize = 24;

/// Окна Open Scene… и New Scene….
#[derive(Default)]
pub(crate) struct ScenePicker {
    pub open: bool,
    pub query: String,
    /// Строка под курсором клавиатуры (номер среди найденных).
    pub cursor: usize,
    /// В прошлом кадре окно было закрыто: при открытии — фокус в поиск.
    shown: bool,
    /// Окно New Scene… открыто; `three_d` — выбранная заготовка.
    pub new_scene: bool,
    pub three_d: bool,
}

/// Сцена в списке выбора.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneEntry {
    /// Путь относительно каталога ассетов (`scenes/main.scene`).
    pub path: String,
    /// Имя файла без `.scene`.
    pub name: String,
    /// Каталог (`scenes/courses`), пусто — корень ассетов.
    pub folder: String,
    /// Стартовая сцена проекта (Project Settings → Player → Default scene или `scenes/main.scene`).
    pub start: bool,
    pub modified: Option<SystemTime>,
}

/// Совпадение строки поиска: все символы запроса по порядку («нечёткий» поиск, как
/// Ctrl+P в редакторах кода). Меньше — лучше; `None` — не подходит.
pub(crate) fn fuzzy_score(text: &str, query: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_lowercase();
    if let Some(at) = text.find(&query) {
        // Подстрока: в начале — лучше всего, затем по месту.
        return Some(at);
    }
    let mut chars = text.char_indices();
    let mut first = None;
    let mut last = 0;
    for wanted in query.chars().filter(|c| !c.is_whitespace()) {
        let (at, _) = chars.by_ref().find(|(_, c)| *c == wanted)?;
        first.get_or_insert(at);
        last = at;
    }
    // Разбросанные буквы хуже подстроки: штраф — длина охвата.
    Some(1000 + last - first.unwrap_or(0))
}

impl Editor {
    /// Ключ проекта для недавних сцен: каталог ассетов.
    fn recent_key(&self) -> Option<String> {
        self.assets_root().map(|root| root.display().to_string())
    }

    /// Стартовая сцена проекта: Project Settings → Player → Default scene, иначе
    /// `scenes/main.scene`, если она есть.
    pub fn start_scene(&self) -> Option<String> {
        let from_settings = self
            .app
            .world
            .resources
            .get::<engine_core::settings::ProjectSettings>()
            .map(|s| s.player.default_scene.trim().replace('\\', "/"))
            .filter(|s| !s.is_empty());
        let scenes = || self.assets.iter().filter(|a| a.importer == "Scene");
        from_settings
            .filter(|path| scenes().any(|a| a.path == *path))
            .or_else(|| scenes().find(|a| a.path == "scenes/main.scene").map(|a| a.path.clone()))
    }

    /// Все сцены проекта: стартовая первой, остальные по пути (без учёта регистра).
    pub fn project_scenes(&self) -> Vec<SceneEntry> {
        let start = self.start_scene();
        let root = self.assets_root();
        let mut scenes: Vec<SceneEntry> = self
            .assets
            .iter()
            .filter(|asset| asset.importer == "Scene")
            .map(|asset| {
                let (folder, file) = asset.path.rsplit_once('/').unwrap_or(("", asset.path.as_str()));
                SceneEntry {
                    path: asset.path.clone(),
                    name: file.trim_end_matches(".scene").to_string(),
                    folder: folder.to_string(),
                    start: start.as_deref() == Some(asset.path.as_str()),
                    modified: root
                        .as_ref()
                        .and_then(|root| std::fs::metadata(root.join(&asset.path)).ok())
                        .and_then(|meta| meta.modified().ok()),
                }
            })
            .collect();
        scenes.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase())));
        scenes
    }

    /// Сцены окна Open Scene… под строкой поиска, лучшие совпадения первыми.
    pub fn scene_picker_matches(&self) -> Vec<SceneEntry> {
        let query = self.scene_picker.query.trim();
        let mut found: Vec<(usize, SceneEntry)> = self
            .project_scenes()
            .into_iter()
            .filter_map(|scene| {
                // Имя важнее каталога: «course» находит course_3 раньше, чем scenes/courses.
                let by_name = fuzzy_score(&scene.name, query);
                let by_path = fuzzy_score(&scene.path, query).map(|s| s + 500);
                by_name.into_iter().chain(by_path).min().map(|score| (score, scene))
            })
            .collect();
        if !query.is_empty() {
            found.sort_by_key(|(score, _)| *score);
        }
        found.into_iter().map(|(_, scene)| scene).collect()
    }

    /// Недавние сцены этого проекта (новые первыми), только существующие.
    pub fn recent_scenes(&self) -> Vec<String> {
        let Some(key) = self.recent_key() else { return Vec::new() };
        self.prefs
            .recent_scenes
            .iter()
            .find(|(root, _)| *root == key)
            .map(|(_, scenes)| {
                scenes
                    .iter()
                    .filter(|path| self.assets.iter().any(|a| a.path == **path))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Запомнить открытую или сохранённую сцену в недавних.
    pub(crate) fn remember_scene(&mut self, path: &str) {
        let Some(key) = self.recent_key() else { return };
        let list = &mut self.prefs.recent_scenes;
        let at = match list.iter().position(|(root, _)| *root == key) {
            Some(at) => at,
            None => {
                list.insert(0, (key, Vec::new()));
                list.truncate(RECENT_PROJECTS);
                0
            }
        };
        let scenes = &mut list[at].1;
        scenes.retain(|s| s != path);
        scenes.insert(0, path.to_string());
        scenes.truncate(RECENT_MAX);
    }

    /// Открыть или закрыть окно Open Scene… (Ctrl+O).
    pub fn show_scene_picker(&mut self, open: bool) {
        self.scene_picker.open = open;
        if open {
            self.scene_picker.query.clear();
            let current = self.scene_path.clone();
            self.scene_picker.cursor =
                self.project_scenes().iter().position(|s| Some(&s.path) == current.as_ref()).unwrap_or(0);
        }
    }

    pub fn is_scene_picker_open(&self) -> bool {
        self.scene_picker.open
    }

    /// Строка поиска окна Open Scene… (для тестов; пользователь печатает в поле).
    pub fn set_scene_picker_query(&mut self, query: &str) {
        self.scene_picker.query = query.to_string();
        self.scene_picker.cursor = 0;
    }

    /// Открыть или закрыть окно New Scene… (Ctrl+N).
    pub fn show_new_scene_dialog(&mut self, open: bool) {
        self.scene_picker.new_scene = open;
        if open {
            self.scene_picker.three_d = self.scene_3d;
        }
    }

    pub fn is_new_scene_dialog_open(&self) -> bool {
        self.scene_picker.new_scene
    }

    /// Окно Open Scene…: поиск, список (стрелки, Enter, двойной щелчок), Open / Cancel.
    pub(crate) fn scene_picker_window(&mut self, ctx: &egui::Context) {
        if !self.scene_picker.open {
            self.scene_picker.shown = false;
            return;
        }
        let first_frame = !std::mem::replace(&mut self.scene_picker.shown, true);
        let matches = self.scene_picker_matches();
        let count = matches.len();
        let total = self.assets.iter().filter(|a| a.importer == "Scene").count();
        let none = egui::Modifiers::NONE;
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::Enter),
                i.consume_key(none, egui::Key::Escape),
            )
        });
        let picker = &mut self.scene_picker;
        if down && count > 0 {
            picker.cursor = (picker.cursor + 1).min(count - 1);
        }
        if up {
            picker.cursor = picker.cursor.saturating_sub(1);
        }
        picker.cursor = picker.cursor.min(count.saturating_sub(1));
        let mut chosen = enter.then(|| matches.get(picker.cursor).map(|s| s.path.clone())).flatten();
        let mut close = escape;
        let current = self.scene_path.clone();
        let dirty = self.dirty;
        let now = SystemTime::now();

        let response = crate::window_kit::dialog(egui::Id::new("scene_picker"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(520.0);
            theme::dialog_title(ui, Icon::Scene, theme::TEXT_MUTED, tr_s("Open Scene"));
            ui.add_space(theme::SP_S);
            let picker = &mut self.scene_picker;
            let before = picker.query.clone();
            let search = theme::search_field(ui, &mut picker.query, tr_s("Search scenes by name or folder"), f32::INFINITY);
            if first_frame || !search.has_focus() && !ui.ctx().memory(|m| m.focused().is_some()) {
                search.request_focus();
            }
            if picker.query != before {
                picker.cursor = 0;
            }
            ui.add_space(theme::SP_S);
            let list_height = (theme::ROW * 14.0).min(ui.ctx().screen_rect().height() * 0.5);
            egui::Frame::none()
                .fill(theme::BG_INSET)
                .stroke(Stroke::new(1.0_f32, theme::LINE_STRONG))
                .rounding(theme::R_S)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(list_height);
                    if matches.is_empty() {
                        ui.add_space(theme::SP_L);
                        ui.vertical_centered(|ui| {
                            let text = if count == 0 && picker.query.trim().is_empty() {
                                tr_s("This project has no .scene files yet: File → Save As… creates one")
                            } else {
                                tr_s("No scene matches the search")
                            };
                            ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
                        });
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("scene_picker_list")
                        .auto_shrink([false, false])
                        .show_rows(ui, theme::ROW, matches.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for index in range {
                                let scene = &matches[index];
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
                                let row = ui.interact(rect, scene_row_id(&scene.path), Sense::click());
                                let selected = index == picker.cursor;
                                theme::list_row(ui.painter(), rect, selected, true, row.hovered());
                                if selected && (up || down) {
                                    row.scroll_to_me(None);
                                }
                                let painter = ui.painter();
                                icons::paint(
                                    painter,
                                    Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(theme::ICON_S + 1.0, theme::ICON_S + 1.0)),
                                    Icon::Scene,
                                    theme::TEXT_MUTED,
                                );
                                let is_current = current.as_deref() == Some(scene.path.as_str());
                                let name = format!("{}{}", scene.name, if is_current && dirty { "*" } else { "" });
                                let name_color = if selected { theme::TEXT_STRONG } else { theme::TEXT };
                                let right = rect.right() - theme::SP_M;
                                let age = crate::sorting::modified_label(now, scene.modified);
                                let age_galley = painter.layout_no_wrap(age, theme::caption(), theme::TEXT_MUTED);
                                let age_left = right - age_galley.size().x;
                                painter.galley(pos2(age_left, rect.center().y - age_galley.size().y * 0.5), age_galley, theme::TEXT_MUTED);
                                let mut tags = Vec::new();
                                if scene.start {
                                    tags.push(tr_s("start scene"));
                                }
                                if is_current {
                                    tags.push(tr_s("open"));
                                }
                                let text_rect = Rect::from_min_max(pos2(rect.left() + 26.0, rect.top()), pos2(age_left - theme::SP_M, rect.bottom()));
                                let clipped = painter.with_clip_rect(text_rect.intersect(painter.clip_rect()));
                                let galley = painter.layout_no_wrap(name, theme::body(), name_color);
                                let name_width = galley.size().x;
                                clipped.galley(pos2(text_rect.left(), rect.center().y - galley.size().y * 0.5), galley, name_color);
                                let mut detail = if scene.folder.is_empty() { String::from("Assets") } else { format!("Assets/{}", scene.folder) };
                                for tag in tags {
                                    detail.push_str("  ·  ");
                                    detail.push_str(tag);
                                }
                                clipped.text(
                                    pos2(text_rect.left() + name_width + theme::SP_M, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    detail,
                                    theme::caption(),
                                    theme::TEXT_MUTED,
                                );
                                if row.clicked() {
                                    picker.cursor = index;
                                }
                                if row.double_clicked() {
                                    chosen = Some(scene.path.clone());
                                }
                            }
                        });
                });
            ui.add_space(theme::SP_S);
            ui.label(
                egui::RichText::new(trf!(
                    "{count} of {total} scenes  ·  Up / Down to choose, Enter to open, Esc to close",
                    count = count,
                    total = total
                ))
                .size(theme::FONT_CAPTION)
                .color(theme::TEXT_MUTED),
            );
            crate::window_kit::dialog_actions(ui, |ui| {
                let can_open = !matches.is_empty();
                if ui.add_enabled_ui(can_open, |ui| crate::window_kit::primary_action(ui, tr_s("Open"))).inner.clicked() {
                    chosen = matches.get(picker.cursor).map(|s| s.path.clone());
                }
                if crate::window_kit::action_button(ui, tr_s("Cancel")).clicked() {
                    close = true;
                }
            });
        });
        if response.should_close() {
            close = true;
        }
        if let Some(path) = chosen {
            self.scene_picker.open = false;
            // Та же сцена без правок — открывать заново незачем.
            if self.scene_path.as_deref() != Some(path.as_str()) || self.dirty {
                self.actions.push(Action::OpenScene(path));
            }
        } else if close {
            self.scene_picker.open = false;
        }
    }

    /// Окно New Scene…: заготовка 2D (камера) или 3D (камера, свет, земля).
    pub(crate) fn new_scene_window(&mut self, ctx: &egui::Context) {
        if !self.scene_picker.new_scene {
            return;
        }
        let none = egui::Modifiers::NONE;
        let (enter, escape) = ctx.input_mut(|i| (i.consume_key(none, egui::Key::Enter), i.consume_key(none, egui::Key::Escape)));
        let mut create = enter;
        let mut close = escape;
        let response = crate::window_kit::dialog(egui::Id::new("new_scene_dialog"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(400.0);
            theme::dialog_title(ui, Icon::Scene, theme::TEXT_MUTED, tr_s("New Scene"));
            ui.label(
                egui::RichText::new(tr_s("The current scene closes; if it has unsaved changes, you are asked first."))
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(theme::SP_L);
            let picker = &mut self.scene_picker;
            theme::segmented(
                ui,
                &mut picker.three_d,
                &[
                    (false, "2D", tr_s("An orthographic Main Camera; the Scene view opens in 2D")),
                    (true, "3D", tr_s("A perspective Main Camera, a Directional Light and a ground plane; the Scene view opens in 3D")),
                ],
            );
            ui.add_space(theme::SP_S);
            let about = if picker.three_d {
                tr_s("Main Camera (perspective), Directional Light, Ground.")
            } else {
                tr_s("Main Camera (orthographic).")
            };
            ui.label(egui::RichText::new(about).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            crate::window_kit::dialog_actions(ui, |ui| {
                if crate::window_kit::primary_action(ui, tr_s("Create")).clicked() {
                    create = true;
                }
                if crate::window_kit::action_button(ui, tr_s("Cancel")).clicked() {
                    close = true;
                }
            });
        });
        if response.should_close() {
            close = true;
        }
        if create {
            self.scene_picker.new_scene = false;
            self.actions.push(if self.scene_picker.three_d { Action::NewScene3D } else { Action::NewScene });
        } else if close {
            self.scene_picker.new_scene = false;
        }
    }

    /// Пункты меню File: Open Scene…, Open Recent Scene ▸.
    pub(crate) fn scene_menu_items(&mut self, ui: &mut egui::Ui) {
        let k = self.prefs.shortcuts.clone();
        let has_scenes = self.assets.iter().any(|a| a.importer == "Scene");
        if theme::item(tr_s("Open Scene…"))
            .icon(Icon::FolderOpen)
            .shortcut(&k.text("open-scene"))
            .enabled_if(has_scenes, tr_s("There are no .scene files in the assets directory yet"))
            .show(ui)
        {
            self.show_scene_picker(true);
        }
        let recent = self.recent_scenes();
        if recent.is_empty() {
            theme::item(tr_s("Open Recent Scene"))
                .icon(Icon::Clock)
                .enabled_if(false, tr_s("Scenes you open in this project are listed here"))
                .show(ui);
        } else {
            theme::submenu(ui, Icon::Clock, tr_s("Open Recent Scene"), |ui| {
                for path in &recent {
                    let current = self.scene_path.as_deref() == Some(path.as_str());
                    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
                    if theme::item(file.trim_end_matches(".scene")).icon(Icon::Scene).shortcut(folder).checked(current).show(ui) {
                        self.actions.push(Action::OpenScene(path.clone()));
                    }
                }
                theme::menu_separator(ui);
                if theme::item(tr_s("Clear Recent Scenes")).show(ui) {
                    if let Some(key) = self.recent_key() {
                        self.prefs.recent_scenes.retain(|(root, _)| *root != key);
                    }
                }
            });
        }
    }
}

/// Строка сцены в окне Open Scene… (для тестов).
pub(crate) fn scene_row_id(path: &str) -> egui::Id {
    egui::Id::new(("scene_picker_row", path))
}

/// Переключатель сцен в строке сцены окна Hierarchy: имя открытой сцены со стрелкой;
/// щелчок — список сцен проекта (стартовая первой), второй щелчок открывает сцену.
pub(crate) fn scene_switcher(editor: &mut Editor, ui: &mut egui::Ui, row: Rect, name: &str) {
    let galley = ui.painter().layout_no_wrap(name.to_string(), theme::strong(theme::FONT_BODY), theme::TEXT);
    let width = (galley.size().x + 24.0).min(row.width() - 30.0).max(40.0);
    let area = Rect::from_min_size(pos2(row.left() + 22.0, row.top() + 2.0), vec2(width, row.height() - 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let background = child.painter().add(egui::Shape::Noop);
    let scenes = editor.project_scenes();
    let current = editor.scene_path.clone();
    let dirty = editor.dirty;
    let shortcuts = editor.prefs.shortcuts.clone();
    let inner = crate::controls::menu_anchor(
        &mut child,
        area.size(),
        false,
        |ui| {
            theme::menu_body(ui);
            theme::menu_section(ui, tr_s("Scenes"));
            if scenes.is_empty() {
                theme::item(tr_s("No scenes in this project"))
                    .enabled_if(false, tr_s("Save the scene with File → Save As…"))
                    .show(ui);
            }
            for scene in &scenes {
                let is_current = current.as_deref() == Some(scene.path.as_str());
                let label = format!("{}{}", scene.name, if is_current && dirty { "*" } else { "" });
                let folder = if scene.start { trf!("{folder}  ·  start", folder = scene.folder) } else { scene.folder.clone() };
                if theme::item(&label).icon(Icon::Scene).shortcut(&folder).checked(is_current).show(ui) && !is_current {
                    editor.actions.push(Action::OpenScene(scene.path.clone()));
                }
            }
            theme::menu_separator(ui);
            if theme::item(tr_s("Open Scene…")).icon(Icon::FolderOpen).shortcut(&shortcuts.text("open-scene")).show(ui) {
                editor.show_scene_picker(true);
            }
            if theme::item(tr_s("New Scene…")).icon(Icon::Plus).shortcut(&shortcuts.text("new-scene")).show(ui) {
                editor.show_new_scene_dialog(true);
            }
        },
    );
    let response = inner.response;
    if response.hovered() || inner.inner.is_some() {
        child.painter().set(background, egui::Shape::rect_filled(area, theme::R_S, theme::BG_HOVER));
    }
    let clipped = ui.painter().with_clip_rect(area);
    clipped.galley(pos2(area.left() + 4.0, area.center().y - galley.size().y * 0.5), galley, theme::TEXT);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(area.right() - 9.0, area.center().y + 1.0), vec2(8.0, 8.0)),
        Icon::ChevronDown,
        theme::TEXT_MUTED,
    );
    response.on_hover_text(tr_s("Switch to another scene of the project"));
}

#[cfg(test)]
mod tests {
    use super::fuzzy_score;

    #[test]
    fn fuzzy_search_prefers_prefixes_then_substrings_then_scattered_letters() {
        assert_eq!(fuzzy_score("course_3", ""), Some(0));
        assert_eq!(fuzzy_score("course_3", "cour"), Some(0));
        assert!(fuzzy_score("menu", "enu").unwrap() < fuzzy_score("course_3", "cs3").unwrap());
        assert!(fuzzy_score("course_3", "cs3").is_some());
        assert_eq!(fuzzy_score("menu", "xyz"), None);
        assert_eq!(fuzzy_score("Course_3", "COURSE"), Some(0));
    }
}
