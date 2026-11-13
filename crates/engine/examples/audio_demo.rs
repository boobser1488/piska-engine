//! Звук без единого файла: эффекты и музыка из синтезатора.
//!
//! Запуск: `cargo run --example audio_demo`
//!
//! * `1`…`8` — эффекты (прыжок, монета, удар, взрыв, лазер, бонус, клик, ошибка);
//! * `M` — включить / выключить музыку, `N` — сменить трек (плавный переход);
//! * `P` — пространственный источник, который кружит вокруг слушателя;
//! * `-` / `+` — громкость музыки, `Space` — пауза всего звука.
//!
//! `ENGINE_FRAMES=N` закрывает окно после N кадров, `ENGINE_AUDIO=off` выключает
//! звуковую карту (звук «играет» вхолостую).

use engine::prelude::*;

const EFFECTS: [(&str, Key, u32); 8] = [
    ("jump", Key::Digit1, 0x4cc9f0),
    ("coin", Key::Digit2, 0xffd166),
    ("hit", Key::Digit3, 0xef476f),
    ("explosion", Key::Digit4, 0xf77f00),
    ("laser", Key::Digit5, 0x9b5de5),
    ("powerup", Key::Digit6, 0x06d6a0),
    ("click", Key::Digit7, 0xcbd5e1),
    ("error", Key::Digit8, 0x8d99ae),
];

component! {
    /// Квадрат-кнопка: по своей клавише играет звук и «подпрыгивает».
    pub struct Pad {
        pub slot: i32 = 0,
        pub flash: f32 = 0.0,
    }
}

impl Component for Pad {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let (clip, key, _) = EFFECTS[self.slot as usize % EFFECTS.len()];
        if ctx.input().key_down(key) {
            self.flash = 1.0;
            // Лёгкий разброс высоты — повторы не звучат «машинно».
            let pitch = 0.95 + 0.1 * ((ctx.time().frame * 7 % 10) as f32 / 10.0);
            if let Some(audio) = ctx.resource_mut::<Audio>() {
                audio.play_with(clip, PlayParams::new().pitch(pitch).volume(0.8));
            }
        }
        self.flash = (self.flash - dt * 4.0).max(0.0);
        let s = 1.0 + 0.35 * self.flash;
        ctx.transform().scale = Vec3::new(s, s, 1.0);
    }
}

component! {
    /// Кружит объект вокруг начала координат.
    pub struct Orbiter {
        pub radius: f32 = 6.0,
        pub speed: f32 = 1.2,
        pub angle: f32 = 0.0,
    }
}

impl Component for Orbiter {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.angle += self.speed * dt;
        ctx.transform().position = Vec3::new(self.angle.cos() * self.radius, self.angle.sin() * self.radius * 0.5 - 3.0, 0.0);
    }
}

component! {
    /// Музыка и общие клавиши.
    pub struct Controller {
        pub music_on: bool = true,
        pub track: i32 = 0,
        pub orbiting: bool = false,
    }
}

impl Controller {
    fn track_name(&self) -> &'static str {
        if self.track % 2 == 0 { "adventure" } else { "calm" }
    }
}

impl Component for Controller {
    fn on_start(&mut self, ctx: &mut Ctx) {
        log::info!("audio_demo: 1-8 effects, M music, N next track, P orbiting source, +/- music volume, Space pause");
        let name = self.track_name();
        if let Some(audio) = ctx.resource_mut::<Audio>() {
            audio.play_music(name, 0.5);
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let emitter = ctx.scene().find_by_name("Emitter");
        let pressed = |key: Key| ctx.input().key_down(key);
        let (music, next, orbit, up, down, pause) = (
            pressed(Key::M),
            pressed(Key::N),
            pressed(Key::P),
            pressed(Key::Up),
            pressed(Key::Down),
            pressed(Key::Space),
        );
        let Some(audio) = ctx.world.resources.get_mut::<Audio>() else {
            return;
        };
        if music {
            self.music_on = !self.music_on;
            if self.music_on {
                audio.play_music(self.track_name(), 1.0);
            } else {
                audio.stop_music(1.0);
            }
        }
        if next {
            self.track += 1;
            if self.music_on {
                audio.play_music(self.track_name(), 1.5);
            }
        }
        if orbit {
            self.orbiting = !self.orbiting;
            if let Some(emitter) = emitter {
                if self.orbiting {
                    audio.play_source(emitter);
                } else {
                    audio.stop_source(emitter);
                }
            }
        }
        let step = if up { 0.1 } else if down { -0.1 } else { 0.0 };
        if step != 0.0 {
            let volume = (audio.bus_volume(Bus::Music) + step).clamp(0.0, 1.0);
            audio.set_bus_volume(Bus::Music, volume);
        }
        if pause {
            if audio.is_paused() {
                audio.resume_all();
            } else {
                audio.pause_all();
            }
        }
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();

    // Весь звук игры — из кода: ни одного файла.
    {
        let audio = app.world.resources.get_mut::<Audio>().expect("installed by engine::app");
        audio.add_clip("jump", synth::sfx::jump());
        audio.add_clip("coin", synth::sfx::coin());
        audio.add_clip("hit", synth::sfx::hit());
        audio.add_clip("explosion", synth::sfx::explosion());
        audio.add_clip("laser", synth::sfx::laser());
        audio.add_clip("powerup", synth::sfx::powerup());
        audio.add_clip("click", synth::sfx::ui_click());
        audio.add_clip("error", synth::sfx::ui_error());
        audio.add_clip("adventure", synth::music::adventure());
        audio.add_clip("calm", synth::music::calm());
        // Собственный звук за пять строк: «сирена» из двух тонов.
        let siren = Tone::new(Wave::Saw, 440.0)
            .sweep_to(660.0)
            .duration(0.5)
            .lowpass(2500.0)
            .volume(0.3)
            .render()
            .then(Tone::new(Wave::Saw, 660.0).sweep_to(440.0).duration(0.5).lowpass(2500.0).volume(0.3).render());
        audio.add_clip("siren", siren);
        audio.set_bus_volume(Bus::Music, 0.6);
    }

    app.spawn("Camera").with(Camera::default()).with(AudioListener::default());
    for (i, (_, _, color)) in EFFECTS.iter().enumerate() {
        app.spawn(format!("Pad {}", i + 1))
            .at(-5.6 + i as f32 * 1.6, 1.5)
            .with(SpriteRenderer::colored(Color::hex(*color)).with_size(1.2, 1.2))
            .with(Pad { slot: i as i32, flash: 0.0 });
    }
    app.spawn("Emitter")
        .at(6.0, -3.0)
        .with(SpriteRenderer::colored(Color::hex(0xffffff)).with_size(0.5, 0.5))
        .with(Orbiter::default())
        .with(AudioSource::named("siren").with_looping(true).with_volume(0.8).with_spatial(1.5, 14.0).manual());
    app.spawn("Controller").with(Controller::default());

    engine::run(app, WindowConfig::titled("Audio demo — 1-8 effects, M music, P orbiting source"))
}
