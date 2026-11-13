//! События интерфейса для игрового кода.
//!
//! Два способа узнать о клике:
//! * **сообщение** — виджет шлёт `Message { name, value }` себе и всем предкам
//!   (ловится в `on_message` скрипта на меню или холсте);
//! * **опрос** — `ctx.ui().clicked("play")` в `on_update` того же кадра.

use engine_core::prelude::*;

/// Что произошло с виджетом.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEventKind {
    /// Кнопка нажата и отпущена над ней (или Enter в фокусе).
    Click,
    /// Переключатель сменил состояние.
    Toggle(bool),
    /// Ползунок сменил значение.
    Value(f32),
    /// Текст поля ввода изменился.
    TextChanged(String),
    /// В поле ввода нажали Enter.
    Submit(String),
}

/// Событие этого кадра.
#[derive(Clone, Debug, PartialEq)]
pub struct UiEvent {
    /// Объект виджета.
    pub source: GameObjectId,
    /// Имя сообщения виджета (поле `message`, а если оно пустое — имя объекта).
    pub name: String,
    pub kind: UiEventKind,
}

/// Состояние интерфейса за текущий кадр: события, наведение, фокус. Лежит в
/// `world.resources`; события живут с начала кадра (до скриптов) до начала следующего.
#[derive(Debug, Default)]
pub struct UiEvents {
    pub(crate) events: Vec<UiEvent>,
    pub(crate) pointer_over_ui: bool,
    pub(crate) hovered: Option<GameObjectId>,
    pub(crate) focused: Option<GameObjectId>,
    pub(crate) editing_text: bool,
}

static EMPTY: UiEvents = UiEvents {
    events: Vec::new(),
    pointer_over_ui: false,
    hovered: None,
    focused: None,
    editing_text: false,
};

impl UiEvents {
    /// Все события кадра в порядке возникновения.
    pub fn events(&self) -> &[UiEvent] {
        &self.events
    }

    /// Нажата ли в этом кадре кнопка с таким именем сообщения.
    pub fn clicked(&self, name: &str) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == UiEventKind::Click && e.name == name)
    }

    /// Нажат ли в этом кадре конкретный виджет.
    pub fn clicked_id(&self, id: GameObjectId) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == UiEventKind::Click && e.source == id)
    }

    /// Новое состояние переключателя, если его переключили в этом кадре.
    pub fn toggled(&self, name: &str) -> Option<bool> {
        self.events.iter().rev().find_map(|e| match e.kind {
            UiEventKind::Toggle(on) if e.name == name => Some(on),
            _ => None,
        })
    }

    /// Новое значение ползунка, если его двигали в этом кадре.
    pub fn value_changed(&self, name: &str) -> Option<f32> {
        self.events.iter().rev().find_map(|e| match e.kind {
            UiEventKind::Value(v) if e.name == name => Some(v),
            _ => None,
        })
    }

    /// Новый текст поля, если его меняли в этом кадре.
    pub fn text_changed(&self, name: &str) -> Option<&str> {
        self.events.iter().rev().find_map(|e| match &e.kind {
            UiEventKind::TextChanged(text) if e.name == name => Some(text.as_str()),
            _ => None,
        })
    }

    /// Текст поля, если в нём нажали Enter в этом кадре.
    pub fn submitted(&self, name: &str) -> Option<&str> {
        self.events.iter().rev().find_map(|e| match &e.kind {
            UiEventKind::Submit(text) if e.name == name => Some(text.as_str()),
            _ => None,
        })
    }

    /// Курсор над элементом интерфейса, который ловит мышь: игре стоит игнорировать клик.
    pub fn pointer_over_ui(&self) -> bool {
        self.pointer_over_ui
    }

    /// Идёт ввод текста в поле: игре стоит игнорировать клавиатуру.
    pub fn wants_keyboard(&self) -> bool {
        self.editing_text
    }

    /// Виджет под курсором.
    pub fn hovered(&self) -> Option<GameObjectId> {
        self.hovered
    }

    /// Виджет в фокусе (клавиатура, поле ввода).
    pub fn focused(&self) -> Option<GameObjectId> {
        self.focused
    }
}

/// `ctx.ui()` / `world.ui()` — состояние интерфейса этого кадра.
pub trait UiContext {
    fn ui(&self) -> &UiEvents;
}

impl UiContext for World {
    fn ui(&self) -> &UiEvents {
        self.resources.get::<UiEvents>().unwrap_or(&EMPTY)
    }
}

impl UiContext for Ctx<'_> {
    fn ui(&self) -> &UiEvents {
        self.world.ui()
    }
}

/// Имя сообщения, которое Enter в поле ввода `name` шлёт вместе с текстом.
pub fn submit_message(name: &str) -> String {
    format!("{name}:submit")
}
