//! Заготовки нового скрипта на Rust (окно «New Rust Script»): пустой скрипт-учебник и
//! готовые начала — движение, игрок, платформер, подбираемый предмет, спавнер, кнопка.
//! Каждая заготовка — рабочий код на коротких формах (`docs/api/gameplay.md`), который
//! сразу делает что-то видимое.

use std::path::{Path, PathBuf};

use crate::i18n::{tr_s, trf};
use crate::rust_scripts::{regenerate_mod, snake_case, validate_rust_name};

/// С чего начать новый скрипт.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScriptTemplate {
    /// Учебный скрипт: движение по стрелкам и десяток примеров в комментариях.
    #[default]
    Empty,
    /// Ездит туда-обратно.
    Mover,
    /// Игрок с видом сверху.
    PlayerTopDown,
    /// Герой платформера: бег и прыжок.
    Platformer,
    /// Герой от третьего лица на `CharacterController`.
    Player3D,
    /// Подбираемый предмет: монета, ключ.
    Collectible,
    /// Создаёт префаб каждые N секунд.
    Spawner,
    /// Кнопка интерфейса со счётчиком нажатий.
    UiButton,
}

impl ScriptTemplate {
    pub const ALL: [ScriptTemplate; 8] = [
        ScriptTemplate::Empty,
        ScriptTemplate::Mover,
        ScriptTemplate::PlayerTopDown,
        ScriptTemplate::Platformer,
        ScriptTemplate::Player3D,
        ScriptTemplate::Collectible,
        ScriptTemplate::Spawner,
        ScriptTemplate::UiButton,
    ];

    /// Название в списке (на языке интерфейса).
    pub fn label(self) -> &'static str {
        match self {
            ScriptTemplate::Empty => tr_s("Empty (with examples)"),
            ScriptTemplate::Mover => tr_s("Mover"),
            ScriptTemplate::PlayerTopDown => tr_s("Player 2D (top-down)"),
            ScriptTemplate::Platformer => tr_s("Platformer 2D"),
            ScriptTemplate::Player3D => tr_s("Player 3D"),
            ScriptTemplate::Collectible => tr_s("Collectible"),
            ScriptTemplate::Spawner => tr_s("Spawner"),
            ScriptTemplate::UiButton => tr_s("UI Button"),
        }
    }

    /// Что делает заготовка и что нужно на объекте (на языке интерфейса).
    pub fn description(self) -> &'static str {
        match self {
            ScriptTemplate::Empty => tr_s("Moves with the arrow keys / WASD and shows the ten most common actions in comments."),
            ScriptTemplate::Mover => tr_s("Moves back and forth between its start and an offset."),
            ScriptTemplate::PlayerTopDown => tr_s("Walks in four directions. With a Rigidbody2D (gravity 0) it stops at walls."),
            ScriptTemplate::Platformer => tr_s("Runs and jumps. Needs a Rigidbody2D and a Collider2D, and ground to stand on."),
            ScriptTemplate::Player3D => tr_s("Walks and jumps on a CharacterController, facing where it goes."),
            ScriptTemplate::Collectible => tr_s("Picked up by the Player: tells the Game object, plays a sound, disappears. Needs a trigger collider."),
            ScriptTemplate::Spawner => tr_s("Creates a prefab every few seconds at its position."),
            ScriptTemplate::UiButton => tr_s("Put it on a UI Button: counts clicks and shows them in a Text named Label."),
        }
    }

    /// Строка в файле, на которой открыть редактор кода (первая строка «тела» скрипта).
    pub fn focus_line(self) -> usize {
        let text = self.text("X");
        text.lines()
            .position(|l| l.trim_start().starts_with("fn update") || l.trim_start().starts_with("fn fixed_update"))
            .or_else(|| text.lines().position(|l| l.trim_start().starts_with("fn ")))
            .map_or(1, |i| i + 2)
    }

    /// Текст файла для компонента `name`.
    pub fn text(self, name: &str) -> String {
        let body = match self {
            ScriptTemplate::Empty => EMPTY,
            ScriptTemplate::Mover => MOVER,
            ScriptTemplate::PlayerTopDown => PLAYER_TOP_DOWN,
            ScriptTemplate::Platformer => PLATFORMER,
            ScriptTemplate::Player3D => PLAYER_3D,
            ScriptTemplate::Collectible => COLLECTIBLE,
            ScriptTemplate::Spawner => SPAWNER,
            ScriptTemplate::UiButton => UI_BUTTON,
        };
        format!("{HEADER}{body}").replace("{Name}", name)
    }
}

/// Выбор заготовки в окне «New Rust Script»: список вариантов и пояснение к выбранному.
pub(crate) fn picker(ui: &mut egui::Ui, template: &mut ScriptTemplate) {
    use crate::controls;
    controls::section(ui, tr_s("Start from"));
    for option in ScriptTemplate::ALL {
        let id = egui::Id::new(("new_rust_script_template", option));
        if controls::radio_with_id(ui, id, *template == option, option.label(), true).clicked() {
            *template = option;
        }
    }
    ui.add_space(crate::theme::SP_S);
    controls::muted(ui, template.description());
    ui.add_space(crate::theme::SP_M);
}

/// Создать скрипт `name` из заготовки в каталоге скриптов `dir` и переписать `mod.rs`
/// (так делает «New Rust Script»; тесты и инструменты зовут это напрямую). Возвращает
/// путь нового файла.
pub fn create_script(dir: &Path, name: &str, template: ScriptTemplate) -> Result<PathBuf, String> {
    let name = name.trim();
    validate_rust_name(name)?;
    let path = dir.join(format!("{}.rs", snake_case(name)));
    if path.exists() {
        return Err(trf!("the file `{path}` already exists; pick another name", path = path.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| trf!("cannot create `{path}`: {error}", path = dir.display(), error = e))?;
    std::fs::write(&path, template.text(name))
        .map_err(|e| trf!("cannot write `{path}`: {error}", path = path.display(), error = e))?;
    regenerate_mod(dir).map_err(|e| e.to_string())?;
    Ok(path)
}

const HEADER: &str = "//! {Name} — скрипт на Rust. Повесьте его на объект: Inspector → Add Component → {Name}.
//! После правки — Build (Ctrl+B): редактор пересоберёт игру и перезапустится с новым кодом.
//! Все короткие формы `ctx.…` — Help → Scripting Cheat Sheet и docs/api/gameplay.md.

use engine::prelude::*;

";

const EMPTY: &str = r#"component! {
    /// {Name}: что делает скрипт (эта строка — подсказка в инспекторе).
    pub struct {Name} {
        /// Скорость, единиц в секунду. Поля видны в инспекторе и сохраняются в сцене.
        pub speed: f32 = 3.0,
    }
}

impl Component for {Name} {
    /// Один раз, перед первым кадром.
    fn start(&mut self, ctx: &mut Ctx) {
        ctx.log("hello!"); // строка в окне Console
    }

    /// Каждый кадр. Сейчас: ехать по стрелкам / WASD и медленно крутиться.
    fn update(&mut self, ctx: &mut Ctx) {
        let dir = ctx.axis2("Horizontal", "Vertical"); // ввод: −1..1 по осям
        ctx.move_by(dir * self.speed * ctx.dt()); // ctx.dt() — секунды кадра
        ctx.rotate_by(30.0 * ctx.dt()); // 30° в секунду

        // Ещё десять частых действий — раскомментируйте нужное:
        // if ctx.pressed("Jump") { ctx.play("sounds/jump.wav"); }   // кнопка и звук
        // if ctx.key_pressed(Key::R) { ctx.reload_scene(); }        // клавиша, сцена
        // let game = ctx.find("Game"); ctx.send(game, "Scored");    // другой объект
        // ctx.spawn_at("prefabs/coin.prefab", ctx.pos() + Vec3::Y); // создать префаб
        // ctx.ui_text("Score", format!("Score: {}", 10));           // текст интерфейса
        // ctx.delay(2.0, |ctx| ctx.destroy_self());                 // через 2 секунды
        // ctx.save("best", 10); let best: i32 = ctx.load("best", 0); // сохранения
        // ctx.me_mut::<SpriteRenderer>().flip_x = true;             // свой компонент
        // ctx.set_velocity(Vec2::new(5.0, 0.0));                    // физика (Rigidbody2D)
        // let x = ctx.rand_range(-3.0, 3.0);                        // случайное число
    }

    /// Кто-то вошёл в триггер (Collider2D / Collider с галочкой is_trigger).
    fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if ctx.has_tag(other, "Player") {
            ctx.log("the player touched me");
        }
    }

    // Другие хуки: fixed_update (шаг физики), late_update (камеры), on_collision,
    // on_click (кнопка), on_message (сообщения), on_destroy.
}
"#;

const MOVER: &str = r#"component! {
    /// {Name}: ездит туда-обратно между стартом и стартом + `offset`.
    pub struct {Name} {
        /// Куда ехать от стартовой точки.
        pub offset: Vec3 = Vec3::new(3.0, 0.0, 0.0),
        /// Секунд на путь в одну сторону.
        pub seconds: f32 = 1.5,
        pub start: Vec3 = Vec3::ZERO,
        pub time: f32 = 0.0,
    }
}

impl Component for {Name} {
    fn start(&mut self, ctx: &mut Ctx) {
        self.start = ctx.pos();
    }

    fn update(&mut self, ctx: &mut Ctx) {
        self.time += ctx.dt();
        // 0 → 1 → 0 → … : ping_pong, а smoothstep смягчает развороты.
        let t = smoothstep(ping_pong(self.time / self.seconds.max(0.01), 1.0));
        ctx.set_pos(self.start + self.offset * t);
    }
}
"#;

const PLAYER_TOP_DOWN: &str = r#"component! {
    /// {Name}: игрок с видом сверху — WASD / стрелки / стик.
    pub struct {Name} {
        /// Скорость, единиц в секунду.
        pub speed: f32 = 5.0,
    }
}

impl Component for {Name} {
    fn update(&mut self, ctx: &mut Ctx) {
        let dir = ctx.axis2("Horizontal", "Vertical");
        if ctx.has::<Rigidbody2D>() {
            ctx.set_velocity(dir * self.speed); // тело упирается в стены
        } else {
            ctx.move_by(dir * self.speed * ctx.dt());
        }
        if dir.x != 0.0 && ctx.has::<SpriteRenderer>() {
            ctx.me_mut::<SpriteRenderer>().flip_x = dir.x < 0.0; // смотреть по ходу
        }
    }
}
"#;

const PLATFORMER: &str = r#"component! {
    /// {Name}: бег по `Horizontal`, прыжок по `Jump`. Нужны Rigidbody2D (lock_rotation)
    /// и Collider2D. Готовое поведение с «временем койота» — Platformer2D.
    pub struct {Name} {
        pub speed: f32 = 6.0,
        pub jump_speed: f32 = 11.0,
    }
}

impl Component for {Name} {
    // Шаг физики: скорость меняется ровно перед тем, как физика её применит.
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        let mut velocity = ctx.velocity2();
        velocity.x = ctx.axis("Horizontal") * self.speed;
        if ctx.grounded() && ctx.pressed("Jump") {
            velocity.y = self.jump_speed;
        }
        ctx.set_velocity(velocity);
    }
}
"#;

const PLAYER_3D: &str = r#"component! {
    /// {Name}: ходит по WASD и прыгает по `Jump` на CharacterController (добавьте его
    /// на этот объект). От первого лица — готовый FpsController.
    pub struct {Name} {
        pub speed: f32 = 5.0,
        pub jump_speed: f32 = 5.0,
    }
}

impl Component for {Name} {
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        let input = ctx.axis2("Horizontal", "Vertical");
        let walk = Vec3::new(input.x, 0.0, -input.y) * self.speed; // «вперёд» — это −Z
        let mut velocity = Vec3::new(walk.x, ctx.velocity().y, walk.z);
        if ctx.grounded() && ctx.pressed("Jump") {
            velocity.y = self.jump_speed;
        }
        ctx.set_velocity(velocity);
        if walk.length_squared() > 0.01 {
            let target = ctx.pos() + walk;
            ctx.look_at(target); // лицом по ходу
        }
    }
}
"#;

const COLLECTIBLE: &str = r#"component! {
    /// {Name}: игрок (тег Player) коснулся — сообщить объекту Game, звук, исчезнуть.
    /// Нужен Collider2D / Collider с галочкой is_trigger.
    pub struct {Name} {
        /// Сколько очков даёт.
        pub points: i32 = 1,
        /// Звук (имя клипа или путь в ассетах; пусто — без звука).
        pub sound: String = String::new(),
    }
}

impl Component for {Name} {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.rotate_by(90.0 * ctx.dt()); // покачиваться, чтобы было заметно
    }

    fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if !ctx.has_tag(other, "Player") {
            return;
        }
        if let Some(game) = ctx.find_by_name("Game") {
            ctx.send_value(game, "Collected", self.points); // ловится в on_message
        }
        if !self.sound.is_empty() {
            ctx.play(&self.sound);
        }
        ctx.destroy_self();
    }
}
"#;

const SPAWNER: &str = r#"component! {
    /// {Name}: создаёт префаб `prefab` каждые `every` секунд (± `spread` по X).
    pub struct {Name} {
        /// Путь к префабу в ассетах.
        pub prefab: String = String::from("prefabs/coin.prefab"),
        pub every: f32 = 2.0,
        pub spread: f32 = 0.0,
        /// Часы до следующего появления.
        pub timer: Timer = Timer::repeating(2.0),
    }
}

impl Component for {Name} {
    fn start(&mut self, ctx: &mut Ctx) {
        self.timer = Timer::repeating(self.every.max(0.05));
        ctx.log(format!("spawning `{}` every {} s", self.prefab, self.every));
    }

    fn update(&mut self, ctx: &mut Ctx) {
        if self.timer.tick(ctx.dt()).just_finished() {
            let x = ctx.rand_range(-self.spread, self.spread);
            ctx.spawn_at(&self.prefab, ctx.pos() + Vec3::new(x, 0.0, 0.0));
        }
    }
}
"#;

const UI_BUTTON: &str = r#"component! {
    /// {Name}: повесьте на кнопку интерфейса (UI Button). Считает нажатия и пишет их в
    /// текст с именем `label` (объект с компонентом Text).
    pub struct {Name} {
        pub label: String = String::from("Label"),
        pub clicks: i32 = 0,
    }
}

impl Component for {Name} {
    fn start(&mut self, ctx: &mut Ctx) {
        ctx.ui_text(&self.label, "Click the button!");
    }

    /// Кнопку нажали (мышь, палец или Enter).
    fn on_click(&mut self, ctx: &mut Ctx) {
        self.clicks += 1;
        ctx.ui_text(&self.label, format!("Clicked {} times", self.clicks));
        ctx.play("click");
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_names_its_component_and_uses_short_hooks() {
        for template in ScriptTemplate::ALL {
            let text = template.text("Coin");
            assert!(text.contains("pub struct Coin {"), "{template:?}");
            assert!(text.contains("impl Component for Coin {"), "{template:?}");
            assert!(!text.contains("{Name}"), "{template:?}");
            assert!(!text.contains("fn on_update(") && !text.contains("fn on_start("), "{template:?} uses the short hooks");
            let declared = crate::rust_scripts::declared_components(&text).unwrap();
            assert_eq!(declared.len(), 1, "{template:?}");
            assert_eq!(declared[0].name, "Coin");
            assert!(template.focus_line() > 5, "{template:?}");
        }
    }

    #[test]
    fn the_empty_template_teaches_in_about_forty_lines() {
        let text = ScriptTemplate::Empty.text("Hero");
        let lines = text.lines().count();
        assert!((30..=60).contains(&lines), "{lines} lines");
        for example in ["ctx.pressed(", "ctx.play(", "ctx.find(", "ctx.spawn_at(", "ctx.ui_text(", "ctx.delay(", "ctx.save(", "ctx.me_mut::", "ctx.set_velocity(", "ctx.rand_range("] {
            assert!(text.contains(example), "{example}");
        }
    }

    #[test]
    fn create_script_writes_the_file_and_mod_rs() {
        let dir = std::env::temp_dir().join(format!("piska-script-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = create_script(&dir, "CoinSpawner", ScriptTemplate::Spawner).unwrap();
        assert!(path.ends_with("coin_spawner.rs"));
        let module = std::fs::read_to_string(dir.join("mod.rs")).unwrap();
        assert!(module.contains("pub mod coin_spawner;"), "{module}");
        assert!(module.contains("app.register::<coin_spawner::CoinSpawner>();"), "{module}");
        assert!(create_script(&dir, "CoinSpawner", ScriptTemplate::Empty).is_err(), "no overwrite");
        assert!(create_script(&dir, "bad name", ScriptTemplate::Empty).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
