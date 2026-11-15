use engine_core::prelude::*;
use engine_render::{Camera, Projection};

component! {
    /// Щелчок левой кнопкой мыши (или касание) по объекту в 2D-сцене: все скрипты этого
    /// объекта получают хук `on_click`, а объект `notify` — сообщение `message`. Область
    /// щелчка — `Collider2D`, если он есть, иначе квадрат `size` × масштаб объекта.
    /// Камера — ортографическая (2D). Кнопкам интерфейса это не нужно: у них `on_click` есть сам.
    pub struct Clickable {
        /// Сообщение при щелчке.
        pub message: String = String::from("Clicked"),
        /// Кому ещё сообщить (имя объекта; пусто — только себе).
        pub notify: String = String::new(),
        /// Размер области щелчка без коллайдера (в единицах мира, до масштаба).
        pub size: Vec2 = Vec2::ONE,
    }
}

/// Точка мира под курсором для главной ортографической камеры.
pub(crate) fn pointer_world_2d(ctx: &Ctx) -> Option<Vec2> {
    let pointer = match ctx.input().touches().first() {
        Some(touch) => touch.position,
        None => ctx.mouse(),
    };
    let screen = ctx.input().screen_size();
    if screen.x <= 0.0 || screen.y <= 0.0 {
        return None;
    }
    let (camera, settings) = ctx
        .scene()
        .iter_active_with::<Camera>()
        .filter(|(_, c)| c.projection == Projection::Orthographic)
        .max_by_key(|(_, c)| c.priority)?;
    let center = ctx.scene().world_position(camera).truncate();
    let half = Vec2::new(settings.ortho_size * screen.x / screen.y, settings.ortho_size);
    let uv = Vec2::new(pointer.x / screen.x - 0.5, 0.5 - pointer.y / screen.y) * 2.0;
    Some(center + uv * half)
}

impl Clickable {
    fn contains(&self, ctx: &Ctx, point: Vec2) -> bool {
        let center = ctx.pos2();
        let scale = ctx.scale().truncate().abs();
        #[cfg(feature = "physics2d")]
        if let Some(collider) = ctx.get::<engine_physics::Collider2D>() {
            let center = center + collider.offset * scale;
            return match collider.shape {
                engine_physics::Shape2D::Circle => {
                    point.distance(center) <= collider.radius * scale.max_element()
                }
                _ => {
                    let half = collider.size * scale * 0.5;
                    (point - center).abs().cmple(half).all()
                }
            };
        }
        let half = self.size * scale * 0.5;
        (point - center).abs().cmple(half).all()
    }
}

impl Component for Clickable {
    fn update(&mut self, ctx: &mut Ctx) {
        let pressed = ctx.mouse_pressed(MouseButton::Left)
            || ctx.input().touches().first().is_some_and(|t| t.phase == TouchPhase::Began);
        if !pressed {
            return;
        }
        let Some(point) = pointer_world_2d(ctx) else { return };
        if !self.contains(ctx, point) {
            return;
        }
        let me = ctx.id;
        ctx.world.dispatch_hook(me, Hooks::CLICK, |c, ctx| c.on_click(ctx));
        ctx.send(me, &self.message);
        if let Some(target) = ctx.find_by_name(&self.notify) {
            ctx.send(target, &self.message);
        }
    }
}
