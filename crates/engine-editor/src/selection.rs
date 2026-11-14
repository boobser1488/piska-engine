//! Выделение нескольких объектов, как в Unity: упорядоченное множество и *активный*
//! объект (последний выбранный). [`Editor::selection`] по-прежнему возвращает активный —
//! весь старый код и тесты работают с ним; операции над выделением (удалить,
//! дублировать, копировать, вставить, сгруппировать, скрыть) берут всё множество.
//!
//! Инвариант: активный объект всегда входит в множество; пустое множество — нет и
//! активного. Код, который просто присваивает `selection = Some(id)`, тем самым выделяет
//! один объект: [`Editor::normalize_selection`] приводит множество к нему.

use crate::i18n::{tr_s};
use engine_core::prelude::*;
use engine_core::serialize::{self, NodeData};

use crate::state::{id_at, path_of, Action, Editor};
use crate::EditorError;

/// Путь объекта в дереве сцены (индексы детей от корня): по нему выделение переживает
/// Undo и Stop, после которых у объектов новые `GameObjectId`.
pub(crate) type ScenePath = Vec<usize>;

impl Editor {
    /// Все выделенные объекты в порядке выбора; активный — [`Editor::selection`].
    pub fn selected(&self) -> Vec<GameObjectId> {
        let scene = &self.app.world.scene;
        let Some(active) = self.selection.filter(|id| scene.contains(*id)) else {
            return Vec::new();
        };
        if self.selected.contains(&active) {
            self.selected.iter().copied().filter(|id| scene.contains(*id)).collect()
        } else {
            vec![active]
        }
    }

    /// Объект входит в выделение.
    pub fn is_selected(&self, id: GameObjectId) -> bool {
        match self.selection {
            Some(active) if active == id => true,
            Some(active) => self.selected.contains(&active) && self.selected.contains(&id),
            None => false,
        }
    }

    /// Выделить объекты `ids`; `active` (или последний из них) становится активным.
    pub fn select_many(&mut self, ids: &[GameObjectId], active: Option<GameObjectId>) {
        self.apply(Action::SelectMany { ids: ids.to_vec(), active });
    }

    /// Ctrl+щелчок: добавить объект к выделению или убрать из него.
    pub fn toggle_selected(&mut self, id: GameObjectId) {
        self.apply(Action::ToggleSelect(id));
    }

    /// Привести множество к инварианту (после действий, которые меняют только активный).
    pub(crate) fn normalize_selection(&mut self) {
        self.selected = self.selected();
        if self.selected.is_empty() {
            self.selection = None;
        }
    }

    /// Выделенные объекты без тех, чей предок тоже выделен, в порядке дерева: их
    /// удаляют, дублируют и двигают (потомки едут вместе с ними).
    pub(crate) fn top_level_selected(&self) -> Vec<GameObjectId> {
        let selected = self.selected();
        if selected.len() <= 1 {
            return selected;
        }
        let scene = &self.app.world.scene;
        tree_order(scene)
            .into_iter()
            .filter(|id| selected.contains(id))
            .filter(|id| {
                let mut parent = scene.parent_of(*id);
                while let Some(p) = parent {
                    if selected.contains(&p) {
                        return false;
                    }
                    parent = scene.parent_of(p);
                }
                true
            })
            .collect()
    }

    /// Пути выделения до правки, которая меняет все `GameObjectId` (Undo, Stop).
    pub(crate) fn selection_paths(&self) -> (Option<ScenePath>, Vec<ScenePath>) {
        let scene = &self.app.world.scene;
        let active = self.selection.and_then(|id| path_of(scene, id));
        let all = self.selected().into_iter().filter_map(|id| path_of(scene, id)).collect();
        (active, all)
    }

    /// Вернуть выделение по путям, снятым [`Editor::selection_paths`].
    pub(crate) fn restore_selection_paths(&mut self, (active, all): (Option<ScenePath>, Vec<ScenePath>)) {
        let scene = &self.app.world.scene;
        self.selection = active.and_then(|path| id_at(scene, &path));
        self.selected = all.iter().filter_map(|path| id_at(scene, path)).collect();
        self.normalize_selection();
    }

    /// Середина выделения в мире: точка вращения в режиме Center.
    pub(crate) fn selection_center(&self) -> Option<Vec3> {
        let ids = self.top_level_selected();
        if ids.is_empty() {
            return None;
        }
        let scene = &self.app.world.scene;
        let sum: Vec3 = ids.iter().map(|id| scene.world_position(*id)).sum();
        Some(sum / ids.len() as f32)
    }

    /// Гизмо вращает и масштабирует выделение вокруг его середины (Center), а не вокруг
    /// активного объекта (Pivot) — переключатель на панели инструментов, как в Unity.
    pub fn pivot_center(&self) -> bool {
        self.pivot_center
    }

    pub fn set_pivot_center(&mut self, center: bool) {
        self.pivot_center = center;
    }

    /// Сколько объектов в буфере обмена (Ctrl+C / Ctrl+X).
    pub fn object_clipboard_len(&self) -> usize {
        self.object_clipboard.len()
    }

    /// Действия над выделением (см. варианты [`Action`]).
    pub(crate) fn apply_selection_action(&mut self, action: Action) -> Result<(), EditorError> {
        match action {
            Action::SelectMany { ids, active } => {
                let scene = &self.app.world.scene;
                let mut unique: Vec<GameObjectId> = Vec::new();
                for id in ids.into_iter().filter(|id| scene.contains(*id)) {
                    if !unique.contains(&id) {
                        unique.push(id);
                    }
                }
                let active = active.filter(|id| unique.contains(id)).or_else(|| unique.last().copied());
                self.selected = unique;
                self.selection = active;
                if self.selection.is_some() {
                    self.open_material(None);
                }
            }
            Action::ToggleSelect(id) => {
                if !self.app.world.scene.contains(id) {
                    return Ok(());
                }
                let mut selected = self.selected();
                if let Some(at) = selected.iter().position(|s| *s == id) {
                    selected.remove(at);
                    self.selection = if self.selection == Some(id) { selected.last().copied() } else { self.selection };
                } else {
                    selected.push(id);
                    self.selection = Some(id);
                    self.open_material(None);
                }
                self.selected = selected;
            }
            Action::SelectAll => {
                let all = tree_order(&self.app.world.scene);
                let active = self.selection.filter(|id| all.contains(id)).or_else(|| all.first().copied());
                self.selected = all;
                self.selection = active;
            }
            Action::DeleteSelection => {
                let ids = self.top_level_selected();
                if ids.is_empty() {
                    return Ok(());
                }
                self.touch();
                for id in &ids {
                    self.app.world.scene.destroy(*id);
                }
                self.app.world.flush_destroyed();
                self.selection = None;
                self.selected.clear();
            }
            Action::DuplicateSelection => {
                let ids = self.top_level_selected();
                if ids.is_empty() {
                    return Ok(());
                }
                self.touch();
                let active = self.selection;
                let mut copies = Vec::new();
                let mut active_copy = None;
                // Одной группой: ссылки между выделенными объектами у копий — на копии.
                let made = self.app.world.duplicate_many(&ids)?;
                for (id, copy) in ids.into_iter().zip(made) {
                    let scene = &mut self.app.world.scene;
                    let parent = scene.get(copy).and_then(|o| o.parent());
                    let base = scene.get(id).map(|o| o.name.clone()).unwrap_or_default();
                    let name = crate::state::unique_name(scene, parent, &base, Some(copy));
                    if let Some(obj) = scene.get_mut(copy) {
                        obj.name = name;
                    }
                    if active == Some(id) {
                        active_copy = Some(copy);
                    }
                    copies.push(copy);
                }
                self.selection = active_copy.or_else(|| copies.last().copied());
                self.selected = copies;
            }
            Action::CopySelection => {
                let scene = &self.app.world.scene;
                self.object_clipboard = self
                    .top_level_selected()
                    .into_iter()
                    .map(|id| serialize::exact_node(scene, id))
                    .collect();
                if !self.object_clipboard.is_empty() {
                    log::info!("copied {} object(s)", self.object_clipboard.len());
                }
            }
            Action::CutSelection => {
                self.apply_selection_action(Action::CopySelection)?;
                self.apply_selection_action(Action::DeleteSelection)?;
            }
            Action::Paste { as_child } => {
                if self.object_clipboard.is_empty() {
                    return Ok(());
                }
                // Как в Unity: вставка — рядом с активным объектом (Ctrl+Shift+V — внутрь).
                let anchor = self.selection.filter(|id| self.app.world.scene.contains(*id));
                let parent = if as_child { anchor } else { anchor.and_then(|id| self.app.world.scene.parent_of(id)) };
                self.touch();
                let nodes: Vec<NodeData> = self.object_clipboard.clone();
                let mut pasted = Vec::new();
                // Новые номера объектов, ссылки внутри вставки — на вставленные копии.
                let made = self.app.world.spawn_copies(&nodes, parent)?;
                for (node, id) in nodes.iter().zip(made) {
                    let world = &mut self.app.world;
                    let name = crate::state::unique_name(&world.scene, parent, &node.name, Some(id));
                    if let Some(obj) = world.scene.get_mut(id) {
                        obj.name = name;
                    }
                    pasted.push(id);
                }
                if let Some(parent) = parent {
                    let path = path_of(&self.app.world.scene, parent);
                    self.hierarchy.expand(path);
                }
                self.selection = pasted.last().copied();
                self.selected = pasted;
            }
            Action::GroupSelection => {
                let ids = self.top_level_selected();
                let Some(center) = self.selection_center() else { return Ok(()) };
                self.touch();
                let scene = &mut self.app.world.scene;
                let parent = self.selection.and_then(|id| scene.parent_of(id)).filter(|p| !ids.contains(p));
                let name = crate::state::unique_name(scene, parent, tr_s("Group"), None);
                let group = scene.spawn(name);
                scene.set_parent(group, parent)?;
                let parent_inverse = parent.map_or(Mat4::IDENTITY, |p| scene.world_matrix(p).inverse());
                if let Some(obj) = scene.get_mut(group) {
                    obj.transform.position = parent_inverse.transform_point3(center);
                }
                for id in &ids {
                    scene.set_parent_keep_world(*id, Some(group))?;
                }
                let path = path_of(scene, group);
                self.hierarchy.expand(path);
                self.selection = Some(group);
                self.selected = vec![group];
            }
            Action::SetSelectionActive(active) => {
                let ids = self.selected();
                if ids.is_empty() {
                    return Ok(());
                }
                self.touch();
                for id in ids {
                    if let Some(obj) = self.app.world.scene.get_mut(id) {
                        obj.active = active;
                    }
                }
            }
            other => unreachable!("not a selection action: {other:?}"),
        }
        Ok(())
    }
}

/// Все объекты сцены в порядке дерева (как в окне Hierarchy, без учёта сворачивания).
pub(crate) fn tree_order(scene: &Scene) -> Vec<GameObjectId> {
    fn walk(scene: &Scene, ids: &[GameObjectId], out: &mut Vec<GameObjectId>) {
        for id in ids {
            out.push(*id);
            if let Some(object) = scene.get(*id) {
                walk(scene, object.children(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(scene, scene.roots(), &mut out);
    out
}

/// Объект, который двигает гизмо: его поза в начале перетаскивания.
#[derive(Clone, Debug)]
pub(crate) struct Moved {
    pub id: GameObjectId,
    pub start: Transform,
    pub world_position: Vec3,
    pub world_rotation: Quat,
    pub parent_inverse: Mat4,
    pub parent_rotation: Quat,
}

/// Что делает гизмо со всем выделением (всё — от позы на момент нажатия).
#[derive(Clone, Copy, Debug)]
pub(crate) enum GizmoOp {
    /// Сдвиг в мире.
    Move(Vec3),
    /// Поворот вокруг точки.
    Rotate { rotation: Quat, pivot: Vec3 },
    /// Масштаб по осям `axes` (единичные мировые векторы) вокруг точки; `factor` —
    /// множители собственного масштаба объектов по X / Y / Z.
    Scale { factor: Vec3, axes: [Vec3; 3], pivot: Vec3 },
}

impl Editor {
    /// Позы верхних уровней выделения — с них гизмо считает перетаскивание.
    pub(crate) fn capture_moved(&self) -> Vec<Moved> {
        let scene = &self.app.world.scene;
        self.top_level_selected()
            .into_iter()
            .filter_map(|id| {
                let object = scene.get(id)?;
                let (_, world_rotation, world_position) = scene.world_matrix(id).to_scale_rotation_translation();
                let parent = object.parent().map(|p| scene.world_matrix(p));
                Some(Moved {
                    id,
                    start: object.transform,
                    world_position,
                    world_rotation,
                    parent_inverse: parent.map_or(Mat4::IDENTITY, |m| m.inverse()),
                    parent_rotation: parent.map_or(Quat::IDENTITY, |m| m.to_scale_rotation_translation().1),
                })
            })
            .collect()
    }

    /// Применить действие гизмо ко всем объектам; одно перетаскивание — один шаг Undo
    /// (правки кадров сливаются, пока кнопка мыши нажата).
    pub(crate) fn apply_gizmo_op(&mut self, moved: &[Moved], op: GizmoOp) {
        let mut changes = Vec::new();
        for m in moved {
            let mut transform = m.start;
            match op {
                GizmoOp::Move(offset) => {
                    transform.position = m.parent_inverse.transform_point3(m.world_position + offset);
                }
                GizmoOp::Rotate { rotation, pivot } => {
                    let world = pivot + rotation * (m.world_position - pivot);
                    transform.position = m.parent_inverse.transform_point3(world);
                    transform.rotation = (m.parent_rotation.inverse() * (rotation * m.world_rotation)).normalize();
                }
                GizmoOp::Scale { factor, axes, pivot } => {
                    transform.scale = m.start.scale * factor;
                    let offset = m.world_position - pivot;
                    let scaled: Vec3 = (0..3).map(|i| axes[i] * axes[i].dot(offset) * factor[i]).sum();
                    transform.position = m.parent_inverse.transform_point3(pivot + scaled);
                }
            }
            let current = self.app.world.scene.get(m.id).map(|o| o.transform);
            if current.is_some_and(|t| t != transform) {
                changes.push((m.id, transform));
            }
        }
        if changes.is_empty() {
            return;
        }
        self.touch();
        for (id, transform) in changes {
            if let Some(object) = self.app.world.scene.get_mut(id) {
                object.transform = transform;
            }
        }
    }

    /// Точка, вокруг которой крутит гизмо: активный объект (Pivot) или середина (Center).
    pub(crate) fn gizmo_pivot(&self) -> Option<Vec3> {
        let active = self.selection.filter(|id| self.app.world.scene.contains(*id))?;
        if self.pivot_center {
            self.selection_center()
        } else {
            Some(self.app.world.scene.world_position(active))
        }
    }

    /// Применить щелчок / рамку с модификаторами: Ctrl — переключить, Shift — добавить,
    /// без модификаторов — заменить выделение.
    pub(crate) fn pick_with_modifiers(&mut self, picked: Vec<GameObjectId>, modifiers: egui::Modifiers) {
        let ctrl = modifiers.command || modifiers.ctrl;
        if ctrl {
            if picked.len() == 1 {
                self.actions.push(Action::ToggleSelect(picked[0]));
            } else {
                let mut ids = self.selected();
                for id in picked {
                    match ids.iter().position(|s| *s == id) {
                        Some(at) => {
                            ids.remove(at);
                        }
                        None => ids.push(id),
                    }
                }
                self.actions.push(Action::SelectMany { ids, active: None });
            }
        } else if modifiers.shift {
            let mut ids = self.selected();
            let active = picked.last().copied().or(self.selection);
            ids.extend(picked);
            self.actions.push(Action::SelectMany { ids, active });
        } else {
            let active = picked.last().copied();
            self.actions.push(Action::SelectMany { ids: picked, active });
        }
    }
}

impl Editor {
    /// Текст поиска окна Hierarchy (как если бы его напечатали): слова — имя,
    /// `t:Тип` — компонент.
    pub fn set_hierarchy_filter(&mut self, text: &str) {
        self.hierarchy.filter = text.to_string();
    }
}
