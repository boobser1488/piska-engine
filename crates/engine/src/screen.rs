//! Экран 2D-игры для скриптов: что видит главная камера и где указатель в мире.
//!
//! ```ignore
//! let area = ctx.view_rect();                          // видимая часть мира (Rect)
//! if ctx.pos().y < area.y { ctx.destroy_self(); }      // упал за нижний край
//! if ctx.pointer_held() {                              // зажата мышь или палец на экране
//!     let target = ctx.pointer_world();                // точка мира под ним
//! }
//! ```
//!
//! Главная камера — та, что рисует кадр: активная [`Camera`] с наибольшим `priority`.
//! Ортографическая — точно; перспективная — прямоугольник в плоскости `z = 0` (для 2D-игр
//! с перспективной камерой, смотрящей вдоль −Z). Камеры нет — камера по умолчанию в
//! начале координат (`ortho_size` 5). Размер экрана ещё неизвестен — считается 16 : 9.

use engine_core::prelude::*;
use engine_render::{Camera, Projection};

/// Короткие формы экрана; `use engine::prelude::*;` уже подключает этот трейт.
pub trait ScreenCtx {
    /// Прямоугольник мира, который видит главная камера (ось Y вверх):
    /// `ctx.view_rect().expand(-0.5).clamp_point(p)` — точка не ближе 0.5 к краю.
    fn view_rect(&self) -> Rect;
    /// Точка мира под пикселем экрана (Unity: `Camera.main.ScreenToWorldPoint`).
    fn screen_to_world(&self, pixels: Vec2) -> Vec2;
    /// Точка мира под указателем: первым пальцем на экране или мышью
    /// (Godot: `get_global_mouse_position`).
    fn pointer_world(&self) -> Vec2;
    /// Указатель «нажат» для игры: палец на экране или зажата левая кнопка мыши не над
    /// интерфейсом (клик по кнопке UI игру не двигает).
    fn pointer_held(&self) -> bool;
}

/// Главная камера: (мировая позиция, камера).
fn main_camera(ctx: &Ctx) -> Option<(Vec3, Camera)> {
    let (id, camera) = ctx.scene().iter_active_with::<Camera>().max_by_key(|(_, c)| c.priority)?;
    Some((ctx.scene().world_position(id), camera.clone()))
}

/// Размер экрана в пикселях и отношение сторон (16 : 9, пока размер неизвестен).
fn screen(ctx: &Ctx) -> (Vec2, f32) {
    let size = ctx.input().screen_size();
    let aspect = if size.x > 0.0 && size.y > 0.0 { size.x / size.y } else { 16.0 / 9.0 };
    (size, aspect)
}

impl ScreenCtx for Ctx<'_> {
    fn view_rect(&self) -> Rect {
        let (_, aspect) = screen(self);
        let (center, half_height) = match main_camera(self) {
            Some((at, camera)) => {
                let half = match camera.projection {
                    Projection::Orthographic => camera.ortho_size,
                    Projection::Perspective => at.z.abs() * (camera.fov_deg.to_radians() * 0.5).tan(),
                };
                (at.truncate(), half)
            }
            None => (Vec2::ZERO, Camera::default().ortho_size),
        };
        Rect::from_center_size(center, Vec2::new(half_height * aspect, half_height) * 2.0)
    }

    fn screen_to_world(&self, pixels: Vec2) -> Vec2 {
        let (size, _) = screen(self);
        let area = self.view_rect();
        if size.x <= 0.0 || size.y <= 0.0 {
            return area.center();
        }
        // Пиксели: (0, 0) — левый верхний угол, Y вниз; мир: Y вверх.
        area.min() + Vec2::new(pixels.x / size.x, 1.0 - pixels.y / size.y) * area.size()
    }

    fn pointer_world(&self) -> Vec2 {
        let pixels = match self.input().touches().first() {
            Some(touch) => touch.position,
            None => self.mouse(),
        };
        self.screen_to_world(pixels)
    }

    fn pointer_held(&self) -> bool {
        if !self.input().touches().is_empty() {
            return true;
        }
        if !self.mouse_held(MouseButton::Left) {
            return false;
        }
        #[cfg(feature = "ui")]
        {
            use engine_ui::prelude::UiContext;
            if self.ui().pointer_over_ui() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_camera(camera: Camera, x: f32, y: f32) -> App {
        let mut app = App::new();
        app.spawn("Main Camera").with(camera).at(x, y);
        app.spawn("Probe");
        app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
        app
    }

    fn probe<R>(app: &mut App, f: impl FnOnce(&mut Ctx) -> R) -> R {
        let id = app.world.scene.find_by_name("Probe").unwrap();
        f(&mut Ctx { world: &mut app.world, id })
    }

    #[test]
    fn the_view_rect_is_what_an_orthographic_camera_sees() {
        let mut app = app_with_camera(Camera::orthographic(6.0), 2.0, 1.0);
        let area = probe(&mut app, |ctx| ctx.view_rect());
        assert!((area.height - 12.0).abs() < 1e-4);
        assert!((area.width - 12.0 * 1280.0 / 720.0).abs() < 1e-3);
        assert!((area.center() - Vec2::new(2.0, 1.0)).length() < 1e-4);
        // Центр экрана — центр камеры; левый верхний угол — min.x, max.y.
        let center = probe(&mut app, |ctx| ctx.screen_to_world(Vec2::new(640.0, 360.0)));
        assert!((center - Vec2::new(2.0, 1.0)).length() < 1e-4);
        let corner = probe(&mut app, |ctx| ctx.screen_to_world(Vec2::ZERO));
        assert!((corner - Vec2::new(area.x, area.max().y)).length() < 1e-4);
    }

    #[test]
    fn the_pointer_is_the_first_finger_or_the_mouse() {
        let mut app = app_with_camera(Camera::orthographic(5.0), 0.0, 0.0);
        assert!(!probe(&mut app, |ctx| ctx.pointer_held()));
        app.world.input.set_mouse_position(Vec2::new(1280.0, 360.0));
        app.world.input.set_mouse_button(MouseButton::Left, true);
        let (held, at) = probe(&mut app, |ctx| (ctx.pointer_held(), ctx.pointer_world()));
        assert!(held);
        assert!((at - Vec2::new(5.0 * 1280.0 / 720.0, 0.0)).length() < 1e-3, "{at}");
    }

    #[test]
    fn without_a_camera_the_default_camera_is_assumed() {
        let mut app = App::new();
        app.spawn("Probe");
        app.world.input.set_screen_size(Vec2::new(800.0, 400.0));
        let area = probe(&mut app, |ctx| ctx.view_rect());
        assert!((area.height - 10.0).abs() < 1e-4 && (area.width - 20.0).abs() < 1e-3, "{area:?}");
        assert!(area.center().length() < 1e-4);
    }
}
