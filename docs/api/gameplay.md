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
