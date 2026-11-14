//! Окно Inspector: поля выделенного объекта.
//!
//! Инспектор ничего не знает о конкретных компонентах: он идёт по полям через
//! reflection (`field_names` / `get_field` / `set_field`) и подбирает виджет по
//! типу значения. Новый компонент, объявленный через `component!`, появляется
//! здесь сам, вместе с подсказками из doc-комментариев.

use crate::i18n::{tr_s, trf, trn};
use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_assets::Guid;
use engine_core::prelude::*;

use crate::catalog;
use crate::icons::{self, Icon};
use crate::project::{asset_icon, DragAsset};
use crate::state::{file_name, Action, AssetEntry, BottomTab, Dialog, Editor, Mode};
use crate::theme::{self, item};

#[derive(Default)]
pub(crate) struct InspectorState {
    /// Углы Эйлера, которые показывает поле Rotation. Пока кватернион объекта
    /// не изменился извне, показываем то, что ввёл пользователь, — без скачков
    /// 180° ↔ −180° и «переворотов» других осей.
    euler: Option<(GameObjectId, Quat, Vec3)>,
    add_filter: String,
    /// Свёрнутые категории меню Add Component.
    add_collapsed: std::collections::BTreeSet<String>,
    /// Поиск в выпадающем списке ассетов.
    pub asset_filter: String,
    /// Скопированные значения компонента: тип и поля.
    clipboard: Option<(&'static str, Vec<(&'static str, Value)>)>,
    /// Значение одного поля (меню поля → Copy).
    pub field_clipboard: Option<Value>,
    /// Где в этом кадре нарисован виджет каждого поля: (компонент, поле) → область.
    pub field_rects: HashMap<(&'static str, &'static str), egui::Rect>,
    /// Насколько содержимое в последнем кадре вышло за ширину окна (0 — не вышло).
    pub overflow: f32,
}

/// Что попросили сделать из меню «⋮» компонента.
pub(crate) enum CardCommand {
    Reset,
    Remove,
    Copy,
    Paste,
    MoveUp,
    MoveDown,
    /// Заголовок компонента с этим номером бросили на эту карточку.
    DropFrom(usize),
}

/// Что тащат за заголовок компонента: его номер в списке компонентов объекта.
struct DragComponent(usize);

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    theme::panel_header(ui, Icon::Inspector, tr_s("Inspector"), |_| {});
    // Выбран материал в окне Project — его редактор вместо объекта.
    if crate::material_editor::show(editor, ui) {
        return;
    }
    // Несколько объектов — общий вид (inspector_multi.rs).
    if editor.selected().len() > 1 {
        crate::inspector_multi::show(editor, ui);
        return;
    }

    let Some(id) = editor
        .selection
        .filter(|id| editor.app.world.scene.contains(*id))
    else {
        empty(editor, ui);
        return;
    };

    // Поля-ссылки на объекты: справочник объектов сцены на этот кадр (inspector_refs.rs).
    crate::inspector_refs::publish(ui.ctx(), &editor.app.world.scene, &[id]);
    // Экземпляр префаба: какие поля переопределены (inspector_prefab.rs).
    let prefab_marks = crate::inspector_prefab::PrefabMarks::of(ui.ctx(), &editor.app.world, id);
    let add_entries = crate::add_menu::entries(&editor.app.world.registry);
    let can_script = editor.config.script_dir.is_some();
    let assets_root = editor.assets_root();
    // Теги и имена слоёв из Project Settings → Tags and Layers.
    let tags_layers = editor.tags_and_layers();
    let mut open_tags = false;
    let Editor {
        app,
        inspector,
        actions,
        assets,
        mode,
        dialog,
        project,
        bottom_tab,
        script_rust,
        prefs,
        terrain_tools,
        material_editor,
        previews,
        ..
    } = editor;
    let world = &mut app.world;
    let Some(object) = world.scene.get_mut(id) else {
        return;
    };
    let mut changed = false;
    inspector.field_rects.clear();

    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Ничто не раздвигает инспектор шире окна: длинный текст переносится.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            let inner_width = ui.available_width();
            let left = ui.max_rect().left();
            if *mode != Mode::Edit {
                egui::Frame::none()
                    .fill(theme::mix(theme::BG_PANEL, theme::accent(), 0.16))
                    .inner_margin(egui::Margin::symmetric(theme::SP_M, theme::SP_S))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            icons::inline_small(ui, Icon::Play, theme::TEXT_MUTED);
                            ui.label(
                                egui::RichText::new(tr_s("Play mode — changes are reverted on Stop"))
                                    .size(theme::FONT_CAPTION)
                                    .color(theme::TEXT_MUTED),
                            );
                        });
                    });
            }

            // --- Объект: флажок, имя, тег, слой, префаб ---
            egui::Frame::none()
                .inner_margin(egui::Margin { left: theme::SP_M, right: theme::SP_M, top: theme::SP_M, bottom: theme::SP_M })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = vec2(theme::SP_S + 2.0, theme::SP_S + 2.0);
                    ui.horizontal(|ui| {
                        changed |= ui
                            .checkbox(&mut object.active, "")
                            .on_hover_text(tr_s("Active: inactive objects are not updated or drawn"))
                            .changed();
                        let (icon, color) = catalog::object_icon(object);
                        icons::inline(ui, icon, if object.prefab().is_some() { theme::PREFAB } else { color });
                        // Поле имени — ровно в оставшуюся ширину: длинное имя прокручивается
                        // внутри поля и ничего не раздвигает.
                        let size = vec2(ui.available_width().max(0.0), theme::CONTROL_H);
                        let name_response = theme::fixed(ui, size, |ui| {
                            crate::controls::text_field(ui, &mut object.name, "", size.x)
                        });
                        changed |= name_response.changed();
                    });
                    ui.horizontal(|ui| {
                        // Длинное имя тега не раздвигает инспектор: текст обрезается.
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        // Каждая половина — ровно своей ширины: в узком окне список обрезается,
                        // а не раздвигает инспектор.
                        // В широком окне строка не шире сетки свойств (подписи + значение).
                        let row = ui.available_width().min(theme::PROP_LABEL_MAX + theme::PROP_VALUE_MAX + theme::SP_S);
                        let half = ((row - theme::SP_M) * 0.5).max(0.0);
                        theme::fixed(ui, vec2(half, theme::FIELD_H), |ui| {
                            caption(ui, tr_s("Tag"), 30.0);
                            changed |= crate::settings::tag_combo(ui, &mut object.tag, &tags_layers, &mut open_tags);
                        });
                        theme::fixed(ui, vec2(half, theme::FIELD_H), |ui| {
                            caption(ui, tr_s("Layer"), 36.0);
                            changed |= crate::settings::layer_combo(ui, &mut object.layer, &tags_layers, &mut open_tags);
                        });
                    });
                    if let Some(prefab) = object.prefab().map(str::to_string) {
                        prefab_row(ui, &prefab, id, actions, assets, project, bottom_tab);
                    }
                });
            separator(ui);

            // --- Transform ---
            let transform_menu = card(
                ui,
                ("component", tr_s("Transform"), usize::MAX),
                Icon::Transform,
                tr_s("Transform"),
                false,
                None,
                |ui| {
                    let transform = &mut object.transform;
                    changed |= property(ui, tr_s("Position"), tr_s("Local position, metres"), |ui| {
                        let mut v = transform.position.to_array();
                        let edited = axes(ui, &mut v, 0.05);
                        transform.position = Vec3::from_array(v);
                        edited
                    });
                    let euler = match inspector.euler {
                        Some((owner, rotation, euler)) if owner == id && rotation == transform.rotation => euler,
                        _ => transform.euler_deg(),
                    };
                    changed |= property(ui, tr_s("Rotation"), tr_s("Euler angles in degrees"), |ui| {
                        // Без «-0.0» и хвостов вроде 89.99999.
                        let mut v = euler.to_array().map(|angle| {
                            let rounded = (angle * 1e3).round() / 1e3;
                            if rounded == 0.0 { 0.0 } else { rounded }
                        });
                        let edited = axes(ui, &mut v, 0.5);
                        if edited {
                            transform.set_euler_deg(Vec3::from_array(v));
                        }
                        inspector.euler = Some((id, transform.rotation, Vec3::from_array(v)));
                        edited
                    });
                    changed |= property(ui, tr_s("Scale"), "", |ui| {
                        let mut v = transform.scale.to_array();
                        let edited = axes(ui, &mut v, 0.02);
                        transform.scale = Vec3::from_array(v);
                        edited
                    });
                },
            );
            if let Some(CardCommand::Reset) = transform_menu {
                object.transform.position = Vec3::ZERO;
                object.transform.rotation = Quat::IDENTITY;
                object.transform.scale = Vec3::ONE;
                changed = true;
            }

            // --- Компоненты ---
            let present: Vec<&'static str> = object.components().map(|c| c.type_name()).collect();
            let clipboard_kind = inspector.clipboard.as_ref().map(|(kind, _)| *kind);
            for (index, component) in object.components_mut().enumerate() {
                let type_name = component.type_name();
                let can_paste = clipboard_kind == Some(type_name);
                let command = card(
                    ui,
                    ("component", type_name, index),
                    catalog::component_icon(type_name),
                    &theme::pretty_type(type_name),
                    can_paste,
                    Some((index, present.len())),
                    |ui| {
                        // Тип не зарегистрирован в этой сборке: данные только для чтения.
                        if let Some(missing) = component.as_any().downcast_ref::<engine_core::missing::MissingScript>() {
                            crate::rust_scripts::missing_script_body(ui, missing);
                            return;
                        }
                        // Рельеф: черновик формы, Regenerate, слои (terrain_tools.rs).
                        if let Some(terrain) = component.as_any_mut().downcast_mut::<engine_render::Terrain>() {
                            changed |= crate::terrain_tools::inspector_body(ui, terrain, id, terrain_tools, assets, &mut inspector.asset_filter, &mut inspector.field_rects);
                            return;
                        }
                        // Меш: слоты материалов по частям, встроенный материал — в группе.
                        if let Some(mesh) = component.as_any_mut().downcast_mut::<engine_render::MeshRenderer>() {
                            changed |= crate::material_editor::mesh_renderer_body(
                                ui,
                                mesh,
                                crate::material_editor::MeshRendererUi {
                                    object: id,
                                    editor: material_editor,
                                    previews,
                                    database: world.resources.get::<engine_assets::AssetDatabase>(),
                                    assets,
                                    filter: &mut inspector.asset_filter,
                                    actions,
                                    field_rects: &mut inspector.field_rects,
                                },
                            );
                            return;
                        }
                        let fields = component.field_names();
                        if fields.is_empty() {
                            ui.label(egui::RichText::new(tr_s("This component has no settings")).color(theme::TEXT_MUTED));
                        }
                        for field in fields {
                            let Some(mut value) = component.get_field(field) else {
                                continue;
                            };
                            // Подзаголовок группы полей (длинные компоненты, catalog.rs).
                            if let Some(group) = crate::catalog::field_group(type_name, field) {
                                ui.add_space(theme::SP_S);
                                theme::section_label(ui, tr_s(group));
                            }
                            let meta = component.field_meta(field);
                            let doc = clean_doc(component.field_doc(field));
                            let edited = property(ui, &theme::pretty_name(field), &doc, |ui| {
                                let widget = ui.scope(|ui| {
                                    value_widget(ui, &mut value, meta, assets, &mut inspector.asset_filter, (type_name, index, *field))
                                });
                                inspector.field_rects.insert((type_name, *field), widget.response.rect);
                                widget.inner
                            });
                            if edited {
                                // Значение вне диапазона поля (−1 для `u32`) просто не применится.
                                changed |= component.set_field(field, &value).is_ok();
                            }
                            if let Some(rect) = inspector.field_rects.get(&(type_name, *field)).copied() {
                                if let Some(base) = crate::inspector_prefab::field_marker(ui, &prefab_marks, type_name, field, &value, rect) {
                                    changed |= component.set_field(field, &base).is_ok();
                                }
                                // Правая кнопка по подписи поля: Copy / Paste / Reset to Default (у
                                // переопределённого в префабе поля там своё меню Revert).
                                let overridden = prefab_marks.overridden(type_name, field, &value);
                                let label = Rect::from_min_max(pos2(ui.max_rect().left(), rect.top()), pos2(rect.left() - 2.0, rect.bottom()));
                                let defaults = &world.registry;
                                let menu = (!overridden)
                                    .then(|| {
                                        field_menu(ui, label, (type_name, index, field), &value, &mut inspector.field_clipboard, &doc, || {
                                            defaults.create(type_name).ok().and_then(|c| c.get_field(field))
                                        })
                                    })
                                    .flatten();
                                if let Some(new) = menu {
                                    changed |= component.set_field(field, &new).is_ok();
                                }
                            }
                        }
                    },
                );
                match command {
                    Some(CardCommand::Reset) => actions.push(Action::ResetComponent { object: id, index }),
                    Some(CardCommand::Remove) => actions.push(Action::RemoveComponent { object: id, index }),
                    Some(CardCommand::MoveUp) => {
                        actions.push(Action::MoveComponent { object: id, from: index, to: index.saturating_sub(1) });
                    }
                    Some(CardCommand::MoveDown) => {
                        actions.push(Action::MoveComponent { object: id, from: index, to: index + 1 });
                    }
                    Some(CardCommand::DropFrom(from)) => {
                        actions.push(Action::MoveComponent { object: id, from, to: index });
                    }
                    Some(CardCommand::Copy) => {
                        let values = component
                            .field_names()
                            .iter()
                            .filter_map(|field| Some((*field, component.get_field(field)?)))
                            .collect();
                        inspector.clipboard = Some((type_name, values));
                    }
                    Some(CardCommand::Paste) => {
                        if let Some((_, values)) = &inspector.clipboard {
                            for (field, value) in values {
                                changed |= component.set_field(field, value).is_ok();
                            }
                        }
                    }
                    None => {}
                }
            }

            // --- Add Component ---
            separator(ui);
            ui.add_space(theme::SP_M);
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(theme::SP_M, 0.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    add_component(
                        ui,
                        inspector,
                        actions,
                        dialog,
                        &add_entries,
                        &mut prefs.recent_components,
                        &present,
                        id,
                        can_script,
                        script_rust,
                    );
                });
            ui.add_space(theme::SP_XL);
            inspector.overflow = (ui.min_rect().right() - left - inner_width).max(0.0);
        });

    let _ = assets_root;
    if changed {
        editor.touch();
    }
    crate::inspector_refs::finish(editor, ui.ctx());
    if open_tags {
        editor.show_project_settings_page("tags-and-layers");
    }
}

/// Инспектор без выделения: подсказка, сводка сцены и горячие клавиши.
fn empty(editor: &Editor, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("inspector_empty")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            theme::empty_state(
                ui,
                Icon::Inspector,
                tr_s("Nothing selected"),
                tr_s("Select an object in the Hierarchy or the Scene view\nto see and edit its components."),
            );
            ui.add_space(theme::SP_XL);
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(theme::SP_XL, 0.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let scene = &editor.app.world.scene;
                    let components: usize = scene
                        .ids()
                        .iter()
                        .filter_map(|id| scene.get(*id))
                        .map(|o| o.components().count())
                        .sum();
                    section_title(ui, tr_s("Scene"));
                    egui::Grid::new("inspector_scene_summary")
                        .num_columns(2)
                        .spacing(vec2(theme::SP_L, theme::SP_S))
                        .show(ui, |ui| {
                            theme::key_value(ui, tr_s("Name"), scene.name.as_str());
                            theme::key_value(ui, tr_s("Objects"), scene.len().to_string());
                            theme::key_value(ui, tr_s("Components"), components.to_string());
                            theme::key_value(
                                ui,
                                tr_s("File"),
                                editor
                                    .scene_path()
                                    .map_or(tr_s("not saved yet").to_string(), |p| format!("Assets/{p}")),
                            );
                        });
                    ui.add_space(theme::SP_XL);
                    section_title(ui, tr_s("Shortcuts"));
                    egui::Grid::new("inspector_shortcuts")
                        .num_columns(2)
                        .spacing(vec2(theme::SP_L, theme::SP_S))
                        .show(ui, |ui| {
                            // Текущие сочетания из реестра (Preferences → Shortcuts).
                            let key = |id: &str| crate::shortcuts::label(ui.ctx(), id);
                            let tools = [key("tool-move"), key("tool-rotate"), key("tool-scale")]
                                .into_iter()
                                .filter(|k| !k.is_empty())
                                .collect::<Vec<_>>()
                                .join("  ");
                            let rows = [
                                (tools, tr_s("Move, rotate, scale")),
                                (key("frame-selected"), tr_s("Frame the selection")),
                                (key("duplicate"), tr_s("Duplicate")),
                                (key("rename"), tr_s("Rename")),
                                (key("delete"), tr_s("Delete")),
                                (key("create-empty"), tr_s("New empty object")),
                                (key("play"), tr_s("Play / Stop")),
                                (key("save"), tr_s("Save the scene")),
                            ];
                            for (keys, what) in rows.into_iter().filter(|(keys, _)| !keys.is_empty()) {
                                ui.label(egui::RichText::new(keys).font(theme::mono()).color(theme::TEXT));
                                ui.label(egui::RichText::new(what).color(theme::TEXT_MUTED));
                                ui.end_row();
                            }
                        });
                });
        });
}

pub(crate) fn section_title(ui: &mut egui::Ui, title: &str) {
    theme::section_label(ui, title);
    ui.add_space(theme::SP_S);
}

fn caption(ui: &mut egui::Ui, text: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::FIELD_H), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, text, theme::body(), theme::TEXT_MUTED);
}

pub(crate) fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, theme::LINE));
}

fn prefab_row(
    ui: &mut egui::Ui,
    prefab: &str,
    id: GameObjectId,
    actions: &mut Vec<Action>,
    assets: &[AssetEntry],
    project: &mut crate::project::ProjectState,
    bottom_tab: &mut BottomTab,
) {
    egui::Frame::none()
        .fill(theme::mix(theme::BG_PANEL, theme::PREFAB, 0.08))
        .rounding(theme::R_S)
        .inner_margin(egui::Margin::symmetric(theme::SP_S + 2.0, theme::SP_XS))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Ссылка на префаб — путь или GUID: показываем имя файла, полный путь — в подсказке.
            let asset = assets.iter().find(|a| a.path == prefab || a.guid.to_string().eq_ignore_ascii_case(prefab));
            let path = asset.map_or(prefab, |a| a.path.as_str()).to_string();
            let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".prefab").to_string();
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Prefab, theme::PREFAB);
                ui.add(egui::Label::new(egui::RichText::new(name).color(theme::PREFAB)).truncate()).on_hover_text(format!(
                    "Assets/{path}\n{}",
                    tr_s("This object is an instance of the prefab: the scene stores only what differs")
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::controls::button(ui, tr_s("Unpack")).on_hover_text(tr_s("Turn the instance into ordinary objects")).clicked() {
                        actions.push(Action::UnpackPrefab(id));
                    }
                    if asset.is_some()
                        && crate::controls::button(ui, tr_s("Select")).on_hover_text(tr_s("Show the prefab in the Project window")).clicked()
                    {
                        project.reveal(&path);
                        *bottom_tab = BottomTab::Project;
                    }
                });
            });
        });
}

/// Меню поля по правой кнопке на его подписи: Copy, Paste (значение того же вида),
/// Reset to Default (значение нового компонента). Возвращает новое значение поля.
fn field_menu(
    ui: &mut egui::Ui,
    label: Rect,
    key: (&'static str, usize, &str),
    value: &Value,
    clipboard: &mut Option<Value>,
    doc: &str,
    default: impl FnOnce() -> Option<Value>,
) -> Option<Value> {
    let response = ui.interact(label, egui::Id::new(("field_menu", key.0, key.1, key.2)), Sense::click());
    let response = if doc.is_empty() { response } else { response.on_hover_text(doc) };
    let mut chosen = None;
    let mut reset = false;
    response.context_menu(|ui| {
        theme::menu_body(ui);
        if item(tr_s("Copy")).icon(Icon::Copy).show(ui) {
            *clipboard = Some(value.clone());
        }
        let fits = clipboard.as_ref().is_some_and(|c| std::mem::discriminant(c) == std::mem::discriminant(value));
        if item(tr_s("Paste")).enabled_if(fits, tr_s("Copy a value of the same kind first")).show(ui) {
            chosen = clipboard.clone();
        }
        theme::menu_separator(ui);
        if item(tr_s("Reset to Default")).icon(Icon::Reset).show(ui) {
            reset = true;
        }
    });
    if reset {
        chosen = default();
    }
    chosen
}

/// Карточка компонента: заголовок с иконкой и меню «⋮», тело сворачивается.
/// Возвращает команду из меню, если её выбрали.
pub(crate) fn card(
    ui: &mut egui::Ui,
    salt: (&str, &str, usize),
    icon: Icon,
    title: &str,
    can_paste: bool,
    position: Option<(usize, usize)>,
    body: impl FnOnce(&mut egui::Ui),
) -> Option<CardCommand> {
    let id = ui.make_persistent_id(salt);
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, true));
    let mut command = None;
    let removable = salt.2 != usize::MAX;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW + 2.0), Sense::hover());
    let menu_rect = Rect::from_center_size(pos2(rect.right() - theme::SP_M - 6.0, rect.center().y), vec2(20.0, 20.0));
    // Компонент можно перетащить за заголовок на другой компонент (Transform — нельзя).
    let header = ui.interact(
        Rect::from_min_max(rect.min, pos2(menu_rect.left() - 2.0, rect.bottom())),
        id.with("header"),
        if position.is_some() { Sense::click_and_drag() } else { Sense::click() },
    );
    if let (Some((index, _)), true) = (position, header.drag_started()) {
        header.dnd_set_drag_payload(DragComponent(index));
    }
    if header.dragged() {
        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
            crate::hierarchy::drag_label(ui.ctx(), pointer, icon, title);
        }
    }
    let drop_here = position.is_some()
        && header.dnd_hover_payload::<DragComponent>().is_some_and(|from| Some(from.0) != position.map(|p| p.0));
    if let (Some(_), Some(from)) = (position, header.dnd_release_payload::<DragComponent>()) {
        if Some(from.0) != position.map(|p| p.0) {
            command = Some(CardCommand::DropFrom(from.0));
        }
    }
    let fill = if header.hovered() { theme::mix(theme::BG_RAISED, Color32::WHITE, 0.03) } else { theme::BG_RAISED };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
    if drop_here {
        ui.painter().rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.5_f32, theme::accent()));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + theme::SP_M + 2.0, rect.center().y), vec2(9.0, 9.0)),
        if open { Icon::ChevronDown } else { Icon::ChevronRight },
        theme::TEXT_MUTED,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + theme::SP_M + 20.0, rect.center().y), vec2(14.0, 14.0)),
        icon,
        theme::TEXT_MUTED,
    );
    ui.painter().text(
        pos2(rect.left() + theme::SP_M + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::strong(theme::FONT_TITLE),
        theme::TEXT,
    );
    if header.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }

    let mut menu_ui = ui.new_child(egui::UiBuilder::new().max_rect(menu_rect));
    let response = crate::controls::menu_anchor(
        &mut menu_ui,
        menu_rect.size(),
        false,
        |ui| {
            theme::menu_body(ui);
            if item(tr_s("Reset")).icon(Icon::Reset).show(ui) {
                command = Some(CardCommand::Reset);
            }
            if removable {
                if item(tr_s("Copy Values")).icon(Icon::Copy).show(ui) {
                    command = Some(CardCommand::Copy);
                }
                if item(tr_s("Paste Values"))
                    .enabled_if(can_paste, tr_s("Copy the values of a component of the same type first"))
                    .show(ui)
                {
                    command = Some(CardCommand::Paste);
                }
                theme::menu_separator(ui);
                if let Some((index, count)) = position {
                    if item(tr_s("Move Up")).enabled_if(index > 0, tr_s("Already the first component")).show(ui) {
                        command = Some(CardCommand::MoveUp);
                    }
                    if item(tr_s("Move Down")).enabled_if(index + 1 < count, tr_s("Already the last component")).show(ui) {
                        command = Some(CardCommand::MoveDown);
                    }
                    theme::menu_separator(ui);
                }
                if item(tr_s("Remove Component")).icon(Icon::Trash).show(ui) {
                    command = Some(CardCommand::Remove);
                }
            }
        },
    )
    .response;
    if response.hovered() {
        ui.painter().rect_filled(menu_rect, theme::R_S, theme::BG_HOVER);
    }
    icons::paint(ui.painter(), menu_rect.shrink(4.0), Icon::More, if response.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
    response.on_hover_text(if removable { tr_s("Reset, copy, paste, move or remove") } else { tr_s("Reset the transform") });

    if open {
        egui::Frame::none()
            .inner_margin(egui::Margin { left: theme::SP_M + 10.0, right: theme::SP_M, top: theme::SP_S, bottom: theme::SP_M })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                body(ui);
            });
    }
    command
}

/// Doc-комментарий поля → текст подсказки.
pub(crate) fn clean_doc(doc: &str) -> String {
    doc.lines().map(str::trim).collect::<Vec<_>>().join(" ").trim().to_string()
}

/// Строка «подпись — виджет» с выровненными колонками. Возвращает, изменено ли значение.
///
/// Геометрия как в Unity (`docs/design/ui-guidelines.md`, «Сетка свойств»): колонка
/// подписей — 40 % ширины в пределах [`theme::PROP_LABEL_MIN`]…[`theme::PROP_LABEL_MAX`];
/// колонка значения — остальное, но не шире [`theme::PROP_VALUE_MAX`] (в широком
/// инспекторе поля не растягиваются на весь экран, а стоят слева). Ничто в строке не
/// может сделать её шире окна: значение рисуется в дочерней области фиксированной ширины.
pub(crate) fn property(ui: &mut egui::Ui, label: &str, doc: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let total = ui.available_width();
    let (width, value_width) = theme::property_columns(total);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SP_S;
        let (rect, response) = ui.allocate_exact_size(vec2(width, theme::FIELD_H), Sense::hover());
        let color = if response.hovered() { theme::TEXT } else { theme::TEXT_MUTED };
        let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), theme::body(), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(width - theme::SP_S);
        let galley = ui.painter().layout_job(job);
        let truncated = galley.rect.width() + 1.0 < ui.painter().layout_no_wrap(label.to_string(), theme::body(), color).rect.width();
        ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y * 0.5), galley, color);
        match (doc.is_empty(), truncated) {
            (false, _) => {
                response.on_hover_text(format!("{label}\n{doc}"));
            }
            (true, true) => {
                response.on_hover_text(label);
            }
            (true, false) => {}
        }
        let column = Rect::from_min_size(
            pos2(rect.right() + theme::SP_S, rect.top()),
            vec2(value_width, theme::FIELD_H),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(column)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_max_width(value_width);
        // Шире колонки ничего не рисуется (по высоте — сколько нужно спискам).
        let clip = ui.clip_rect();
        child.set_clip_rect(Rect::from_min_max(
            pos2(column.left().max(clip.left()), clip.top()),
            pos2((column.right() + 1.0).min(clip.right()), clip.bottom()),
        ));
        let changed = add(&mut child);
        // Высота строки — по содержимому (списки); ширина — ровно колонка.
        let used = child.min_rect();
        ui.allocate_rect(Rect::from_min_max(column.min, pos2(column.right(), used.bottom().max(column.bottom()))), Sense::hover());
        changed
    })
    .inner
}

/// Ряд чисел с цветными метками осей X / Y / Z (W).
pub(crate) fn axes(ui: &mut egui::Ui, values: &mut [f32], speed: f64) -> bool {
    axes_mixed(ui, values, &[], speed).is_some()
}

/// То же для нескольких объектов: `mixed[i]` — у объектов разные значения оси `i`,
/// поле показывает «—»; потянуть или вписать — задать всем. Возвращает номер изменённой
/// оси.
pub(crate) fn axes_mixed(ui: &mut egui::Ui, values: &mut [f32], mixed: &[bool], speed: f64) -> Option<usize> {
    const LABELS: [(&str, Color32); 4] = [
        ("X", theme::AXIS_X),
        ("Y", theme::AXIS_Y),
        ("Z", theme::AXIS_Z),
        ("W", theme::TEXT_DISABLED),
    ];
    let count = values.len() as f32;
    let mut changed = None;
    ui.scope(|ui| {
        let spacing = theme::SP_S;
        ui.spacing_mut().item_spacing.x = spacing;
        ui.spacing_mut().button_padding.x = 4.0;
        // Ячейки делят колонку значения поровну; узкая колонка — ячейки сжимаются, а не
        // раздвигают окно.
        let cell = ((ui.available_width() - spacing * (count - 1.0)) / count).max(12.0);
        for (index, (value, (label, color))) in values.iter_mut().zip(LABELS).enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(cell, theme::FIELD_H), Sense::hover());
            let tag = Rect::from_min_size(rect.min, vec2(12.0, rect.height()));
            let field = Rect::from_min_max(pos2(tag.right(), rect.top()), rect.max);
            let mut drag = egui::DragValue::new(value).speed(speed).max_decimals(3);
            if mixed.get(index).copied().unwrap_or(false) {
                drag = drag.custom_formatter(|_, _| "—".to_string());
            }
            // Ответ — на всё поле: в режиме ввода egui сужает текстовое поле внутри.
            let mut response = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(field), |ui| ui.add_sized(field.size(), drag)).inner;
            response.rect = field;
            ui.painter().text(
                tag.center() - vec2(1.0, 0.0),
                Align2::CENTER_CENTER,
                label,
                theme::caption(),
                theme::mix(color, theme::TEXT, 0.25),
            );
            // Тонкая полоса цвета оси на левом крае поля, как в Unity.
            ui.painter().rect_filled(
                Rect::from_min_size(field.min + vec2(0.0, 1.0), vec2(2.0, field.height() - 2.0)),
                egui::Rounding { nw: theme::R_S, sw: theme::R_S, ne: 0.0, se: 0.0 },
                color,
            );
            if response.changed() {
                changed = Some(index);
            }
        }
    });
    changed
}

/// Виджет по типу значения. Возвращает `true`, если значение изменили.
pub(crate) fn value_widget(
    ui: &mut egui::Ui,
    value: &mut Value,
    meta: FieldMeta,
    assets: &[AssetEntry],
    asset_filter: &mut String,
    salt: impl std::hash::Hash + Copy,
) -> bool {
    let width = ui.available_width();
    match value {
        Value::Bool(flag) => ui.checkbox(flag, "").changed(),
        Value::Int(number) => crate::controls::number_field(ui, number, 0.1, width).changed(),
        Value::Float(number) => crate::controls::number_field(ui, number, 0.05, width).changed(),
        Value::Str(text) => crate::controls::text_field(ui, text, "", width).changed(),
        Value::Vec2(x, y) => {
            let mut v = [*x, *y];
            let changed = axes(ui, &mut v, 0.05);
            (*x, *y) = (v[0], v[1]);
            changed
        }
        Value::Vec3(x, y, z) => {
            let mut v = [*x, *y, *z];
            let changed = axes(ui, &mut v, 0.05);
            (*x, *y, *z) = (v[0], v[1], v[2]);
            changed
        }
        Value::Quat(x, y, z, w) => {
            let mut v = [*x, *y, *z, *w];
            let changed = axes(ui, &mut v, 0.01);
            (*x, *y, *z, *w) = (v[0], v[1], v[2], v[3]);
            changed
        }
        Value::Color(r, g, b, a) => color_widget(ui, r, g, b, a, salt),
        Value::Enum(current) => {
            let mut changed = false;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            crate::controls::combo_ex(ui, egui::Id::new(("enum_field", salt)), theme::pretty_type(current), width, |ui| {
                    for variant in meta.variants {
                        if ui.selectable_label(current == variant, theme::pretty_type(variant)).clicked() {
                            *current = variant.to_string();
                            changed = true;
                        }
                    }
                });
            changed
        }
        Value::Asset(guid_text) => asset_widget(ui, guid_text, meta, assets, asset_filter, salt),
        Value::Object(uid) => crate::inspector_refs::object_field(ui, uid, meta, salt, false),
        Value::List(items) => list_widget(ui, items, meta, assets, asset_filter, salt),
        Value::Nil => {
            ui.label(egui::RichText::new("—").color(theme::TEXT_MUTED));
            false
        }
    }
}

/// Цвет: образец с палитрой и шестнадцатеричный код, который можно вписать.
pub(crate) fn color_widget(ui: &mut egui::Ui, r: &mut f32, g: &mut f32, b: &mut f32, a: &mut f32, salt: impl std::hash::Hash) -> bool {
    // Цвет движка линейный — как и у этого виджета egui.
    let mut rgba = [*r, *g, *b, *a];
    let mut changed = false;
    ui.spacing_mut().interact_size = vec2(44.0, theme::FIELD_H);
    changed |= crate::controls::color_button_rgba(ui, &mut rgba).changed();
    let srgb = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], 1.0);
    let [red, green, blue, _] = Color32::from(srgb).to_array();
    let id = egui::Id::new(("color_hex", salt));
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .filter(|_| ui.memory(|m| m.has_focus(id)))
        .unwrap_or_else(|| format!("#{red:02x}{green:02x}{blue:02x}"));
    let edit = crate::controls::text_field_font(ui, Some(id), &mut text, "", f32::INFINITY, theme::mono());
    if edit.changed() {
        if let Ok(color) = Color32::from_hex(text.trim()) {
            let linear = egui::Rgba::from(color);
            rgba[0] = linear.r();
            rgba[1] = linear.g();
            rgba[2] = linear.b();
            changed = true;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    (*r, *g, *b, *a) = (rgba[0], rgba[1], rgba[2], rgba[3]);
    changed
}

/// Список: элементы своими виджетами, удаление, добавление копии последнего.
fn list_widget(
    ui: &mut egui::Ui,
    items: &mut Vec<Value>,
    meta: FieldMeta,
    assets: &[AssetEntry],
    asset_filter: &mut String,
    salt: impl std::hash::Hash + Copy,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for (index, value) in items.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, theme::FIELD_H), Sense::hover());
                ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, index.to_string(), theme::mono(), theme::TEXT_DISABLED);
                let width = (ui.available_width() - 24.0).max(0.0);
                theme::fixed(ui, vec2(width, theme::FIELD_H), |ui| {
                    changed |= value_widget(ui, value, meta, assets, asset_filter, egui::Id::new(salt).with(index));
                });
                if icons::small_button(ui, Icon::Close, tr_s("Remove this element")).clicked() {
                    remove = Some(index);
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(trn!("{n} element", "{n} elements", items.len())).color(theme::TEXT_MUTED));
            let add = ui.add_enabled_ui(!items.is_empty(), |ui| crate::controls::button(ui, tr_s("Add"))).inner;
            let add = add.on_disabled_hover_text(
                tr_s("The element type of an empty list is unknown here: add the first element in code or in the scene file"),
            );
            if add.on_hover_text(tr_s("Add a copy of the last element")).clicked() {
                if let Some(last) = items.last().cloned() {
                    items.push(last);
                    changed = true;
                }
            }
        });
    });
    if let Some(index) = remove {
        items.remove(index);
        changed = true;
    }
    changed
}

/// Слот ассета: кнопка с именем; по щелчку — список с поиском. Принимает
/// перетаскивание из окна Project.
pub(crate) fn asset_widget(
    ui: &mut egui::Ui,
    guid_text: &mut String,
    meta: FieldMeta,
    assets: &[AssetEntry],
    filter: &mut String,
    salt: impl std::hash::Hash,
) -> bool {
    let current = Guid::parse(guid_text).and_then(|guid| assets.iter().find(|a| a.guid == guid));
    let kind = if meta.asset_kind.is_empty() { tr_s("Asset") } else { tr_s(meta.asset_kind) };
    let (label, label_color) = match current {
        Some(asset) => (file_name(&asset.path).to_string(), theme::TEXT),
        None if guid_text.is_empty() => (trf!("None ({kind})", kind = kind), theme::TEXT_MUTED),
        None => (tr_s("Missing asset").to_string(), theme::ERROR),
    };
    let fits = |asset: &AssetEntry| meta.asset_kind.is_empty() || asset.importer == meta.asset_kind;
    let mut changed = false;

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::FIELD_H), Sense::click());
    let popup = egui::Id::new(("asset_popup", salt));
    let open = ui.memory(|m| m.is_popup_open(popup));
    let border = if open {
        theme::accent()
    } else if response.hovered() {
        theme::LINE_HOVER
    } else {
        theme::LINE_STRONG
    };
    crate::controls::paint_frame(ui.painter(), rect, theme::R_S, theme::BG_INSET, border);
    let (icon, icon_color) = current.map_or((asset_icon(meta.asset_kind).0, theme::TEXT_DISABLED), |a| asset_icon(&a.importer));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 11.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
        icon,
        icon_color,
    );
    let picker = Rect::from_min_max(pos2(rect.right() - 18.0, rect.top()), rect.max);
    let text_rect = Rect::from_min_max(pos2(rect.left() + 21.0, rect.top()), pos2(picker.left() - 2.0, rect.bottom()));
    ui.painter()
        .with_clip_rect(text_rect)
        .text(text_rect.left_center(), Align2::LEFT_CENTER, &label, theme::body(), label_color);
    // Кнопка выбора — как кружок-«мишень» у поля ссылки в Unity.
    ui.painter().vline(picker.left(), picker.y_range().shrink(3.0), Stroke::new(1.0_f32, theme::LINE_SOFT));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(picker.center(), vec2(10.0, 10.0)),
        Icon::Target,
        if response.hovered() { theme::TEXT } else { theme::TEXT_MUTED },
    );
    if response.clicked() {
        filter.clear();
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    if let Some(asset) = current {
        response.clone().on_hover_text(format!("Assets/{}", asset.path));
    }

    egui::popup_below_widget(ui, popup, &response, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(rect.width().max(260.0));
        theme::search_field(ui, filter, &trf!("Search {kind} assets", kind = kind), f32::INFINITY).request_focus();
        ui.add_space(theme::SP_S);
        let needle = filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("asset_list", popup))
            .max_height(300.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if picker_row(ui, Icon::Close, theme::TEXT_MUTED, tr_s("None"), "", guid_text.is_empty()) {
                    guid_text.clear();
                    changed = true;
                    ui.memory_mut(|m| m.close_popup());
                }
                let mut shown = 0;
                for asset in assets
                    .iter()
                    .filter(|asset| fits(asset))
                    .filter(|asset| needle.is_empty() || asset.path.to_lowercase().contains(&needle))
                {
                    shown += 1;
                    let (icon, color) = asset_icon(&asset.importer);
                    let folder = asset.path.rsplit_once('/').map_or("", |(dir, _)| dir);
                    let selected = current.is_some_and(|c| c.guid == asset.guid);
                    if picker_row(ui, icon, color, file_name(&asset.path), folder, selected) {
                        *guid_text = asset.guid.to_string();
                        changed = true;
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
                if shown == 0 {
                    ui.add_space(theme::SP_M);
                    ui.label(egui::RichText::new(trf!("No {kind} assets match", kind = kind)).color(theme::TEXT_MUTED));
                }
            });
    });

    if let Some(dragged) = response.dnd_hover_payload::<DragAsset>() {
        let color = if meta.asset_kind.is_empty() || dragged.importer == meta.asset_kind {
            theme::OK
        } else {
            theme::ERROR
        };
        ui.painter().rect_stroke(rect, theme::R_S, Stroke::new(1.5_f32, color));
    }
    if let Some(dropped) = response.dnd_release_payload::<DragAsset>() {
        if meta.asset_kind.is_empty() || dropped.importer == meta.asset_kind {
            *guid_text = dropped.guid.to_string();
            changed = true;
        } else {
            log::warn!(
                "`{}` is a {} asset, but this field takes {}",
                dropped.path,
                dropped.importer,
                meta.asset_kind
            );
        }
    }
    changed
}

/// Строка выпадающего списка ассетов.
fn picker_row(ui: &mut egui::Ui, icon: Icon, color: Color32, name: &str, folder: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW), Sense::click());
    let hovered = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, theme::R_S, theme::selection_fill());
    } else if hovered {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(theme::ICON_S + 1.0, theme::ICON_S + 1.0)),
        icon,
        color,
    );
    let name_galley = ui.painter().layout_no_wrap(name.to_string(), theme::body(), Color32::PLACEHOLDER);
    let name_width = name_galley.size().x;
    ui.painter().galley(
        pos2(rect.left() + 26.0, rect.center().y - name_galley.size().y * 0.5),
        name_galley,
        if hovered || selected { theme::TEXT_STRONG } else { theme::TEXT },
    );
    if !folder.is_empty() {
        ui.painter().text(
            pos2(rect.left() + 26.0 + name_width + theme::SP_M, rect.center().y),
            Align2::LEFT_CENTER,
            folder,
            theme::caption(),
            theme::TEXT_DISABLED,
        );
    }
    response.clicked()
}

/// Заголовок категории в меню Add Component. `collapsed`: `Some` — заголовок сворачивает
/// категорию (`true` — щелчок развернул бы её), `None` — просто подпись. Возвращает «щёлкнули».
fn category_header(ui: &mut egui::Ui, title: &str, icon: Icon, count: usize, collapsed: Option<bool>) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW), Sense::hover());
    let response = ui.interact(rect, egui::Id::new(("add_category", title)), Sense::click());
    if response.hovered() && collapsed.is_some() {
        ui.painter().rect_filled(rect, 0.0, theme::BG_HOVER);
    }
    let mut x = rect.left() + 12.0;
    if let Some(collapsed) = collapsed {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x, rect.center().y), vec2(9.0, 9.0)),
            if collapsed { Icon::ChevronRight } else { Icon::ChevronDown },
            theme::TEXT_MUTED,
        );
        x += 14.0;
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(x, rect.center().y), vec2(12.0, 12.0)), icon, theme::TEXT_MUTED);
    ui.painter().text(
        pos2(x + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        theme::strong(theme::FONT_SECTION),
        theme::TEXT_MUTED,
    );
    ui.painter().text(
        pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        count.to_string(),
        theme::caption(),
        theme::TEXT_DISABLED,
    );
    collapsed.is_some() && response.clicked()
}

/// Кнопка Add Component и окно выбора с поиском и категориями.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_component(
    ui: &mut egui::Ui,
    inspector: &mut InspectorState,
    actions: &mut Vec<Action>,
    dialog: &mut Option<Dialog>,
    entries: &[crate::add_menu::AddEntry],
    recent: &mut Vec<String>,
    present: &[&'static str],
    id: GameObjectId,
    can_script: bool,
    script_rust: &mut bool,
) {
    let popup = ui.make_persistent_id("add_component_popup");
    let button = theme::primary_button_wide(ui, Icon::Plus, tr_s("Add Component"));
    if button.clicked() {
        inspector.add_filter.clear();
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    egui::popup_below_widget(ui, popup, &button, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(button.rect.width().clamp(280.0, 420.0));
        let search = theme::search_field(ui, &mut inspector.add_filter, tr_s("Search components"), f32::INFINITY);
        search.request_focus();
        ui.add_space(theme::SP_S);
        let query = inspector.add_filter.trim().to_string();
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut first_match: Option<&'static str> = None;
        let mut picked: Option<&'static str> = None;
        let mut toggled: Option<&'static str> = None;
        egui::ScrollArea::vertical()
            .id_salt("add_component_list")
            .max_height(360.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut entry_row = |ui: &mut egui::Ui, entry: &crate::add_menu::AddEntry, hint: Option<&str>| {
                    let already = present.contains(&entry.kind);
                    if !already && first_match.is_none() {
                        first_match = Some(entry.kind);
                    }
                    let mut row = item(&entry.label)
                        .icon(catalog::component_icon(entry.kind))
                        .enabled_if(!already, tr_s("Already on this object"));
                    if let Some(hint) = hint {
                        row = row.shortcut(hint);
                    }
                    if row.show(ui) {
                        picked = Some(entry.kind);
                    }
                };
                if query.is_empty() {
                    let recently = crate::add_menu::recent_entries(entries, recent);
                    if !recently.is_empty() {
                        category_header(ui, tr_s("Recent"), Icon::Refresh, recently.len(), None);
                        for entry in recently {
                            entry_row(ui, entry, Some(entry.category));
                        }
                    }
                    for (category, items) in crate::add_menu::grouped(entries) {
                        let collapsed = inspector.add_collapsed.contains(category);
                        let icon = crate::add_menu::category_icon(category);
                        if category_header(ui, tr_s(category), icon, items.len(), Some(collapsed)) {
                            toggled = Some(category);
                        }
                        if !collapsed {
                            for entry in items {
                                entry_row(ui, entry, None);
                            }
                        }
                    }
                } else {
                    // Поиск по всем категориям: лучшие совпадения первыми.
                    let found = crate::add_menu::search(entries, &query);
                    for entry in &found {
                        entry_row(ui, entry, Some(entry.category));
                    }
                    if found.is_empty() {
                        ui.add_space(theme::SP_M);
                        ui.label(egui::RichText::new(tr_s("No component matches")).color(theme::TEXT_MUTED));
                    }
                }
            });
        if let Some(category) = toggled {
            if !inspector.add_collapsed.remove(category) {
                inspector.add_collapsed.insert(category.to_string());
            }
        }
        if picked.is_none() && enter {
            picked = first_match;
        }
        if let Some(kind) = picked {
            actions.push(Action::AddComponent { object: id, kind: kind.to_string() });
            crate::add_menu::note_used(recent, kind);
            ui.memory_mut(|memory| memory.close_popup());
        }
        theme::menu_separator(ui);
        // Как в Unity: новый скрипт создаётся прямо отсюда и сразу вешается на объект.
        // Скрипт на Rust повесится на объект, когда редактор перезапустится с новым кодом.
        if item(tr_s("New Rust Script…")).icon(Icon::Script).show(ui) {
            *script_rust = true;
            *dialog = Some(Dialog::NewScript {
                name: inspector.add_filter.trim().to_string(),
                object: Some(id),
            });
            ui.memory_mut(|memory| memory.close_popup());
        }
        if item(tr_s("New C++ Script…"))
            .icon(Icon::Script)
            .enabled_if(can_script, "Start the editor with `EditorConfig::cpp_scripts(dir)` to create scripts")
            .show(ui)
        {
            *script_rust = false;
            *dialog = Some(Dialog::NewScript {
                name: inspector.add_filter.trim().to_string(),
                object: Some(id),
            });
            ui.memory_mut(|memory| memory.close_popup());
        }
        // Сценарий не собирается: `ScenarioRunner` вешается на объект сразу.
        if item(tr_s("New Scenario Script…")).icon(Icon::Script).show(ui) {
            *dialog = Some(Dialog::NewScenario {
                name: inspector.add_filter.trim().to_string(),
                folder: String::new(),
                object: Some(id),
            });
            ui.memory_mut(|memory| memory.close_popup());
        }
    });
}
