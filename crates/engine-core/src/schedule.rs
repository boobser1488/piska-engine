//! Отложенные вызовы без корутин: «через N секунд», «каждые N секунд», «уничтожить через N».
//!
//! Планировщик — ресурс мира ([`Scheduler`]); обычно им пользуются через `Ctx`:
//!
//! ```
//! use engine_core::prelude::*;
//!
//! component! {
//!     pub struct Bomb { pub fuse: f32 = 3.0 }
//! }
//! impl Component for Bomb {
//!     fn on_start(&mut self, ctx: &mut Ctx) {
//!         // Через `fuse` секунд этому объекту придёт сообщение «Explode».
//!         ctx.after(self.fuse, "Explode", Value::Nil);
//!     }
//!     fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
//!         if message.name == "Explode" {
//!             ctx.destroy_self();
//!         }
//!     }
//! }
//! ```
//!
//! Правила, общие для всех таймеров:
//! * время идёт по `time.delta` (с учётом `time_scale`: на паузе таймеры стоят) —
//!   если не вызвать [`Scheduler::unscaled`];
//! * таймер, привязанный к объекту («владелец»), умирает вместе с ним — в том числе
//!   при замене сцены; пока владелец неактивен в иерархии, таймер не идёт;
//! * таймер без владельца (созданный напрямую через планировщик) переживает смену сцены.

use crate::object::GameObjectId;
use crate::reflect::Value;
use crate::world::World;

/// Идентификатор таймера для [`Scheduler::cancel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

enum Action {
    Message {
        target: GameObjectId,
        name: String,
        value: Value,
    },
    Destroy(GameObjectId),
    Call(Box<dyn FnMut(&mut World)>),
}

struct Entry {
    id: TimerId,
    remaining: f32,
    /// `Some` — повторяющийся.
    interval: Option<f32>,
    /// Сколько срабатываний осталось (только для повторяющихся); `None` — без предела.
    times_left: Option<u32>,
    unscaled: bool,
    owner: Option<GameObjectId>,
    /// Стоять, пока владелец неактивен в иерархии.
    needs_active: bool,
    /// `None` только пока действие выполняется.
    action: Option<Action>,
    dead: bool,
}

/// Очередь таймеров. Лежит в `world.resources`; создаётся при первом обращении.
#[derive(Default)]
pub struct Scheduler {
    entries: Vec<Entry>,
    next_id: u64,
}

impl Scheduler {
    fn push(&mut self, delay: f32, interval: Option<f32>, owner: Option<GameObjectId>, needs_active: bool, action: Action) -> TimerId {
        self.next_id += 1;
        let id = TimerId(self.next_id);
        self.entries.push(Entry {
            id,
            remaining: delay.max(0.0),
            interval: interval.map(|i| i.max(0.0)),
            times_left: None,
            unscaled: false,
            owner,
            needs_active,
            action: Some(action),
            dead: false,
        });
        id
    }

    /// Через `delay` секунд отправить `target` сообщение `name` (хук `on_message`).
    /// Таймер умирает вместе с `target`.
    pub fn send_after(&mut self, target: GameObjectId, delay: f32, name: &str, value: Value) -> TimerId {
        let action = Action::Message {
            target,
            name: name.to_string(),
            value,
        };
        self.push(delay, None, Some(target), true, action)
    }

    /// Отправлять сообщение каждые `interval` секунд (первое — через `interval`).
    /// Остановить: [`cancel`](Self::cancel) или [`limit`](Self::limit).
    pub fn send_every(&mut self, target: GameObjectId, interval: f32, name: &str, value: Value) -> TimerId {
        let action = Action::Message {
            target,
            name: name.to_string(),
            value,
        };
        self.push(interval, Some(interval), Some(target), true, action)
    }

    /// Один раз вызвать `f` через `delay` секунд. Без владельца: переживает смену сцены
    /// (привязать к объекту — [`owned_by`](Self::owned_by)).
    pub fn call_after(&mut self, delay: f32, f: impl FnOnce(&mut World) + 'static) -> TimerId {
        let mut f = Some(f);
        let action = Action::Call(Box::new(move |world| {
            if let Some(f) = f.take() {
                f(world);
            }
        }));
        self.push(delay, None, None, true, action)
    }

    /// Вызывать `f` каждые `interval` секунд.
    pub fn call_every(&mut self, interval: f32, f: impl FnMut(&mut World) + 'static) -> TimerId {
        self.push(interval, Some(interval), None, true, Action::Call(Box::new(f)))
    }

    /// Unity: `Destroy(obj, delay)`. Работает и на неактивном объекте.
    pub fn destroy_after(&mut self, target: GameObjectId, delay: f32) -> TimerId {
        self.push(delay, None, Some(target), false, Action::Destroy(target))
    }

    /// Отменить таймер. `false`, если такого уже нет (сработал или отменён раньше).
    pub fn cancel(&mut self, id: TimerId) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id && !e.dead) {
            Some(entry) => {
                entry.dead = true;
                true
            }
            None => false,
        }
    }

    /// Отменить все таймеры объекта. Возвращает, сколько отменено.
    pub fn cancel_owned_by(&mut self, owner: GameObjectId) -> usize {
        let mut count = 0;
        for entry in self.entries.iter_mut().filter(|e| !e.dead && e.owner == Some(owner)) {
            entry.dead = true;
            count += 1;
        }
        count
    }

    /// Отменить вообще всё.
    pub fn cancel_all(&mut self) {
        self.entries.clear();
    }

    pub fn is_pending(&self, id: TimerId) -> bool {
        self.entries.iter().any(|e| e.id == id && !e.dead)
    }

    /// Сколько таймеров ждёт.
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|e| !e.dead).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Секунд до ближайшего срабатывания таймера (без учёта масштаба времени).
    pub fn remaining(&self, id: TimerId) -> Option<f32> {
        self.entries.iter().find(|e| e.id == id && !e.dead).map(|e| e.remaining)
    }

    /// Считать по реальному времени: не замедляется и не замирает вместе с `time_scale`
    /// (таймеры интерфейса на паузе игры).
    pub fn unscaled(&mut self, id: TimerId) -> TimerId {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.unscaled = true;
        }
        id
    }

    /// Привязать таймер к объекту: умрёт вместе с ним и будет стоять, пока он неактивен.
    pub fn owned_by(&mut self, id: TimerId, owner: GameObjectId) -> TimerId {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.owner = Some(owner);
        }
        id
    }

    /// Повторяющийся таймер сработает не более `times` раз и исчезнет.
    pub fn limit(&mut self, id: TimerId, times: u32) -> TimerId {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.times_left = Some(times);
            if times == 0 {
                entry.dead = true;
            }
        }
        id
    }
}

/// Шаг планировщика: вызывается из `App::tick` раз в кадр, перед `on_update`.
pub(crate) fn run(world: &mut World) {
    let (dt, unscaled_dt) = (world.time.delta, world.time.unscaled_delta);

    // Фаза 1: продвинуть часы, выяснить, кто и сколько раз срабатывает.
    let mut due: Vec<(usize, u32)> = Vec::new();
    {
        let World { scene, resources, .. } = world;
        let Some(scheduler) = resources.get_mut::<Scheduler>() else {
            return;
        };
        for (index, entry) in scheduler.entries.iter_mut().enumerate() {
            if entry.dead {
                continue;
            }
            if let Some(owner) = entry.owner {
                if !scene.contains(owner) {
                    entry.dead = true;
                    continue;
                }
                if entry.needs_active && !scene.is_active(owner) {
                    continue;
                }
            }
            entry.remaining -= if entry.unscaled { unscaled_dt } else { dt };
            if entry.remaining > 0.0 {
                continue;
            }
            let mut count = 1;
            if let Some(interval) = entry.interval {
                // Большой кадр может «пропустить» несколько срабатываний — не теряем их,
                // но и не устраиваем лавину (не больше 16 за кадр).
                entry.remaining += interval;
                while entry.remaining <= 0.0 && count < 16 && interval > 0.0 {
                    entry.remaining += interval;
                    count += 1;
                }
                if entry.remaining <= 0.0 {
                    entry.remaining = interval;
                }
            }
            if let Some(left) = entry.times_left {
                count = count.min(left);
            }
            due.push((index, count));
        }
    }

    // Фаза 2: выполнить. Записи не удаляются до конца, поэтому индексы стабильны,
    // а `cancel` из колбэка просто помечает запись.
    for (index, count) in due {
        for _ in 0..count {
            let Some(entry) = scheduler_entry(world, index) else {
                break;
            };
            if entry.dead {
                break;
            }
            let Some(mut action) = entry.action.take() else {
                break;
            };
            match &mut action {
                Action::Message { target, name, value } => world.send_message(*target, name, value.clone()),
                Action::Destroy(target) => world.scene.destroy(*target),
                Action::Call(f) => f(world),
            }
            if let Some(entry) = scheduler_entry(world, index) {
                entry.action = Some(action);
                match (&mut entry.times_left, entry.interval) {
                    (_, None) => entry.dead = true,
                    (Some(left), Some(_)) => {
                        *left = left.saturating_sub(1);
                        if *left == 0 {
                            entry.dead = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(scheduler) = world.resources.get_mut::<Scheduler>() {
        scheduler.entries.retain(|e| !e.dead);
    }
    world.flush_messages();
}

fn scheduler_entry(world: &mut World, index: usize) -> Option<&mut Entry> {
    world.resources.get_mut::<Scheduler>()?.entries.get_mut(index)
}
