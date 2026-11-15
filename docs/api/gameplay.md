# Геймплей-API

Всё, к чему игровой программист тянется каждый день: время, таймеры, твины, смена сцен,
запросы к сцене, сохранения, случайные числа, математика и ввод (клавиатура, мышь, геймпад).

Всё лежит в `engine_core` и доступно одной строкой:

```rust
use engine::prelude::*;
```

Код ниже написан в стиле Unity: компонент объявляется `component!`, поведение — хуками
`Component`. В компонентах корутин нет — вместо них **таймеры** (§3) и **твины** (§4). Длинные
последовательности с ожиданиями (катсцены, обучение, волны) удобнее писать сценарием на языке
Scenario (`scenario.md`): там `wait(1)` можно поставить посреди цикла.

* [1. Порядок кадра](#1-порядок-кадра)
* [2. Время](#2-время)
* [3. Таймеры и отложенные вызовы](#3-таймеры-и-отложенные-вызовы)
* [4. Твины](#4-твины)
* [5. Сцены](#5-сцены)
* [6. Запросы, активность, префабы](#6-запросы-активность-префабы)
* [7. Сохранения](#7-сохранения)
* [8. Случайные числа](#8-случайные-числа)
* [9. Математика](#9-математика)
* [10. Ввод](#10-ввод)
* [11. Конечный автомат](#11-конечный-автомат)
* [12. Рецепты](#12-рецепты)
* [13. Для редактора и встраивания](#13-для-редактора-и-встраивания)
* [14. Чего здесь нет](#14-чего-здесь-нет)

<!-- beginner:begin -->
**Новичку:** начните с [первого скрипта за 10 минут](#первый-скрипт-за-10-минут), потом
загляните в [«Как сделать…»](#как-сделать) — там 30+ готовых рецептов. Всё короткое из этих
двух частей собрано в [шпаргалке](#шпаргалка-короткие-формы) (она же в редакторе: Help →
Scripting Cheat Sheet). Справочник (§1–§14) — дальше, когда понадобятся подробности.

* [Первый скрипт за 10 минут](#первый-скрипт-за-10-минут)
* [Как сделать…](#как-сделать)
* [Шпаргалка: короткие формы](#шпаргалка-короткие-формы)
* [Старая форма ↔ короткая](#старая-форма--короткая)
* [Готовые поведения](#готовые-поведения)
* [Целая игра: звёзды в корзину](#целая-игра-звёзды-в-корзину)

## Первый скрипт за 10 минут

### Шаг 1. Скрипт

В редакторе: выделите объект → Inspector → **Add Component → New Script…** (или Assets →
New Rust Script…), выберите заготовку **Empty (with examples)**, имя `Hero`. Редактор
создаст `src/scripts/hero.rs`, допишет его в `src/scripts/mod.rs` (регистрировать руками
ничего не надо), соберёт игру и повесит `Hero` на объект. Нажмите Play — объект едет по
стрелкам / WASD.

Скрипт — это компонент: данные объявляет `component!` (поля видны в инспекторе и
сохраняются в сцене), поведение — хуки `impl Component`:

```rust
use engine::prelude::*;

component! {
    /// Герой. Эта строка — подсказка в инспекторе.
    pub struct Hero {
        pub speed: f32 = 5.0,
    }
}

impl Component for Hero {
    fn start(&mut self, ctx: &mut Ctx) {
        ctx.log("hello!"); // окно Console
    }

    fn update(&mut self, ctx: &mut Ctx) {
        // здесь всё, что происходит каждый кадр
    }
}
```

Хуки: `start` (один раз), `update` (каждый кадр), `fixed_update` (шаг физики),
`late_update` (после всех `update` — камеры), `on_collision(other)`, `on_trigger(other)`,
`on_click` (кнопка), `on_message(message)`, `on_destroy`. Переопределяйте только нужные.
`ctx` — «я и весь мир»: через него скрипт двигает объект, читает ввод, находит других.

### Шаг 2. Движение

`ctx.dt()` — сколько секунд длился кадр: умножайте на него скорость, и движение будет
одинаковым при любом FPS.

```rust
fn update(&mut self, ctx: &mut Ctx) {
    ctx.move_by(Vec2::new(self.speed * ctx.dt(), 0.0)); // вправо, `speed` единиц в секунду
    ctx.rotate_by(90.0 * ctx.dt());                     // 90° в секунду
}
```

`ctx.pos()` / `ctx.set_pos(..)` — мировая позиция, `ctx.set_angle(..)`, `ctx.set_scale(..)`,
`ctx.look_at(..)`, `ctx.forward()`. Вектор можно передать `Vec2` (2D: глубина `z` не
меняется) или `Vec3`.

### Шаг 3. Ввод

```rust
fn update(&mut self, ctx: &mut Ctx) {
    let dir = ctx.axis2("Horizontal", "Vertical"); // WASD, стрелки, крестовина, стик
    ctx.move_by(dir * self.speed * ctx.dt());
    if ctx.pressed("Jump") {                        // Space или кнопка A геймпада
        ctx.log("jump!");
    }
    if ctx.key_pressed(Key::R) {                    // конкретная клавиша
        ctx.reload_scene();
    }
}
```

Действия (`Horizontal`, `Vertical`, `Jump`, `Fire1`, `Submit`, `Cancel` и ваши) настраиваются
в Project Settings → Input. Опечатка в имени не роняет игру: в Console один раз появится
`Hero: there is no input axis `Horizntal`. Did you mean `Horizontal`? …` с файлом и строкой.

### Шаг 4. Столкновения

Добавьте объекту `Rigidbody2D` и `Collider2D`, монете — `Collider2D` с галочкой
`is_trigger` и тег `Coin`. Физика сама вызовет хук:

```rust
fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
    if ctx.has_tag(other, "Coin") {
        ctx.destroy(other);
        self.score += 1;
    }
}
```

С физическим телом двигайте скорость, а не позицию: `ctx.set_velocity(dir * self.speed)`
(лучше в `fixed_update`). `ctx.grounded()` — стоит ли объект на земле.

### Шаг 5. Создать объект

Префаб — заготовка объекта в `assets/prefabs/` (перетащите объект из Hierarchy в Project).

```rust
if ctx.pressed("Fire1") {
    let bullet = ctx.spawn_at("prefabs/bullet.prefab", ctx.pos());
    ctx.on(bullet).set_velocity(Vec2::new(12.0, 0.0)); // `on(id)` — то же для другого объекта
    ctx.on(bullet).destroy_self_after(2.0);
}
```

### Шаг 6. Интерфейс

Текст в интерфейсе — объект с компонентом `Text`; к нему обращаются по имени объекта:

```rust
ctx.ui_text("Score", format!("Score: {}", self.score));
ctx.ui_show("GameOver"); // и ctx.ui_hide("GameOver")
if ctx.clicked("restart") { ctx.reload_scene(); } // или хук on_click на самой кнопке
```

### Шаг 7. Звук

```rust
ctx.play("sounds/coin.wav"); // файл из ассетов или имя клипа из audio.add_clip(..)
ctx.play("sfx:coin");        // встроенный звук: sfx:jump, sfx:hurt, sfx:lose… (audio.md)
ctx.music("music:calm");     // встроенная музыка или файл: ctx.music("sounds/theme.ogg")
```

### Шаг 8. Следующая сцена

```rust
if self.score >= 10 {
    ctx.save("best", self.score); // переживёт перезапуск игры
    ctx.load_scene("level2");     // scenes/level2.scene
}
```

Прочитать сохранённое: `self.best = ctx.load("best", 0);`. Готово: это вся игра «собери
десять монет». Дальше — рецепты.

### Целая игра: звёзды в корзину

Самая маленькая честная версия «Звездопада» (`games/template`): корзина ездит, звёзды
падают, счёт растёт. Почти всё — готовые поведения, которые вешаются в редакторе; код
нужен только для счёта.

Сцена (`assets/scenes/main.scene`, собирается в редакторе):

* **Main Camera** — `Camera` (`ortho_size` 6);
* **Player** (тег `Player`) — `SpriteRenderer`, `Rigidbody2D` (Dynamic), `Collider2D` и
  `Paddle2D` (клавиши или палец, не дальше края экрана);
* **Ground** — `SpriteRenderer` и `Collider2D` под корзиной;
* **Game** (над верхним краем экрана, y = 6.8) — `Score` (скрипт ниже) и `Spawner`:
  `prefab` = `prefabs/star.prefab`, `interval` 1, `across_view` вкл.;
* **Canvas** с текстом **ScoreText** (`Text`).

Префаб звезды (`assets/prefabs/star.prefab`) — `SpriteRenderer`, `Rigidbody2D`
(`gravity_scale` 0.25), `Collider2D` с `is_trigger` и `Collectible`: `message` = `Caught`,
`notify` = `Game`, `sound` = `sfx:coin`. Хотите, чтобы упавшие звёзды исчезали, — добавьте
`OffScreen`.

Весь код игры, `src/lib.rs`:

```rust
pub mod scripts; // скрипты, созданные в редакторе
use engine::prelude::*;

engine::game! {
    name: "Mini Starfall",
    scene: "scenes/main.scene",
    components: [Score],
}

component! {
    /// Счёт: каждая пойманная звезда — очко.
    pub struct Score {
        pub points: i32 = 0,
    }
}

impl Component for Score {
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        if message.is("Caught") {
            self.points += 1;
            ctx.ui_text("ScoreText", format!("Score: {}", self.points));
        }
    }
}
```

Полная игра — жизни, рекорд, экран «Game over» с рестартом, музыка и интро на Scenario —
это `games/template/src/lib.rs` (около 70 строк) и шаблон **Starfall** в Piska Hub.
Тест этого примера: `crates/engine/tests/mini_starfall.rs`.

### Если что-то не нашлось

Короткие формы не паникуют. `ctx.me_mut::<Rigidbody2D>()` на объекте без тела,
`ctx.find("Gmae")`, `ctx.spawn("prefabs/nope.prefab")` пишут в Console **один раз на место
вызова** понятную ошибку — что искали, на каком объекте, что есть рядом, как починить:

```text
`Hero` has no `Rigidbody2D` component (ctx.me_mut::<Rigidbody2D>()). It has: Transform,
SpriteRenderer, Collider2D, Hero. Add it in the Inspector (Add Component → Rigidbody2D) or in
code (`ctx.add(Rigidbody2D::default())`); until then this call changes nothing (src/scripts/hero.rs:14)
```

а вызов ничего не делает: `me_mut` отдаёт временную «заглушку», `find` и `spawn` —
`GameObjectId::NONE`, с которым любые короткие формы ничего не делают. Ошибки копятся и в
ресурсе `ScriptErrors` (`world.resources.get::<ScriptErrors>()`) — удобно в тестах.

## Как сделать…

Каждый рецепт — целый компонент или кусок хука; `use engine::prelude::*;` в начале файла.

### …движение с постоянной скоростью

```rust
component! { pub struct Cloud { pub speed: f32 = 1.0 } }
impl Component for Cloud {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.move_by(Vec2::new(self.speed * ctx.dt(), 0.0));
    }
}
```

Готовое: `Mover` (Add Component → Behaviours).

### …вращение

```rust
fn update(&mut self, ctx: &mut Ctx) {
    ctx.rotate_by(180.0 * ctx.dt());                         // 2D: вокруг Z
    // ctx.rotate_by(Vec3::new(0.0, 90.0 * ctx.dt(), 0.0)); // 3D: вокруг Y
}
```

Готовое: `Rotator`.

### …игрока с видом сверху

```rust
component! { pub struct Walker { pub speed: f32 = 5.0 } }
impl Component for Walker {
    fn update(&mut self, ctx: &mut Ctx) {
        let dir = ctx.axis2("Horizontal", "Vertical");
        ctx.move_by(dir * self.speed * ctx.dt());
    }
}
```

Готовое: `PlayerMove2D` (с `Rigidbody2D` упирается в стены).

### …бег и прыжок (платформер)

```rust
component! { pub struct Runner { pub speed: f32 = 6.0, pub jump: f32 = 11.0 } }
impl Component for Runner {
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        let mut v = ctx.velocity2();
        v.x = ctx.axis("Horizontal") * self.speed;
        if ctx.grounded() && ctx.pressed("Jump") {
            v.y = self.jump;
        }
        ctx.set_velocity(v);
    }
}
```

Нужны `Rigidbody2D` (`lock_rotation`) и `Collider2D`. Готовое с «временем койота» и буфером
прыжка — `Platformer2D`.

### …героя от третьего лица

```rust
fn fixed_update(&mut self, ctx: &mut Ctx) {
    let input = ctx.axis2("Horizontal", "Vertical");
    let walk = Vec3::new(input.x, 0.0, -input.y) * 5.0;
    let mut v = Vec3::new(walk.x, ctx.velocity().y, walk.z);
    if ctx.grounded() && ctx.pressed("Jump") {
        v.y = 5.0;
    }
    ctx.set_velocity(v); // на объекте CharacterController
}
```

Готовое (относительно камеры, с поворотом по ходу): `PlayerMove3D`; от первого лица —
`FpsController` (`fps.md`).

### …камеру, которая следует за игроком

```rust
component! {
    pub struct CameraFollow { pub target: ObjectRef = ObjectRef::NONE, pub smoothing: f32 = 5.0 }
}
impl Component for CameraFollow {
    fn late_update(&mut self, ctx: &mut Ctx) {
        let target = ctx.obj(self.target); // поле назначается перетаскиванием в инспекторе
        if !ctx.exists(target) { return; }
        let goal = ctx.on(target).pos().truncate().extend(ctx.pos().z);
        let k = 1.0 - (-self.smoothing * ctx.dt()).exp();
        ctx.set_pos(ctx.pos().lerp(goal, k));
    }
}
```

Готовое: `Follow`; для 2D с границами и тряской — `CameraController2D` (`2d.md`).

### …подбор монеты

```rust
fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
    if ctx.has_tag(other, "Player") {
        let game = ctx.find("Game");
        ctx.send_value(game, "Coin", 1);
        ctx.play("sounds/coin.wav");
        ctx.destroy_self();
    }
}
```

Готовое: `Collectible`.

### …счёт на экране

```rust
fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
    if message.is("Coin") {
        self.score += message.get::<i32>().unwrap_or(1);
        ctx.ui_text("Score", format!("Score: {}", self.score));
    }
}
```

### …жизни и урон

```rust
component! { pub struct Hp { pub hp: i32 = 3 } }
impl Component for Hp {
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        if message.is("Damage") {
            self.hp -= message.get::<i32>().unwrap_or(1);
            if self.hp <= 0 { ctx.destroy_self(); }
        }
    }
}
// у шипов:
fn on_collision(&mut self, ctx: &mut Ctx, other: GameObjectId) {
    ctx.send_value(other, "Damage", 1);
}
```

Готовые: `Health` и `Damage`.

### …выстрел

```rust
component! { pub struct Gun { pub cooldown: Cooldown = Cooldown::new(0.25) } }
impl Component for Gun {
    fn update(&mut self, ctx: &mut Ctx) {
        self.cooldown.tick(ctx.dt());
        if ctx.held("Fire1") && self.cooldown.trigger() {
            let bullet = ctx.spawn_at("prefabs/bullet.prefab", ctx.pos());
            ctx.on(bullet).set_velocity(Vec2::new(15.0, 0.0));
            ctx.on(bullet).destroy_self_after(2.0);
        }
    }
}
```

### …появление врагов по таймеру

```rust
fn start(&mut self, ctx: &mut Ctx) {
    ctx.repeat(2.0, |ctx| {
        let x = ctx.rand_range(-6.0, 6.0);
        ctx.spawn_at("prefabs/enemy.prefab", Vec2::new(x, 5.0));
    });
}
```

Готовое: `Spawner` (`across_view` — по всей ширине экрана; `enabled` и `interval` можно
менять из скрипта: `ctx.me_mut::<Spawner>().interval = 0.5`).

### …корзину или ракетку, которая ездит за пальцем

Готовое: `Paddle2D` (`speed`, `follow_pointer`, `view_margin` — отступ от края экрана,
`flip_sprite`). Своими руками:

```rust
fn update(&mut self, ctx: &mut Ctx) {
    let mut run = ctx.axis("Horizontal");
    if ctx.pointer_held() {
        run = ((ctx.pointer_world().x - ctx.pos().x) * 2.0).clamp(-1.0, 1.0);
    }
    let area = ctx.view_rect().expand(-0.8); // видимая часть мира без полосы у края
    let x = ctx.pos().x + run * self.speed * ctx.dt();
    ctx.set_pos(Vec2::new(x.clamp(area.x, area.max().x), ctx.pos().y));
}
```

### …что-то, упавшее за край экрана

```rust
fn update(&mut self, ctx: &mut Ctx) {
    if ctx.pos().y < ctx.view_rect().y {
        let game = ctx.find("Game");
        ctx.send(game, "Missed");
        ctx.destroy_self();
    }
}
```

Готовое: `OffScreen` (`message`, `notify`, `margin`; срабатывает, только если объект уже
побывал на экране).

### …сделать что-то через N секунд

```rust
ctx.delay(1.5, |ctx| ctx.ui_hide("Hint"));
ctx.destroy_self_after(3.0); // исчезнуть через 3 секунды (готовое: DestroyAfter)
```

### …мигание

```rust
fn update(&mut self, ctx: &mut Ctx) {
    let on = (ctx.elapsed() * 4.0).sin() > 0.0;
    ctx.me_mut::<SpriteRenderer>().color.a = if on { 1.0 } else { 0.3 };
}
```

### …плавно передвинуть объект в точку

```rust
ctx.move_to(Vec2::new(4.0, 0.0), 0.5); // за полсекунды, с мягким стартом и концом
ctx.scale_to(Vec2::splat(1.5), 0.2);
```

Сложнее (цвет, цепочки, повтор) — твины, §4.

### …ездящую платформу

```rust
component! {
    pub struct Platform { pub offset: Vec3 = Vec3::new(3.0, 0.0, 0.0), pub start: Vec3 = Vec3::ZERO, pub t: f32 = 0.0 }
}
impl Component for Platform {
    fn start(&mut self, ctx: &mut Ctx) { self.start = ctx.pos(); }
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        self.t += ctx.dt();
        let k = smoothstep(ping_pong(self.t / 2.0, 1.0));
        ctx.set_pos(self.start + self.offset * k);
    }
}
```

### …найти другой объект

```rust
let game = ctx.find("Game");           // по имени (опечатка → подсказка в Console)
let player = ctx.find_tag("Player");    // по тегу
let boss = ctx.first_with::<Boss>();    // по компоненту
for enemy in ctx.all_with::<Enemy>() { ctx.on(enemy).destroy_self(); }
```

Без сообщения об ошибке («есть ли такой?») — длинные формы `ctx.find_by_name(..)` /
`ctx.find_by_tag(..)` возвращают `Option`.

### …ссылку на объект в инспекторе

```rust
component! { pub struct Door { pub switch: ObjectRef = ObjectRef::NONE } }
// в хуке:
let switch = ctx.obj(self.switch);
```

Подробно (`ComponentRef<T>`, префабы) — §6.5.

### …прочитать или поменять чужой компонент

```rust
let enemy = ctx.find("Boss");
ctx.on(enemy).me_mut::<Health>().hp -= 1;
let hp = ctx.on(enemy).me::<Health>().hp;
```

### …свой компонент без `Option`

```rust
ctx.me_mut::<SpriteRenderer>().flip_x = true;
ctx.with::<Light, _>(|light| light.intensity = 2.0);
if !ctx.has::<AudioSource>() { ctx.add(AudioSource::default()); }
```

Свой собственный скрипт — это `self`, а не `ctx.me::<Self>()`.

### …послать сообщение

```rust
ctx.send(door, "Open");             // ловится в on_message: message.is("Open")
ctx.send_value(player, "Heal", 2);  // со значением: message.get::<i32>()
```

### …кнопку в интерфейсе

Повесьте скрипт на объект кнопки (`Button`): хук `on_click` вызовется при нажатии мышью,
пальцем или Enter.

```rust
fn on_click(&mut self, ctx: &mut Ctx) {
    ctx.load_scene("level1");
}
```

Опросом из любого скрипта: `if ctx.clicked("play") { … }` (имя — поле `message` кнопки).

### …щелчок по объекту мира

Добавьте объекту `Clickable` (Behaviours) — и хук `on_click` любого скрипта на нём
сработает при щелчке мышью или касании (2D, ортографическая камера).

### …показать и спрятать экран

```rust
ctx.ui_show("PausePanel");
ctx.ui_hide("PausePanel");
```

### …паузу

```rust
if ctx.pressed("Cancel") {
    let paused = ctx.time_scale() == 0.0;
    ctx.set_time_scale(if paused { 1.0 } else { 0.0 });
}
```

### …звук и музыку

```rust
ctx.play("sounds/jump.wav");
ctx.play_at("sounds/boom.wav", ctx.pos()); // объёмный: громче рядом со слушателем
ctx.music("sounds/theme.ogg");
ctx.stop_music();
ctx.play("sfx:hurt");     // встроенные звуки — без файлов и без add_clip
ctx.music("music:calm");  // уже играет — не начнётся заново (удобно после рестарта)
```

### …следующий уровень, рестарт, выход

```rust
ctx.load_scene("level2");
ctx.reload_scene(); // та же сцена заново: все объекты и скрипты как в начале
ctx.quit();
```

`reload_scene` перезагружает сцену, с которой стартовала игра (`engine::game!` /
`engine::load_scene`), последнюю из `load_scene(..)` или открытую в редакторе при Play.
Сохранения (`ctx.save`) и музыка (`ctx.music`) переживают перезагрузку.

### …рекорд, который переживёт перезапуск

```rust
fn start(&mut self, ctx: &mut Ctx) { self.best = ctx.load("best", 0); }
// при новом рекорде:
ctx.save("best", self.best);
```

### …случайное число

```rust
let x = ctx.rand_range(-3.0, 3.0); // дробное
let die = ctx.rand_int(1, 6);      // целое, 6 входит
if ctx.rand() < 0.1 { /* 10% */ }
```

### …луч: что впереди / под ногами

```rust
if let Some(hit) = ctx.raycast2d(ctx.pos2(), Vec2::X, 5.0) {
    ctx.log(format!("in front: {}", ctx.name_of(hit.object)));
}
// 3D: ctx.raycast(ctx.pos(), ctx.forward(), 50.0)
```

### …силу и толчок

```rust
ctx.add_force(Vec3::Y * 30.0); // в fixed_update, пока сила действует
let v = ctx.velocity();
ctx.set_velocity(v * 0.5);     // затормозить вдвое
```

### …объекты в радиусе (3D)

```rust
for id in ctx.overlap_sphere(ctx.pos(), 3.0) {
    ctx.send(id, "Explosion");
}
```

### …повернуться к цели

```rust
let target = ctx.find_tag("Player");
let at = ctx.on(target).pos();
ctx.look_at(at); // 3D: «вперёд» (−Z) на цель
```

### …выключить объект

```rust
ctx.set_active_self(false);               // себя
ctx.on(door).set_active_self(false);      // другой объект
```

### …написать в Console

```rust
ctx.log(format!("hp = {}", self.hp)); // с именем объекта: "[Hero] hp = 3"
ctx.warn("no checkpoint yet");
```

### …проверить скрипт тестом

```rust
#[test]
fn hero_walks_right() {
    let mut app = engine::app();
    let hero = app.spawn("Hero").with(Hero::default()).id();
    app.world.input.set_key(Key::D, true);
    app.run_headless(60, 1.0 / 60.0);
    assert!(app.world.scene.world_position(hero).x > 4.0);
}
```

## Шпаргалка: короткие формы

Каждая короткая форма — тонкая обёртка над длинной; **обе остаются рабочими**, их можно
смешивать в одном скрипте. Та же таблица — в редакторе (Help → Scripting Cheat Sheet).
Короткие формы физики, звука и интерфейса — трейты `PhysicsCtx`, `AudioCtx`, `UiCtx`; их
подключает `use engine::prelude::*;`.

<!-- cheat-sheet:begin (генерируется из engine_core::script::CHEAT_SHEET; тест сверяет) -->
| раздел | короткая форма | пример | длинная форма (тоже работает) |
|---|---|---|---|
| Game | `engine::game! { name, scene, components }` | `engine::game! { name: "My Game", scene: "scenes/main.scene", components: [Player] }` | `fn register + fn build_with + fn build + engine::game_main!(..)` |
| Hooks | `fn start(&mut self, ctx)` | `fn start(&mut self, ctx: &mut Ctx) { ctx.log("hi"); }` | `fn on_start(&mut self, ctx)` |
| Hooks | `fn update(&mut self, ctx)` | `fn update(&mut self, ctx: &mut Ctx) { ctx.move_by(Vec2::X * ctx.dt()); }` | `fn on_update(&mut self, ctx, dt)` |
| Hooks | `fn fixed_update(&mut self, ctx)` | `fn fixed_update(&mut self, ctx: &mut Ctx) { ctx.set_velocity(Vec2::X); }` | `fn on_fixed_update(&mut self, ctx, dt)` |
| Hooks | `fn late_update(&mut self, ctx)` | `fn late_update(&mut self, ctx: &mut Ctx) { /* camera */ }` | `fn on_late_update(&mut self, ctx, dt)` |
| Hooks | `fn on_collision(&mut self, ctx, other)` | `if ctx.has_tag(other, "Enemy") { ctx.destroy_self(); }` | `fn on_collision_enter(&mut self, ctx, other)` |
| Hooks | `fn on_trigger(&mut self, ctx, other)` | `if ctx.has_tag(other, "Player") { ctx.play("coin"); }` | `fn on_trigger_enter(&mut self, ctx, other)` |
| Hooks | `fn on_click(&mut self, ctx)` | `fn on_click(&mut self, ctx: &mut Ctx) { ctx.load_scene("level1"); }` | `on_message from a Button / ctx.ui().clicked(..)` |
| Hooks | `message.is(name) / message.get::<T>()` | `if message.is("Damage") { self.hp -= message.get::<i32>().unwrap_or(1); }` | `message.name == .. / T::from_value(&message.value)` |
| Time | `ctx.dt()` | `ctx.move_by(Vec2::X * 3.0 * ctx.dt())` | `the dt parameter / ctx.time().delta` |
| Time | `ctx.elapsed()` | `let wave = ctx.elapsed().sin();` | `ctx.time().elapsed as f32` |
| Moving | `ctx.pos() / ctx.pos2()` | `let p = ctx.pos2();` | `ctx.world_position()` |
| Moving | `ctx.set_pos(p)` | `ctx.set_pos(Vec2::new(0.0, 3.0))` | `ctx.transform().position = .. (local)` |
| Moving | `ctx.move_by(delta)` | `ctx.move_by(Vec2::new(speed * ctx.dt(), 0.0))` | `ctx.transform().translate(..)` |
| Moving | `ctx.rotate_by(deg)` | `ctx.rotate_by(90.0 * ctx.dt())` | `ctx.transform().rotate_z(radians)` |
| Moving | `ctx.angle() / ctx.set_angle(deg)` | `ctx.set_angle(45.0)` | `transform().angle_z() / set_angle_z(radians)` |
| Moving | `ctx.scale() / ctx.set_scale(s)` | `ctx.set_scale(Vec2::splat(2.0))` | `ctx.transform().scale` |
| Moving | `ctx.look_at(point)` | `ctx.look_at(target_pos)` | `ctx.transform().look_at(point, Vec3::Y)` |
| Moving | `ctx.forward() / right() / up()` | `ctx.move_by(ctx.forward() * ctx.dt())` | `scene.world_rotation(id) * Vec3::NEG_Z` |
| Moving | `ctx.move_to(p, secs) / scale_to` | `ctx.move_to(Vec2::new(4.0, 0.0), 0.5);` | `ctx.start_tween(Tween::position(id, p).duration(secs))` |
| Input | `ctx.key(Key::W) / key_pressed / key_released` | `if ctx.key_pressed(Key::Space) { .. }` | `ctx.input().key(..) / key_down / key_up` |
| Input | `ctx.axis("Horizontal")` | `let run = ctx.axis("Horizontal");` | `ctx.input().axis(..)` |
| Input | `ctx.axis2(x, y)` | `let dir = ctx.axis2("Horizontal", "Vertical");` | `Vec2::new(input.axis(x), input.axis(y))` |
| Input | `ctx.pressed("Jump") / held / released` | `if ctx.pressed("Jump") { .. }` | `ctx.input().button_down / button / button_up` |
| Input | `ctx.mouse() / mouse_pressed(b) / mouse_held(b)` | `if ctx.mouse_pressed(MouseButton::Left) { .. }` | `ctx.input().mouse_position() / mouse_button_down` |
| Screen | `ctx.view_rect()` | `if ctx.pos().y < ctx.view_rect().y { ctx.destroy_self(); }` | `camera.ortho_size × screen aspect, by hand` |
| Screen | `ctx.pointer_world() / pointer_held()` | `if ctx.pointer_held() { let target = ctx.pointer_world(); }` | `touches() / mouse_position() + camera math + ui().pointer_over_ui()` |
| Screen | `ctx.screen_to_world(pixels)` | `let p = ctx.screen_to_world(ctx.mouse());` | `Unity: Camera.main.ScreenToWorldPoint, by hand` |
| Components | `ctx.me::<T>() / ctx.me_mut::<T>()` | `ctx.me_mut::<SpriteRenderer>().flip_x = true;` | `ctx.get::<T>() / get_mut::<T>() (Option)` |
| Components | `ctx.with::<T, _>(\|t\| ..)` | `ctx.with::<Light, _>(\|l\| l.intensity = 2.0);` | `if let Some(t) = ctx.get_mut::<T>() { .. }` |
| Components | `ctx.has::<T>() / ctx.add(component)` | `if !ctx.has::<Light>() { ctx.add(Light::default()); }` | `ctx.object().has / object_mut().add` |
| Objects | `ctx.find("Name")` | `let game = ctx.find("Game");` | `ctx.find_by_name(..) (Option)` |
| Objects | `ctx.find_tag("Tag") / find_all_tag` | `let player = ctx.find_tag("Player");` | `ctx.find_by_tag / find_all_by_tag` |
| Objects | `ctx.first_with::<T>() / all_with::<T>()` | `for coin in ctx.all_with::<Coin>() { .. }` | `ctx.find_with::<T>()` |
| Objects | `ctx.obj(self.target)` | `let target = ctx.obj(self.target);` | `ctx.resolve(self.target) (Option)` |
| Objects | `ctx.on(id).…` | `ctx.on(enemy).me_mut::<Health>().hp -= 1;` | `ctx.get_on_mut::<T>(id), scene.get_mut(id)` |
| Objects | `ctx.exists(id) / has_tag(id, tag) / name_of(id)` | `if ctx.exists(target) { .. }` | `ctx.scene().get(id).is_some() / .tag == ..` |
| Messages | `ctx.send(id, "Name")` | `ctx.send(game, "Caught");` | `ctx.send_message(id, name, Value::Nil)` |
| Messages | `ctx.send_value(id, "Name", v)` | `ctx.send_value(player, "Damage", 2);` | `ctx.send_message(id, name, Value::Int(2))` |
| Lifetime | `ctx.spawn(prefab) / spawn_at / spawn_child` | `let coin = ctx.spawn_at("prefabs/coin.prefab", ctx.pos());` | `ctx.instantiate_at(..) (Result)` |
| Lifetime | `ctx.destroy_self() / destroy(id) / destroy_self_after(s)` | `ctx.destroy_self_after(2.0);` | `(unchanged)` |
| Lifetime | `ctx.set_active_self(on)` | `ctx.on(door).set_active_self(false);` | `ctx.set_active(ctx.id, on)` |
| Timers | `ctx.delay(secs, \|ctx\| ..)` | `ctx.delay(1.0, \|ctx\| ctx.destroy_self());` | `ctx.call_after(secs, \|world\| ..) / ctx.after(secs, msg, value)` |
| Timers | `ctx.repeat(secs, \|ctx\| ..)` | `ctx.repeat(2.0, \|ctx\| { ctx.spawn("prefabs/enemy.prefab"); });` | `ctx.call_every / ctx.every(secs, msg, value)` |
| Timers | `Timer / Cooldown fields` | `pub shot: Cooldown = Cooldown::new(0.3),` | `(unchanged)` |
| Physics | `ctx.velocity() / velocity2() / set_velocity(v)` | `ctx.set_velocity(Vec2::new(run * 5.0, ctx.velocity2().y));` | `ctx.get_mut::<Rigidbody2D>().velocity` |
| Physics | `ctx.add_force(f)` | `ctx.add_force(Vec3::Y * 20.0);` | `physics3d.add_force(id, f)` |
| Physics | `ctx.grounded()` | `if ctx.grounded() && ctx.pressed("Jump") { .. }` | `CharacterController::grounded / a raycast down` |
| Physics | `ctx.raycast(from, dir, max) / raycast2d` | `if let Some(hit) = ctx.raycast2d(ctx.pos2(), Vec2::NEG_Y, 1.0) { .. }` | `Physics2D / Physics3D::raycast(.., Some(id))` |
| Physics | `ctx.overlap_sphere(center, r)` | `for id in ctx.overlap_sphere(ctx.pos(), 3.0) { .. }` | `physics3d.overlap_sphere(..)` |
| Sound | `ctx.play("clip") / play_at(clip, p)` | `ctx.play("sounds/jump.wav");` | `ctx.resource_mut::<Audio>().play(..)` |
| Sound | `ctx.play("sfx:coin") / music("music:calm")` | `ctx.play("sfx:hurt");` | `audio.add_clip("hurt", synth::sfx::hurt()) + play("hurt")` |
| Sound | `ctx.music("clip") / stop_music()` | `ctx.music("theme");` | `audio.play_music(clip, 1.0)` |
| UI | `ctx.ui_text("Name", text)` | `ctx.ui_text("Score", format!("Score: {}", self.score));` | `find_by_name + get_component_mut::<Text>().set(..)` |
| UI | `ctx.ui_show("Name") / ui_hide` | `ctx.ui_show("GameOver");` | `scene.ui_set_visible(id, true)` |
| UI | `ctx.clicked("button")` | `if ctx.clicked("restart") { ctx.reload_scene(); }` | `ctx.ui().clicked(..)` |
| Shaders | `ctx.set_shader_param(name, v)` | `ctx.set_shader_param("amount", self.burn);` | `ctx.me_mut::<MaterialParams>().set(..) / SpriteShader / PostEffect` |
| Scenes | `ctx.load_scene(name) / reload_scene() / quit()` | `ctx.load_scene("level2");` | `(unchanged)` |
| Saving | `ctx.save(key, v) / ctx.load(key, default)` | `self.best = ctx.load("best", 0);` | `ctx.save_data().set_int(..) + save() / int(..)` |
| Random | `ctx.rand() / rand_range(a, b) / rand_int(a, b)` | `let x = ctx.rand_range(-3.0, 3.0);` | `ctx.random().f32() / range / int_range_inclusive` |
| Log | `ctx.log(text) / ctx.warn(text)` | `ctx.log(format!("hp = {}", self.hp));` | `log::info!(..)` |
<!-- cheat-sheet:end -->

## Старая форма ↔ короткая

Старый код не надо переписывать: всё, что было, компилируется и работает как раньше
(это проверяют `crates/engine/tests/compat.rs` и игры в `games/`). Новые формы — только
добавлены.

| было (работает) | можно короче |
|---|---|
| `fn on_update(&mut self, ctx: &mut Ctx, dt: f32)` | `fn update(&mut self, ctx: &mut Ctx)` + `ctx.dt()` |
| `fn on_start` / `on_fixed_update` / `on_late_update` | `start` / `fixed_update` / `late_update` |
| `fn on_collision_enter` / `on_trigger_enter` | `on_collision` / `on_trigger` |
| `if let Some(b) = ctx.get_mut::<Rigidbody2D>() { b.velocity.x = 5.0; }` | `ctx.me_mut::<Rigidbody2D>().velocity.x = 5.0;` |
| `ctx.transform().position.x += 1.0 * dt;` | `ctx.move_by(Vec2::X * ctx.dt());` |
| `ctx.input().axis("Horizontal")` | `ctx.axis("Horizontal")` |
| `ctx.input().button_down("Jump")` | `ctx.pressed("Jump")` |
| `if let Some(g) = ctx.find_by_name("Game") { ctx.send_message(g, "Caught", Value::Nil); }` | `let g = ctx.find("Game"); ctx.send(g, "Caught");` |
| `match ctx.instantiate_at(path, pos) { Ok(id) => …, Err(e) => log::error!(…) }` | `let id = ctx.spawn_at(path, pos);` |
| `ctx.call_after(1.0, \|world\| …)` | `ctx.delay(1.0, \|ctx\| …)` |
| `if let Some(a) = ctx.resource_mut::<Audio>() { a.play("coin"); }` | `ctx.play("coin");` |
| `find_by_name("Score")` + `get_component_mut::<Text>(id)` + `set(..)` | `ctx.ui_text("Score", text);` |
| `let s = ctx.save_data(); s.set_int("best", 5); s.save()?;` | `ctx.save("best", 5);` |
| `ctx.random().range(a, b)` | `ctx.rand_range(a, b)` |

Если переопределены и длинный, и короткий хук одного вида (`on_update` и `update`), работает
длинный — как в старом коде. Маска `fn hooks()` общая: `Hooks::UPDATE` включает обе формы.
Имена, которые уже были заняты длинными формами с другим смыслом, остались прежними, поэтому
у коротких форм свои имена: `ctx.delay` / `ctx.repeat` (а `ctx.after` / `ctx.every` — таймеры
с сообщением), `ctx.set_active_self(on)` (а `ctx.set_active(id, on)`), `ctx.obj(field)` (а
`ctx.object()` — свой `GameObject`), `ctx.elapsed()` (а `ctx.time()` — вся структура `Time`).

## Готовые поведения

Add Component → **Behaviours**: маленькие скрипты движка, написанные короткими формами.
Исходники — `crates/engine/src/behaviours/` (читайте как примеры); в коде —
`engine::behaviours::Mover` и т. д.

| поведение | что делает | что нужно на объекте |
|---|---|---|
| `Mover` | едет со скоростью `velocity` | — |
| `Rotator` | вращается (`degrees_per_second`) | — |
| `Follow` | следует за `target` с плавностью `smoothing` (в `late_update`) | — |
| `DestroyAfter` | исчезает через `seconds` | — |
| `Spawner` | создаёт `prefab` каждые `interval` с (меняется на ходу), всего `count` (0 — без конца), разброс `spread` или по ширине экрана (`across_view`, `view_margin`); `enabled` — пауза | — |
| `Health` | жизни: сообщения `Damage` / `Heal`, на нуле — `Died` и (если `destroy_on_death`) исчезает | — |
| `Damage` | при касании шлёт `Damage` (`amount`) объектам с тегом `only_tag` | коллайдер |
| `Collectible` | подбирается объектом с тегом `collector_tag`: сообщение `message` (`value`) ему и объекту `notify`, звук `sound`, исчезает | коллайдер-триггер |
| `OffScreen` | побывал на экране и ушёл за край (`margin`): сообщение `message` себе и объекту `notify`, исчезает (`destroy`) | 2D-камера |
| `Clickable` | щелчок по объекту → хук `on_click` у скриптов объекта, сообщение `message` | 2D, ортографическая камера |
| `PlayerMove2D` | ходит по осям `Horizontal` / `Vertical` | (`Rigidbody2D` с гравитацией 0) |
| `Paddle2D` | влево-вправо по оси `Horizontal` или за пальцем / зажатой мышью (`follow_pointer`), не дальше края экрана (`view_margin`), `flip_sprite`; с `Rigidbody2D` скорость по Y остаётся телу | (`Rigidbody2D`) |
| `Platformer2D` | бег, прыжок по `Jump`, время койота, буфер прыжка | `Rigidbody2D`, `Collider2D` |
| `PlayerMove3D` | ходьба относительно камеры, поворот по ходу, прыжок | `CharacterController` |

<!-- beginner:end -->

---

## 1. Порядок кадра

`app.tick(dt)` (то же делает окно; `app.run_headless(n, dt)` — `n` раз подряд):

1. `dt` умножается на `time.time_scale` → `time.delta`; обновляются `elapsed`, `unscaled_*`, `frame`.
2. `Subsystem::pre_update` (здесь геймпады пишут своё состояние в `input`).
3. `on_start` у новых компонентов.
4. Фиксированные шаги (0…5 за кадр): `on_fixed_update` скриптов → физика.
5. **Таймеры, затем твины** (§3, §4).
6. `on_update`, затем `on_late_update`. После каждой фазы доставляются сообщения (`on_message`).
7. `post_update` подсистем.
8. Конец кадра: отложенные `destroy` (с `on_destroy`) → **смена сцены, если её просили** (§5) → события
   кадра становятся читаемыми → сбрасывается «нажато в этом кадре».

Из этого следует:

* твин, запущенный в `on_update`, начинает двигаться со следующего кадра;
* `destroy` и `load_scene` не прерывают текущий кадр: объект живёт и обновляется до самого конца;
* событие (`ctx.emit`), отправленное в кадре N, читается в кадре N+1.

Внутри каждой фазы объекты идут **в порядке иерархии** (родитель раньше детей, братья — как в окне
иерархии). Если нужно, чтобы одни скрипты отработали раньше других независимо от иерархии, у типа
задают **порядок выполнения** (как Script Execution Order в Unity):

```rust
impl Component for InputReader {
    fn execution_order(&self) -> i32 { -100 }   // меньше — раньше; по умолчанию 0
}
```

Число действует на `on_start`, `on_fixed_update`, `on_update`, `on_late_update`: сначала все
компоненты с −100, потом с 0, потом с +100; при равных числах — иерархия, затем порядок компонентов
на объекте. Число из кода перекрывает файл `assets/project.settings` (по имени типа; его пишет окно
Edit → Project Settings… редактора). Подробности и гарантии — `script-order.md`.

**Какие хуки есть у компонента.** Необязательный метод `hooks()` говорит движку, какие хуки
компонент реализует; остальные не вызываются вовсе — так тысячи «компонентов-данных» ничего не стоят
в фазах кадра. Ничего не объявили — получаете все хуки, как раньше:

```rust
impl Component for Health {
    fn hooks(&self) -> Hooks { Hooks::NONE }                    // только данные
}
impl Component for Turret {
    fn hooks(&self) -> Hooks { Hooks::START | Hooks::UPDATE }   // остальные хуки не нужны
    fn on_start(&mut self, ctx: &mut Ctx) { /* … */ }
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) { /* … */ }
}
```

Флаги: `START`, `UPDATE`, `FIXED_UPDATE`, `LATE_UPDATE`, `DESTROY`, `MESSAGE`, `COLLISION_ENTER`,
`TRIGGER_ENTER`, `NONE`, `ALL`. `hooks()` спрашивается один раз, при добавлении компонента; хук, которого
нет в ответе, не вызовется, даже если он написан, — перечисляйте все свои. Компоненты движка, которые
хранят только данные (`SpriteRenderer`, `MeshRenderer`, `Camera`, `Light`, коллайдеры, тела,
`AudioSource`, компоненты интерфейса), уже объявляют `Hooks::NONE`.

---

## 2. Время

```rust
pub struct Time {
    pub delta: f32,            // секунд за кадр, УЖЕ с учётом time_scale (Unity: deltaTime)
    pub unscaled_delta: f32,   // реальная длительность кадра (Unity: unscaledDeltaTime)
    pub elapsed: f64,          // игровое время с запуска, идёт с time_scale (Unity: time)
    pub unscaled_elapsed: f64, // реальное время с запуска (Unity: unscaledTime)
    pub frame: u64,            // номер кадра
    pub fixed_delta: f32,      // шаг on_fixed_update и физики, по умолчанию 1/60
    pub time_scale: f32,       // 1 — норма, 0 — пауза, 0.3 — замедление
    pub fixed_alpha: f32,
}
impl Time { pub fn fps(&self) -> f32; pub fn is_paused(&self) -> bool; }
```

В хуках `dt` — это уже `time.delta`. Менять масштаб:

```rust
ctx.set_time_scale(0.0);      // пауза  (то же: ctx.world.time.time_scale = 0.0)
ctx.set_time_scale(0.25);     // замедленная съёмка
let s = ctx.time_scale();
```

Масштаб действует **со следующего кадра**. Что он затрагивает:

| | при `time_scale = 0` |
|---|---|
| `dt` в `on_update` / `on_late_update` | 0 |
| фиксированные шаги (`on_fixed_update`, физика) | не идут; накопитель не «взрывается», после паузы шаги идут с обычной частотой |
| таймеры, твины | стоят (кроме `unscaled`, §3–4) |
| ввод, `on_message`, рендер | работают как обычно — вы можете открыть меню на паузе |

Отрицательный или нечисловой масштаб безопасен: отрицательный считается нулём, `NaN`/∞ — единицей.

Интерфейсу на паузе нужно реальное время — берите `ctx.time().unscaled_delta` или `unscaled`-твины и таймеры.

Начальные `fixed_delta`, самый длинный кадр (`Time::max_delta`) и `time_scale` задаёт
раздел `time` файла `project.settings` (Edit → Project Settings → Time), его применяет
`engine::mount_assets`; код, выполненный после, их перекрывает. Качество графики из кода:
`ctx.set_quality(QualityLevel::Low)` / `ctx.quality()` — см. `docs/api/settings.md`.

---

## 3. Таймеры и отложенные вызовы

### 3.1. Через `Ctx` (основной способ)

«Перезвони мне через N секунд» = сообщение самому себе, которое придёт в `on_message`:

```rust
component! {
    pub struct Bomb { pub fuse: f32 = 3.0 }
}
impl Component for Bomb {
    fn on_start(&mut self, ctx: &mut Ctx) {
        ctx.after(self.fuse, "Explode", Value::Nil);                  // один раз
        ctx.every(0.5, "Blink", Value::Nil);                          // каждые 0.5 с
    }
    fn on_message(&mut self, ctx: &mut Ctx, m: &Message) {
        match m.name.as_str() {
            "Explode" => ctx.destroy_self(),
            "Blink" => { /* мигнуть */ }
            _ => {}
        }
    }
}
```

| Вызов | Что делает |
|---|---|
| `ctx.after(delay, name, value) -> TimerId` | через `delay` с. сообщение `name` этому объекту |
| `ctx.every(interval, name, value) -> TimerId` | то же каждые `interval` с. (первое — через `interval`) |
| `ctx.send_after(target, delay, name, value) -> TimerId` | сообщение другому объекту |
| `ctx.call_after(delay, \|world\| { .. }) -> TimerId` | замыкание `FnOnce(&mut World) + 'static` |
| `ctx.call_every(interval, \|world\| { .. }) -> TimerId` | замыкание `FnMut(&mut World) + 'static` |
| `ctx.destroy_after(target, delay) -> TimerId` | Unity `Destroy(obj, delay)` |
| `ctx.destroy_self_after(delay) -> TimerId` | то же для себя |
| `ctx.cancel_timer(id) -> bool` | отменить; `false`, если уже сработал/отменён |
| `ctx.cancel_my_timers() -> usize` | Unity `CancelInvoke()` |

Значение `Value` — это то же, что в сообщениях: `Value::Nil`, `Value::Int(7)`, `Value::Float(1.5)`,
`Value::Str("x".into())`, `Value::Vec3(x, y, z)`… Читается в `on_message` по `m.value`.

Выстрел и «исчезни через две секунды»:

```rust
let bullet = ctx.instantiate_at("prefabs/bullet.prefab", ctx.world_position())?;
ctx.destroy_after(bullet, 2.0);
```

### 3.2. Правила

* Время идёт по `time.delta`: на паузе таймеры стоят.
* **Таймер принадлежит объекту** (адресат сообщения / `self` для `call_*`) и умирает вместе с ним —
  в том числе когда сцена заменяется. Сообщение мёртвому объекту не доставляется и ошибки нет.
* Пока владелец неактивен в иерархии (`set_active(false)`), его таймеры стоят и продолжают, когда
  объект включили. Исключение: `destroy_after` работает и на неактивном объекте.
* Повторяющийся таймер при долгом кадре срабатывает несколько раз подряд (не больше 16 за кадр).
* Порядок срабатывания — порядок создания. Сообщения приходят в `on_message` в конце фазы таймеров.

### 3.3. Через `Scheduler` напрямую

Нужен, когда таймеру не нужен владелец-объект или надо настроить его подробнее
(`world.scheduler()` — ресурс, создаётся сам):

```rust
let id = world.scheduler().send_every(target, 1.0, "Tick", Value::Nil);
world.scheduler().limit(id, 5);       // не более 5 срабатываний
world.scheduler().unscaled(id);       // реальное время: идёт и на паузе (таймеры меню)
world.scheduler().owned_by(id, who);  // привязать к объекту (для call_*)
world.scheduler().cancel(id);
world.scheduler().cancel_owned_by(obj);
world.scheduler().is_pending(id); world.scheduler().remaining(id); world.scheduler().len();
```

`call_after` / `call_every` из `Scheduler` **без владельца переживают смену сцены** — удобно для
глобальных вещей вроде автосохранения: `world.scheduler().call_every(60.0, |w| { .. })`.

### 3.4. `Timer` и `Cooldown` — значения внутри компонента

Для случаев, когда проще хранить состояние в самом компоненте и «тикать» его своим `dt`.
Оба типа можно объявлять полями `component!` (сохраняются в сцене).

```rust
component! {
    pub struct Turret {
        pub reload: Cooldown = Cooldown::new(0.4),
        pub wave: Timer = Timer::repeating(10.0),
    }
}
impl Component for Turret {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.reload.tick(dt);
        if self.wave.tick(dt).just_finished() { /* новая волна */ }
        if ctx.input().button("Fire1") && self.reload.trigger() {
            /* выстрел */
        }
    }
}
```

`Timer`: `once(s)`, `repeating(s)`, `tick(dt) -> &mut Self`, `just_finished()`, `times_finished()`
(при большом `dt` бывает >1), `finished()`, `elapsed()`, `remaining()`, `fraction()`, `duration()`,
`set_duration(s)`, `reset()`, `pause()`, `unpause()`, `is_paused()`. Одноразовый после срабатывания
стоит в `finished()` до `reset()`.

`Cooldown`: `new(s)` (готова сразу), `starting_cold(s)`, `tick(dt)`, `ready()`, `trigger() -> bool`
(если готова — запускает перезарядку и возвращает `true`), `start()`, `reset()`, `remaining()`,
`fraction()` (0 → 1, для полоски способности), `duration()`, `set_duration(s)`.

---

## 4. Твины

Твин плавно меняет значение за время. Он знает **объект** и **путь** к полю и умирает вместе с объектом.

```rust
use engine::prelude::*;

fn punch(ctx: &mut Ctx, target: GameObjectId) {
    ctx.start_tween(
        Tween::scale(target, Vec3::splat(1.3))
            .duration(0.15)
            .ease(Ease::OutBack)
            .yoyo()
            .loops(2),                      // вырос и вернулся
    );
    ctx.start_tween(
        Tween::field(target, "SpriteRenderer.color", Color::RED)
            .duration(0.15)
            .yoyo()
            .loops(2),
    );
}
```

### 4.1. Конструкторы

| Конструктор | Анимирует |
|---|---|
| `Tween::position(id, Vec3)` | локальная позиция `Transform` |
| `Tween::scale(id, Vec3)` | локальный масштаб |
| `Tween::rotation(id, Quat)` | поворот по кратчайшей дуге (slerp) |
| `Tween::euler(id, Vec3)` | углы Эйлера в градусах, покомпонентно (можно крутить на 360°) |
| `Tween::rotation_z(id, f32)` | поворот вокруг Z в радианах, покомпонентно (для 2D) |
| `Tween::field(id, "Компонент.поле", значение)` | любое поле любого компонента (см. ниже) |

**Путь** — `"Компонент.поле"` или `"Компонент.поле.канал"`. Канал: `x y z w` или `r g b a`.
Примеры: `"SpriteRenderer.color"`, `"SpriteRenderer.color.a"` (только прозрачность), `"Health.hp"`,
`"Transform.position.y"`, `"Transform.scale.x"`, `"Light.intensity"`.

Анимируются поля типов `f32`, целые (округляются), `Vec2`, `Vec3`, `Quat`, `Color`.
Целевое значение — любой тип поля: `Tween::field(id, "Health.hp", 0)`, `.. "Sprite.color", Color::RED`,
`.. "Transform.position.y", 4.0f32`. Тип `to` должен совпадать с типом поля (число к числу, вектор
к вектору). Неверный путь не паникует: в лог идёт предупреждение, твин снимается.

### 4.2. Настройка (цепочкой)

| | |
|---|---|
| `.duration(s)` | длительность одного прохода, по умолчанию 1 с; `0` — за один кадр |
| `.delay(s)` | пауза перед стартом |
| `.ease(Ease::…)` | кривая (см. ниже), по умолчанию `Linear` |
| `.from(значение)` | начальное значение; по умолчанию — текущее **на момент старта анимации** (после `delay`) |
| `.loops(n)` | всего проходов, по умолчанию 1 |
| `.forever()` | бесконечно (до `cancel` или смерти объекта; завершения не будет) |
| `.yoyo()` | каждый второй проход идёт назад. `loops(2).yoyo()` = туда и обратно |
| `.unscaled()` | реальное время: идёт на паузе (анимация меню) |
| `.on_complete(target, "Имя", value)` | по завершении послать `target` сообщение (`on_message`) |

**Кривые** `Ease`: `Linear`, `InQuad OutQuad InOutQuad`, `InCubic OutCubic InOutCubic`,
`InQuart OutQuart InOutQuart`, `InSine OutSine InOutSine`, `InExpo OutExpo InOutExpo`,
`InCirc OutCirc InOutCirc`, `InBack OutBack InOutBack` (с «отскоком» за границу),
`InElastic OutElastic InOutElastic`, `InBounce OutBounce InOutBounce`, `SmoothStep`.
`Ease::ALL` — полный список; `ease.apply(t)` — значение кривой для `t` в 0..1.
`Ease` — `reflect_enum!`, его можно держать полем компонента.

### 4.3. Последовательности

```rust
let id = ctx.start_sequence(
    Sequence::new()
        .then(Tween::position(coin, Vec3::new(0.0, 1.0, 0.0)).duration(0.3).ease(Ease::OutQuad))
        .join(Tween::scale(coin, Vec3::splat(1.5)).duration(0.3))   // одновременно с предыдущим then
        .wait(0.2)                                                  // пауза
        .then(Tween::scale(coin, Vec3::ZERO).duration(0.2))
        .on_complete(coin, "Collected", Value::Nil),                // когда закончилась вся цепочка
);
```

* `then` — после того, как закончилось всё добавленное раньше; `join` — вместе с предыдущим `then`;
  `wait(s)` — пауза перед следующим `then`; `.unscaled()` — вся цепочка по реальному времени.
* «Откуда» каждого шага берётся в момент его старта — шаги видят результат предыдущих.
* `.forever()` внутри последовательности проигрывается один раз (иначе она бы не кончилась).

### 4.4. Управление

```rust
let id = ctx.start_tween(..);          // TweenId
ctx.cancel_tween(id);                  // останавливает на текущем значении, сообщение не шлётся
ctx.cancel_tweens_of(target);          // все твины объекта
world.tweens().is_running(id); world.tweens().len(); world.tweens().cancel_all();
```

`TweenId` последовательности снимает всю цепочку целиком.

По завершению (не отменённого и не бесконечного) в следующем кадре читается событие:

```rust
for e in ctx.events::<TweenFinished>() { /* e.id: TweenId, e.target: Option<GameObjectId> */ }
```

### 4.5. Правила

* Объект уничтожен (или сцена заменена) — твин пропадает **молча**, `on_complete` не отправляется.
  Старые `GameObjectId` не «оживают», даже если слот переиспользован новым объектом.
* Пока объект неактивен (`set_active(false)`), его твины стоят.
* Время — `time.delta`: пауза и замедление действуют, `.unscaled()` игнорирует их.
* Если два твина пишут в одно поле, побеждает тот, что запущен позже (идут в порядке запуска).
  Для разных каналов одного поля (`color` и `color.a`) это тоже верно: запускайте канал **после** всего поля.
* Твин на `Transform.position` объекта с динамическим `Rigidbody` физика перезапишет — для
  физических тел анимируйте кинематические или используйте скорости.

---

## 5. Сцены

### 5.1. Загрузка из скрипта

```rust
component! { pub struct ExitDoor { pub next: String = String::from("level2") } }
impl Component for ExitDoor {
    fn on_trigger_enter(&mut self, ctx: &mut Ctx, _other: GameObjectId) {
        ctx.load_scene(&self.next);    // выполнится В КОНЦЕ КАДРА
    }
}
```

| Вызов | |
|---|---|
| `ctx.load_scene(name)` | заменить сцену (Unity `LoadScene`) |
| `ctx.load_scene_additive(name)` | добавить объекты сцены к текущей, ничего не удаляя |
| `ctx.unload_scene(name)` | убрать то, что добавил `load_scene_additive(name)` |
| `ctx.reload_scene()` | загрузить заново последнюю сцену, загруженную через `load_scene` |
| `ctx.scene_name() -> &str` | имя текущей сцены (поле `name` файла сцены) |
| `ctx.dont_destroy_on_load(id)` | Unity `DontDestroyOnLoad` |

То же есть у `world.scenes()` (`load`, `load_additive`, `unload`, `reload`) — для кода вне компонентов.

Запрос выполняется **после `on_late_update` и отложенных `destroy`** — скрипт, который его
отправил, сидит в заменяемой сцене и спокойно доживает кадр. У заменяемых объектов вызывается `on_destroy`;
`on_start` новых — в следующем кадре. Несколько запросов за кадр выполняются по порядку.

### 5.2. Откуда берутся сцены

Имя из `load_scene("level2")` разрешает `SceneSource`:

```rust
world.scenes().set_source(DirScenes::new("assets/scenes"));   // читает level2 или level2.scene
world.scenes().set_source(MemoryScenes::default()             // для тестов / include_str!
    .with("level2", include_str!("../assets/scenes/level2.scene")));
```

По умолчанию — текущий каталог. Свой источник — `impl SceneSource { fn load(&self, name) -> Result<String, SceneError> }`.

### 5.3. События и ошибки

Если сцена не найдена или не собирается (неизвестный компонент, неверный тип поля), **текущая сцена
остаётся нетронутой**, в лог идёт `error!`, а в следующем кадре читается событие:

```rust
for e in ctx.events::<SceneLoaded>() { /* e.name, e.additive */ }
for e in ctx.events::<SceneLoadFailed>() { /* e.name, e.error */ }
```

### 5.4. «Не уничтожать при загрузке»

```rust
fn on_start(&mut self, ctx: &mut Ctx) { ctx.dont_destroy_on_load(ctx.id); }
```

Объект (вместе со всей иерархией, **флаг ставится на корень**) переживает `load_scene` /
`World::load_scene` / `load_scene_str`, его `GameObjectId` остаётся действительным, `on_start`
повторно не вызывается. Типичные кандидаты: музыка, менеджер игры, камера HUD. Уничтожить такой объект можно
обычным `destroy`. Редакторская кнопка Stop возвращает сцену к снимку и убирает всё, в том числе такие объекты.

Если в новой сцене есть свои копии постоянного объекта (например, второй «Music»), дубликаты нужно убирать
самостоятельно — как в Unity.

### 5.5. Таймеры, твины и смена сцены

Таймеры и твины, привязанные к объектам, исчезают вместе с ними. Таймеры без владельца
(`world.scheduler().call_after(..)`) смену сцены переживают.

---

## 6. Запросы, активность, префабы

### 6.1. Поиск

На `Scene` (`ctx.scene()`, `world.scene`) и частично на `Ctx`:

| | |
|---|---|
| `scene.find_by_name(n)` / `ctx.find_by_name(n)` | `Option<GameObjectId>` |
| `scene.find_by_tag(t)` / `ctx.find_by_tag(t)` | первый с тегом |
| `scene.find_all_by_tag(t)` / `ctx.find_all_by_tag(t)` | все с тегом (включая неактивные) |
| `scene.find_all_active_by_tag(t)`, `scene.count_with_tag(t)` | |
| `scene.find_with::<T>()` / `ctx.find_with::<T>()` | `Vec<GameObjectId>` с компонентом `T`, в порядке иерархии |
| `scene.iter_with::<T>()` | итератор `(GameObjectId, &T)`; `iter_active_with::<T>()` — только активные |
| `scene.visit_with::<T>(\|id, t\| ..)`, `scene.visit_active_with::<T>(..)` | то же без списка (`visit_active_with` — только активные) |
| `scene.count_with::<T>()` | сколько объектов с `T` (и неактивных) |
| `scene.for_each_with_mut::<T>(\|id, t\| ..)` | изменить все `T` |
| `scene.sort_by_hierarchy(&mut ids)` | упорядочить id как в окне иерархии |
| `scene.find_in_children::<T>(id)` | Unity `GetComponentInChildren`: сам объект, затем потомки |
| `scene.find_all_in_children::<T>(id)` | `GetComponentsInChildren` |
| `scene.find_in_parent::<T>(id)` | `GetComponentInParent`: сам объект, затем предки |
| `scene.component_in_children::<T>(id)` / `_mut` | сразу `Option<&T>` |
| `scene.component_in_parent::<T>(id)` / `_mut` | |
| `scene.find_child(parent, "Name")`, `scene.find_path(root, "Arm/Hand")` | потомок по имени / пути |
| `scene.parent_of(id)`, `scene.root_of(id)`, `ctx.parent()`, `ctx.children()` | |
| `ctx.get_on::<T>(id)` / `get_on_mut` | компонент на чужом объекте |

Запросы по типу компонента (`find_with`, `iter_with`, `visit_*_with`, `count_with`,
`for_each_with_mut`) идут по индексу сцены и стоят пропорционально числу объектов с `T`, а не
размеру сцены — их можно звать каждый кадр. Поиск по имени и тегу по-прежнему обходит всю сцену:
найдите один раз в `on_start` и храните `GameObjectId`.

Пока выполняется хук компонента, **сам этот компонент временно «вынут»** из объекта: из своего хука
ищите себя через `self`, а не `ctx.get::<Self>()`.

```rust
// Ближайший враг в радиусе:
let me = ctx.world_position();
let nearest = ctx.scene().iter_active_with::<Enemy>()
    .map(|(id, _)| (id, ctx.scene().world_position(id).distance(me)))
    .filter(|(_, d)| *d < 8.0)
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(id, _)| id);
```

### 6.2. Активность

```rust
ctx.set_active(id, false);        // Unity SetActive(false)
ctx.is_active(id);                // activeInHierarchy: сам объект И все предки
ctx.scene_mut().set_active(id, true);
```

Неактивный объект **и всё его поддерево**: не получает `on_start` / `on_update` / `on_fixed_update`,
не рисуется, не участвует в физике (рендер и физика спрашивают `scene.is_active`). Таймеры и твины
объекта стоят. Объект остаётся в сцене; `find_*` его находят (кроме `*_active_*`).
Сообщения неактивному объекту, который уже стартовал, доставляются.

### 6.3. Префабы

```rust
let id = ctx.instantiate("prefabs/bullet.prefab")?;                              // в начале координат
let id = ctx.instantiate_at("prefabs/bullet.prefab", Vec3::new(1.0, 2.0, 0.0))?; // мировая позиция
let id = ctx.instantiate_under("prefabs/bullet.prefab", parent)?;                // ребёнок, локально (0,0,0)
let id = ctx.instantiate_with("prefabs/bullet.prefab", pos, Quat::from_rotation_z(a), Some(parent))?;
let copy = ctx.duplicate(id)?;                                                   // копия живого объекта
```

Без `parent` поза мировая, с `parent` — локальная. Масштаб берётся из корня префаба. Ссылка на префаб —
путь или GUID, как её понимает `world.prefabs` (в `engine::mount_assets` — база ассетов). В тестах:
`app.world.prefabs = Box::new(MemoryPrefabs::default().with("bullet.prefab", prefab))`.
Ошибки — `SceneError` (префаб не найден, родитель уничтожен и т. п.), с понятным текстом.

### 6.4. Мелочи на `Ctx`

`ctx.resource_or_default::<T>() -> &mut T` — глобальное состояние игры (§12), `ctx.events::<E>()`,
`ctx.random()`, `ctx.save_data()`, `ctx.time_scale()` / `set_time_scale`.
Для 2D-поворота: `transform.angle_z()` и `transform.set_angle_z(radians)`.

### 6.5. Ссылки на объекты в полях (`ObjectRef`, `ComponentRef<T>`)

Вместо `ctx.find_by_name("HUD")` (ломается, если объект переименовать) объявите поле-ссылку
и назначьте его в инспекторе — перетащите строку из Hierarchy или выберите объект кнопкой-
«мишенью». Ссылка хранит **стабильный номер** объекта, а не имя: переименование,
перестановка, сохранение и загрузка её не ломают.

```rust
use engine::prelude::*;

component! {
    pub struct Door {
        /// Любой объект сцены.
        pub lever: ObjectRef = ObjectRef::NONE,
        /// Только объект с компонентом `Tilemap` (инспектор предлагает только такие).
        pub walls: ComponentRef<Tilemap> = ComponentRef::NONE,
        /// Списки тоже можно.
        pub lamps: Vec<ObjectRef> = Vec::new(),
    }
}

impl Component for Door {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if let Some(lever) = ctx.resolve(self.lever) {          // Option<GameObjectId>
            let _on = ctx.is_active(lever);
        }
        if let Some(map) = self.walls.component(ctx.scene()) {   // Option<&Tilemap>
            let _ = map;
        }
        let _id = self.walls.get(ctx.scene());                   // объект, если на нём есть Tilemap
    }
}
```

* Назначить из кода: `ObjectRef::new(scene, id)`, `ctx.object_ref(id)`, `ComponentRef::<T>::new(scene, id)`
  (`NONE`, если у объекта нет `T`). `r.is_none()`, `r.uid()`.
* Разрешение — один поиск в таблице сцены (`scene.resolve(r)` / `scene.find_by_uid(uid)`).
  Уничтоженный объект, объект другой сцены, не назначенное поле — `None`.
* В файле сцены: у каждого объекта `id: 123…` (выдаётся при создании; в старых файлах его нет —
  номер выдаётся при загрузке и пишется при следующем сохранении), поле — `"lever": Object(123…)`,
  `Object(0)` — «не назначено». Ссылка на объект, которого нет в файле, при загрузке становится
  `None` с предупреждением в логе (какой объект, какое поле).
* **Копии.** Duplicate / Ctrl+D, вставка, `world.duplicate_many(&ids)`, `world.spawn_copies(&nodes, parent)`
  и повторная аддитивная загрузка той же сцены перенумеровывают копируемую группу: ссылки внутри
  группы у копий указывают на копии, ссылки наружу — на те же объекты.
* **Префабы.** Ссылки внутри префаба (корень → свой ребёнок) у каждого экземпляра указывают на
  *его* детей. Номера детей экземпляра выводятся из номера экземпляра (`uid ^ номер в префабе`, как
  `fileID` в Unity) — в сцену их писать не нужно. Ассет префаба не может ссылаться на объекты
  сцены: при Save as Prefab такие поля становятся `None` (в консоли — предупреждение с их списком,
  `Prefab::from_object_report` возвращает этот список). Ссылка *экземпляра* на объект сцены —
  обычное переопределение и сохраняется.
* C++: `get_field` поля-ссылки отдаёт сам объект (`ENGINE_VALUE_OBJECT`, 0 — нет), `set_field`
  принимает объект или `nil`. Scenario: `obj.get("Door.lever")` — ручка объекта или `nil`,
  `obj.set("Door.lever", other)` — назначить. Объявлять поля-ссылки можно только в Rust.

### 6.6. Переопределения в экземплярах префабов

Экземпляр хранит в сцене ссылку на префаб и только отличия от него. Отличия бывают у корня
и у любого ребёнка (путь — имена *из префаба*: `"Body/Wheel"`):

| что | где в файле сцены |
|---|---|
| поле компонента (корня или ребёнка) | `overrides: [(path: "Light", component: "Glow", field: "power", value: Float(5.0))]` |
| Transform ребёнка | `overrides: [(path: "Body", component: "Transform", field: "position", value: Vec3(..))]` |
| активность / имя / тег / слой ребёнка | `overrides: [(path: "Body", component: "GameObject", field: "active", value: Bool(false))]` (`name` — `Str`, `layer` — `Int`) |
| компонент добавлен / удалён у корня | `components: [..]` / `removed: ["Rigidbody"]` |
| компонент добавлен / удалён у ребёнка | `child_components: [(path: "Body/Tip", added: [(kind: "Health")], removed: ["Marker"])]` |
| имя экземпляра | `name` узла; пустое — имя корня префаба |

Сохраняет всё это редактор сам (сравнивает экземпляр с префабом); правка префаба доходит до
полей, которые в экземпляре не переопределены. Переименованный ребёнок экземпляра остаётся тем же
объектом (не превращается в «добавленного»). `world.instantiate("…")` называет экземпляр именем
корня префаба (раньше — именем файла). Что переопределено — `serialize::instance_overrides(..)`;
каким объект был в префабе — `serialize::prefab_source_node(..)`.

### 6.7. Файлы данных (`.ron`, `.json`)

Таблицы уровней, баланса, волн — в `assets/data/`, читаются через базу ассетов (из каталога
проекта и из пака экспортированной игры одинаково):

```rust
#[derive(serde::Deserialize)]
struct LevelInfo { name: String, need: u32 }

let db = ctx.resource::<AssetDatabase>().expect("assets are mounted");
let levels: Vec<LevelInfo> = db.load_data("data/levels.ron")?;   // или .json
```

Формат — по расширению. Ошибка (`AssetError::Import`) называет файл, строку и столбец. Каждый вызов
читает файл заново (кэша и слежения за файлом нет — перечитайте, когда нужно). Разобрать
готовый текст: `engine::assets::parse_data::<T>("data/x.ron", &text)`.

---

## 7. Сохранения

Аналог `PlayerPrefs`: ключ → значение в RON-файле.

```rust
fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    // Читает `<данные ОС>/MyGame/save.ron`: %APPDATA% (Windows),
    // ~/Library/Application Support (macOS), $XDG_DATA_HOME или ~/.local/share (Linux).
    app.world.resources.insert(SaveData::open("MyGame"));
    // …
}

// В компоненте:
let save = ctx.save_data();
let best = save.int("best_score", 0);               // нет ключа → значение по умолчанию
if score > best {
    save.set_int("best_score", score);
    save.save().ok();                                // на диск — ТОЛЬКО явно
}
```

| | |
|---|---|
| `SaveData::open(game)` | файл в каталоге данных игры, сразу читается |
| `SaveData::with_backend(b)` | любой `SaveBackend` (веб: `localStorage`) |
| `SaveData::in_memory()` | без диска (тесты) |
| `bool(key, default)`, `int`, `float`, `string`, `vec2`, `vec3` | чтение с значением по умолчанию |
| `set_bool(key, v)`, `set_int`, `set_float`, `set_string`, `set_vec2`, `set_vec3` | запись в памяти |
| `get::<T>(key)` / `set(key, &v)` | любой тип поля компонента: `Color`, `Quat`, `Vec<T>`… |
| `has_key`, `keys()`, `delete_key`, `delete_all`, `len`, `is_dirty` | |
| `save() -> Result<(), SaveError>` | записать (файл — атомарно: временный файл + переименование) |
| `reload()` | выбросить несохранённое и прочитать заново |

* Значение другого типа читается как «нет значения» — вернётся значение по умолчанию.
* **Повреждённый файл** не роняет игру: предупреждение в логе, значения по умолчанию, а нечитаемый файл
  остаётся рядом как `save.ron.corrupt`. Следующий `save()` перезапишет `save.ron`.
* Если `SaveData` не подключён, `ctx.save_data()` создаёт набор в памяти и пишет предупреждение —
  данные не переживут выход. Всегда вставляйте `SaveData::open(..)` при старте.
* Автосохранения нет: `save()` зовите на контрольных точках, при выходе в меню, по таймеру.
* Свой бэкенд: `impl SaveBackend { fn read(&mut self) -> Result<Option<String>, SaveError>; fn write(&mut self, text: &str) -> Result<(), SaveError>; }`.
  Для веба — `localStorage`; игровой код не меняется.
* Сохранение переназначенных клавиш: `save.set_string("bind.jump", &binding.to_text())`, обратно —
  `Binding::parse(&save.string("bind.jump", ""))` (§10.4).

---

## 8. Случайные числа

`Random` — детерминированный генератор (xoshiro256++, без внешних зависимостей), ресурс мира:

```rust
let r = ctx.random();                      // &mut Random
let x = r.range(-1.0, 1.0);                // f32 в [-1, 1)
let die = r.int_range_inclusive(1, 6);     // 1..=6;  int_range(0, n) — как Unity: n НЕ входит
if r.chance(0.25) { /* 25% */ }
let enemy = r.pick(&enemies);              // Option<&T>
r.shuffle(&mut deck);                      // тасует на месте
let spread = r.in_unit_circle() * 2.0;     // Vec2 внутри круга; on_unit_circle — на окружности
let dir = r.on_unit_sphere();              // Vec3; in_unit_sphere — внутри шара
let hp = r.gaussian(100.0, 15.0);          // нормальное распределение
let i = r.weighted_index(&[5.0, 1.0, 1.0]);// Option<usize> по весам
let c = r.color_hsv(0.8, 1.0);             // случайный оттенок
```

Остальное: `f32()`, `f64()`, `coin_flip()`, `sign()`, `next_u32()`, `next_u64()`, `color()`.

**Зерно.** По умолчанию фиксированное (`DEFAULT_SEED`) — игра и тест проходят одинаково. Для «настоящей» случайности:

```rust
app.world.resources.insert(Random::from_entropy());   // при старте
app.world.random().seed(12345);                       // воспроизводимый прогон / «сид дня»
```

Одно зерно даёт одну последовательность на любой платформе. Отдельный генератор для подсистемы
(например, уровня): `let mut level_rng = Random::new(seed);`.

---

## 9. Математика

Свободные функции и типы из `prelude` (`use engine::prelude::*`):

| | |
|---|---|
| `lerp(a, b, t)` | без ограничения `t` (Unity `LerpUnclamped`); для вектора — `a.lerp(b, t)` из glam |
| `inverse_lerp(a, b, v)` | где `v` между `a` и `b` (0, если `a == b`) |
| `remap(v, from_min, from_max, to_min, to_max)` | |
| `smoothstep(t)`, `smootherstep(t)` | 0..1 → 0..1, ограничивает `t` |
| `move_towards(cur, target, max_delta)`, `move_towards_vec2/_vec3` | шаг без перелёта |
| `smooth_damp(cur, target, &mut vel, smooth_time, dt)`, `smooth_damp_vec2/_vec3` | пружина без перелёта; `vel` хранит вызывающий |
| `exp_decay(cur, target, decay, dt)` | сглаживание, не зависящее от fps: `x = exp_decay(x, goal, 8.0, dt)` |
| `repeat(t, len)`, `ping_pong(t, len)`, `approx_eq(a, b)` | |
| `wrap_angle(a)`, `delta_angle(from, to)`, `lerp_angle(a, b, t)`, `move_towards_angle(..)` | **радианы**, кратчайшая дуга |
| `delta_angle_deg(..)`, `lerp_angle_deg(..)` | то же в градусах |
| `angle_of(Vec2)`, `from_angle(a)` | `atan2` и обратно |

```rust
// Камера плавно ползёт к цели:
let mut vel = Vec3::ZERO; // поле компонента
let pos = smooth_damp_vec3(ctx.transform().position, target, &mut self.vel, 0.25, dt);
ctx.transform().position = pos;

// Поворот спрайта к цели по кратчайшей дуге, не быстрее 360°/с:
let want = angle_of(to_target);
let a = move_towards_angle(ctx.transform().angle_z(), want, 360f32.to_radians() * dt);
ctx.transform().set_angle_z(a);
```

### `Rect`

Прямоугольник по осям: левый нижний угол + размер (Y вверх). Поле компонента допустимо (в сцене — список из 4 чисел).

```rust
let r = Rect::new(0.0, 0.0, 4.0, 2.0);       // x, y, width, height
Rect::from_min_max(min, max); Rect::from_center_size(center, size);
r.min(); r.max(); r.center(); r.size(); r.area();
r.contains(p);                 // границы включены
r.overlaps(&o);                // касание сторонами — не пересечение
r.intersection(&o) -> Option<Rect>; r.union(&o); r.expand(1.0); r.translate(d);
r.clamp_point(p); r.point_at(Vec2::splat(0.5)); r.normalized_point(p);
```

### `Color`

`Color` хранится **в линейном пространстве**; всё «дизайнерское» (hex, HSV) переводится из sRGB.

```rust
Color::hex(0xff7a59)              // как в графическом редакторе
Color::hex_rgba(0xff7a5980)       // с альфой
Color::parse_hex("#ff7a59")       // Option<Color>: "#rgb", "#rrggbb", "#rrggbbaa", решётка необязательна
Color::srgb(1.0, 0.5, 0.2)
Color::from_hsv(h, s, v)          // всё 0..1, h циклический; c.to_hsv() -> (h, s, v)
a.lerp(b, t)                      // все 4 канала, в линейном пространстве
c.with_alpha(0.5); c.scaled(2.0)  // scaled: умножает RGB (HDR-яркость), альфа не трогается
c.multiply(tint); c.luminance(); c.to_hex_string()   // "#ff7a59"
Color::RED / GREEN / BLUE / YELLOW / CYAN / MAGENTA / WHITE / BLACK / TRANSPARENT
```

---

## 10. Ввод

Игровой код читает **действия** (именованные кнопки и оси), а не клавиши. К действию привязаны
источники: клавиша, кнопка мыши, кнопка геймпада, стик/триггер. Прежние вызовы работают как раньше
и теперь реагируют и на геймпад:

```rust
let run = ctx.input().axis("Horizontal");        // −1..1: A/D, ←/→, крестовина, левый стик X
let up  = ctx.input().axis("Vertical");          // S/W, ↓/↑, крестовина, левый стик Y (вверх — плюс)
if ctx.input().button_down("Jump")  { }          // Space или South (A / ✕)
if ctx.input().button("Fire1")      { }          // LCtrl или West (X / □)
// "Submit": Enter, South;  "Cancel": Escape, East
```

### 10.1. Чтение

| | |
|---|---|
| `axis(name) -> f32` | цифровая часть (−1/0/+1) + аналоговая (с мёртвой зоной), в пределах −1..1 |
| `button(name)` / `button_down(name)` / `button_up(name)` | удерживается / нажато / отпущено |
| `button_value(name) -> f32` | 0..1: у цифровых 0 или 1, у триггера — степень нажатия |
| `key(k)`, `key_down(k)`, `key_up(k)` | клавиши напрямую |
| `mouse_button(b)`, `mouse_button_down(b)`, `mouse_button_up(b)`, `mouse_position()`, `mouse_delta()`, `scroll_delta()` | мышь |
| `touches()`, `text_input()`, `screen_size()` | |

Неизвестное имя действия возвращает 0/`false` и пишет предупреждение в лог.

### 10.2. Геймпад

Бэкенд (`GamepadSubsystem`, подключён в `engine::app()`) опрашивает `gilrs` и пишет состояние в `Input`.
Нет геймпада или платформа не поддерживается — подсистема молча ничего не делает.

```rust
let input = ctx.input();
input.gamepads();                                    // Vec<GamepadId>
input.gamepad_count(); input.is_gamepad_connected(id); input.gamepad_name(id);
input.gamepad_button(id, GamepadButton::South);      // + gamepad_button_down / _up
input.gamepad_axis(id, GamepadAxis::LeftStickX);     // сырое значение, без мёртвой зоны
input.gamepad_left_stick(id, 0.2) -> Vec2;           // с круговой мёртвой зоной, длина ≤ 1
input.gamepad_right_stick(id, 0.2) -> Vec2;
input.any_gamepad_button(GamepadButton::Start);      // на любом из геймпадов
input.any_gamepad_axis(GamepadAxis::RightTrigger);   // ось с наибольшим отклонением
```

`GamepadButton`: `South East West North` (A B X Y / ✕ ○ □ △), `LeftBumper RightBumper`,
`LeftTrigger RightTrigger` (как кнопки), `Select Start Mode`, `LeftStick RightStick` (нажатие),
`DPadUp DPadDown DPadLeft DPadRight`.
`GamepadAxis`: `LeftStickX LeftStickY RightStickX RightStickY` (−1..1, вверх и вправо — плюс),
`LeftTrigger RightTrigger` (0..1).

Для тестов состояние пишется вручную (подключает геймпад само):

```rust
app.world.input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickX, 1.0);
app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::South, true);
app.world.input.connect_gamepad(GamepadId(1), "Test Pad");
app.world.input.disconnect_gamepad(GamepadId(1));   // отпускает его кнопки
```

### 10.3. Привязки и переназначение

Действия по умолчанию можно задать без кода — раздел `input` файла `project.settings`
(Edit → Project Settings → Input). `engine::mount_assets` записывает их в `Input`; привязки,
которые код делает **после** `mount_assets`, побеждают («код побеждает»), действия, которых
в настройках нет, остаются от кода. Подробно — `docs/api/settings.md`.

`Binding` — источник сигнала:

```rust
Binding::Key(Key::Space)
Binding::Mouse(MouseButton::Left)
Binding::Pad(GamepadButton::South)                                  // любой геймпад
Binding::PadAxis { axis: GamepadAxis::LeftTrigger, positive: true } // стик/триггер как кнопка
// Key, MouseButton, GamepadButton приводятся в Binding через `.into()`
```

Ось `PadAxis` «нажата», когда отклонение ≥ порога (0.5; `input.set_axis_press_threshold(t)`),
«отпущена» ниже 70% порога — дрожание стика не даёт серий нажатий.

**Кнопки:**

```rust
input.set_button_bindings("Dash", vec![Key::LShift.into(), GamepadButton::RightBumper.into()]);
input.add_button_binding("Dash", Binding::Mouse(MouseButton::Right));
input.remove_button_binding("Dash", Binding::Mouse(MouseButton::Right));   // -> bool
input.rebind_button("Jump", Binding::Key(Key::Space), Binding::Key(Key::F)); // заменить на месте
input.button_bindings("Jump") -> &[Binding];
```

**Оси:** значение = (любая «плюс»-привязка − любая «минус»-привязка) + аналоговые оси, всё в −1..1.

```rust
input.set_axis_bindings("Steer",
    AxisBindings::pair(Key::A, Key::D)          // минус / плюс
        .also(Key::Left, Key::Right)            // ещё одна пара
        .also(GamepadButton::DPadLeft, GamepadButton::DPadRight)
        .with_analog(GamepadAxis::LeftStickX)   // аналоговая ось
        .with_dead_zone(0.15));                 // мёртвая зона аналоговых осей (по умолчанию 0.2)

let axis = input.axis_bindings_mut("Steer");   // правка на месте: axis.negative / positive / analog / dead_zone
axis.analog.push(AnalogBinding::inverted(GamepadAxis::RightStickX));
```

**Мёртвая зона:** `|v| ≤ dead_zone` → 0, дальше значение растягивается на весь диапазон
(при зоне 0.2 отклонение 0.6 даёт 0.5, а не скачок с 0 до 0.6). `apply_dead_zone(v, zone)` доступна отдельно.

**Старый API** `input.bind_axis(name, &[neg keys], &[pos keys])` и `input.bind_button(name, &[keys])`
заменяют только **клавиатурные** привязки; геймпад/мышь, если они были, остаются.
Полная замена — `set_axis_bindings` / `set_button_bindings`. Удалить действие — `input.remove_action(name)`.
Список: `button_names()`, `axis_names()`, `has_button(n)`, `has_axis(n)`.

### 10.4. Экран «нажмите новую кнопку»

```rust
// Пока ждём нажатия на экране переназначения (каждый кадр):
if let Some(binding) = ctx.input().last_pressed_binding() {
    if binding != Binding::Key(Key::Escape) {                // Escape — отмена
        ctx.world.input.rebind_button("Jump", old, binding);
        let text = binding.to_text();                        // "Key:F", "Pad:South", "PadAxis:LeftStickX-"
        ctx.save_data().set_string("bind.jump", &text);      // и в сохранение
    }
}
// При старте: Binding::parse("Key:F") -> Option<Binding>
```

`last_pressed_binding()` возвращает первый источник, нажатый **в этом кадре**: клавишу, кнопку мыши,
кнопку геймпада или стик/триггер, отклонённый за порог.

### 10.5. Ввод и фиксированный шаг

«Нажато в этом кадре» внутри `on_fixed_update` означает «нажато с прошлого фиксированного шага».
Так короткое нажатие не теряется, даже если в кадре не было шага физики, и не засчитывается дважды.
Это верно для **всех** источников: клавиши, мышь (включая `mouse_button_up`), кнопки геймпада,
оси-как-кнопки, `button_down` / `button_up`, `gamepad_button_down` / `_up`.
Поэтому весь ввод для персонажа можно читать прямо в `on_fixed_update` (как в примере платформера).

Бэкенды вызывают `begin_fixed_step` / `end_fixed_step` и `end_frame` сами; в тестах это делает `app.tick`.

---

## 11. Конечный автомат

`StateMachine<S>` — крошечный помощник для состояний врага, босса, меню, всей игры. Переход
запрашивается `change`, применяется в `tick`; поэтому весь код кадра видит одно состояние,
а вход и выход ловятся ровно один раз.

```rust
reflect_enum! { pub enum Mode { Patrol, Chase, Attack } }

component! {
    pub struct Guard { pub mode: StateMachine<Mode> = StateMachine::new(Mode::Patrol) }
}
impl Component for Guard {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.mode.tick(dt);                              // применить запрошенный переход
        if self.mode.just_entered(Mode::Chase) { /* крик «Стой!» */ }
        match self.mode.current() {
            Mode::Patrol => if see_player(ctx) { self.mode.change(Mode::Chase) },
            Mode::Chase if self.mode.time_in_state() > 5.0 => self.mode.change(Mode::Patrol),
            _ => {}
        }
    }
}
# fn see_player(_: &mut Ctx) -> bool { false }
```

`new(s)`, `current()`, `is(s)`, `previous()`, `change(s)`, `tick(dt)`, `time_in_state()`,
`just_entered(s)`, `just_exited(s)`, `has_pending()`. Состояние — любой `Copy + PartialEq` тип;
для поля компонента — `reflect_enum!` (в сцене автомат хранит только текущее состояние).

---

## 12. Рецепты

### Глобальное состояние игры

Синглтон в `resources`: счёт, жизни, настройки. Доступен из любого скрипта и из тестов.

```rust
#[derive(Default)]
pub struct GameState { pub score: i32, pub lives: i32 }

// При старте (необязательно): app.world.resources.insert(GameState { score: 0, lives: 3 });

// В скрипте — создаётся значением по умолчанию при первом обращении:
let state = ctx.resource_or_default::<GameState>();
state.score += 10;

// Только читать (None, если ещё не создан):
let lives = ctx.resource::<GameState>().map_or(0, |s| s.lives);
```

Для общения между скриптами без прямых ссылок — события: `ctx.emit(PlayerDied)`, читать в следующем
кадре `for e in ctx.events::<PlayerDied>() { .. }`.

### Респаун с задержкой

```rust
fn on_message(&mut self, ctx: &mut Ctx, m: &Message) {
    if m.name == "Died" {
        let me = ctx.id;
        ctx.set_active(me, false);
        // Таймер БЕЗ владельца: `ctx.after` / `ctx.call_after` привязаны к объекту и стоят,
        // пока он неактивен, — выключенный объект сам себя не разбудит.
        ctx.world.scheduler().call_after(2.0, move |world| {
            world.scene.set_active(me, true);
        });
    }
}
```

### Всплывающая цифра урона

```rust
let label = ctx.instantiate_at("prefabs/damage_text.prefab", pos)?;
ctx.start_tween(Tween::position(label, pos + Vec3::Y * 1.5).duration(0.6).ease(Ease::OutQuad));
ctx.start_tween(Tween::field(label, "SpriteRenderer.color.a", 0.0f32).duration(0.6).delay(0.2));
ctx.destroy_after(label, 0.8);
```

### Экран паузы

```rust
if ctx.input().button_down("Cancel") {
    let paused = ctx.time_scale() == 0.0;
    ctx.set_time_scale(if paused { 1.0 } else { 0.0 });
    ctx.set_active(menu, !paused);
}
// Анимация меню на паузе: Tween::..(..).unscaled(), таймеры — world.scheduler().unscaled(id)
```

### Тест геймплея

```rust
let mut app = engine::app();
app.register::<Player>();
app.world.scenes().set_source(MemoryScenes::default().with("level2", LEVEL2));
app.world.random().seed(1);
app.world.input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickX, 1.0);
app.run_headless(120, 1.0 / 60.0);
assert!(app.world.scene.find_by_name("Player").is_some());
```

---

## 13. Для редактора и встраивания

* `world.reset_gameplay_state()` — снять все таймеры, твины и отложенные запросы сцены и вернуть
  `time_scale = 1`. Редактор зовёт это при выходе из Play (иначе таймеры без владельца и пауза пережили бы Stop).
* `world.run_scheduled()` — шаг таймеров и твинов; `App::tick` вызывает его сам. Нужен только тому, кто гоняет
  `world.update(dt)` в обход `App`.
* Всё состояние геймплея — обычные ресурсы (`Scheduler`, `Tweens`, `SceneManager`, `Random`, `SaveData`);
  их можно подменить или вынуть через `world.resources`.
* Новый компонент на Rust удобнее всего создать в редакторе: Assets → **New Rust Script…** пишет файл
  из шаблона в `src/scripts/`, регистрирует его в `src/scripts/mod.rs`, а Build пересобирает игру и
  перезапускает редактор с новым кодом — сцена и выделение остаются (`editor.md`, «Скрипты на Rust»).
  Компонент из сцены, которого больше нет в коде, редактор хранит как «missing script» и сохраняет
  нетронутым (`engine_core::missing`).

---

## 14. Чего здесь нет

* **Корутин** — их заменяют таймеры (`ctx.after`), твины и `StateMachine`.
* **Пула объектов.** Без хуков `on_enable` / `on_disable` пул не может сбросить состояние объекта сам;
  делайте его на `set_active` + собственный метод `reset()` в компоненте.
* **Геймпады по номеру в действиях.** Привязка `Binding::Pad` реагирует на *любой* геймпад; для кооператива
  на одном экране читайте `input.gamepad_button(id, ..)` / `gamepad_axis(id, ..)` конкретного геймпада напрямую.
* **Вибрации геймпада** и сенсорных осей (гироскоп).
* **Автосохранения** и облачных сохранений — только явный `SaveData::save()`.
* **Кривых Безье и пользовательских кривых** в `Ease` — набор фиксированный (29 кривых).
