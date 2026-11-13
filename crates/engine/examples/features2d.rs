//! Возможности 2D: анимированный персонаж на карте тайлов, эффекты частиц, след и камера.
//! Запуск: `cargo run --example features2d` — A/D или стрелки, Пробел — прыжок, E — взрыв.
//!
//! Без окна: `ENGINE_SCREENSHOT=out.png ENGINE_FRAMES=150 cargo run --example features2d`
//! (в этом режиме персонаж ходит сам).

use engine::prelude::*;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Левый нижний угол карты в мире; карта 24×10 тайлов.
const ORIGIN: Vec2 = Vec2::new(-12.0, -5.0);
const MAP_ROWS: usize = 10;

component! {
    /// Игрок: бег, прыжок, выбор клипа анимации, пыль при приземлении.
    pub struct Hero {
        pub speed: f32 = 5.0,
        pub jump_speed: f32 = 10.5,
        /// Ходит сам (для скриншота без окна).
        pub autopilot: bool = false,
        pub clock: f32 = 0.0,
        pub direction: f32 = 1.0,
        pub was_grounded: bool = true,
        pub previous_vy: f32 = 0.0,
        pub coins: i32 = 0,
        pub exploded: bool = false,
    }
}

impl Hero {
    fn grounded(ctx: &mut Ctx) -> bool {
        if ctx.get::<Rigidbody2D>().is_some_and(|b| b.velocity.y > 0.5) {
            return false;
        }
        let position = ctx.world_position().truncate();
        let Some(physics) = ctx.resource::<Physics2D>() else {
            return false;
        };
        [-0.3, 0.3].into_iter().any(|dx| {
            physics
                .raycast(position + Vec2::new(dx, 0.0), Vec2::NEG_Y, 0.9, Some(ctx.id))
                .is_some()
        })
    }
}

impl Component for Hero {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.clock += dt;
        let (mut run, mut jump) = (ctx.input().axis("Horizontal"), ctx.input().button_down("Jump"));
        let position = ctx.world_position();
        if self.autopilot {
            if position.x > 7.5 {
                self.direction = -1.0;
            } else if position.x < -9.5 {
                self.direction = 1.0;
            }
            run = self.direction;
            jump = self.clock % 1.9 < dt;
        }
        let grounded = Self::grounded(ctx);
        let Some(body) = ctx.get_mut::<Rigidbody2D>() else {
            return;
        };
        body.velocity.x = run * self.speed;
        if jump && grounded {
            body.velocity.y = self.jump_speed;
        }
        let vy = body.velocity.y;

        // Приземление: облачко пыли у ног и лёгкий толчок камеры.
        if grounded && !self.was_grounded && self.previous_vy < -4.0 {
            ctx.world
                .scene
                .build("Dust")
                .at(position.x, position.y - 0.8)
                .with(ParticleSystem::dust().with_layer(2, -1))
                .id();
            if let Some(camera) = ctx.find_by_tag("MainCamera") {
                ctx.send_message(camera, "shake", Value::Float(0.12));
            }
        }
        self.was_grounded = grounded;
        self.previous_vy = vy;

        // Взрыв по кнопке E (в автопилоте — один раз, чтобы он попал на скриншот).
        let boom = ctx.input().key_down(Key::E) || (self.autopilot && !self.exploded && self.clock > 2.0);
        if boom {
            self.exploded = true;
            ctx.world
                .scene
                .build("Explosion")
                .at(position.x + 1.6 * self.direction, position.y - 0.4)
                .with(ParticleSystem::explosion().with_layer(3, 0))
                .id();
            if let Some(camera) = ctx.find_by_tag("MainCamera") {
                ctx.send_message(camera, "shake", Value::Float(0.5));
            }
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let velocity = ctx.get::<Rigidbody2D>().map_or(Vec2::ZERO, |b| b.velocity);
        let grounded = self.was_grounded;
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            if velocity.x.abs() > 0.3 {
                sprite.flip_x = velocity.x < 0.0;
            }
        }
        if let Some(animator) = ctx.get_mut::<SpriteAnimator>() {
            let clip = if !grounded {
                if velocity.y > 1.0 {
                    "jump"
                } else if velocity.y > -1.0 {
                    "peak"
                } else {
                    "fall"
                }
            } else if velocity.x.abs() > 0.3 {
                "run"
            } else {
                "idle"
            };
            animator.play(clip);
        }
    }

    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if message.name == "coin" {
            self.coins += 1;
            log::info!("coins: {}", self.coins);
        }
    }
}

component! {
    /// Монета: крутится (аниматор), по касанию игрока рассыпается блёстками.
    pub struct Coin {}
}

impl Component for Coin {
    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        let is_player = ctx.scene().get(other).is_some_and(|o| o.tag == "Player");
        if !is_player {
            return;
        }
        let at = ctx.world_position();
        ctx.world
            .scene
            .build("Sparkle")
            .at(at.x, at.y)
            .with(ParticleSystem::pickup_sparkle().with_layer(3, 1))
            .id();
        ctx.send_message(other, "coin", Value::Nil);
        ctx.destroy_self();
    }
}

component! {
    /// Огонёк, летающий по восьмёрке; за ним тянется светящийся след.
    pub struct Wisp {
        pub center: Vec2 = Vec2::ZERO,
        pub time: f32 = 0.0,
    }
}

impl Component for Wisp {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt;
        let t = self.time;
        let p = self.center + Vec2::new((t * 0.9).sin() * 5.0, (t * 1.8).sin() * 1.1);
        ctx.transform().position = p.extend(0.0);
    }
}

/// Мировая позиция центра тайла `(x, row)` карты (строка 0 — верхняя).
fn tile_center(x: usize, row: usize) -> Vec2 {
    ORIGIN + Vec2::new(x as f32 + 0.5, (MAP_ROWS - 1 - row) as f32 + 0.5)
}

fn build(app: &mut App) {
    let assets = app.world.resources.get::<AssetDatabase>().expect("assets");
    let tileset: AssetHandle<Texture> = assets.handle("sprites/tileset.png").expect("tileset");
    let hero_sheet: AssetHandle<Texture> = assets.handle("sprites/hero_sheet.png").expect("hero sheet");
    let coin_sheet: AssetHandle<Texture> = assets.handle("sprites/coin_sheet.png").expect("coin sheet");

    app.spawn("Environment").with(Environment {
        bloom: 0.5,
        vignette: 0.22,
        exposure: 1.25,
        ..Environment::default()
    });

    // Камера следит за героем, сглаженно, и не выезжает за карту; тряска — сообщением "shake".
    app.spawn("Camera")
        .tag("MainCamera")
        .with(Camera {
            clear_color: Color::hex(0x3a5a96),
            ..Camera::orthographic(5.0)
        })
        .with(
            CameraController2D::following("Player")
                .with_offset(0.0, 1.5)
                .with_bounds(Vec2::new(-3.1, 0.0), Vec2::new(3.1, 0.0)),
        );

    // Далёкие холмы: цветные прямоугольники на дальнем слое.
    for (x, w, h, shade) in [(-8.0, 9.0, 3.2, 0x3a4f86), (1.0, 11.0, 4.4, 0x33477c), (9.5, 8.0, 2.6, 0x3a4f86)] {
        app.spawn("Hill")
            .at(x, -3.0 + h * 0.5)
            .with(SpriteRenderer::colored(Color::hex(shade)).with_size(w, h).with_order(-20, 0));
    }

    // Фон: кусты, цветы и два факела (слой -5, без коллайдеров).
    let mut background = Tilemap::default()
        .with_tileset(tileset, 4, 2)
        .with_layer(-5, 0);
    background.set_tile(0, MAP_ROWS - 1, 0); // карта той же высоты, что и земля
    for (x, tile) in [(2, 5), (3, 5), (6, 6), (9, 5), (14, 6), (16, 5), (17, 5), (20, 6)] {
        background.set_tile(x, 7, tile);
    }
    for torch in [4, 12, 19] {
        background.set_tile(torch, 7, 8);
    }
    app.spawn("Background").at(ORIGIN.x, ORIGIN.y).with(background);

    // Земля: карта записана текстом. 1 — трава, 2 — земля, 3 — камень, 4 — деревянная
    // платформа (одна сторона), 7 — ящик. Всё непустое твёрдое, а 4 — «облачко».
    let ground = Tilemap::from_rows(&[
        "24*.",
        "24*.",
        "24*.",
        "15*. 4 4 4 4 5*.",
        "24*.",
        "5*. 3*4 16*.",
        "21*. 3*3",
        "10*. 7 10*. 3*3",
        "24*1",
        "24*2",
    ])
    .with_tileset(tileset, 4, 2)
    .with_solid_nonempty()
    .with_one_way_tiles(&[4]);
    app.spawn("Ground").at(ORIGIN.x, ORIGIN.y).with(ground);

    // Передний план: трава перед героем.
    let mut foreground = Tilemap::default()
        .with_tileset(tileset, 4, 2)
        .with_layer(4, 0);
    foreground.set_tile(0, MAP_ROWS - 1, 0);
    for x in [1, 7, 11, 15, 18] {
        foreground.set_tile(x, 7, 6);
    }
    app.spawn("Foreground").at(ORIGIN.x, ORIGIN.y).with(foreground);

    // Игрок: лист 6×3 кадра по 16 пикселей, клипы строками.
    let animator = SpriteAnimator::new()
        .with_clip("idle: 0-3 @5")
        .with_clip("run: 6-11 @13")
        .with_clip("jump: 12 @1")
        .with_clip("peak: 13 @1")
        .with_clip("fall: 14 @1")
        .starting_with("idle");
    app.spawn("Hero")
        .tag("Player")
        .at(-9.0, -2.0)
        .with(SpriteRenderer::sheet(hero_sheet, 6, 3).with_size(1.6, 1.6).with_order(2, 0))
        .with(animator)
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D {
            size: Vec2::new(0.7, 1.4),
            offset: Vec2::new(0.0, -0.1),
            friction: 0.0,
            ..Collider2D::default()
        })
        .with(Hero {
            autopilot: std::env::var("ENGINE_SCREENSHOT").is_ok(),
            ..Hero::default()
        });

    // Монеты с анимацией вращения, у каждой лёгкое мерцание блёсток.
    for (x, y) in [(-5.6, -0.3), (-3.2, -1.9), (-2.2, -1.9), (4.5, 2.7), (5.5, 2.7), (10.5, -0.2)] {
        app.spawn("Coin")
            .at(x, y)
            .with(SpriteRenderer::sheet(coin_sheet, 4, 1).with_size(0.7, 0.7).with_order(1, 0))
            .with(SpriteAnimator::new().with_clip("spin: 0-3 @7"))
            .with(Collider2D::circle(0.4).trigger())
            .with(Coin {})
            .id();
    }

    // Костры на факелах: огонь + дым + свечение; частицы — добавочные (светятся через блум).
    for torch in [4, 12, 19] {
        let top = tile_center(torch, 7) + Vec2::new(0.0, 0.45);
        app.spawn("Torch fire")
            .at(top.x, top.y)
            .with(ParticleSystem::fire().scaled(0.7).with_layer(1, 0));
        app.spawn("Torch smoke")
            .at(top.x, top.y + 0.3)
            .with(ParticleSystem { emission_rate: 4.0, ..ParticleSystem::smoke().scaled(0.6) }.with_layer(1, -1));
    }

    // Искры из кузницы на каменном блоке справа.
    app.spawn("Forge sparks")
        .at(10.4, -0.8)
        .with(ParticleSystem { emission_rate: 28.0, ..ParticleSystem::sparks() }.with_layer(1, 0));

    // Огонёк со следом и блёстками.
    app.spawn("Wisp")
        .at(0.0, 2.0)
        .with(Wisp { center: Vec2::new(-3.0, 2.2), time: 0.0 })
        .with(TrailRenderer::glowing(Color::rgb(0.9, 2.2, 3.0), 0.35, 0.9).with_layer(3, 0))
        .with(ParticleSystem {
            emission_rate: 18.0,
            lifetime: Vec2::new(0.4, 0.8),
            start_speed: Vec2::new(0.1, 0.5),
            start_size: Vec2::new(0.1, 0.2),
            start_color: Color::rgb(0.8, 2.0, 3.0),
            start_color_max: Color::rgb(1.5, 3.0, 3.0),
            shape: EmitterShape::Circle,
            shape_size: Vec3::splat(0.15),
            blend: ParticleBlend::Additive,
            flat: true,
            ..ParticleSystem::default()
        }.with_layer(3, 1));
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    engine::mount_assets(&mut app, ASSETS)?;
    app.register::<Hero>().register::<Coin>().register::<Wisp>();
    build(&mut app);
    engine::run(app, WindowConfig::titled("2D features: sprite sheets, particles, tilemaps"))
}
