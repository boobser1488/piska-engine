//! `engine-ui` — игровой интерфейс (HUD, меню) в стиле Unity uGUI.
//!
//! Интерфейс — обычные объекты сцены с компонентами: [`Canvas`] в корне,
//! [`RectTransform`] с якорями у каждого элемента, [`LayoutGroup`] для столбиков и
//! строк, виджеты [`Image`], [`Text`], [`Button`], [`Toggle`], [`Slider`],
//! [`ProgressBar`], [`InputField`]. Всё сохраняется в сценах и правится в инспекторе.
//!
//! [`UiSystem`] раскладывает холсты, обрабатывает мышь и клавиатуру и собирает список
//! отрисовки [`engine_render::Overlay`], который рендер рисует поверх кадра после
//! пост-обработки. Игра узнаёт о кликах сообщениями (`on_message`) или опросом
//! `ctx.ui().clicked("play")`; см. `docs/api/ui.md`.
#![forbid(unsafe_code)]

mod builder;
mod components;
mod draw;
mod events;
mod layout;
mod system;
mod text;
pub mod theme;

pub use builder::{UiNode, UiScene};
pub use components::{
    Alignment, Anchor, Button, Canvas, CanvasGroup, FillDirection, Image, InputField, LayoutDirection,
    LayoutGroup, ProgressBar, RectMask, RectTransform, ScaleMode, Slider, Text, TextAlign, Toggle,
    ToggleStyle, VerticalAlign,
};
pub use events::{submit_message, UiContext, UiEvent, UiEventKind, UiEvents};
pub use layout::{canvas_scale, resolve_rect, Rect};
pub use system::UiSystem;

use engine_core::app::App;
use engine_core::reflect::TypeRegistry;

/// Всё, что нужно игре: попадает в `engine::prelude`.
pub mod prelude {
    pub use crate::builder::{UiNode, UiScene};
    pub use crate::components::{
        Alignment, Anchor, Button, Canvas, CanvasGroup, FillDirection, Image, InputField,
        LayoutDirection, LayoutGroup, ProgressBar, RectMask, RectTransform, ScaleMode, Slider, Text,
        TextAlign, Toggle, ToggleStyle, VerticalAlign,
    };
    pub use crate::events::{UiContext, UiEvent, UiEventKind, UiEvents};
    pub use engine_assets::Font;
}

/// Зарегистрировать компоненты интерфейса в реестре типов (для загрузки сцен).
pub fn register_components(registry: &mut TypeRegistry) {
    registry
        .register::<Canvas>()
        .register::<RectTransform>()
        .register::<LayoutGroup>()
        .register::<RectMask>()
        .register::<CanvasGroup>()
        .register::<Image>()
        .register::<Text>()
        .register::<Button>()
        .register::<Toggle>()
        .register::<Slider>()
        .register::<ProgressBar>()
        .register::<InputField>();
    // Категория меню Add Component в редакторе.
    registry.categorize(
        &[
            "Canvas", "RectTransform", "LayoutGroup", "RectMask", "CanvasGroup", "Image", "Text", "Button",
            "Toggle", "Slider", "ProgressBar", "InputField",
        ],
        "UI",
    );
}

/// Подключить интерфейс к приложению: компоненты, ресурсы, подсистема.
/// Вызывается из `engine::app()`.
pub fn install(app: &mut App) {
    register_components(&mut app.world.registry);
    app.world.resources.insert(UiEvents::default());
    app.world.resources.insert(engine_render::Overlay::default());
    app.add_subsystem(UiSystem::new());
}
