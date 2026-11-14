//! Переопределения экземпляра префаба в инспекторе: поле компонента, значение которого
//! отличается от префаба, отмечено полосой цвета акцента у левого края строки (как в
//! Unity); правый щелчок по строке — меню «Revert» (вернуть значение из префаба).
//! Работает и для корня экземпляра, и для его детей (их переопределения сохраняются
//! путём до ребёнка, см. `engine_core::serialize`).
//!
//! Узел префаба читается с диска, поэтому он кешируется в памяти egui на ~1 секунду
//! (правка самого префаба видна в инспекторе почти сразу, а кадр не читает файл).

use std::sync::Arc;

use egui::{pos2, vec2, Rect, Sense};
use engine_core::prelude::*;
use engine_core::serialize::{self, NodeData};

use crate::i18n::tr_s;
use crate::theme;

/// Как долго кеш узла префаба считается свежим, секунд.
const CACHE_SECONDS: f64 = 1.0;

/// Что известно о префабе выделенного объекта на этот кадр.
#[derive(Clone, Default)]
pub(crate) struct PrefabMarks {
    base: Option<Arc<NodeData>>,
}

impl PrefabMarks {
    /// Сведения для объекта `id` (корня экземпляра префаба или его ребёнка).
    pub(crate) fn of(ctx: &egui::Context, world: &World, id: GameObjectId) -> Self {
        let in_prefab = {
            let mut cursor = Some(id);
            let mut found = false;
            while let Some(current) = cursor {
                let Some(object) = world.scene.get(current) else { break };
                if object.prefab().is_some() {
                    found = true;
                    break;
                }
                cursor = object.parent();
            }
            found
        };
        if !in_prefab {
            return Self::default();
        }
        let uid = world.scene.get(id).map_or(0, |o| o.uid());
        let key = egui::Id::new(("inspector_prefab_base", uid));
        let now = ctx.input(|i| i.time);
        if let Some((at, base)) = ctx.data(|d| d.get_temp::<(f64, Option<Arc<NodeData>>)>(key)) {
            if now - at < CACHE_SECONDS && now >= at {
                return Self { base };
            }
        }
        let base = serialize::prefab_source_node(&world.scene, id, world.prefabs.as_ref(), &world.registry).map(Arc::new);
        ctx.data_mut(|d| d.insert_temp(key, (now, base.clone())));
        Self { base }
    }

    /// Значение поля в префабе (`None` — у префаба нет такого компонента или поля).
    pub(crate) fn base_value(&self, kind: &str, field: &str) -> Option<&Value> {
        let base = self.base.as_ref()?;
        base.components.iter().find(|c| c.kind == kind)?.fields.get(field)
    }

    /// Поле переопределено в экземпляре: значение отличается от префаба.
    pub(crate) fn overridden(&self, kind: &str, field: &str, current: &Value) -> bool {
        self.base_value(kind, field).is_some_and(|base| base != current)
    }

}

/// Отметка переопределённого поля и меню Revert. `value_rect` — область виджета значения
/// (строка — от левого края карточки до его правого края). Возвращает значение префаба,
/// если выбрали «Revert».
pub(crate) fn field_marker(
    ui: &mut egui::Ui,
    marks: &PrefabMarks,
    kind: &'static str,
    field: &'static str,
    current: &Value,
    value_rect: Rect,
) -> Option<Value> {
    if !marks.overridden(kind, field, current) {
        return None;
    }
    let left = ui.max_rect().left();
    let row = Rect::from_min_max(pos2(left, value_rect.top()), pos2(value_rect.left() - theme::SP_S, value_rect.bottom()));
    // Полоса у левого края строки — цвет акцента (ui-guidelines: «переопределено»).
    ui.painter().rect_filled(
        Rect::from_min_size(pos2(left - theme::SP_S, row.top() + 1.0), vec2(2.0, row.height() - 2.0)),
        1.0,
        theme::accent(),
    );
    let response = ui.interact(row, egui::Id::new(("prefab_override", kind, field)), Sense::click());
    let response = response.on_hover_text(tr_s("Overridden in this prefab instance. Right-click to revert it to the prefab value"));
    let mut revert = None;
    response.context_menu(|ui| {
        if crate::controls::menu_item(ui, tr_s("Revert"), "", true).clicked() {
            revert = marks.base_value(kind, field).cloned();
            ui.close_menu();
        }
    });
    revert
}

impl crate::state::Editor {
    /// Поле `field` компонента `kind` выделенного объекта отличается от его префаба (для
    /// тестов и подсветки в инспекторе). `false` — объект не из префаба.
    pub fn is_field_overridden(&self, kind: &str, field: &str) -> bool {
        let Some(id) = self.selection().filter(|id| self.app.world.scene.contains(*id)) else {
            return false;
        };
        let world = &self.app.world;
        let Some(base) = serialize::prefab_source_node(&world.scene, id, world.prefabs.as_ref(), &world.registry) else {
            return false;
        };
        let current = world
            .scene
            .get(id)
            .and_then(|o| o.components().find(|c| c.type_name() == kind))
            .and_then(|c| c.get_field(field));
        let base = base.components.iter().find(|c| c.kind == kind).and_then(|c| c.fields.get(field));
        matches!((base, current), (Some(b), Some(c)) if *b != c)
    }

    /// Идентификатор области строки переопределённого поля (правый щелчок — меню Revert).
    pub fn prefab_override_row_id(kind: &'static str, field: &'static str) -> egui::Id {
        egui::Id::new(("prefab_override", kind, field))
    }
}
