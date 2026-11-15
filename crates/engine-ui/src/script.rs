//! Короткие формы интерфейса для скриптов: `ctx.ui_text("Score", format!(..))`,
//! `ctx.ui_show("GameOver")` / `ctx.ui_hide(..)`, `ctx.clicked("play")`. Элемент ищется по
//! имени объекта. Длинные формы (`find_by_name` + `get_component_mut::<Text>`,
//! `scene.ui_set_visible`, `ctx.ui().clicked`) остаются.

use std::panic::Location;

use engine_core::prelude::*;
use engine_core::script::teach;

use crate::builder::UiScene;
use crate::components::Text;
use crate::events::UiContext;

/// Интерфейс из скрипта: `use engine::prelude::*;` уже подключает этот трейт.
pub trait UiCtx {
    /// Заменить текст элемента `name` (объект с компонентом `Text`).
    fn ui_text(&mut self, name: &str, text: impl AsRef<str>);
    /// Показать элемент `name` с детьми.
    fn ui_show(&mut self, name: &str);
    /// Спрятать элемент `name` с детьми.
    fn ui_hide(&mut self, name: &str);
    /// Нажата ли в этом кадре кнопка с этим именем сообщения (или именем объекта).
    fn clicked(&self, name: &str) -> bool;
}

/// Объект интерфейса по имени; нет — обучающая ошибка (один раз на место вызова).
fn element(ctx: &mut Ctx, site: &Location<'_>, name: &str, call: &str) -> Option<GameObjectId> {
    if let Some(id) = ctx.find_by_name(name) {
        return Some(id);
    }
    let texts: Vec<String> = ctx
        .scene()
        .iter_with::<Text>()
        .filter_map(|(id, _)| ctx.scene().get(id).map(|o| o.name.clone()))
        .collect();
    let me = ctx.name().to_string();
    teach(ctx.world, site, &format!("ui:{name}"), || {
        format!(
            "{me}: ctx.{call}(\"{name}\") found no UI element named `{name}`.{} Name the object in the Hierarchy \
             (or `.name(\"{name}\")` when building UI in code)",
            engine_core::error::did_you_mean(name, texts.iter().map(String::as_str))
        )
    });
    None
}

impl UiCtx for Ctx<'_> {
    #[track_caller]
    fn ui_text(&mut self, name: &str, text: impl AsRef<str>) {
        let site = Location::caller();
        let Some(id) = element(self, site, name, "ui_text") else { return };
        if let Some(label) = self.scene_mut().get_component_mut::<Text>(id) {
            label.set(text.as_ref());
            return;
        }
        let me = self.name().to_string();
        teach(self.world, site, &format!("ui-text:{name}"), || {
            format!("{me}: ctx.ui_text(\"{name}\"): the object `{name}` has no Text component")
        });
    }

    #[track_caller]
    fn ui_show(&mut self, name: &str) {
        if let Some(id) = element(self, Location::caller(), name, "ui_show") {
            self.scene_mut().ui_set_visible(id, true);
        }
    }

    #[track_caller]
    fn ui_hide(&mut self, name: &str) {
        if let Some(id) = element(self, Location::caller(), name, "ui_hide") {
            self.scene_mut().ui_set_visible(id, false);
        }
    }

    fn clicked(&self, name: &str) -> bool {
        self.ui().clicked(name)
    }
}
