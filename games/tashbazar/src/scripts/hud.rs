use engine::prelude::*;

use crate::game::format_time;
use crate::scripts::map_zone::zone_at;
use crate::scripts::movement_course::MovementCourse;

component! {
    /// HUD: прицел, спидометр (ед/с), координаты, компас и название места, секундомер
    /// полосы препятствий. Стоит на холсте `ui/hud.prefab`; тексты — поля-ссылки на детей
    /// холста, игрок и полоса — ссылки на объекты сцены.
    pub struct Hud {
        pub player: ObjectRef = ObjectRef::NONE,
        pub course: ObjectRef = ObjectRef::NONE,
        pub speed_text: ObjectRef = ObjectRef::NONE,
        pub position_text: ObjectRef = ObjectRef::NONE,
        pub place_text: ObjectRef = ObjectRef::NONE,
        pub course_panel: ObjectRef = ObjectRef::NONE,
        pub course_text: ObjectRef = ObjectRef::NONE,
        pub hint_text: ObjectRef = ObjectRef::NONE,
        /// Показывать спидометр (экран «Настройки»).
        pub show_speed: bool = true,
        /// Показывать подсказку по клавишам первые секунды.
        pub hint_seconds: f32 = 8.0,
    }
}

/// Румб для курса (0° — север, по часовой стрелке).
fn compass_point(heading: f32) -> &'static str {
    const POINTS: [&str; 8] = ["С", "СВ", "В", "ЮВ", "Ю", "ЮЗ", "З", "СЗ"];
    POINTS[((heading.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

fn set_text(scene: &mut Scene, reference: ObjectRef, text: &str) {
    if let Some(id) = scene.resolve(reference) {
        if let Some(t) = scene.get_component_mut::<Text>(id) {
            t.set(text);
        }
    }
}

impl Component for Hud {
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(player) = ctx.resolve(self.player) else { return };
        if !ctx.is_active(player) {
            return;
        }
        let Some(controller) = ctx.get_on::<FpsController>(player).cloned() else { return };
        let position = ctx.scene().world_position(player);

        let speed = if self.show_speed { format!("{:.0}", controller.units_per_second()) } else { String::new() };
        set_text(ctx.scene_mut(), self.speed_text, &speed);

        // Курс: 0° — север (−Z), по часовой; yaw контроллера растёт против часовой.
        let heading = (-controller.yaw_deg).rem_euclid(360.0);
        let mut flags = String::new();
        if controller.noclip {
            flags.push_str("  ·  NOCLIP");
        }
        if controller.ducked {
            flags.push_str("  ·  присед");
        }
        let readout = format!(
            "{} {:03.0}°   x {:.1}  y {:.1}  z {:.1}{flags}",
            compass_point(heading),
            heading,
            position.x,
            position.y,
            position.z
        );
        set_text(ctx.scene_mut(), self.position_text, &readout);

        let place = zone_at(ctx.scene(), position + Vec3::Y * 0.5).map(|(_, title)| title).unwrap_or_default();
        set_text(ctx.scene_mut(), self.place_text, &place);

        // Полоса препятствий.
        let course = ctx.resolve(self.course).and_then(|id| ctx.get_on::<MovementCourse>(id).cloned());
        let panel = ctx.resolve(self.course_panel);
        let visible = course.as_ref().is_some_and(|c| c.running || c.armed);
        if let Some(panel) = panel {
            if ctx.scene().get(panel).is_some_and(|o| o.active != visible) {
                ctx.scene_mut().ui_set_visible(panel, visible);
            }
        }
        if let Some(course) = course.filter(|_| visible) {
            let best = course.best(ctx.save_data());
            let total = course.checkpoints.len().saturating_sub(1);
            let taken = (course.next.max(1) as usize - 1).min(total);
            let mut line = if course.armed {
                format!("Полоса: сойдите со старта   ·   отметки 0/{total}")
            } else {
                format!("{}   ·   отметки {taken}/{total}", format_time(course.time))
            };
            if best > 0.0 {
                line.push_str(&format!("   ·   лучшее {}", format_time(best)));
            }
            if course.falls > 0 {
                line.push_str(&format!("   ·   падений {}", course.falls));
            }
            set_text(ctx.scene_mut(), self.course_text, &line);
        }

        // Подсказка по клавишам гаснет через несколько секунд игры.
        if self.hint_seconds > 0.0 {
            self.hint_seconds -= ctx.time().unscaled_delta;
            if self.hint_seconds <= 0.0 {
                if let Some(hint) = ctx.resolve(self.hint_text) {
                    ctx.scene_mut().ui_set_visible(hint, false);
                }
            }
        }
    }
}
