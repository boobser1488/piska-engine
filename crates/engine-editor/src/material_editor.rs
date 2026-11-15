//! Материалы в редакторе: редактор ассета `.material` в инспекторе (с Undo, запись на
//! диск при каждом изменении, перерисовка сцены и миниатюр), список материалов
//! `MeshRenderer` по частям модели, создание материалов и извлечение их из модели.

use crate::i18n::{n_, tr_s, trf, trn};
use std::collections::HashMap;

use egui::{pos2, vec2, Color32, Rect, Sense};
use engine_assets::{AlphaMode, AssetDatabase, AssetHandle, Guid, Material, Texture};
use engine_core::prelude::*;
use engine_core::reflect::FieldMeta;
use engine_render::MeshRenderer;

use crate::icons::{self, Icon};
use crate::inspector::{asset_widget, color_widget, property};
use crate::preview::{PreviewShape, Previews, Thumb};
use crate::state::{file_name, Action, AssetEntry, Editor};
use crate::theme;
use crate::EditorError;

/// Состояние редактора материалов; живёт в [`Editor`].
#[derive(Default)]
pub(crate) struct MaterialEditor {
    /// Ассет `.material`, открытый в инспекторе (`None` — инспектор показывает объект).
    pub guid: Option<Guid>,
    /// Копия материала, которую правит инспектор, и её GUID.
    loaded: Option<(Guid, Material)>,
    error: Option<String>,
    undo: Vec<Material>,
    redo: Vec<Material>,
    /// Идёт правка (тянут ползунок, выбирают цвет) — это один шаг Undo.
    editing: bool,
    /// Материалы, которые рендерер должен перечитать (их файлы только что записаны).
    pub stale: Vec<Guid>,
    /// Имена частей моделей для слотов `MeshRenderer` (по GUID модели).
    part_names: HashMap<Guid, Vec<String>>,
    /// Сколько материалов описано в файле модели (для «Extract materials»).
    model_material_count: HashMap<Guid, usize>,
    /// Миниатюры текстур для слотов.
    texture_thumbs: HashMap<Guid, Option<egui::TextureHandle>>,
}

const UNDO_LIMIT: usize = 100;

const TEXTURE: FieldMeta = FieldMeta { variants: &[], asset_kind: n_("Texture") };
const MATERIAL: FieldMeta = FieldMeta { variants: &[], asset_kind: n_("Material") };
const SHADER: FieldMeta = FieldMeta { variants: &[], asset_kind: n_("Shader") };

/// Поля встроенного материала `MeshRenderer` (раздел «Inline material (legacy)»).
const INLINE_FIELDS: [&str; 8] = ["color", "texture", "metallic", "roughness", "emissive", "tiling", "grid", "cast_shadows"];

impl MaterialEditor {
    /// Забыть, что было загружено (ассеты перечитаны: файл мог измениться снаружи).
    pub fn assets_changed(&mut self) {
        self.loaded = None;
        self.part_names.clear();
        self.model_material_count.clear();
        self.texture_thumbs.clear();
    }
}

impl Editor {
    /// Открыть материал (путь относительно каталога ассетов) в инспекторе — как щелчок
    /// по нему в окне Project. `false` — такого материала нет.
    pub fn inspect_material(&mut self, relative_path: &str) -> bool {
        let guid = self
            .app
            .world
            .resources
            .get::<AssetDatabase>()
            .and_then(|db| db.guid_of(relative_path))
            .filter(|guid| self.assets.iter().any(|a| a.guid == *guid && a.importer == "Material"));
        self.open_material(guid);
        guid.is_some()
    }

    pub(crate) fn open_material(&mut self, guid: Option<Guid>) {
        let editor = &mut self.material_editor;
        if editor.guid != guid {
            editor.guid = guid;
            editor.undo.clear();
            editor.redo.clear();
            editor.editing = false;
            editor.loaded = None;
            editor.error = None;
        }
    }

    /// Материал, открытый в инспекторе (в том виде, в каком он сейчас на диске).
    pub fn inspected_material(&mut self) -> Option<Material> {
        load(self);
        self.material_editor.loaded.as_ref().map(|(_, m)| m.clone())
    }

    /// Изменить открытый в инспекторе материал — тот же путь, что у полей инспектора:
    /// один шаг Undo, запись файла, перерисовка сцены и миниатюр.
    pub fn edit_material(&mut self, edit: impl FnOnce(&mut Material)) -> bool {
        load(self);
        let Some((_, before)) = self.material_editor.loaded.clone() else {
            return false;
        };
        let mut after = before.clone();
        edit(&mut after);
        if after == before {
            return false;
        }
        push_undo(&mut self.material_editor, before);
        commit(self, after);
        true
    }

    /// Отменить последнюю правку материала. `false` — отменять нечего.
    pub fn undo_material(&mut self) -> bool {
        load(self);
        let Some(previous) = self.material_editor.undo.pop() else {
            return false;
        };
        if let Some((_, current)) = self.material_editor.loaded.clone() {
            self.material_editor.redo.push(current);
        }
        self.material_editor.editing = false;
        commit(self, previous);
        true
    }

    /// Вернуть отменённую правку материала.
    pub fn redo_material(&mut self) -> bool {
        load(self);
        let Some(next) = self.material_editor.redo.pop() else {
            return false;
        };
        if let Some((_, current)) = self.material_editor.loaded.clone() {
            self.material_editor.undo.push(current);
        }
        self.material_editor.editing = false;
        commit(self, next);
        true
    }

    /// Инспектор показывает материал, и у него есть что отменить (Ctrl+Z идёт ему).
    pub(crate) fn material_undo_available(&self) -> bool {
        self.material_editor.guid.is_some() && !self.material_editor.undo.is_empty()
    }

    pub(crate) fn material_redo_available(&self) -> bool {
        self.material_editor.guid.is_some() && !self.material_editor.redo.is_empty()
    }

    /// Создать `New Material.material` в каталоге `folder` и открыть его в инспекторе.
    pub(crate) fn create_material(&mut self, folder: &str) -> Result<String, EditorError> {
        self.require_assets_root("create a material")?;
        let database = self.app.world.resources.get_mut::<AssetDatabase>().ok_or_else(|| EditorError::Other("no assets".into()))?;
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let mut relative = format!("{prefix}New Material.material");
        let mut suffix = 1;
        while database.guid_of(&relative).is_some() || database.root().join(&relative).exists() {
            relative = format!("{prefix}New Material {suffix}.material");
            suffix += 1;
        }
        let handle = database.save_material(&relative, &Material::new().color(Color::hex(0xd0d0d0)))?;
        log::info!("created `Assets/{relative}`");
        log::debug!("material GUID {}", handle.guid());
        Ok(relative)
    }

    /// Назначить материал слоту `slot` (части меша) `MeshRenderer` объекта.
    pub(crate) fn assign_material(&mut self, object: GameObjectId, material: Guid, slot: usize) -> bool {
        let Some(renderer) = self.app.world.scene.get_mut(object).and_then(|o| o.get_mut::<MeshRenderer>()) else {
            log::warn!("this object has no MeshRenderer: add one to give it a material");
            return false;
        };
        if renderer.materials.len() <= slot {
            renderer.materials.resize(slot + 1, AssetHandle::none());
        }
        renderer.materials[slot] = AssetHandle::from_guid(material);
        self.touch();
        true
    }

    /// Создать файлы `.material` из материалов, описанных в модели объекта (рядом с
    /// моделью), и назначить их частям. Возвращает пути созданных файлов.
    pub(crate) fn extract_materials(&mut self, object: GameObjectId) -> Result<Vec<String>, EditorError> {
        let mesh = self
            .app
            .world
            .scene
            .get(object)
            .and_then(|o| o.get::<MeshRenderer>())
            .map(|r| r.mesh)
            .filter(|m| !m.is_none())
            .ok_or_else(|| EditorError::Other(tr_s("extracting materials needs a MeshRenderer with a model file").into()))?;
        let database = self.app.world.resources.get_mut::<AssetDatabase>().ok_or_else(|| EditorError::Other("no assets".into()))?;
        let materials = database.model_materials(mesh.guid())?;
        let extras = database.mesh_extras(mesh.guid())?;
        if materials.is_empty() {
            return Err(EditorError::Other("the model file describes no materials".into()));
        }
        let parts = database.load_mesh(mesh.guid())?.parts();
        let model = database.relative_path_of(mesh.guid()).unwrap_or("model").to_string();
        let (dir, stem) = match model.rsplit_once('/') {
            Some((dir, name)) => (format!("{dir}/"), name.rsplit_once('.').map_or(name, |(s, _)| s).to_string()),
            None => (String::new(), model.rsplit_once('.').map_or(model.as_str(), |(s, _)| s).to_string()),
        };
        let mut created = Vec::new();
        let mut handles = Vec::new();
        for (index, material) in materials.iter().enumerate() {
            let name: String = extras.materials[index]
                .name
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let relative = format!("{dir}{stem}_{name}.material", dir = dir, stem = stem, name = name);
            handles.push(database.save_material(&relative, material)?);
            created.push(relative);
        }
        // Слот части — материал, на который ссылается эта часть в файле.
        let slots: Vec<AssetHandle<Material>> = parts
            .iter()
            .map(|part| part.material.and_then(|m| handles.get(m).copied()).unwrap_or(handles[0]))
            .collect();
        if let Some(renderer) = self.app.world.scene.get_mut(object).and_then(|o| o.get_mut::<MeshRenderer>()) {
            renderer.materials = slots;
        }
        self.touch();
        log::info!("extracted {} material(s) from `Assets/{model}`", created.len());
        Ok(created)
    }
}

fn push_undo(editor: &mut MaterialEditor, before: Material) {
    editor.undo.push(before);
    if editor.undo.len() > UNDO_LIMIT {
        editor.undo.remove(0);
    }
    editor.redo.clear();
}

/// Загрузить открытый материал с диска, если он ещё не загружен.
fn load(editor: &mut Editor) {
    let Some(guid) = editor.material_editor.guid else {
        return;
    };
    if editor.material_editor.loaded.as_ref().is_some_and(|(g, _)| *g == guid) {
        return;
    }
    let Some(database) = editor.app.world.resources.get::<AssetDatabase>() else {
        return;
    };
    match database.load_material(guid) {
        Ok(material) => {
            editor.material_editor.loaded = Some((guid, material));
            editor.material_editor.error = None;
        }
        Err(e) => {
            editor.material_editor.loaded = None;
            editor.material_editor.error = Some(e.to_string());
        }
    }
}

/// Записать материал на диск и сказать всем, кто его рисует, перечитать его.
fn commit(editor: &mut Editor, material: Material) {
    let Some(guid) = editor.material_editor.guid else {
        return;
    };
    let Some(database) = editor.app.world.resources.get_mut::<AssetDatabase>() else {
        return;
    };
    let Some(relative) = database.relative_path_of(guid).map(str::to_string) else {
        return;
    };
    match database.save_material(&relative, &material) {
        Ok(_) => editor.material_editor.error = None,
        Err(e) => {
            log::error!("cannot save `Assets/{relative}`: {e}");
            editor.material_editor.error = Some(e.to_string());
        }
    }
    editor.material_editor.loaded = Some((guid, material));
    editor.material_editor.stale.push(guid);
    editor.previews.invalidate(guid);
}

// ----------------------------------------------------------------------------------
// Редактор материала в инспекторе
// ----------------------------------------------------------------------------------

/// Инспектор для открытого материала. `false` — материала нет (инспектор покажет объект).
pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) -> bool {
    let Some(guid) = editor.material_editor.guid else {
        return false;
    };
    let Some(asset) = editor.assets.iter().find(|a| a.guid == guid).cloned() else {
        editor.open_material(None);
        return false;
    };
    load(editor);
    let pixels_per_point = ui.ctx().pixels_per_point();

    egui::ScrollArea::vertical().id_salt("material_inspector").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        egui::Frame::none().inner_margin(egui::Margin::same(theme::SP_L)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Заголовок: миниатюра, имя, Undo / Redo.
            ui.horizontal(|ui| {
                let (square, _) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::hover());
                ui.painter().rect_filled(square, theme::R_S, theme::BG_INSET);
                match editor.previews.thumbnail(guid, 72.0 * pixels_per_point) {
                    Thumb::Ready(texture) => {
                        ui.painter().image(texture, square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    _ => icons::paint(ui.painter(), square.shrink(20.0), Icon::Sphere, theme::TEXT_DISABLED),
                }
                ui.vertical(|ui| {
                    ui.add_space(theme::SP_S);
                    ui.add(
                        egui::Label::new(egui::RichText::new(file_name(&asset.path)).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT))
                            .truncate(),
                    );
                    ui.label(egui::RichText::new(tr_s("Material")).color(theme::TEXT_MUTED));
                    ui.horizontal(|ui| {
                        let undo = ui.add_enabled_ui(!editor.material_editor.undo.is_empty(), |ui| crate::controls::button(ui, tr_s("Undo"))).inner.on_hover_text("Ctrl+Z");
                        let redo = ui.add_enabled_ui(!editor.material_editor.redo.is_empty(), |ui| crate::controls::button(ui, tr_s("Redo"))).inner.on_hover_text("Ctrl+Y");
                        if undo.clicked() {
                            editor.undo_material();
                        }
                        if redo.clicked() {
                            editor.redo_material();
                        }
                    });
                });
            });
            if let Some(error) = &editor.material_editor.error {
                ui.add_space(theme::SP_S);
                ui.label(egui::RichText::new(error).color(theme::ERROR));
            }
        });

        let Some((_, before)) = editor.material_editor.loaded.clone() else {
            return;
        };
        let mut material = before.clone();
        let assets = editor.assets.clone();
        let root = editor.assets_root();
        let mut filter = std::mem::take(&mut editor.inspector.asset_filter);
        // Изменение — только то, о котором сообщил виджет (кнопка цвета egui может
        // пересчитать значение без участия пользователя).
        let mut edited = false;
        egui::Frame::none().inner_margin(egui::Margin::symmetric(theme::SP_L, 0.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            // Свой шейдер (docs/api/shaders.md) и его параметры.
            section(ui, tr_s("Shader"));
            edited |= property(ui, tr_s("Shader"), tr_s("A custom shader (.shader of kind surface or unlit); empty — the standard material"), |ui| {
                let mut text = if material.shader.is_none() { String::new() } else { material.shader.guid().to_string() };
                let changed = asset_widget(ui, &mut text, SHADER, &assets, &mut filter, ("material_shader", guid));
                if changed {
                    material.shader = Guid::parse(&text).map_or(AssetHandle::none(), AssetHandle::from_guid);
                }
                changed
            });
            if !material.shader.is_none() {
                let database = editor.app.world.resources.get::<AssetDatabase>();
                let shader = material.shader.guid();
                edited |= crate::shader_params_ui::field_ui(ui, database, shader, &mut material.shader_params, &assets, &mut filter, ("material_params", guid));
            }
            let thumbs = &mut editor.material_editor.texture_thumbs;
            let mut slot = |ui: &mut egui::Ui, label: &str, doc: &str, handle: &mut AssetHandle<Texture>, salt: &str| {
                property(ui, label, doc, |ui| texture_slot(ui, handle, &assets, &mut filter, thumbs, root.as_deref(), salt))
            };

            section(ui, tr_s("Surface"));
            edited |= property(ui, tr_s("Base Colour"), tr_s("Multiplied by the base texture; alpha is transparency"), |ui| {
                let c = &mut material.base_color;
                color_widget(ui, &mut c.r, &mut c.g, &mut c.b, &mut c.a, ("material_base", guid))
            });
            edited |= slot(ui, tr_s("Base Texture"), tr_s("Colour texture (sRGB)"), &mut material.base_color_texture, "base");
            edited |= property(ui, tr_s("Metallic"), tr_s("0 — plastic, wood; 1 — metal"), |ui| slider(ui, &mut material.metallic, 0.0..=1.0));
            edited |= property(ui, tr_s("Roughness"), tr_s("0 — mirror, 1 — matte"), |ui| slider(ui, &mut material.roughness, 0.0..=1.0));
            edited |= slot(ui, tr_s("Metal / Rough Map"), tr_s("glTF layout: green — roughness, blue — metallic (linear)"), &mut material.metallic_roughness_texture, "mr");

            section(ui, tr_s("Normal Map"));
            edited |= slot(ui, tr_s("Normal Map"), tr_s("Tangent-space normals, OpenGL convention (green up)"), &mut material.normal_texture, "normal");
            edited |= property(ui, tr_s("Strength"), tr_s("0 — flat, 1 — as in the texture"), |ui| slider(ui, &mut material.normal_strength, 0.0..=2.0));

            section(ui, tr_s("Emission"));
            edited |= property(ui, tr_s("Emissive Colour"), tr_s("Glow added on top of lighting"), |ui| {
                let c = &mut material.emissive;
                let mut alpha = 1.0;
                color_widget(ui, &mut c.r, &mut c.g, &mut c.b, &mut alpha, ("material_emissive", guid))
            });
            edited |= property(ui, tr_s("Intensity"), tr_s("Above 1 makes bloom"), |ui| slider(ui, &mut material.emissive_intensity, 0.0..=20.0));
            edited |= slot(ui, tr_s("Emissive Map"), tr_s("Multiplies the emissive colour (sRGB)"), &mut material.emissive_texture, "emissive");

            section(ui, tr_s("Occlusion"));
            edited |= slot(ui, tr_s("Occlusion Map"), tr_s("Red channel darkens ambient light in creases"), &mut material.occlusion_texture, "ao");
            edited |= property(ui, tr_s("Strength"), "", |ui| slider(ui, &mut material.occlusion_strength, 0.0..=1.0));

            section(ui, tr_s("Tiling"));
            edited |= property(ui, tr_s("UV Tiling"), tr_s("Texture repeats along U and V"), |ui| pair(ui, &mut material.uv_scale, 0.05));
            edited |= property(ui, tr_s("UV Offset"), "", |ui| pair(ui, &mut material.uv_offset, 0.01));
            edited |= property(ui, tr_s("World Tiling"), tr_s("Repeat the base texture every N metres in world space (0 — use mesh UV)"), |ui| {
                crate::controls::number_field_ex(ui, &mut material.world_tiling, 0.0..=100.0, 0.05, "", tr_s(" m"), 3, 96.0).changed()
            });

            section(ui, tr_s("Rendering"));
            edited |= property(ui, tr_s("Alpha"), tr_s("Opaque, cut out below a threshold (leaves), or blended (glass)"), |ui| {
                theme::segmented(
                    ui,
                    &mut material.alpha_mode,
                    &[
                        (AlphaMode::Opaque, tr_s("Opaque"), tr_s("Alpha is ignored")),
                        (AlphaMode::Cutout, tr_s("Cutout"), tr_s("Pixels below the threshold are not drawn (also in shadows)")),
                        (AlphaMode::Blend, tr_s("Blend"), tr_s("Mixed with what is behind; drawn after opaque objects")),
                    ],
                )
            });
            if material.alpha_mode == AlphaMode::Cutout {
                edited |= property(ui, tr_s("Threshold"), "", |ui| slider(ui, &mut material.alpha_cutoff, 0.0..=1.0));
            }
            edited |= property(ui, tr_s("Double-Sided"), tr_s("Draw back faces too, lit from their own side"), |ui| ui.checkbox(&mut material.double_sided, "").changed());
            edited |= property(ui, tr_s("Cast Shadows"), "", |ui| ui.checkbox(&mut material.cast_shadows, "").changed());
            edited |= property(ui, tr_s("Receive Shadows"), "", |ui| ui.checkbox(&mut material.receive_shadows, "").changed());
            edited |= property(ui, tr_s("Unlit"), tr_s("No lighting: the colour as is (plus emission)"), |ui| ui.checkbox(&mut material.unlit, "").changed());
            edited |= property(ui, tr_s("Dev Grid"), tr_s("Metre grid on top — for level prototypes"), |ui| ui.checkbox(&mut material.grid, "").changed());
            ui.add_space(theme::SP_XL);
        });
        editor.inspector.asset_filter = filter;

        let pointer_down = ui.input(|i| i.pointer.any_down());
        if edited && material != before {
            if !editor.material_editor.editing {
                push_undo(&mut editor.material_editor, before);
                editor.material_editor.editing = true;
            }
            commit(editor, material);
        } else if !pointer_down && !ui.ctx().wants_keyboard_input() {
            editor.material_editor.editing = false;
        }
    });
    true
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(theme::SP_M);
    theme::section_label(ui, title);
    ui.add_space(theme::SP_XS);
}

fn slider(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
    ui.add(egui::Slider::new(value, range).max_decimals(3)).changed()
}

fn pair(ui: &mut egui::Ui, value: &mut Vec2, speed: f64) -> bool {
    let width = (ui.available_width() - theme::SP_M) * 0.5;
    let x = crate::controls::number_field_ex(ui, &mut value.x, f32::MIN..=f32::MAX, speed, tr_s("U "), "", 3, width).changed();
    let y = crate::controls::number_field_ex(ui, &mut value.y, f32::MIN..=f32::MAX, speed, tr_s("V "), "", 3, width).changed();
    x || y
}

/// Слот текстуры: миниатюра и поле выбора (список, перетаскивание из окна Project).
fn texture_slot(
    ui: &mut egui::Ui,
    handle: &mut AssetHandle<Texture>,
    assets: &[AssetEntry],
    filter: &mut String,
    thumbs: &mut HashMap<Guid, Option<egui::TextureHandle>>,
    root: Option<&std::path::Path>,
    salt: &str,
) -> bool {
    let (square, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
    ui.painter().rect_filled(square, theme::R_S, theme::BG_INSET);
    if !handle.is_none() {
        let guid = handle.guid();
        let texture = thumbs.entry(guid).or_insert_with(|| {
            let path = assets.iter().find(|a| a.guid == guid).zip(root).map(|(a, root)| root.join(&a.path))?;
            let image = image::open(&path).ok()?.thumbnail(64, 64).to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            Some(ui.ctx().load_texture(format!("material_slot_{guid}", guid = guid), egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()), egui::TextureOptions::LINEAR))
        });
        if let Some(texture) = texture {
            ui.painter().image(texture.id(), square.shrink(2.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    let mut text = if handle.is_none() { String::new() } else { handle.guid().to_string() };
    let changed = asset_widget(ui, &mut text, TEXTURE, assets, filter, ("material_texture", salt));
    if changed {
        *handle = Guid::parse(&text).map_or(AssetHandle::none(), AssetHandle::from_guid);
    }
    changed
}

/// Выбор формы большого предпросмотра материала.
pub(crate) fn shape_switch(previews: &mut Previews, ui: &mut egui::Ui) {
    let mut shape = previews.detail.shape;
    if theme::segmented(
        ui,
        &mut shape,
        &[
            (PreviewShape::Sphere, tr_s("Sphere"), tr_s("Preview on a sphere")),
            (PreviewShape::Cube, tr_s("Cube"), tr_s("Preview on a cube")),
            (PreviewShape::Plane, tr_s("Plane"), tr_s("Preview on a flat plane")),
        ],
    ) {
        previews.detail.shape = shape;
    }
}

// ----------------------------------------------------------------------------------
// MeshRenderer: материалы по частям
// ----------------------------------------------------------------------------------

/// Что нужно телу `MeshRenderer` в инспекторе.
pub(crate) struct MeshRendererUi<'a> {
    pub object: GameObjectId,
    pub editor: &'a mut MaterialEditor,
    pub previews: &'a mut Previews,
    pub database: Option<&'a AssetDatabase>,
    pub assets: &'a [AssetEntry],
    pub filter: &'a mut String,
    pub actions: &'a mut Vec<Action>,
    pub field_rects: &'a mut HashMap<(&'static str, &'static str), Rect>,
}

/// Тело карточки `MeshRenderer`: форма и меш, слоты материалов по частям, оттенок, а
/// встроенный материал — в сворачиваемой группе, только пока материалы не назначены.
pub(crate) fn mesh_renderer_body(ui: &mut egui::Ui, mesh: &mut MeshRenderer, ctx: MeshRendererUi) -> bool {
    let MeshRendererUi { object, editor, previews, database, assets, filter, actions, field_rects } = ctx;
    let mut changed = false;
    let mut field = |ui: &mut egui::Ui, mesh: &mut MeshRenderer, name: &'static str, filter: &mut String| {
        let Some(mut value) = mesh.get_field(name) else { return false };
        let meta = mesh.field_meta(name);
        let doc = crate::inspector::clean_doc(mesh.field_doc(name));
        let edited = property(ui, &theme::pretty_name(name), &doc, |ui| {
            let widget = ui.scope(|ui| crate::inspector::value_widget(ui, &mut value, meta, assets, filter, ("MeshRenderer", 0usize, name)));
            field_rects.insert(("MeshRenderer", name), widget.response.rect);
            widget.inner
        });
        edited && mesh.set_field(name, &value).is_ok()
    };
    changed |= field(ui, mesh, "shape", filter);
    let mesh_top = ui.cursor().top();
    changed |= field(ui, mesh, "mesh", filter);
    let mesh_bottom = ui.min_rect().bottom();
    // Правая кнопка по подписи поля Mesh: модель — «Edit in Modeler», чужой меш —
    // «Edit a Copy in Modeler» (без программы Piska Modeler — неактивно с подсказкой).
    let model_file = (!mesh.mesh.is_none()).then(|| database.and_then(|db| db.path_of(mesh.mesh.guid()))).flatten();
    if let Some(file) = model_file {
        if let Some(entry) = crate::modeler_link::ModelerEntry::for_path(&file) {
            // Подпись поля — левая колонка сетки свойств (само поле не перекрывается).
            let left = ui.max_rect().left();
            let label = egui::Rect::from_min_max(egui::pos2(left, mesh_top), egui::pos2(left + ui.available_width() * 0.35, mesh_bottom));
            let response = ui.interact(label, egui::Id::new(("mesh_modeler_menu", object)), egui::Sense::click());
            let installed = crate::modeler_link::modeler_executable().is_some();
            response.context_menu(|ui| {
                theme::menu_body(ui);
                if theme::item(tr_s(entry.label()))
                    .icon(crate::icons::Icon::Cube)
                    .enabled_if(installed, tr_s(crate::modeler_link::MODELER_MISSING))
                    .show(ui)
                {
                    actions.push(match entry {
                        crate::modeler_link::ModelerEntry::Edit => Action::EditInModeler(file.clone()),
                        crate::modeler_link::ModelerEntry::EditCopy => Action::EditCopyInModeler(file.clone()),
                    });
                }
            });
        }
    }

    // Части модели и сколько материалов в её файле.
    let model = (!mesh.mesh.is_none()).then(|| mesh.mesh.guid());
    let names: Vec<String> = match (model, database) {
        (Some(guid), Some(db)) => editor
            .part_names
            .entry(guid)
            .or_insert_with(|| match db.load_mesh(guid) {
                Ok(data) => data.parts().iter().enumerate().map(|(i, p)| if p.name.is_empty() { trf!("Part {i}", i = i) } else { p.name.clone() }).collect(),
                Err(_) => Vec::new(),
            })
            .clone(),
        _ => Vec::new(),
    };
    let file_materials = match (model, database) {
        (Some(guid), Some(db)) => {
            *editor.model_material_count.entry(guid).or_insert_with(|| db.mesh_extras(guid).map(|e| e.materials.len()).unwrap_or(0))
        }
        _ => 0,
    };

    ui.add_space(theme::SP_S);
    theme::section_label(ui, tr_s("Materials"));
    let slots = names.len().max(mesh.materials.len()).max(1);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let mut slots_changed = false;
    for slot in 0..slots {
        let label = names.get(slot).cloned().unwrap_or_else(|| trf!("Element {slot}", slot = slot));
        let current = mesh.materials.get(slot).copied().unwrap_or_default();
        let mut text = if current.is_none() { String::new() } else { current.guid().to_string() };
        let mut edit = None;
        let edited = property(ui, &label, tr_s("Material of this part of the mesh; drag a .material here"), |ui| {
            let (square, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            ui.painter().rect_filled(square, theme::R_S, theme::BG_INSET);
            if !current.is_none() {
                if let Thumb::Ready(texture) = previews.thumbnail(current.guid(), 26.0 * pixels_per_point) {
                    ui.painter().image(texture, square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
            let width = (ui.available_width() - if current.is_none() { 0.0 } else { 40.0 + theme::SP_M * 2.0 }).max(40.0);
            let changed = ui
                .allocate_ui(vec2(width, theme::CONTROL_H), |ui| asset_widget(ui, &mut text, MATERIAL, assets, filter, ("material_slot", object, slot)))
                .inner;
            if !current.is_none() && crate::controls::button(ui, tr_s("Edit")).on_hover_text(tr_s("Open the material in the Inspector")).clicked() {
                edit = Some(current.guid());
            }
            changed
        });
        if let Some(guid) = edit {
            actions.push(Action::InspectMaterial(Some(guid)));
        }
        if edited {
            if mesh.materials.len() <= slot {
                mesh.materials.resize(slot + 1, AssetHandle::none());
            }
            mesh.materials[slot] = Guid::parse(&text).map_or(AssetHandle::none(), AssetHandle::from_guid);
            slots_changed = true;
        }
    }
    if slots_changed {
        // Пустые слоты в конце не нужны; ни одного — снова встроенный материал.
        while mesh.materials.last().is_some_and(|m| m.is_none()) {
            mesh.materials.pop();
        }
        changed = true;
    }
    if model.is_some() && file_materials > 0 {
        changed |= property(ui, tr_s("Model Materials"), tr_s("Without assigned materials, use the materials described in the model file"), |ui| {
            ui.checkbox(&mut mesh.model_materials, "").changed()
        });
        ui.horizontal(|ui| {
            // Под колонкой значений, как поля выше.
            ui.add_space(theme::property_columns(ui.available_width()).0 + theme::SP_S);
            if ui
                .button(tr_s("Extract materials from model"))
                .on_hover_text(trn!("Create {n} .material file next to the model from its embedded materials and assign it", "Create {n} .material files next to the model from its embedded materials and assign them", file_materials))
                .clicked()
            {
                actions.push(Action::ExtractMaterials(object));
            }
        });
    }
    changed |= field(ui, mesh, "tint", filter);

    if !mesh.uses_material_assets() {
        ui.add_space(theme::SP_S);
        theme::foldout(ui, egui::Id::new(("inline_material", object)), tr_s("Inline material (legacy)"), true, |ui| {
            for name in INLINE_FIELDS {
                changed |= field(ui, mesh, name, filter);
            }
        });
    } else {
        changed |= field(ui, mesh, "cast_shadows", filter);
    }
    changed
}
