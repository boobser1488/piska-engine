//! Значения-таймеры для хранения внутри компонентов: [`Timer`] и [`Cooldown`].
//!
//! Они не знают про мир: компонент сам «тикает» их своим `dt` в `on_update`.
//! Оба типа можно объявлять полями в `component!` (в сцене они сохраняются как списки чисел).

use crate::reflect::{FieldValue, Value};

/// Таймер «через N секунд» или «каждые N секунд». Unity: `Invoke` / `InvokeRepeating`,
/// только без строковых имён методов.
///
/// ```
/// use engine_core::prelude::*;
///
/// let mut spawn = Timer::repeating(0.5);
/// let mut spawned = 0;
/// for _ in 0..10 {
///     if spawn.tick(0.1).just_finished() {
///         spawned += 1;
///     }
/// }
/// assert_eq!(spawned, 2);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    duration: f32,
    elapsed: f32,
    repeating: bool,
    finished: bool,
    just_finished: bool,
    times_finished: u32,
    paused: bool,
}

impl Default for Timer {
    fn default() -> Self {
        Self::once(1.0)
    }
}

impl Timer {
    /// Сработает один раз через `seconds`, дальше «стоит» в состоянии `finished`.
    pub fn once(seconds: f32) -> Self {
        Self::new(seconds, false)
    }

    /// Срабатывает каждые `seconds`.
    pub fn repeating(seconds: f32) -> Self {
        Self::new(seconds, true)
    }

    fn new(seconds: f32, repeating: bool) -> Self {
        Self {
            duration: seconds.max(0.0),
            elapsed: 0.0,
            repeating,
            finished: false,
            just_finished: false,
            times_finished: 0,
            paused: false,
        }
    }

    /// Продвинуть таймер на `dt` секунд. Возвращает `&mut self`, чтобы писать
    /// `if timer.tick(dt).just_finished() { … }`.
    pub fn tick(&mut self, dt: f32) -> &mut Self {
        self.just_finished = false;
        self.times_finished = 0;
        if self.paused || (self.finished && !self.repeating) {
            return self;
        }
        self.elapsed += dt.max(0.0);
        if self.elapsed >= self.duration {
            self.finished = true;
            self.just_finished = true;
            if self.repeating && self.duration > 0.0 {
                // Сколько раз «щёлкнул» за этот тик (при большом dt — несколько).
                self.times_finished = (self.elapsed / self.duration) as u32;
                self.elapsed %= self.duration;
            } else {
                self.times_finished = 1;
                self.elapsed = self.duration;
            }
        }
        self
    }

    /// Сработал в последнем `tick`.
    pub fn just_finished(&self) -> bool {
        self.just_finished
    }

    /// Сколько раз сработал в последнем `tick` (для `repeating` при большом `dt` бывает больше 1).
    pub fn times_finished(&self) -> u32 {
        self.times_finished
    }

    /// Одноразовый: дошёл до конца (остаётся `true` до `reset`). Повторяющийся: срабатывал хотя бы раз.
    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn remaining(&self) -> f32 {
        (self.duration - self.elapsed).max(0.0)
    }

    /// Прогресс 0..=1 текущего круга.
    pub fn fraction(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_duration(&mut self, seconds: f32) {
        self.duration = seconds.max(0.0);
    }

    pub fn is_repeating(&self) -> bool {
        self.repeating
    }

    /// Начать сначала (и снять флаги срабатывания).
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.finished = false;
        self.just_finished = false;
        self.times_finished = 0;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn unpause(&mut self) {
        self.paused = false;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

/// Перезарядка способности: `if self.shoot.trigger() { … }`; в `on_update` — `self.shoot.tick(dt)`.
///
/// ```
/// use engine_core::prelude::*;
///
/// let mut dash = Cooldown::new(2.0);
/// assert!(dash.trigger());   // готова — применили, пошла перезарядка
/// assert!(!dash.trigger());  // ещё нет
/// dash.tick(2.0);
/// assert!(dash.ready());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Cooldown {
    duration: f32,
    remaining: f32,
}

impl Default for Cooldown {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl Cooldown {
    /// Новая перезарядка длиной `seconds`; сразу готова к применению.
    pub fn new(seconds: f32) -> Self {
        Self {
            duration: seconds.max(0.0),
            remaining: 0.0,
        }
    }

    /// То же, но начинает «остывшей»: первый раз можно будет применить через `seconds`.
    pub fn starting_cold(seconds: f32) -> Self {
        Self {
            duration: seconds.max(0.0),
            remaining: seconds.max(0.0),
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.remaining = (self.remaining - dt.max(0.0)).max(0.0);
    }

    pub fn ready(&self) -> bool {
        self.remaining <= 0.0
    }

    /// Если готова — запустить перезарядку и вернуть `true`; иначе `false` и ничего не меняется.
    pub fn trigger(&mut self) -> bool {
        if self.ready() {
            self.remaining = self.duration;
            true
        } else {
            false
        }
    }

    /// Запустить перезарядку принудительно (даже если ещё не остыла).
    pub fn start(&mut self) {
        self.remaining = self.duration;
    }

    /// Сделать готовой немедленно.
    pub fn reset(&mut self) {
        self.remaining = 0.0;
    }

    pub fn remaining(&self) -> f32 {
        self.remaining
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_duration(&mut self, seconds: f32) {
        self.duration = seconds.max(0.0);
    }

    /// Сколько уже перезарядилось, 0..=1 (1 — готова). Для полоски на иконке способности.
    pub fn fraction(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (1.0 - self.remaining / self.duration).clamp(0.0, 1.0)
        }
    }
}

/// В сцене: `List([Float(duration), Float(elapsed), Bool(repeating)])`.
impl FieldValue for Timer {
    const EXPECTED: &'static str = "List(duration, elapsed, repeating)";
    fn to_value(&self) -> Value {
        Value::List(vec![
            Value::Float(self.duration),
            Value::Float(self.elapsed),
            Value::Bool(self.repeating),
        ])
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(items) => match items.as_slice() {
                [Value::Float(duration), Value::Float(elapsed), Value::Bool(repeating)] => {
                    let mut timer = Timer::new(*duration, *repeating);
                    timer.elapsed = elapsed.clamp(0.0, timer.duration);
                    Some(timer)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// В сцене: `Vec2(duration, remaining)`.
impl FieldValue for Cooldown {
    const EXPECTED: &'static str = "Vec2(duration, remaining)";
    fn to_value(&self) -> Value {
        Value::Vec2(self.duration, self.remaining)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Vec2(duration, remaining) => Some(Self {
                duration: duration.max(0.0),
                remaining: remaining.clamp(0.0, duration.max(0.0)),
            }),
            _ => None,
        }
    }
}
