//! HUD: здоровье дрона и реактора, волна, счёт и множитель, опыт, рывок, шкала босса,
//! красная виньетка, крупные надписи и экранные стики для касаний.
//!
//! Компонент `Hud` на менеджере каждый кадр обновляет содержимое холста `HUD`
//! (он лежит в сцене как экземпляр префаба `ui/hud.prefab`).

use engine::prelude::*;

use crate::scripts::game_manager::Game;
use crate::data::CAMPAIGN_WAVES;
use crate::sim::*;
use crate::ui::{cyan, muted, grouped, set_bar, set_bar_color, set_text, set_text_color, Ui};

component! {
    /// Настройки HUD.
    pub struct Hud {
        /// Ниже этой доли здоровья дрона включается красная виньетка.
        pub danger_below: f32 = 0.38,
        /// Цвет надписи «волна пройдена».
        pub clear_color: Color = Color::hex(0x7dffb0),
        /// Цвет надписей о боссе.
        pub boss_color: Color = Color::hex(0xff6a7a),
    }
}

impl Component for Hud {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<Game>() else { return };
        if game.hud_visible() {
            let boss = game.sim.boss().map(|b| (b.kind.info().name.as_str(), (b.hp / b.max_hp).clamp(0.0, 1.0)));
            game.ui.update_hud(&mut ctx.world.scene, &game.sim, boss, game.clock, self.danger_below);
        }
        ctx.world.resources.insert(game);
    }
}

impl Ui {
    pub fn update_hud(&mut self, scene: &mut Scene, sim: &Sim, boss_name: Option<(&str, f32)>, t: f32, danger_below: f32) {
        let p = &sim.player;
        set_text(
            scene,
            self.hp_text,
            &if p.alive { format!("ДРОН   {:.0} / {:.0}", p.hp.max(0.0).ceil(), sim.stats.max_hp) } else { format!("ДРОН   восстановление {:.0}", p.respawn_t.max(0.0).ceil()) },
        );
        set_bar(scene, self.hp_bar, if p.alive { p.hp / sim.stats.max_hp } else { 0.0 });
        let hp_frac = p.hp / sim.stats.max_hp;
        set_bar_color(scene, self.hp_bar, if hp_frac < 0.3 { Color::hex(0xff4d5e) } else { Color::hex(0x4be07a) });
        set_text(scene, self.core_text, &format!("РЕАКТОР   {:.0} / {:.0}", sim.core.hp.ceil(), sim.core.max_hp));
        let core_frac = sim.core.hp / sim.core.max_hp;
        set_bar(scene, self.core_bar, core_frac);
        set_bar_color(scene, self.core_bar, if core_frac < 0.3 { Color::hex(0xff4d5e) } else { Color::hex(0xff9a2e) });
        let wave_label = if sim.mode == Mode::Campaign { format!("ВОЛНА {} / {}", sim.wave.max(1), CAMPAIGN_WAVES) } else { format!("ВОЛНА {}", sim.wave.max(1)) };
        set_text(scene, self.wave_text, &wave_label);
        set_text(
            scene,
            self.left_text,
            &match sim.phase {
                crate::sim::Phase::Intermission => format!("Следующая волна через {:.0}", sim.intermission_t.max(0.0).ceil()),
                crate::sim::Phase::Cleared => "Волна пройдена".to_string(),
                _ => format!("Врагов осталось: {}", sim.remaining()),
            },
        );
        match boss_name {
            Some((name, frac)) => {
                scene.ui_set_visible(self.boss_name, true);
                scene.ui_set_visible(self.boss_bar, true);
                set_text(scene, self.boss_name, name);
                set_bar(scene, self.boss_bar, frac);
            }
            None => {
                scene.ui_set_visible(self.boss_name, false);
                scene.ui_set_visible(self.boss_bar, false);
            }
        }
        set_text(scene, self.score_text, &grouped(sim.score));
        set_text(scene, self.mult_text, &format!("×{:.1}", sim.mult()));
        set_bar(scene, self.combo_bar, if sim.combo_t > 0.0 { (sim.combo_t / 3.0).min(1.0) } else { 0.0 });
        set_bar(scene, self.xp_bar, sim.xp / sim.xp_need);
        set_text(scene, self.level_text, &format!("Уровень {}   ·   обломки {:.0} / {:.0}", sim.level, sim.xp.floor(), sim.xp_need.ceil()));
        let dash_ready = p.dash_cd <= 0.0;
        set_bar(scene, self.dash_bar, 1.0 - (p.dash_cd / sim.stats.dash_cooldown).clamp(0.0, 1.0));
        set_text_color(scene, self.dash_text, if dash_ready { cyan() } else { muted() });
        set_text(scene, self.dash_text, if dash_ready { "РЫВОК — ПРОБЕЛ" } else { "РЫВОК" });
        // Виньетка низкого здоровья.
        let danger = if p.alive { ((danger_below - hp_frac) / danger_below).clamp(0.0, 1.0) } else { 0.0 };
        let core_danger = ((0.3 - core_frac) / 0.3).clamp(0.0, 1.0);
        let d = danger.max(core_danger * 0.8);
        let a = if d > 0.0 { d * (0.45 + 0.25 * (t * 6.0).sin()) } else { 0.0 };
        if (a - self.last_vignette).abs() > 0.004 {
            self.last_vignette = a;
            if let Some(img) = scene.get_component_mut::<Image>(self.vignette) {
                img.color = Color::rgba(1.0, 1.0, 1.0, a);
            }
        }
    }

    /// Показать крупную надпись по центру и плавно погасить.
    pub fn banner(&self, ctx: &mut Ctx, text: &str, sub: &str, color: Color) {
        set_text(ctx.scene_mut(), self.banner, text);
        set_text(ctx.scene_mut(), self.banner_sub, sub);
        set_text_color(ctx.scene_mut(), self.banner, color);
        for id in [self.banner, self.banner_sub] {
            ctx.cancel_tweens_of(id);
            if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
                t.outline_color = t.outline_color.with_alpha(0.92);
            }
        }
        let sub_color = muted();
        set_text_color(ctx.scene_mut(), self.banner_sub, sub_color);
        for (id, delay) in [(self.banner, 1.4), (self.banner_sub, 1.4)] {
            ctx.start_tween(Tween::field(id, "Text.color.a", 0.0f32).from(1.0f32).duration(0.7).delay(delay).ease(Ease::InQuad));
            ctx.start_tween(Tween::field(id, "Text.outline_color.a", 0.0f32).from(0.92f32).duration(0.7).delay(delay).ease(Ease::InQuad));
        }
        ctx.start_tween(Tween::field(self.banner, "RectTransform.position", Vec2::new(0.0, -230.0)).from(Vec2::new(0.0, -200.0)).duration(0.35).ease(Ease::OutBack));
    }

    /// Положение экранных стиков (в пикселях экрана) и видимость.
    pub fn set_touch_ui(&self, scene: &mut Scene, scale: f32, visible: bool, left: Option<(Vec2, Vec2)>, right: Option<(Vec2, Vec2)>) {
        let [lb, lk, rb, rk] = self.touch_nodes;
        scene.ui_set_visible(self.dash_btn, visible);
        let place = |scene: &mut Scene, base: GameObjectId, knob: GameObjectId, v: Option<(Vec2, Vec2)>| {
            match v {
                Some((origin, current)) => {
                    scene.ui_set_visible(base, true);
                    scene.ui_set_visible(knob, true);
                    let o = origin / scale;
                    let c = current / scale;
                    if let Some(rt) = scene.get_component_mut::<RectTransform>(base) {
                        rt.pivot = Vec2::splat(0.5);
                        rt.position = o;
                    }
                    if let Some(rt) = scene.get_component_mut::<RectTransform>(knob) {
                        rt.pivot = Vec2::splat(0.5);
                        rt.position = c;
                    }
                }
                None => {
                    scene.ui_set_visible(base, false);
                    scene.ui_set_visible(knob, false);
                }
            }
        };
        place(scene, lb, lk, left);
        place(scene, rb, rk, right);
    }
}
