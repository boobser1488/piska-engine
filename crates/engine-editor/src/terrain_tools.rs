//! Инструменты рельефа: GameObject → 3D Object → Terrain / Water, кисти в виде Scene
//! (поднять, опустить, сгладить, выровнять, раскрасить слой) с кругом на
//! поверхности, «Snap to Terrain», инспектор `Terrain` с черновиком параметров
//! формы и кнопкой Regenerate (сохранить или сбросить скульпт — спрашивает).
//!
//! Мазок кисти — один шаг Undo: он открывается при нажатии (`touch`) и закрывается,
//! когда кнопку отпустили. Правки хранятся в поле `edits` компонента и сохраняются
//! вместе со сценой.

use std::collections::HashMap;

use egui::{vec2, Align2, PointerButton, Rect, Stroke};
use engine_assets::AssetDatabase;
use engine_core::prelude::*;
use engine_core::reflect::Reflect;
use engine_render::{BrushTool, MeshRenderer, Terrain, TerrainBrush, Water, TERRAIN_LAYERS, TERRAIN_SHAPE_FIELDS};

use crate::icons::Icon;
use crate::inspector::{clean_doc, property, value_widget};
use crate::state::{AssetEntry, Editor, Mode};
use crate::theme::{self, item};
use crate::view3d::Projector;

/// Состояние инструментов рельефа.
#[derive(Default)]
pub(crate) struct TerrainTools {
    /// Выбранная кисть; `None` — обычные гизмо.
    pub tool: Option<BrushTool>,
    pub brush: TerrainBrush,
    /// Точка рельефа под курсором (мир) — центр круга кисти.
    pub hover: Option<Vec3>,
    /// Идёт мазок (кнопка нажата на рельефе).
    stroking: bool,
    /// Черновик параметров формы: (объект, копия компонента).
    draft: Option<(GameObjectId, Terrain)>,
    /// Открыт вопрос «сохранить скульпт?» для этого объекта.
    confirm: Option<GameObjectId>,
}

const TOOLS: [(Option<BrushTool>, &str, &str); 6] = [
    (None, "Off", "No brush: select and move objects as usual"),
    (Some(BrushTool::Raise), "Raise", "Raise the ground under the brush"),
    (Some(BrushTool::Lower), "Lower", "Lower the ground under the brush"),
    (Some(BrushTool::Smooth), "Smooth", "Smooth bumps and sharp edges"),
    (Some(BrushTool::Flatten), "Flatten", "Pull the ground to a set height (Ctrl+click picks the height)"),
    (Some(BrushTool::Paint), "Paint", "Paint a texture layer over the automatic ones"),
];

/// Выделенный объект с рельефом.
fn selected_terrain(editor: &Editor) -> Option<GameObjectId> {
    let id = editor.selection?;
    editor.app.world.scene.get_component::<Terrain>(id).is_some().then_some(id)
}

/// Кисть рельефа сейчас перехватывает левую кнопку мыши в виде Scene.
pub(crate) fn brush_active(editor: &Editor) -> bool {
    editor.terrain_tools.tool.is_some()
        && editor.mode == Mode::Edit
        && editor.scene_3d
        && selected_terrain(editor).is_some()
}

/// Пункты меню GameObject → 3D Object.
pub(crate) fn menu_items(editor: &mut Editor, ui: &mut egui::Ui) {
    if item("Terrain").icon(Icon::Plane).show(ui) {
        create_terrain(editor);
    }
    if item("Water").icon(Icon::Plane).show(ui) {
        create_water(editor);
    }
}

/// Пункт меню GameObject: поставить выделенный объект на рельеф.
pub(crate) fn snap_menu_item(editor: &mut Editor, ui: &mut egui::Ui) {
    let can = editor
        .selection
        .is_some_and(|id| editor.app.world.scene.get_component::<Terrain>(id).is_none());
    if item("Snap to Terrain")
        .icon(Icon::Move)
        .enabled_if(can, "Select an object above a terrain")
        .show(ui)
    {
        snap_selection(editor);
    }
}

/// Создать рельеф с центром в начале координат.
pub(crate) fn create_terrain(editor: &mut Editor) -> GameObjectId {
    editor.touch();
    // Больше суши, чем в образце: новый рельеф — холмы, а не берег и мокрое дно.
    let terrain = Terrain { size: 512.0, land_bias: 0.6, ..Terrain::default() };
    let half = terrain.extent() * 0.5;
    let id = editor.app.world.scene.spawn("Terrain");
    if let Some(object) = editor.app.world.scene.get_mut(id) {
        object.transform.position = Vec3::new(-half, 0.0, -half);
        object.add(terrain);
    }
    editor.selection = Some(id);
    id
}

/// Создать воду (море до горизонта) на нулевой высоте.
pub(crate) fn create_water(editor: &mut Editor) -> GameObjectId {
    editor.touch();
    let id = editor.app.world.scene.spawn("Water");
    if let Some(object) = editor.app.world.scene.get_mut(id) {
        object.add(Water::default());
    }
    editor.selection = Some(id);
    id
}

/// Поставить выделенный объект на рельеф под ним (меш — нижней гранью).
pub(crate) fn snap_selection(editor: &mut Editor) -> bool {
    let Some(id) = editor.selection else { return false };
    // Нижняя грань меша — на землю; у остальных — позиция объекта.
    let offset = {
        let world = &editor.app.world;
        world.scene.get(id).and_then(|object| object.get::<MeshRenderer>()).map_or(0.0, |mesh| {
            let (min, _) = crate::view3d::mesh_bounds(
                mesh,
                world.resources.get::<AssetDatabase>(),
                &mut editor.viewport.mesh_bounds,
            );
            let scale = world.scene.world_matrix(id).to_scale_rotation_translation().0;
            -min.y * scale.y
        })
    };
    editor.touch();
    let done = engine_render::snap_to_terrain(&mut editor.app.world, id, offset);
    if !done {
        log::warn!("Snap to Terrain: there is no active terrain under the selected object");
    }
    done
}

/// Панель кистей, круг кисти и мазки в виде Scene (3D).
pub(crate) fn scene_view(
    editor: &mut Editor,
    ui: &egui::Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    projector: &Projector,
) {
    let Some(id) = selected_terrain(editor).filter(|_| editor.mode == Mode::Edit) else {
        editor.terrain_tools.hover = None;
        editor.terrain_tools.stroking = false;
        return;
    };
    let rect = editor.viewport.rect;
    toolbar(ui.ctx(), rect, &mut editor.terrain_tools);
    if editor.terrain_tools.tool.is_none() {
        editor.terrain_tools.hover = None;
        return;
    }

    // Точка рельефа под курсором.
    let pointer = ui.ctx().pointer_latest_pos().filter(|p| rect.contains(*p) && response.hovered());
    let origin = editor.app.world.scene.world_matrix(id).w_axis.truncate();
    let hit = pointer.and_then(|p| {
        let (ray_origin, direction) = projector.ray(p);
        engine_render::terrain_raycast(&editor.app.world, ray_origin, direction, 20_000.0).filter(|hit| hit.object == id)
    });
    editor.terrain_tools.hover = hit.map(|h| h.point);

    // Круг кисти на поверхности.
    if let Some(center) = editor.terrain_tools.hover {
        let world = &editor.app.world;
        let Some(terrain) = world.scene.get_component::<Terrain>(id) else { return };
        let sampler = terrain.sampler(world.resources.get::<AssetDatabase>());
        let radius = editor.terrain_tools.brush.radius;
        let ring = |r: f32| -> Vec<egui::Pos2> {
            (0..=64)
                .filter_map(|k| {
                    let a = k as f32 / 64.0 * std::f32::consts::TAU;
                    let (x, z) = (center.x + a.cos() * r, center.z + a.sin() * r);
                    let y = origin.y + sampler.height_local(x - origin.x, z - origin.z) + 0.15;
                    projector.project(Vec3::new(x, y, z))
                })
                .collect()
        };
        let accent = theme::accent();
        painter.add(egui::Shape::line(ring(radius), Stroke::new(2.0_f32, accent)));
        let inner = radius * (1.0 - editor.terrain_tools.brush.falloff.clamp(0.0, 1.0));
        if inner > 0.05 {
            painter.add(egui::Shape::line(ring(inner), Stroke::new(1.0_f32, accent.gamma_multiply(0.6))));
        }
        if let Some(p) = projector.project(center) {
            painter.circle_filled(p, 2.5, accent);
        }
    }

    // Ctrl+щелчок с «Flatten» — взять высоту под курсором.
    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    if editor.terrain_tools.tool == Some(BrushTool::Flatten) && ctrl {
        if response.clicked_by(PointerButton::Primary) {
            if let Some(center) = editor.terrain_tools.hover {
                editor.terrain_tools.brush.target_height = center.y - origin.y;
            }
        }
        return;
    }

    // Мазок: пока левая кнопка зажата на виде.
    let down = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    if !down {
        editor.terrain_tools.stroking = false;
        return;
    }
    let Some(center) = editor.terrain_tools.hover else { return };
    if !editor.terrain_tools.stroking {
        editor.terrain_tools.stroking = true;
        editor.touch(); // один шаг Undo на весь мазок
    }
    let dt = ui.input(|i| i.stable_dt).clamp(1.0 / 240.0, 0.05);
    let mut brush = editor.terrain_tools.brush;
    brush.tool = editor.terrain_tools.tool.unwrap_or(BrushTool::Raise);
    let World { scene, resources, .. } = &mut editor.app.world;
    let assets = resources.get::<AssetDatabase>();
    if let Some(terrain) = scene.get_component_mut::<Terrain>(id) {
        terrain.apply_brush(&brush, center.x - origin.x, center.z - origin.z, dt, assets);
    }
    editor.dirty = true;
    ui.ctx().request_repaint();
}

/// Панель кистей внизу слева вида Scene.
fn toolbar(ctx: &egui::Context, rect: Rect, tools: &mut TerrainTools) {
    egui::Area::new(egui::Id::new("terrain_toolbar"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_bottom() + vec2(10.0, -10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Terrain").strong());
                    for (tool, label, hint) in TOOLS {
                        if ui.selectable_label(tools.tool == tool, label).on_hover_text(hint).clicked() {
                            tools.tool = tool;
                        }
                    }
                });
                if tools.tool.is_none() {
                    return;
                }
                let brush = &mut tools.brush;
                ui.horizontal(|ui| {
                    ui.label("Size");
                    ui.add(egui::Slider::new(&mut brush.radius, 1.0..=200.0).logarithmic(true).suffix(" m"));
                    ui.label("Strength");
                    ui.add(egui::Slider::new(&mut brush.strength, 0.01..=1.0));
                    ui.label("Falloff");
                    ui.add(egui::Slider::new(&mut brush.falloff, 0.0..=1.0));
                });
                match tools.tool {
                    Some(BrushTool::Flatten) => {
                        ui.horizontal(|ui| {
                            ui.label("Height");
                            ui.add(egui::DragValue::new(&mut brush.target_height).speed(0.1).suffix(" m"));
                            ui.label(egui::RichText::new("Ctrl+click picks the height").color(theme::TEXT_MUTED));
                        });
                    }
                    Some(BrushTool::Paint) => {
                        ui.horizontal(|ui| {
                            ui.label("Layer");
                            let current = brush.layer.map_or("Erase paint", |k| TERRAIN_LAYERS[k]);
                            egui::ComboBox::from_id_salt("terrain_paint_layer").selected_text(current).show_ui(ui, |ui| {
                                for (k, name) in TERRAIN_LAYERS.iter().enumerate() {
                                    ui.selectable_value(&mut brush.layer, Some(k), *name);
                                }
                                ui.selectable_value(&mut brush.layer, None, "Erase paint")
                                    .on_hover_text("Back to the automatic height / slope layers");
                            });
                        });
                    }
                    _ => {}
                }
            });
        });
}

/// Строка поля компонента (тот же виджет, что в общем инспекторе).
fn field_row<T: Reflect>(
    ui: &mut egui::Ui,
    component: &mut T,
    field: &'static str,
    label: &str,
    assets: &[AssetEntry],
    asset_filter: &mut String,
    rects: &mut HashMap<(&'static str, &'static str), Rect>,
) -> bool {
    let Some(mut value) = component.get_field(field) else { return false };
    let meta = component.field_meta(field);
    let doc = clean_doc(component.field_doc(field));
    let edited = property(ui, label, &doc, |ui| {
        let widget = ui.scope(|ui| value_widget(ui, &mut value, meta, assets, asset_filter, ("Terrain", field)));
        rects.insert(("Terrain", field), widget.response.rect);
        widget.inner
    });
    edited && component.set_field(field, &value).is_ok()
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::SP_M);
    theme::section_label(ui, text);
    ui.add_space(theme::SP_XS);
}

/// Перенести параметры формы из черновика; `keep` — сохранить скульпт.
fn regenerate(terrain: &mut Terrain, draft: &Terrain, keep: bool) {
    for field in TERRAIN_SHAPE_FIELDS {
        if let Some(value) = draft.get_field(field) {
            let _ = terrain.set_field(field, &value);
        }
    }
    if !keep {
        terrain.clear_edits();
    }
}

/// Тело карточки `Terrain` в инспекторе. Возвращает, изменился ли компонент.
pub(crate) fn inspector_body(
    ui: &mut egui::Ui,
    terrain: &mut Terrain,
    id: GameObjectId,
    tools: &mut TerrainTools,
    assets: &[AssetEntry],
    asset_filter: &mut String,
    rects: &mut HashMap<(&'static str, &'static str), Rect>,
) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 2.0;

    // --- Форма: черновик, применяется кнопкой Regenerate ---
    heading(ui, "Shape");
    if tools.draft.as_ref().is_none_or(|(owner, _)| *owner != id) {
        tools.draft = Some((id, terrain.clone()));
        tools.confirm = None;
    }
    let (_, draft) = tools.draft.as_mut().expect("draft was just set");
    for field in TERRAIN_SHAPE_FIELDS {
        field_row(ui, draft, field, &theme::pretty_name(field), assets, asset_filter, rects);
    }
    let differs = TERRAIN_SHAPE_FIELDS.iter().any(|f| draft.get_field(f) != terrain.get_field(f));
    ui.add_space(theme::SP_S);
    ui.horizontal(|ui| {
        let regenerate_clicked = ui
            .add_enabled(differs, egui::Button::new("Regenerate"))
            .on_hover_text("Apply the shape settings above and rebuild the terrain")
            .clicked();
        if ui.add_enabled(differs, egui::Button::new("Revert")).on_hover_text("Forget the unapplied shape settings").clicked() {
            *draft = terrain.clone();
        }
        if regenerate_clicked {
            if terrain.edits.is_empty() {
                regenerate(terrain, draft, true);
                changed = true;
            } else {
                tools.confirm = Some(id);
            }
        }
    });
    if differs {
        ui.label(egui::RichText::new("Shape settings are applied with Regenerate").color(theme::WARN));
    }

    if tools.confirm == Some(id) {
        let mut answer: Option<Option<bool>> = None;
        egui::Window::new("Regenerate Terrain")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.label("The terrain has sculpting and painting.\nKeep it on top of the new shape, or discard it?");
                ui.add_space(theme::SP_M);
                ui.horizontal(|ui| {
                    if ui.button("Keep Sculpting").clicked() {
                        answer = Some(Some(true));
                    }
                    if ui.button("Discard Sculpting").clicked() {
                        answer = Some(Some(false));
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(None);
                    }
                });
            });
        if let Some(choice) = answer {
            tools.confirm = None;
            if let Some(keep) = choice {
                if let Some((_, draft)) = &tools.draft {
                    regenerate(terrain, draft, keep);
                    changed = true;
                }
            }
        }
    }

    // --- Отрисовка ---
    heading(ui, "Rendering");
    for field in ["view_distance", "lod_distance", "cast_shadows", "sea_level", "snow_height", "desert", "texture_scale"] {
        changed |= field_row(ui, terrain, field, &theme::pretty_name(field), assets, asset_filter, rects);
    }

    // --- Слои ---
    heading(ui, "Layers");
    const LAYER_FIELDS: [(&str, &str); 5] = [
        ("grass_texture", "grass_color"),
        ("dirt_texture", "dirt_color"),
        ("rock_texture", "rock_color"),
        ("sand_texture", "sand_color"),
        ("snow_texture", "snow_color"),
    ];
    for (name, (texture, color)) in TERRAIN_LAYERS.iter().zip(LAYER_FIELDS) {
        changed |= field_row(ui, terrain, texture, &format!("{name} Texture"), assets, asset_filter, rects);
        changed |= field_row(ui, terrain, color, &format!("{name} Tint"), assets, asset_filter, rects);
    }

    // --- Скульпт ---
    heading(ui, "Sculpting");
    ui.horizontal(|ui| {
        let tiles = terrain.edits.tile_count();
        let text = if tiles == 0 {
            "No sculpting or painting yet".to_string()
        } else {
            format!("Edited {tiles} tile(s) of 64 × 64 samples")
        };
        ui.label(egui::RichText::new(text).color(theme::TEXT_MUTED));
        if ui.add_enabled(tiles > 0, egui::Button::new("Clear")).on_hover_text("Remove all sculpting and painting").clicked() {
            terrain.clear_edits();
            changed = true;
        }
    });
    ui.label(
        egui::RichText::new("Brushes: pick a tool in the Scene view (3D) toolbar, then drag over the terrain.")
            .color(theme::TEXT_MUTED),
    );
    if changed {
        if let Some((owner, draft)) = &mut tools.draft {
            if *owner == id {
                // Черновик формы следит за остальными полями (правки, текстуры).
                let keep: Vec<(&str, Value)> = TERRAIN_SHAPE_FIELDS.iter().filter_map(|f| Some((*f, draft.get_field(f)?))).collect();
                *draft = terrain.clone();
                for (field, value) in keep {
                    let _ = draft.set_field(field, &value);
                }
            }
        }
    }
    changed
}

impl Editor {
    /// Кисть рельефа в виде Scene: `None` — обычные гизмо (кнопка Off на панели Terrain).
    pub fn set_terrain_tool(&mut self, tool: Option<BrushTool>) {
        self.terrain_tools.tool = tool;
    }

    pub fn terrain_tool(&self) -> Option<BrushTool> {
        self.terrain_tools.tool
    }

    /// Размер, сила, мягкость, высота (Flatten) и слой (Paint) кисти рельефа.
    pub fn set_terrain_brush(&mut self, brush: TerrainBrush) {
        self.terrain_tools.brush = brush;
    }

    pub fn terrain_brush(&self) -> TerrainBrush {
        self.terrain_tools.brush
    }

    /// Точка рельефа под курсором в последнем кадре (центр круга кисти).
    pub fn terrain_brush_point(&self) -> Option<Vec3> {
        self.terrain_tools.hover
    }

    /// GameObject → 3D Object → Terrain: рельеф 512 м с центром в начале координат.
    pub fn create_terrain(&mut self) -> GameObjectId {
        create_terrain(self)
    }
}
