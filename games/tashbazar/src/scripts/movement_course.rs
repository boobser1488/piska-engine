use engine::prelude::*;

use crate::game::{sounds, CourseFinished};
use crate::scripts::checkpoint::Checkpoint;

component! {
    /// Полоса препятствий на время: отметки по порядку (первая — старт, последняя —
    /// финиш). Секундомер идёт с момента, когда игрок покинул старт; упал ниже
    /// `fail_below_y` после первой отметки — возвращается на последнюю взятую. Лучшее
    /// время хранится в сохранениях под ключом `save_key`.
    pub struct MovementCourse {
        /// Отметки по порядку.
        pub checkpoints: Vec<ObjectRef> = Vec::new(),
        /// Игрок (объект с `FpsController`).
        pub player: ObjectRef = ObjectRef::NONE,
        /// Ниже этой высоты (ноги, м) — падение: игрок возвращается на последнюю отметку.
        pub fail_below_y: f32 = 0.6,
        /// Падения считаются, пока номер следующей отметки в `fail_from..=fail_to`
        /// (участок по крышам; к финишу на земле — уже нет).
        pub fail_from: i32 = 2,
        pub fail_to: i32 = 99,
        /// Ключ лучшего времени в сохранениях.
        pub save_key: String = "course.best".to_string(),
        /// Яркость подсветки следующей отметки.
        pub glow: f32 = 2.5,
        /// Идёт круг (во время игры).
        pub running: bool = false,
        /// Игрок на старте, секундомер ждёт (во время игры).
        pub armed: bool = false,
        /// Время круга, с (во время игры).
        pub time: f32 = 0.0,
        /// Номер следующей отметки (во время игры).
        pub next: i32 = 0,
        /// Сколько раз игрок падал за круг (во время игры).
        pub falls: i32 = 0,
    }
}

impl MovementCourse {
    /// Отметка с номером `index`: объект, основание, компонент.
    fn checkpoint(&self, scene: &Scene, index: usize) -> Option<(GameObjectId, Vec3, Checkpoint)> {
        let id = scene.resolve(*self.checkpoints.get(index)?)?;
        Some((id, scene.world_position(id), scene.get_component::<Checkpoint>(id)?.clone()))
    }

    /// Поставить игрока на старт и ждать, пока он с него сойдёт.
    pub fn arm(&mut self, scene: &mut Scene) {
        self.running = false;
        self.armed = true;
        self.time = 0.0;
        self.next = 1;
        self.falls = 0;
        self.return_player(scene, 0);
    }

    /// Остановить круг (выход в меню).
    pub fn stop(&mut self) {
        self.running = false;
        self.armed = false;
    }

    /// Вернуть игрока на отметку `index` (ноги — в основание, взгляд — по отметке).
    pub fn return_player(&self, scene: &mut Scene, index: usize) {
        let (Some(player), Some((_, base, checkpoint))) = (scene.resolve(self.player), self.checkpoint(scene, index)) else {
            return;
        };
        if let Some(object) = scene.get_mut(player) {
            object.transform.position = base + Vec3::Y * 0.01;
        }
        if let Some(controller) = scene.get_component_mut::<FpsController>(player) {
            controller.velocity = Vec3::ZERO;
            controller.yaw_deg = checkpoint.respawn_yaw_deg;
            controller.pitch_deg = 0.0;
            controller.ducked = false;
        }
    }

    /// Лучшее время из сохранений (0 — круга ещё не было).
    pub fn best(&self, save: &SaveData) -> f32 {
        save.float(&self.save_key, 0.0)
    }

    fn highlight(&self, scene: &mut Scene) {
        let next = self.next.max(0) as usize;
        let active = self.running || self.armed;
        for (index, reference) in self.checkpoints.iter().enumerate() {
            let Some(id) = scene.resolve(*reference) else { continue };
            let Some(beacon) = scene.get_component::<Checkpoint>(id).and_then(|c| scene.resolve(c.beacon)) else {
                continue;
            };
            let on = active && (index == next || (self.armed && index == 0));
            let glow = if on { self.glow } else { 0.0 };
            let color = if index + 1 == self.checkpoints.len() { Color::rgb(1.0, 0.75, 0.2) } else { Color::rgb(0.25, 0.85, 1.0) };
            if let Some(renderer) = scene.get_component_mut::<MeshRenderer>(beacon) {
                renderer.emissive = color.scaled(glow);
            }
        }
    }
}

impl Component for MovementCourse {
    fn hooks(&self) -> Hooks {
        Hooks::FIXED_UPDATE | Hooks::UPDATE
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if !(self.running || self.armed) || self.checkpoints.len() < 2 {
            return;
        }
        let Some(player) = ctx.resolve(self.player) else { return };
        let feet = ctx.scene().world_position(player);
        if self.armed {
            let Some((_, base, start)) = self.checkpoint(ctx.scene(), 0) else { return };
            if !start.contains(base, feet) {
                self.armed = false;
                self.running = true;
                self.time = 0.0;
            }
            return;
        }
        self.time += dt;
        let next = self.next.max(1) as usize;
        if let Some((_, base, checkpoint)) = self.checkpoint(ctx.scene(), next) {
            if checkpoint.contains(base, feet) {
                self.next += 1;
                if self.next as usize >= self.checkpoints.len() {
                    self.running = false;
                    let previous_best = self.best(ctx.save_data());
                    if previous_best <= 0.0 || self.time < previous_best {
                        let key = self.save_key.clone();
                        let save = ctx.save_data();
                        save.set_float(&key, self.time);
                        if let Err(error) = save.save() {
                            log::warn!("best course time was not saved: {error}");
                        }
                    }
                    ctx.emit(CourseFinished { time: self.time, previous_best });
                    sounds::play(ctx.world, "finish", 0.9, 1.0);
                } else {
                    sounds::play(ctx.world, "checkpoint", 0.8, 1.0);
                }
                return;
            }
        }
        // Упал с крыш — назад на последнюю взятую отметку.
        if self.next >= self.fail_from && self.next <= self.fail_to && feet.y < self.fail_below_y {
            self.falls += 1;
            let back = (self.next - 1).max(0) as usize;
            self.return_player(ctx.scene_mut(), back);
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        self.highlight(ctx.scene_mut());
    }
}
