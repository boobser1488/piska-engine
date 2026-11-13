//! Страница Tags and Layers: список тегов (добавить, переименовать, удалить) и имена 32 слоёв.
//!
//! Сцены по-прежнему хранят у объекта текст тега и номер слоя. Переименование тега
//! переименовывает его и у объектов открытой сцены; удаление тега, который кто-то носит,
//! спрашивает подтверждение — объекты сохраняют текст, а Inspector показывает его как
//! «missing».

use egui::{vec2, Rect, Sense};
use engine_core::settings::{TagsAndLayers, LAYER_COUNT};

use super::rows::{hairline, WIDE_W};
use super::{Confirm, SettingsUi};
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;

/// Что сделать со списком тегов после кадра.
enum TagOp {
    Add(String),
    Rename(usize, String),
    Remove(usize),
}

impl Editor {
    /// Поле «новый тег» на странице Tags and Layers.
    pub fn new_tag_field_id() -> egui::Id {
        egui::Id::new("settings_new_tag")
    }

    /// Кнопка удаления тега `tag` на странице Tags and Layers.
    pub fn remove_tag_button_id(tag: &str) -> egui::Id {
        egui::Id::new(("settings_remove_tag", tag.to_string()))
    }

    /// Теги проекта (без встроенного `Untagged`), как их предлагает Inspector.
    pub fn project_tags(&self) -> Vec<String> {
        self.project_settings_draft().tags_and_layers.tags
    }
}

pub(super) fn tags_and_layers(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = TagsAndLayers::default();
    editor.ensure_project_draft();
    let tags = editor.project_settings_draft().tags_and_layers.tags;
    let counts: Vec<usize> = tags.iter().map(|t| editor.app.world.scene.count_with_tag(t)).collect();

    s.group("Tags");
    let settings = &mut editor.settings;
    let row = s.block(
        "Tag list",
        "Find objects by tag in code: scene.find_by_tag(\"Enemy\"). Untagged is built in.",
        tags != defaults.tags,
        |ui| {
            let mut op = None;
            ui.spacing_mut().item_spacing.y = 0.0;
            let builtin = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover()).0;
            ui.painter().text(
                builtin.left_center() + vec2(theme::SP_S, 0.0),
                egui::Align2::LEFT_CENTER,
                "Untagged",
                theme::body(),
                theme::TEXT_DISABLED,
            );
            ui.painter().text(
                builtin.right_center() - vec2(theme::SP_S, 0.0),
                egui::Align2::RIGHT_CENTER,
                "built in",
                theme::caption(),
                theme::TEXT_DISABLED,
            );
            for (index, tag) in tags.iter().enumerate() {
                hairline(ui);
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW + 2.0), Sense::hover());
                if response.hovered() {
                    ui.painter().rect_filled(rect, 0.0, theme::hover_fill());
                }
                let field = Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(WIDE_W, theme::FIELD_H));
                let editing = settings.tag_edit.as_ref().is_some_and(|(i, _)| *i == index);
                let mut text = if editing { settings.tag_edit.as_ref().map(|(_, t)| t.clone()).unwrap_or_default() } else { tag.clone() };
                let edit = ui.put(
                    field,
                    egui::TextEdit::singleline(&mut text)
                        .id(egui::Id::new(("settings_tag", index)))
                        .vertical_align(egui::Align::Center)
                        .desired_width(WIDE_W),
                );
                if edit.changed() || (edit.has_focus() && !editing) {
                    settings.tag_edit = Some((index, text.clone()));
                }
                if edit.lost_focus() {
                    settings.tag_edit = None;
                    if text.trim() != tag {
                        op = Some(TagOp::Rename(index, text.trim().to_string()));
                    }
                }
                let used = counts.get(index).copied().unwrap_or(0);
                if used > 0 {
                    ui.painter().text(
                        egui::pos2(field.right() + theme::SP_M, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{used} in this scene"),
                        theme::caption(),
                        theme::TEXT_MUTED,
                    );
                }
                let remove = Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), vec2(20.0, 20.0));
                let click = ui.interact(remove, Editor::remove_tag_button_id(tag), Sense::click());
                if click.hovered() {
                    ui.painter().rect_filled(remove, theme::R_S, theme::BG_HOVER);
                }
                icons::paint(ui.painter(), remove.shrink(5.0), Icon::Close, if click.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
                if click.on_hover_text("Remove this tag").clicked() {
                    op = Some(TagOp::Remove(index));
                }
            }
            hairline(ui);
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut settings.new_tag)
                        .id(Editor::new_tag_field_id())
                        .hint_text(egui::RichText::new("New tag").color(theme::TEXT_DISABLED))
                        .vertical_align(egui::Align::Center)
                        .min_size(vec2(0.0, theme::FIELD_H))
                        .desired_width(WIDE_W),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = ui.add_enabled_ui(!settings.new_tag.trim().is_empty(), |ui| theme::icon_text_button(ui, Icon::Plus, "Add Tag")).inner;
                if (add.clicked() || enter) && !settings.new_tag.trim().is_empty() {
                    op = Some(TagOp::Add(std::mem::take(&mut settings.new_tag)));
                }
            });
            op
        },
    );
    if row.reset {
        editor.project_draft_mut().tags_and_layers.tags = defaults.tags.clone();
    }
    match row.inner.flatten() {
        Some(TagOp::Add(tag)) => {
            if !editor.project_draft_mut().tags_and_layers.add_tag(&tag) {
                editor.settings.status[super::SettingsScope::Project.index()] =
                    Some((format!("Tag “{}” already exists or is reserved", tag.trim()), true));
            }
        }
        Some(TagOp::Rename(index, new)) => rename_tag(editor, index, &new),
        Some(TagOp::Remove(index)) => {
            if let Some(tag) = tags.get(index) {
                let used = counts.get(index).copied().unwrap_or(0);
                if used > 0 {
                    editor.settings.confirm = Some(Confirm::RemoveTag(tag.clone(), used));
                } else {
                    remove_tag(editor, tag);
                }
            }
        }
        None => {}
    }

    s.group("Layers");
    s.note("Objects keep a layer number (0–31); the name is what the Inspector and the collision matrix show.");
    let layers = &mut editor.project_draft_mut().tags_and_layers;
    if layers.layers.len() != LAYER_COUNT {
        layers.layers.resize(LAYER_COUNT, String::new());
    }
    // Как в Unity: 0–7 — встроенные, 8–31 — свои.
    for index in 0..LAYER_COUNT {
        let label = if index < 8 { format!("Builtin Layer {index}") } else { format!("User Layer {index}") };
        let hint = if index == 0 { "Default" } else { "Unnamed" };
        s.text(&label, "", &mut layers.layers[index], &defaults.layers[index], hint);
    }
}

/// Переименовать тег номер `index` в проекте и у объектов открытой сцены.
fn rename_tag(editor: &mut Editor, index: usize, new: &str) {
    let draft = &mut editor.project_draft_mut().tags_and_layers;
    let Some(old) = draft.tags.get(index).cloned() else {
        return;
    };
    if new.is_empty() || new == "Untagged" || draft.tags.iter().any(|t| t == new) {
        editor.settings.status[super::SettingsScope::Project.index()] =
            Some((format!("Cannot rename “{old}” to “{new}”: the name is empty, reserved or taken"), true));
        return;
    }
    draft.tags[index] = new.to_string();
    let mut renamed = 0;
    for id in editor.app.world.scene.ids() {
        if let Some(object) = editor.app.world.scene.get_mut(id) {
            if object.tag == old {
                object.tag = new.to_string();
                renamed += 1;
            }
        }
    }
    if renamed > 0 {
        editor.dirty = true;
    }
}

/// Убрать тег из проекта (объекты сохраняют текст тега).
pub(super) fn remove_tag(editor: &mut Editor, tag: &str) {
    editor.project_draft_mut().tags_and_layers.tags.retain(|t| t != tag);
}

// ---------------------------------------------------------------------------
// Inspector: выпадающие списки Tag и Layer
// ---------------------------------------------------------------------------

impl Editor {
    /// Теги и имена слоёв проекта (черновик окна Project Settings или то, что действует).
    pub(crate) fn tags_and_layers(&self) -> TagsAndLayers {
        self.settings
            .project
            .as_ref()
            .map(|p| p.tags_and_layers.clone())
            .or_else(|| {
                self.app.world.resources.get::<engine_core::settings::ProjectSettings>().map(|p| p.tags_and_layers.clone())
            })
            .unwrap_or_default()
            .normalized()
    }

    /// Выпадающий список Tag в Inspector.
    pub fn inspector_tag_combo_id() -> egui::Id {
        egui::Id::new("inspector_tag_combo")
    }

    /// Выпадающий список Layer в Inspector.
    pub fn inspector_layer_combo_id() -> egui::Id {
        egui::Id::new("inspector_layer_combo")
    }
}

/// Список Tag: Untagged, теги проекта, «Add Tag…» (ставит `open_tags`). Тег объекта, которого
/// нет в проекте, показывается с пометкой «missing» и не теряется.
pub(crate) fn tag_combo(ui: &mut egui::Ui, tag: &mut String, names: &TagsAndLayers, open_tags: &mut bool) -> bool {
    let before = tag.clone();
    let missing = !names.has_tag(tag);
    let shown = if tag.is_empty() { "Untagged".to_string() } else if missing { format!("{tag} (missing)") } else { tag.clone() };
    let response = egui::ComboBox::from_id_salt(Editor::inspector_tag_combo_id())
        .selected_text(egui::RichText::new(shown).color(if missing { theme::WARN } else { theme::TEXT }))
        .width(ui.available_width())
        .height(320.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(tag.is_empty() || tag == "Untagged", "Untagged").clicked() {
                tag.clear();
            }
            for name in &names.tags {
                if ui.selectable_label(tag == name, name.as_str()).clicked() {
                    *tag = name.clone();
                }
            }
            if missing {
                let _ = ui.selectable_label(true, egui::RichText::new(format!("{before} (missing)")).color(theme::WARN));
            }
            ui.separator();
            if ui.selectable_label(false, "Add Tag…").clicked() {
                *open_tags = true;
            }
        });
    response.response.on_hover_text("Tag: find objects by tag in code. Edit the list in Project Settings → Tags and Layers");
    *tag != before
}

/// Список Layer: именованные слои (и текущий, если у него нет имени), «Add Layer…».
pub(crate) fn layer_combo(ui: &mut egui::Ui, layer: &mut u32, names: &TagsAndLayers, open_tags: &mut bool) -> bool {
    let before = *layer;
    let label = |i: u32| format!("{i}: {}", names.layer_name(i));
    let response = egui::ComboBox::from_id_salt(Editor::inspector_layer_combo_id())
        .selected_text(names.layer_name(*layer))
        .width(ui.available_width())
        .height(360.0)
        .show_ui(ui, |ui| {
            for i in 0..LAYER_COUNT as u32 {
                let named = names.layers.get(i as usize).is_some_and(|n| !n.is_empty());
                if named || i == *layer {
                    ui.selectable_value(layer, i, label(i));
                }
            }
            ui.separator();
            if ui.selectable_label(false, "Add Layer…").clicked() {
                *open_tags = true;
            }
        });
    response.response.on_hover_text("Layer 0–31: camera culling and the physics collision matrix. Name layers in Project Settings → Tags and Layers");
    *layer != before
}
