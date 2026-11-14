//! Инспектор при нескольких выделенных объектах, как в Unity: шапка «N objects»,
//! Transform и компоненты, которые есть у *всех* выделенных. Поля с разными значениями
//! показывают «—»; правка (одним шагом Undo) задаёт значение всем объектам — у векторов
//! только изменённую ось.

use crate::i18n::{tr_s, trn};

use engine_core::prelude::*;

use crate::icons::{self, Icon};
use crate::inspector::{axes_mixed, card, clean_doc, property, separator, value_widget, CardCommand};
use crate::state::{Action, Editor};
use crate::theme;

/// Одна правка, собранная за кадр.
enum Edit {
    /// Поле компонента целиком.
    Field { kind: &'static str, field: &'static str, value: Value },
    /// Одна ось векторного поля компонента.
    FieldAxis { kind: &'static str, field: &'static str, axis: usize, value: f32 },
    /// Ось позиции / поворота (градусы) / масштаба.
    Transform { part: usize, axis: usize, value: f32 },
    Active(bool),
}

/// Поле общего компонента: значения у всех объектов (первое — у активного).
struct FieldValues {
    name: &'static str,
    meta: FieldMeta,
    doc: String,
    values: Vec<Value>,
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let ids = {
        let mut ids = editor.selected();
        // Активный — первым: его значения и показываются.
        if let Some(active) = editor.selection() {
            ids.retain(|id| *id != active);
            ids.insert(0, active);
        }
        ids
    };
    crate::inspector_refs::publish(ui.ctx(), &editor.app.world.scene, &ids);
    let scene = &editor.app.world.scene;
    let objects: Vec<&GameObject> = ids.iter().filter_map(|id| scene.get(*id)).collect();
    if objects.len() < 2 {
        return;
    }
    let count = objects.len();
    let actives: Vec<bool> = objects.iter().map(|o| o.active).collect();
    let transforms: Vec<[Vec3; 3]> = objects
        .iter()
        .map(|o| [o.transform.position, o.transform.euler_deg(), o.transform.scale])
        .collect();
    // Общие компоненты — по порядку у активного объекта.
    let shared: Vec<&'static str> = objects[0]
        .components()
        .map(|c| c.type_name())
        .filter(|kind| objects.iter().all(|o| o.components().any(|c| c.type_name() == *kind)))
        .collect();
    let mut fields: Vec<(&'static str, Vec<FieldValues>)> = Vec::new();
    for kind in &shared {
        let first = objects[0].components().find(|c| c.type_name() == *kind).expect("shared");
        let rows = first
            .field_names()
            .iter()
            .map(|name| FieldValues {
                name,
                meta: first.field_meta(name),
                doc: clean_doc(first.field_doc(name)),
                values: objects
                    .iter()
                    .filter_map(|o| o.components().find(|c| c.type_name() == *kind)?.get_field(name))
                    .collect(),
            })
            .collect();
        fields.push((kind, rows));
    }
    let names: Vec<String> = objects.iter().map(|o| o.name.clone()).collect();

    let mut edits: Vec<Edit> = Vec::new();
    let mut commands: Vec<(&'static str, CardCommand)> = Vec::new();
    let mut transform_reset = false;
    let Editor { inspector, assets, .. } = editor;
    egui::ScrollArea::vertical()
        .id_salt("inspector_multi_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            // --- Шапка: флажок (смешанный — «частично»), «N objects», имена ---
            egui::Frame::none()
                .inner_margin(egui::Margin::same(theme::SP_M))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mixed = actives.iter().any(|a| *a != actives[0]);
                        let mut active = actives[0];
                        let response = ui.add(egui::Checkbox::without_text(&mut active).indeterminate(mixed));
                        if response.on_hover_text(tr_s("Active: applies to every selected object")).changed() {
                            edits.push(Edit::Active(if mixed { true } else { active }));
                        }
                        icons::inline(ui, Icon::Object, theme::TEXT_MUTED);
                        ui.label(egui::RichText::new(trn!("{n} object", "{n} objects", count)).color(theme::TEXT));
                    });
                    ui.add_space(theme::SP_XS);
                    ui.add(
                        egui::Label::new(egui::RichText::new(names.join(", ")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED))
                            .truncate(),
                    )
                    .on_hover_text(names.join("\n"));
                });
            separator(ui);

            // --- Transform ---
            let reset = card(ui, ("multi", tr_s("Transform"), usize::MAX), Icon::Transform, tr_s("Transform"), false, None, |ui| {
                for (part, (label, speed)) in [(tr_s("Position"), 0.05), (tr_s("Rotation"), 0.5), (tr_s("Scale"), 0.02)].into_iter().enumerate() {
                    property(ui, label, "", |ui| {
                        let mut values = transforms[0][part].to_array();
                        let mixed: Vec<bool> = (0..3)
                            .map(|axis| transforms.iter().any(|t| (t[part][axis] - transforms[0][part][axis]).abs() > 1e-4))
                            .collect();
                        match axes_mixed(ui, &mut values, &mixed, speed) {
                            Some(axis) => {
                                edits.push(Edit::Transform { part, axis, value: values[axis] });
                                true
                            }
                            None => false,
                        }
                    });
                }
            });
            transform_reset = matches!(reset, Some(CardCommand::Reset));

            // --- Общие компоненты ---
            for (kind, rows) in &fields {
                let command = card(
                    ui,
                    ("multi", kind, 0),
                    crate::catalog::component_icon(kind),
                    &theme::pretty_type(kind),
                    false,
                    None,
                    |ui| {
                        for row in rows {
                            let Some(first) = row.values.first() else { continue };
                            let mixed = row.values.iter().any(|v| v != first);
                            property(ui, &theme::pretty_name(row.name), &row.doc, |ui| {
                                let mut value = first.clone();
                                match (&mut value, mixed) {
                                    (Value::Vec2(..) | Value::Vec3(..) | Value::Quat(..), _) => {
                                        let parts = |v: &Value| -> Vec<f32> {
                                            match v {
                                                Value::Vec2(x, y) => vec![*x, *y],
                                                Value::Vec3(x, y, z) => vec![*x, *y, *z],
                                                Value::Quat(x, y, z, w) => vec![*x, *y, *z, *w],
                                                _ => Vec::new(),
                                            }
                                        };
                                        let mut values = parts(first);
                                        let mask: Vec<bool> = (0..values.len())
                                            .map(|i| row.values.iter().any(|v| parts(v).get(i) != values.get(i)))
                                            .collect();
                                        if let Some(axis) = axes_mixed(ui, &mut values, &mask, 0.05) {
                                            edits.push(Edit::FieldAxis { kind, field: row.name, axis, value: values[axis] });
                                            return true;
                                        }
                                        false
                                    }
                                    (Value::Float(number), true) => {
                                        let changed = crate::controls::number_field_mixed(ui, number, 0.05, f32::INFINITY).changed();
                                        if changed {
                                            edits.push(Edit::Field { kind, field: row.name, value: value.clone() });
                                        }
                                        changed
                                    }
                                    (Value::Int(number), true) => {
                                        let changed = crate::controls::number_field_mixed(ui, number, 0.1, f32::INFINITY).changed();
                                        if changed {
                                            edits.push(Edit::Field { kind, field: row.name, value: value.clone() });
                                        }
                                        changed
                                    }
                                    (Value::Object(uid), true) => {
                                        // Поле-ссылка: «—», выбор применяется ко всем (inspector_refs.rs).
                                        let changed = crate::inspector_refs::object_field(ui, uid, row.meta, ("multi", *kind, row.name), true);
                                        if changed {
                                            edits.push(Edit::Field { kind, field: row.name, value: value.clone() });
                                        }
                                        changed
                                    }
                                    (Value::Bool(flag), true) => {
                                        let changed = ui.add(egui::Checkbox::without_text(flag).indeterminate(true)).changed();
                                        if changed {
                                            edits.push(Edit::Field { kind, field: row.name, value: Value::Bool(true) });
                                        }
                                        changed
                                    }
                                    (_, true) => {
                                        // Остальные типы: «—» и кнопка «как у активного».
                                        let clicked = theme::button(ui, tr_s("—  (mixed: set all like the active object)"))
                                            .on_hover_text(tr_s("The selected objects have different values here"))
                                            .clicked();
                                        if clicked {
                                            edits.push(Edit::Field { kind, field: row.name, value: first.clone() });
                                        }
                                        clicked
                                    }
                                    (_, false) => {
                                        let changed = value_widget(ui, &mut value, row.meta, assets, &mut inspector.asset_filter, ("multi", *kind, row.name));
                                        if changed {
                                            edits.push(Edit::Field { kind, field: row.name, value: value.clone() });
                                        }
                                        changed
                                    }
                                }
                            });
                        }
                    },
                );
                if let Some(command) = command {
                    commands.push((kind, command));
                }
            }
            if !shared.is_empty() || !fields.is_empty() {
                separator(ui);
            }
            ui.add_space(theme::SP_M);
            ui.label(
                egui::RichText::new(tr_s("Only the components that every selected object has are shown."))
                    .size(theme::FONT_CAPTION)
                    .color(theme::TEXT_DISABLED),
            );
            ui.add_space(theme::SP_XL);
        });

    // --- Применить правки ко всем объектам (один шаг Undo) ---
    if !edits.is_empty() || transform_reset {
        editor.touch();
    }
    for id in &ids {
        let Some(object) = editor.app.world.scene.get_mut(*id) else { continue };
        if transform_reset {
            object.transform.position = Vec3::ZERO;
            object.transform.rotation = Quat::IDENTITY;
            object.transform.scale = Vec3::ONE;
        }
        for edit in &edits {
            match edit {
                Edit::Active(active) => object.active = *active,
                Edit::Transform { part, axis, value } => match part {
                    0 => object.transform.position[*axis] = *value,
                    1 => {
                        let mut euler = object.transform.euler_deg();
                        euler[*axis] = *value;
                        object.transform.set_euler_deg(euler);
                    }
                    _ => object.transform.scale[*axis] = *value,
                },
                Edit::Field { kind, field, value } => {
                    if let Some(component) = object.components_mut().find(|c| c.type_name() == *kind) {
                        let _ = component.set_field(field, value);
                    }
                }
                Edit::FieldAxis { kind, field, axis, value } => {
                    if let Some(component) = object.components_mut().find(|c| c.type_name() == *kind) {
                        let updated = match component.get_field(field) {
                            Some(Value::Vec2(mut x, mut y)) => {
                                if let Some(v) = [&mut x, &mut y].into_iter().nth(*axis) {
                                    *v = *value;
                                }
                                Some(Value::Vec2(x, y))
                            }
                            Some(Value::Vec3(mut x, mut y, mut z)) => {
                                if let Some(v) = [&mut x, &mut y, &mut z].into_iter().nth(*axis) {
                                    *v = *value;
                                }
                                Some(Value::Vec3(x, y, z))
                            }
                            Some(Value::Quat(mut x, mut y, mut z, mut w)) => {
                                if let Some(v) = [&mut x, &mut y, &mut z, &mut w].into_iter().nth(*axis) {
                                    *v = *value;
                                }
                                Some(Value::Quat(x, y, z, w))
                            }
                            _ => None,
                        };
                        if let Some(updated) = updated {
                            let _ = component.set_field(field, &updated);
                        }
                    }
                }
            }
        }
    }
    for (kind, command) in commands {
        for id in &ids {
            let index = editor
                .app
                .world
                .scene
                .get(*id)
                .and_then(|o| o.components().position(|c| c.type_name() == kind));
            let Some(index) = index else { continue };
            match command {
                CardCommand::Reset => editor.actions.push(Action::ResetComponent { object: *id, index }),
                CardCommand::Remove => editor.actions.push(Action::RemoveComponent { object: *id, index }),
                _ => {}
            }
        }
    }
    crate::inspector_refs::finish(editor, ui.ctx());
}
