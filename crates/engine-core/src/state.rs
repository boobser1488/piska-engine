//! Крошечный конечный автомат для состояний врага, босса, меню или всей игры.
//!
//! Состояния — обычный `enum` (удобно `reflect_enum!`, тогда автомат можно держать полем
//! компонента). Переход запрашивается `change`, применяется в следующем `tick`, поэтому
//! весь код кадра видит одно и то же состояние, а «вход» и «выход» можно поймать ровно один раз.
//!
//! ```
//! use engine_core::prelude::*;
//!
//! reflect_enum! {
//!     pub enum Mode { Menu, Playing, GameOver }
//! }
//!
//! let mut mode = StateMachine::new(Mode::Menu);
//! mode.change(Mode::Playing);
//! mode.tick(0.016);
//! assert!(mode.just_entered(Mode::Playing));
//! assert_eq!(mode.previous(), Some(Mode::Menu));
//! mode.tick(0.5);
//! assert!(!mode.just_entered(Mode::Playing));
//! assert!(mode.time_in_state() > 0.49);
//! ```

use crate::reflect::{FieldValue, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct StateMachine<S: Copy + PartialEq> {
    current: S,
    previous: Option<S>,
    requested: Option<S>,
    time_in_state: f32,
    entered: bool,
}

impl<S: Copy + PartialEq> StateMachine<S> {
    /// Начальное состояние; `just_entered` для него истинно до первого `tick`.
    pub fn new(initial: S) -> Self {
        Self {
            current: initial,
            previous: None,
            requested: None,
            time_in_state: 0.0,
            entered: true,
        }
    }

    pub fn current(&self) -> S {
        self.current
    }

    pub fn is(&self, state: S) -> bool {
        self.current == state
    }

    /// Из какого состояния пришли в текущее.
    pub fn previous(&self) -> Option<S> {
        self.previous
    }

    /// Попросить переход; сработает в ближайшем `tick`. Последний запрос за кадр побеждает.
    pub fn change(&mut self, next: S) {
        self.requested = Some(next);
    }

    /// Применить запрошенный переход и продвинуть время. Вызывать один раз за кадр,
    /// в начале `on_update` (или там, где удобно владельцу автомата).
    pub fn tick(&mut self, dt: f32) {
        self.entered = false;
        if let Some(next) = self.requested.take() {
            if next != self.current {
                self.previous = Some(self.current);
                self.current = next;
                self.time_in_state = 0.0;
                self.entered = true;
                return;
            }
        }
        self.time_in_state += dt.max(0.0);
    }

    /// Секунд в текущем состоянии.
    pub fn time_in_state(&self) -> f32 {
        self.time_in_state
    }

    /// Переход в `state` случился в последнем `tick`.
    pub fn just_entered(&self, state: S) -> bool {
        self.entered && self.current == state
    }

    /// Из `state` вышли в последнем `tick`.
    pub fn just_exited(&self, state: S) -> bool {
        self.entered && self.previous == Some(state)
    }

    /// Есть непримённый запрос на переход.
    pub fn has_pending(&self) -> bool {
        self.requested.is_some()
    }
}

impl<S: Copy + PartialEq + Default> Default for StateMachine<S> {
    fn default() -> Self {
        Self::new(S::default())
    }
}

/// В сцене автомат — это его текущее состояние.
impl<S: FieldValue + Copy + PartialEq> FieldValue for StateMachine<S> {
    const EXPECTED: &'static str = S::EXPECTED;
    fn to_value(&self) -> Value {
        self.current.to_value()
    }
    fn from_value(value: &Value) -> Option<Self> {
        S::from_value(value).map(Self::new)
    }
    fn meta() -> crate::reflect::FieldMeta {
        S::meta()
    }
}
