//! Готовые поведения: вешаются из инспектора (Add Component → Behaviours) и читаются как
//! примеры скриптов — каждое написано короткими формами (`docs/api/gameplay.md`) в своём
//! файле этого каталога.
//!
//! | поведение | что делает |
//! |---|---|
//! | [`Mover`] | едет с постоянной скоростью |
//! | [`Rotator`] | вращается |
//! | [`Follow`] | плавно следует за объектом из поля `target` |
//! | [`DestroyAfter`] | исчезает через N секунд |
//! | [`Spawner`] | создаёт префаб каждые N секунд (в точке или по ширине экрана) |
//! | [`Health`] / [`Damage`] | жизни; урон при касании |
//! | [`Collectible`] | подбирается при касании: сообщение, звук, исчезает |
//! | [`OffScreen`] | ушёл за край экрана: сообщение, исчезает |
//! | [`Clickable`] | щелчок мышью по объекту → хук `on_click` и сообщение |
//! | [`PlayerMove2D`] | вид сверху: ходит по осям `Horizontal` / `Vertical` |
//! | [`Paddle2D`] | влево-вправо клавишами или за пальцем, не дальше края экрана |
//! | [`Platformer2D`] | бег и прыжок с «временем койота» |
//! | [`PlayerMove3D`] | от третьего лица на `CharacterController` |

mod clickable;
mod collectible;
mod destroy_after;
mod follow;
mod health;
mod mover;
mod off_screen;
mod paddle_2d;
#[cfg(feature = "physics2d")]
mod platformer_2d;
mod player_move_2d;
#[cfg(feature = "physics3d")]
mod player_move_3d;
mod spawner;

pub use clickable::Clickable;
pub use collectible::Collectible;
pub use destroy_after::DestroyAfter;
pub use follow::Follow;
pub use health::{Damage, Health};
pub use mover::{Mover, Rotator};
pub use off_screen::OffScreen;
pub use paddle_2d::Paddle2D;
#[cfg(feature = "physics2d")]
pub use platformer_2d::Platformer2D;
pub use player_move_2d::PlayerMove2D;
#[cfg(feature = "physics3d")]
pub use player_move_3d::PlayerMove3D;
pub use spawner::Spawner;

/// Раздел Add Component, в котором лежат готовые поведения.
pub const CATEGORY: &str = "Behaviours";

/// Зарегистрировать готовые поведения (это делает `engine::app()`).
pub fn register(registry: &mut engine_core::reflect::TypeRegistry) {
    registry
        .register_in::<Mover>(CATEGORY)
        .register_in::<Rotator>(CATEGORY)
        .register_in::<Follow>(CATEGORY)
        .register_in::<DestroyAfter>(CATEGORY)
        .register_in::<Spawner>(CATEGORY)
        .register_in::<Health>(CATEGORY)
        .register_in::<Damage>(CATEGORY)
        .register_in::<Collectible>(CATEGORY)
        .register_in::<OffScreen>(CATEGORY)
        .register_in::<Clickable>(CATEGORY)
        .register_in::<PlayerMove2D>(CATEGORY)
        .register_in::<Paddle2D>(CATEGORY);
    #[cfg(feature = "physics2d")]
    registry.register_in::<Platformer2D>(CATEGORY);
    #[cfg(feature = "physics3d")]
    registry.register_in::<PlayerMove3D>(CATEGORY);
}
