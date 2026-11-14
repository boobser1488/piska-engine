//! Ввод: клавиатура, мышь, геймпад и экранные стики для касаний.

use engine::prelude::*;

use crate::bot;
use crate::profile::*;
use crate::scripts::game_manager::*;
use crate::sim::*;
use crate::sound;
use crate::ui::{Screen, Ui};

impl Game {
    pub(crate) fn world_of_screen(&self, p: Vec2, screen: Vec2) -> Vec2 {
        Vec2::new((p.x - screen.x * 0.5) / screen.y * 2.0 * self.ortho, -(p.y - screen.y * 0.5) / screen.y * 2.0 * self.ortho)
    }

    /// Центр кнопки рывка на экране (пиксели) и её радиус.
    pub(crate) fn dash_button(&self, screen: Vec2, scale: f32) -> (Vec2, f32) {
        (Vec2::new(screen.x - 116.0 * scale, screen.y - 216.0 * scale), self.cfg.dash_button_radius * scale)
    }

    pub(crate) fn hud_scale(&self, ctx: &Ctx, screen: Vec2) -> f32 {
        ctx.scene().get_component::<Canvas>(self.ui.hud).map_or(1.0, |c| c.scale_for(screen))
    }

    pub(crate) fn gather_input(&mut self, ctx: &mut Ctx) -> PlayerInput {
        let screen = ctx.input().screen_size();
        let scale = self.hud_scale(ctx, screen);
        let input = ctx.input();
        let mut mv = Vec2::new(input.axis("Horizontal"), input.axis("Vertical")).clamp_length_max(1.0);
        let mut aim = Vec2::ZERO;
        let mut fire = false;
        let mut dash = input.button_down("Dash");

        // Касания: левая половина — движение, правая — прицел и огонь.
        let touches: Vec<Touch> = input.touches().to_vec();
        if !touches.is_empty() {
            self.touch.seen = true;
        }
        let (dash_center, dash_radius) = self.dash_button(screen, scale);
        for t in &touches {
            let assigned = self.touch.left.is_some_and(|(id, _)| id == t.id) || self.touch.right.is_some_and(|(id, _)| id == t.id);
            if t.phase == TouchPhase::Began && !assigned {
                if self.touch.seen && t.position.distance(dash_center) < dash_radius {
                    dash = true;
                } else if t.position.x < screen.x * 0.5 && self.touch.left.is_none() {
                    self.touch.left = Some((t.id, t.position));
                } else if t.position.x >= screen.x * 0.5 && self.touch.right.is_none() {
                    self.touch.right = Some((t.id, t.position));
                }
            }
        }
        let radius = self.cfg.stick_radius * scale;
        let mut left_view = None;
        let mut right_view = None;
        let mut touch_mv = None;
        let mut touch_aim = None;
        if let Some((id, origin)) = self.touch.left {
            match touches.iter().find(|t| t.id == id && t.phase != TouchPhase::Ended) {
                Some(t) => {
                    let d = (t.position - origin) / radius;
                    touch_mv = Some(d.clamp_length_max(1.0));
                    left_view = Some((origin, origin + d.clamp_length_max(1.0) * radius));
                }
                None => self.touch.left = None,
            }
        }
        if let Some((id, origin)) = self.touch.right {
            match touches.iter().find(|t| t.id == id && t.phase != TouchPhase::Ended) {
                Some(t) => {
                    let d = (t.position - origin) / radius;
                    touch_aim = Some(d.clamp_length_max(1.0));
                    right_view = Some((origin, origin + d.clamp_length_max(1.0) * radius));
                }
                None => self.touch.right = None,
            }
        }
        if let Some(m) = touch_mv {
            if m.length() > 0.12 {
                mv = m;
            }
        }
        if let Some(a) = touch_aim {
            if a.length() > 0.2 {
                aim = a.normalize();
                fire = true;
            }
        }
        // Правый стик геймпада и стрелки клавиатуры: прицел и огонь.
        let stick = Vec2::new(input.axis("AimX"), input.axis("AimY"));
        if stick.length() > 0.3 {
            aim = stick.normalize();
            fire = true;
            self.mouse_aim = false;
        }
        // Мышь: прицел в точку, огонь по кнопке.
        let mouse_active = input.mouse_delta().length_squared() > 0.0 || input.mouse_button_down(MouseButton::Left);
        if mouse_active && touches.is_empty() {
            self.mouse_aim = true;
        }
        if self.mouse_aim && stick.length() <= 0.3 && touches.is_empty() {
            let w = self.world_of_screen(input.mouse_position(), screen);
            let from = self.sim.player.pos;
            let d = w - from;
            if d.length_squared() > 0.01 {
                aim = d.normalize();
            }
            if input.button("Fire") && !ctx.ui().pointer_over_ui() {
                fire = true;
            }
        }
        // Гашетка геймпада работает и без правого стика.
        if input.gamepad_count() > 0 && input.button_value("Fire") > 0.3 && !self.mouse_aim {
            fire = true;
        }
        if self.touch.seen {
            let (l, r) = (left_view, right_view);
            self.ui.set_touch_ui(ctx.scene_mut(), scale, true, l, r);
        }
        if self.touch.seen && mv.length_squared() == 0.0 && left_view.is_none() {
            // нет движения
        }
        PlayerInput { mv, aim, fire, dash }
    }
}

component! {
    /// Управление: движение, прицел и огонь с клавиатуры, мыши, геймпада и двух
    /// экранных стиков (они показываются после первого касания).
    pub struct TouchControls {
        /// Радиус экранного стика в единицах холста.
        pub stick_radius: f32 = 70.0,
        /// Радиус кнопки рывка на экране.
        pub dash_button_radius: f32 = 52.0,
    }
}

impl Component for TouchControls {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<Game>() else { return };
        game.cfg.stick_radius = self.stick_radius;
        game.cfg.dash_button_radius = self.dash_button_radius;
        game.input = if game.screen == Screen::Playing { game.gather_input(ctx) } else { PlayerInput::default() };
        ctx.world.resources.insert(game);
    }
}
