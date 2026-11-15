# Игровая логика на C++

Компоненты в этом движке пишутся на Rust **или на C++** (сценарии поверх них — на языке Scenario,
см. `scenario.md`). C++-компонент
ничем не хуже Rust-компонента: он вешается на объект, виден в инспекторе, сохраняется в сцену,
получает те же хуки (`on_start`, `on_update`, столкновения, сообщения…) и может сделать почти всё,
что делает Rust-скрипт: двигать объекты, создавать префабы, пускать лучи, играть звук, менять
интерфейс, сохранять прогресс. Целая игра на одном C++ — `crates/engine/examples/cpp_game.rs`
(Rust там только собирает сцену).

Всё лежит в одном заголовке `crates/engine-cpp-bridge/include/engine_component.hpp` (он подключает
`engine_api.h` — C ABI, который генерирует `cbindgen`; руками его не правят). Стандарт — C++17,
исключения и RTTI включены, зависимостей кроме стандартной библиотеки нет. Заголовок собирается без
предупреждений на `g++ -Wall -Wextra` и MSVC `/W4`.

* [1. Первый скрипт](#1-первый-скрипт)
* [2. Сборка: встроенный плагин, DLL, горячая перезагрузка](#2-сборка-встроенный-плагин-dll-горячая-перезагрузка)
* [3. Компонент: хуки и поля](#3-компонент-хуки-и-поля)
* [4. Объекты и иерархия](#4-объекты-и-иерархия)
* [5. Поля чужих компонентов и `Value`](#5-поля-чужих-компонентов-и-value)
* [6. Время](#6-время)
* [7. Ввод](#7-ввод)
* [8. Сообщения, таймеры, твины, сцены](#8-сообщения-таймеры-твины-сцены)
* [9. Физика](#9-физика)
* [10. Звук](#10-звук)
* [11. Интерфейс](#11-интерфейс)
* [12. Сохранения, случайные числа, лог](#12-сохранения-случайные-числа-лог)
* [13. Свои сервисы из Rust](#13-свои-сервисы-из-rust)
* [14. Безопасность и правила вызова](#14-безопасность-и-правила-вызова)
* [15. Соответствие Rust API ↔ C++ API](#15-соответствие-rust-api--c-api)
* [16. Чего из C++ нет и почему](#16-чего-из-c-нет-и-почему)
* [17. C++ в своей игре](#17-c-в-своей-игре)

---

## 1. Первый скрипт

Каждый `.cpp` в `plugins/cpp-gameplay/scripts/` — отдельный скрипт; других списков править не нужно.

```cpp
// plugins/cpp-gameplay/scripts/Spinner2.cpp
#include "engine_component.hpp"

struct Spinner2 : engine::Component {
    float speed_deg = 90.0f;                       // значение по умолчанию

    void on_update(engine::Object self, float dt) override {
        auto r = self.rotation();                  // углы Эйлера, градусы
        self.set_rotation({r.x, r.y + speed_deg * dt, r.z});
    }
};
ENGINE_FIELDS(Spinner2, ENGINE_FIELD(Spinner2, speed_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Spinner2);
```

* `ENGINE_FIELDS` перечисляет поля, видимые движку: они в инспекторе и в файле сцены.
  Члены, не перечисленные там, — обычные приватные данные скрипта: они не сохраняются и
  сбрасываются при каждой перезагрузке плагина.
* `ENGINE_COMPONENT(Тип)` регистрирует класс под его именем. Имя — то, что видно в
  **Add Component** и в `kind:` файла сцены.

**Из редактора:** выделите объект → **Add Component → New C++ Script…** → введите имя
(`EnemyPatrol`: латиница, цифры, `_`, с заглавной). Редактор создаёт `scripts/EnemyPatrol.cpp` из
шаблона, открывает его во вкладке Code, запускает сборку (`cmake --build`), после сборки плагин
перезагружается и скрипт сам вешается на объект. Это работает, когда редактор запущен с
`EditorConfig::default().cpp_scripts("plugins/cpp-gameplay/scripts")` и командой сборки CMake
(см. `docs/api/editor.md`).

---

## 2. Сборка: встроенный плагин, DLL, горячая перезагрузка

Один и тот же код компилируется двумя способами.

**Встроенный (статический) плагин.** `build.rs` крейта `engine-cpp-bridge` собирает все файлы
`plugins/cpp-gameplay` одной единицей трансляции и линкует в программу. CMake не нужен, горячей
перезагрузки нет. Так работают тесты и `cargo run --example cpp_game` без собранной DLL:

```rust
let mut plugins = engine::cpp::CppPlugins::default();
engine::cpp::builtin_gameplay(&mut plugins)?;
plugins.register_components(&mut app.world.registry);
app.add_subsystem(plugins);
```

**Динамическая библиотека с горячей перезагрузкой.**

```bash
cmake -S plugins/cpp-gameplay -B build -G Ninja      # один раз (или любой другой генератор)
cmake --build build                                   # после каждой правки
cargo run --example cpp_game                          # подхватит plugins/cpp-gameplay/bin/piska_gameplay.dll
```

Игра грузит копию библиотеки (`engine::add_cpp_plugins(&mut app, [path])`) и раз в полсекунды
смотрит, не изменился ли оригинал. Пересобрали — `cmake --build` положил новую DLL — движок
уничтожает все C++-экземпляры старой версии, **запоминая значения полей из `ENGINE_FIELDS`**,
выгружает её, загружает новую и создаёт экземпляры заново с теми же значениями. Игра не
закрывается, сцена не перезагружается. Не сохраняются только члены, не попавшие в `ENGINE_FIELDS`
(например, запомненные `engine::Object` и таймеры — после перезагрузки заведите их в `on_start`).
Если новая версия не загрузилась (файл ещё пишется, ошибка), работает прежняя.
`ENGINE_CPP_BUILD_DIR` указывает каталог сборки для тестов редактора.

**Версии.** ABI плагина (`ENGINE_ABI_VERSION`, сейчас 3) должен совпадать с ABI движка. Плагин,
собранный по старым заголовкам, не загрузится — движок скажет:
*«plugin … was built for plugin ABI v2, but this engine uses v3. Rebuild the plugin against the
current include/engine_api.h»*. Пересоберите плагин теми же заголовками, что у движка.

Свои плагины: скопируйте `plugins/cpp-gameplay/CMakeLists.txt` — нужен лишь путь к заголовкам
(`ENGINE_INCLUDE_DIR`), `-Wno-invalid-offsetof` больше не требуется.

---

## 3. Компонент: хуки и поля

```cpp
struct Component {
    virtual void on_start(Object self);                          // один раз, перед первым обновлением
    virtual void on_update(Object self, float dt);               // каждый кадр
    virtual void on_fixed_update(Object self, float dt);         // фиксированный шаг, перед физикой
    virtual void on_late_update(Object self, float dt);          // после всех on_update
    virtual void on_collision_enter(Object self, Object other);  // 2D и 3D
    virtual void on_trigger_enter(Object self, Object other);    // 2D и 3D
    virtual void on_destroy(Object self);                        // в конце кадра, где вызван destroy
    virtual void on_message(Object self, const char* name, const Value& value);
};
```

Порядок кадра тот же, что у Rust (`docs/api/gameplay.md`, §1). `dt` — уже с учётом `time_scale`.

**`on_message`** получает: `send_message` (и `send_after` / `send_every`), конец твина (`on_complete`),
действия виджетов интерфейса — `Button` присылает `Nil`, `Toggle` — `Bool`, `Slider` — `Float`,
`InputField` — `Str` (и `"<message>:submit"` по Enter); сообщения кнопок приходят и родителям.

Движок вызывает только те хуки, которые класс **переопределил**: для остальных `component<T>()`
отдаёт пустой указатель (определяется при компиляции по типу `&T::on_update`), поэтому
компонент с одним `on_update` не стоит ничего в других фазах кадра. Плагин, собранный со старым
заголовком, по-прежнему получает все хуки.

Хуки столкновений вызываются движком одинаково для 2D и 3D физики. Хуков «выход из
столкновения/триггера» в движке нет (см. §16).

### Поля

```cpp
ENGINE_FIELDS(Guard,
    ENGINE_FIELD(Guard, speed, ENGINE_FIELD_F32),          // float
    ENGINE_FIELD(Guard, hp, ENGINE_FIELD_I32),             // std::int32_t
    ENGINE_FIELD(Guard, alert, ENGINE_FIELD_BOOL),         // bool
    ENGINE_FIELD(Guard, patrol, ENGINE_FIELD_VEC2),        // engine::Vec2
    ENGINE_FIELD(Guard, home, ENGINE_FIELD_VEC3),          // engine::Vec3
    ENGINE_FIELD(Guard, tint, ENGINE_FIELD_COLOR),         // engine::Color (линейный RGBA)
    ENGINE_FIELD(Guard, facing, ENGINE_FIELD_QUAT),        // engine::Quat
    ENGINE_FIELD(Guard, label, ENGINE_FIELD_STRING),       // std::string
    ENGINE_ENUM_FIELD(Guard, mode, "Patrol", "Chase", "Attack"),   // enum class Mode : int { Patrol, Chase, Attack }
    ENGINE_ASSET_FIELD(Guard, icon, "Texture"));           // std::string с GUID ассета
```

Члены должны быть публичными (`struct`). Перечисление — любой `enum`/`enum class` со значениями
0, 1, 2… по порядку имён; в инспекторе — выпадающий список. Из Rust и из файла сцены поле
читается и пишется как `Value` (`Str("hello")`, `Enum("Chase")`, `Asset("<guid>")`…).
Несовпавший по типу `Value` отвергается тем же сообщением, что у Rust-компонентов:
`field 'Guard.hp' expects Int (32 bit), but got Str`.

---

## 4. Объекты и иерархия

`engine::Object` — лёгкий копируемый хэндл (индекс + поколение). Храните его в полях класса
сколько угодно, но перед использованием проверяйте `alive()`.

```cpp
engine::Object player = engine::find_by_tag("Player");
if (!player) return;                       // «не нашли» — это нулевой хэндл
if (player.alive()) { ... }
```

| Что | C++ |
|---|---|
| Поиск | `find_by_name(n)`, `find_by_tag(t)`, `find_all_by_tag(t) → vector<Object>`, `find_with_component("Light") → vector<Object>`, `obj.find_child(name)` |
| Создание | `spawn(name)` (пустой объект), `instantiate(prefab)`, `instantiate(prefab, pos, euler_deg, parent)`, `instantiate_under(prefab, parent)`, `obj.duplicate()` |
| Уничтожение | `obj.destroy()` (в конце кадра, с детьми), `obj.destroy_after(sec) → Timer`, `obj.dont_destroy_on_load()` |
| Иерархия | `parent()`, `set_parent(p, keep_world=false)`, `child_count()`, `child(i)`, `children()` |
| Состояние | `set_active(b)`, `active()`, `active_in_hierarchy()`, `name()`, `set_name`, `tag()`, `set_tag`, `layer()`, `set_layer` |
| Transform | `position()`/`set_position` (локальная), `world_position()`/`set_world_position`, `rotation()`/`set_rotation` (локальные углы Эйлера в градусах), `world_rotation()`/`set_world_rotation`, `scale()`/`set_scale`, `right()`, `up()`, `forward()` (мировые; вперёд = −Z), `look_at(точка, вверх)`, `translate(дельта)` |
| Компоненты | `add_component("Rigidbody2D")`, `remove_component(t)`, `has_component(t)` |

Путь префаба — тот же, что в Rust (`instantiate("prefabs/bullet.prefab")`, относительно каталога
ассетов, или GUID). Без `parent` поза мировая, с `parent` — локальная. Не нашёлся префаб — ошибка
в логе и нулевой хэндл.

```cpp
void shoot(engine::Object self) {
    const engine::Object bullet = engine::instantiate("prefabs/bullet.prefab", self.world_position(), self.world_rotation());
    if (!bullet) return;
    bullet.destroy_after(2.0f);
    engine::physics::set_velocity(bullet, self.forward() * 20.0f);
}
```

`Vec3`, `Vec2` — обычные структуры с арифметикой (`+ - * /`, `length()`, `normalized()`, `dot`, `cross`,
`distance`, `Vec3::lerp`). `Color` — линейный RGBA, `Quat` — (x, y, z, w).

---

## 5. Поля чужих компонентов и `Value`

Любое поле любого компонента (на Rust или C++) читается и пишется по именам — как в инспекторе:

```cpp
engine::Object lamp = engine::find_by_name("Lamp");
float intensity;
if (lamp.get("Light", "intensity", intensity)) lamp.set("Light", "intensity", intensity * 0.5f);

lamp.set("SpriteRenderer", "color", engine::Color{1, 0, 0, 1});
label.set("Text", "text", "Счёт: 5");                       // UTF-8
label.set("Text", "align", engine::Value::enum_name("Center"));
sprite.set("SpriteRenderer", "texture", engine::Value::asset("3f2a…"));   // GUID

engine::Value v = lamp.get("Light", "kind");                // любое значение, Nil если нет
```

Типизированное `get(component, field, T& out)` есть для `float`, `int`, `bool`, `Vec2`, `Vec3`, `Color`,
`Quat`, `std::string`. Число приводится к типу поля (`1` в `float`-поле, `2.0` в `int`-поле → 2,
строка `"Center"` в поле-перечисление). Нет компонента, нет поля или тип не подходит —
`false` и понятное сообщение в логе (`... has no field 'x' (available fields: ...)`).

`engine::Value` — динамическое значение (`Nil, Bool, Int, Float, Str, Vec2, Vec3, Quat, Color,
Enum, Asset, Object`) с неявными конструкторами и `to_float()`, `to_int()`, `to_bool()`,
`to_string()`, `to_vec3()`, `to_object()`… Оно же — аргумент сообщений, ключ сохранений, аргумент сервисов.

Нюанс: на время хука сам компонент «вынут» из объекта (как в Rust), поэтому собственные поля
читайте напрямую (`speed`), а не через `self.get("MyType", "speed", …)`; `has_component` по имени
собственного типа внутри его хука даёт `false`.

---

## 6. Время

```cpp
float dt   = engine::delta_time();          // секунд за кадр с учётом time_scale (то же, что dt в хуке)
float real = engine::unscaled_delta_time(); // без time_scale — для интерфейса на паузе
float t    = engine::time_seconds();        // игровое время с запуска (double: elapsed_time())
double r   = engine::unscaled_elapsed_time();
uint64_t f = engine::frame_count();
float fixed = engine::fixed_delta_time();
engine::set_time_scale(0.0f);               // пауза; 1 — норма; действует со следующего кадра
float s = engine::time_scale();
EngineTime all = engine::time_info();       // всё сразу
```

---

## 7. Ввод

Семантика фиксированного шага такая же, как в Rust: внутри `on_fixed_update` «нажато» значит
«нажато с прошлого фиксированного шага» — читайте ввод персонажа прямо там, нажатие не потеряется.
Это верно для клавиш, мыши, кнопок и осей-как-кнопок, геймпада.

```cpp
using namespace engine;
float run = input::axis("Horizontal");            // действия: -1..1 (A/D, стрелки, стик)
if (input::button_down("Jump")) { ... }            // button / button_down / button_up / button_value
if (input::pressed("Jump")) { ... }                // короткие имена, как в Rust: pressed / held / released
Vec2 move = input::axis2("Horizontal", "Vertical"); // две оси, длина ≤ 1; engine::dt() — как ctx.dt()
if (input::key_down("Space") || input::key("LShift")) { ... }   // клавиши по имени
if (input::mouse_button_down(0)) { Vec2 p = input::mouse_position(); }
Vec2 look = input::mouse_delta(); Vec2 wheel = input::scroll_delta(); Vec2 size = input::screen_size();
if (input::gamepad_count() > 0) {
    Vec2 stick = input::left_stick(0);             // pad: номер или -1 («любой»), мёртвая зона 0.2
    bool fire = input::gamepad_button_down("South");
    float lt = input::gamepad_axis("LeftTrigger");
}
for (const input::Touch& t : input::touches()) { /* t.id, t.position, t.delta, t.phase */ }
std::string typed = input::text();                 // набранный в этом кадре текст
```

Имена клавиш — как в `Key` движка: `A`…`Z`, `Digit0`…`Digit9` (или просто `"1"`), `Left Right Up Down`,
`Space Enter Escape Tab Backspace Delete Insert Home End PageUp PageDown`, `LShift RShift LCtrl RCtrl
LAlt RAlt`, `F1`…`F12`. Кнопки геймпада: `South East West North LeftBumper RightBumper LeftTrigger
RightTrigger Select Start Mode LeftStick RightStick DPadUp DPadDown DPadLeft DPadRight`; оси:
`LeftStickX LeftStickY RightStickX RightStickY LeftTrigger RightTrigger`. Опечатка в имени — ошибка
в логе с перечнем допустимых и `false`.

Привязки действий (`Input::bind_axis`, переназначение) настраиваются на стороне Rust.

---

## 8. Сообщения, таймеры, твины, сцены

```cpp
enemy.send_message("Hit", 10.0f);                       // число
enemy.send_message("Spawned", engine::Value("wave 3")); // любое значение
engine::Timer t = self.send_after(1.5f, "Explode");     // «перезвони через 1.5 с» (сообщение самому себе)
self.send_every(0.5f, "Blink");                         // каждые 0.5 с
t.cancel();   self.cancel_timers();                      // отмена; таймеры умирают вместе с объектом
other.destroy_after(2.0f);

// Твин: плавное изменение поля. Путь — "Компонент.поле" или "Компонент.поле.канал".
engine::TweenParams p(0.3f, "OutBack");  p.yoyo = true;  p.loops = 2;
sprite.tween("SpriteRenderer.color", engine::Color{1, 0, 0, 1}, p);
sprite.tween("Transform.position.y", 4.0f);              // без параметров: 1 секунда, линейно
p.on_complete = "Done";                                  // по концу — сообщение этому же объекту
```

Правила таймеров и твинов (пауза, неактивные объекты, смерть владельца) — как в Rust
(`docs/api/gameplay.md`, §3–4). Кривые: `Linear`, `InQuad`…`InOutBounce`, `SmoothStep`.

Сцены (смена выполняется **в конце кадра**, как в Rust):

```cpp
engine::load_scene("level2");   engine::load_scene_additive("hud");   engine::unload_scene("hud");
engine::reload_scene();         std::string name = engine::scene_name();
engine::quit();                 // закрыть игру (в редакторе — выйти из Play)
```

---

## 9. Физика

Физика не часть C++-API напрямую: фасад движка регистрирует **сервисы** (§13), а
`engine::physics::` — обёртки над ними. Без физики в приложении вызовы вернут нейтральное значение
и один раз напишут в лог, что сервиса нет.

```cpp
using namespace engine;
// Лучи. `ignore` — обычно сам стреляющий; триггеры игнорируются; мир — на момент последнего шага.
physics::RaycastHit hit = physics::raycast(origin, direction, 50.0f, self);          // 3D
if (hit) log_infof("hit %s at %.2f", hit.object.name().c_str(), hit.distance);       // object, point, normal, distance
physics::RaycastHit h2 = physics::raycast2d({p.x, p.y}, {0, -1}, 1.0f, self);        // 2D

std::vector<Object> near = physics::overlap_sphere(center, 3.0f);     // 3D, до 64 объектов
std::vector<Object> near2 = physics::overlap_circle({x, y}, 2.0f);    // 2D

// Тела. Скорость — это поля Rigidbody / Rigidbody2D:
Vec3 v = physics::velocity(body);   physics::set_velocity(body, {0, 8, 0});
Vec2 v2 = physics::velocity2d(body);  physics::set_velocity2d(body, {v2.x, 11});
physics::apply_impulse(body, {0, 5, 0});                 // 3D: Δv = импульс / масса
physics::apply_impulse_at(body, impulse, world_point);   physics::apply_torque_impulse(body, torque);
physics::add_force(body, f);                             // сила на один шаг — зовите в каждом on_fixed_update
physics::radial_impulse(center, radius, speed);          // взрыв, возвращает задетые тела
float m = physics::mass(body);  bool asleep = physics::is_sleeping(body);
physics::add_velocity2d(body2d, {0, 6});                 // 2D: мгновенное Δv (импульсов у Rigidbody2D нет)
physics::set_gravity({0, -20, 0}); physics::set_gravity2d({0, -30});

// Контроллер персонажа (3D): вы задаёте скорость, движок — гравитацию и ступеньки
physics::character_move(self, {dir.x * 5, 0, dir.z * 5});
if (physics::character_grounded(self)) { ... }
```

Столкновения — хуки `on_collision_enter` / `on_trigger_enter` (оба измерения). Тип тела, форму
коллайдера, слои и т. п. настраивают поля компонентов (`self.set("Collider2D", "is_trigger", true)`).

---

## 10. Звук

Клип — имя из `Audio::add_clip` (на стороне Rust, `synth::sfx::coin()` и т. п.) или GUID ассета.

```cpp
using namespace engine;
audio::play("coin");                                // шина Sfx
audio::play("hit", 0.5f, 1.2f);                     // громкость, высота
audio::play_at("boom", self.world_position());      // пространственный звук относительно AudioListener
audio::play_ui("click");                            // шина Ui
audio::PlayParams p; p.looping = true; p.pan = -0.5f; p.bus = "Sfx";
audio::Voice engine_sound = audio::play("engine", p);
engine_sound.set_pitch(1.0f + speed * 0.1f);  engine_sound.fade_out(0.5f);
if (engine_sound.playing()) { ... }
audio::play_music("theme", 1.0f);   audio::stop_music(0.5f);
audio::set_bus_volume("Music", 0.3f);  audio::set_bus_muted("Sfx", true);
audio::pause_all(); audio::resume_all(); audio::stop_all();  int n = audio::voice_count();
audio::play_source(self);                           // компонент AudioSource этого объекта
```

`Voice` нулевой, если клип не нашёлся (подсказка — в логе): все операции над ним безвредны.

---

## 11. Интерфейс

События этого кадра (имя виджета — его поле `message`, по умолчанию имя объекта):

```cpp
if (engine::ui::clicked("pause")) { ... }
float volume; if (engine::ui::value_changed("volume", volume)) { ... }
bool on;      if (engine::ui::toggled("vsync", on)) { ... }
std::string s; if (engine::ui::submitted("chat", s)) { ... }   // Enter в поле; text_changed — на каждую правку
if (engine::input::mouse_button_down(0) && !engine::ui::pointer_over_ui()) shoot();
if (engine::ui::wants_keyboard()) return;                      // игрок печатает в поле
```

То же приходит в `on_message` (кнопки шлют сообщение виджету и всем его предкам).

Изменение виджетов — это обычные поля, сервисов не нужно:

```cpp
engine::ui::set_text("ScoreText", "Score: 12");             // по имени объекта
engine::ui::set_text(label_object, "Здоровье");
engine::ui::set_value("HealthBar", 0.4f);                   // ProgressBar или Slider
engine::ui::set_visible("PauseMenu", true);
obj.set("Image", "color", engine::Color{0, 1, 0, 1});       // любое другое поле
```

Строить виджеты из кода (`ui_button` и т. п.) умеет только Rust; из C++ их либо готовят в сцене
(RON, редактор), либо создают как префаб (`instantiate`). Пауза: `engine::set_time_scale(0)` —
интерфейс продолжает работать.

---

## 12. Сохранения, случайные числа, лог

```cpp
engine::save::set("best", 120);                    // в памяти; Value: число, строка, Vec3…
int best = engine::save::get_int("best", 0);       // get_float / get_bool / get_string / get(key) → Value
engine::save::has("best");  engine::save::erase("best");
engine::save::flush();                              // на диск ТОЛЬКО явно: автосохранения нет

engine::rng::value();                  // [0, 1)
engine::rng::range(-1.0f, 1.0f);       // [lo, hi)
engine::rng::integer(1, 6);            // [1, 6], обе границы входят
engine::rng::chance(0.25f);  engine::rng::in_unit_circle();  engine::rng::on_unit_sphere();
engine::rng::seed(12345);              // воспроизводимый прогон

engine::log_info("hello");  engine::log_warnf("hp = %d", hp);  engine::log_errorf("no target %s", name);
```

Генератор и сохранения — те же ресурсы мира, что у Rust: `SaveData` подключает игра
(`app.world.resources.insert(SaveData::open("MyGame"))`), иначе набор живёт в памяти с
предупреждением. Зерно по умолчанию фиксировано — игра воспроизводима.

---

## 13. Свои сервисы из Rust

Всё, чего нет в ядре (физика, звук, интерфейс), доступно через таблицу **именованных сервисов** —
`engine_cpp_bridge::CppServices` в `world.resources`. Фасад `engine::app()` регистрирует свои
(`engine::cpp_services`); игра может добавить собственные и звать их из C++:

```rust
engine::cpp_services::register(&mut app.world, "game.difficulty", |world, args| {
    Ok(vec![Value::Float(world.resources.get::<Difficulty>().map_or(1.0, |d| d.0))])
});
```

```cpp
auto r = engine::call_service("game.difficulty", {}, 1);       // vector<Value>
float d = r.empty() ? 1.0f : r[0].to_float();
if (engine::service_available("game.difficulty")) { ... }
```

Аргументы и результаты — `Value`; объект передаётся числом (`object_arg` / `object_value`
в Rust, `Value(Object)` / `to_object()` в C++). Если сервиса нет — `call_service` возвращает пустой
список (`status` — `ENGINE_SERVICE_MISSING`), в лог один раз идёт «called service X, but the
engine has no such service», игра не падает. Сервис, который отказался от аргументов или упал
(`panic` перехватывается), даёт `ENGINE_SERVICE_BAD_ARGS` и запись в лог.

Имена сервисов фасада: `physics3d.{raycast, overlap_sphere, apply_impulse, apply_impulse_at,
apply_torque_impulse, add_force, radial_impulse, mass, is_sleeping, gravity, set_gravity}`,
`physics2d.{raycast, overlap_circle, velocity_change, gravity, set_gravity}`, `audio.{play, stop,
is_playing, set_volume, set_pitch, fade_out, play_music, stop_music, is_music_playing, voice_count,
stop_all, pause_all, resume_all, set_bus_volume, bus_volume, set_bus_muted, play_source,
stop_source}`, `ui.{clicked, toggled, value_changed, text_changed, submitted, pointer_over_ui,
wants_keyboard}`. Точные аргументы — в `crates/engine/src/cpp_services.rs` и обёртках заголовка.

---

## 14. Безопасность и правила вызова

* **Где можно звать API.** Только изнутри хуков компонента (и `EnginePlugin::on_fixed_update`).
  В конструкторе компонента, инициализаторах членов, статических объектах и `on_load` мира нет:
  вызов вернёт нейтральное значение (0, нулевой хэндл, пустую строку) и запишет в лог
  *«`set_position` was called outside of a component hook…»*. Инициализацию переносите в `on_start`.
  До загрузки плагина (статические инициализаторы) вызов `engine::…` печатает понятное сообщение в
  stderr и завершает процесс — это ошибка программиста, а не поведение «неопределённое».
* **Устаревшие хэндлы.** Каждый вызов проверяет поколение объекта. Уничтоженный объект и нулевой
  хэндл дают в логе ошибку (*«`get_position` got a stale object handle … Check `obj.alive()`»*) и
  нейтральное значение; запись — не делает ничего. Новый объект в освободившемся слоте старый хэндл
  не «оживляет». Не больше 10 сообщений на функцию, дальше — одна строка «further errors … not logged».
  `alive()` не пишет в лог.
* **`destroy` откладывается до конца кадра**, поэтому `self.destroy()` в `on_update`, в обработчике
  столкновения или в `on_message` безопасен: компонент и объект доживают кадр, дальше срабатывает
  `on_destroy`. Хэндл после этого — устаревший. `remove_component` собственного компонента тоже
  вступает в силу после выхода из хука.
* **Что внутри хука не вызывает хуков.** Функции API только ставят в очередь сообщения, уничтожение,
  смену сцены и таймеры; чужой хук не вызывается посреди вашего. `instantiate` / `add_component`
  вызывают конструктор нового C++-компонента (но не его `on_start` — он придёт перед следующим
  обновлением).
* **Исключения.** Каждый хук обёрнут: `std::exception` и `...` ловятся, в лог идёт
  *«C++ exception in Type::on_update: what»* (первые 5 на тип), хук пропускается, компонент и игра
  живут дальше. Исключение из конструктора — компонент не создаётся (ошибка в логе).
  Вне хуков (в ваших потоках) исключения — ваша забота. Паники Rust границу не пересекают.
* **Потоки.** API однопоточный: звать только из потока, вызвавшего хук.
* **Строки** — UTF-8, `const char*` с нулём; `nullptr` трактуется как пустая строка.
* **ABI.** Несовпадение `ENGINE_ABI_VERSION` — ошибка загрузки с просьбой пересобрать плагин (§2).

---

## 15. Соответствие Rust API ↔ C++ API

| Rust | C++ | Примечание |
|---|---|---|
| `Component::{on_start, on_update, on_fixed_update, on_late_update, on_destroy, on_message, on_collision_enter, on_trigger_enter}` | то же имя, `engine::Component` | полное соответствие |
| `component!` + reflection | `ENGINE_FIELDS` + `ENGINE_FIELD/ENGINE_ENUM_FIELD/ENGINE_ASSET_FIELD` | `f32, i32, bool, Vec2, Vec3, Color, Quat, String, enum, ассет`; нет `i64`, `Vec<T>`, `Rect`, `Timer` |
| `ctx.transform()`, `world_position()` | `Object::{position, rotation, scale, world_*}` | поворот в градусах Эйлера (`Quat` напрямую нет) |
| `ctx.get::<T>()`, `get_on::<T>(id)` | `obj.get/set(component, field, …)` | по именам полей, не по типу; Rust-структуру компонента из C++ не получить |
| `add_boxed`, `remove::<T>()`, `has::<T>()` | `add_component / remove_component / has_component` | по имени типа |
| `scene.find_by_name/tag`, `find_all_by_tag`, `find_with::<T>` | `find_by_name/tag`, `find_all_by_tag`, `find_with_component` | |
| `find_child`, `parent_of`, `children`, `find_path`, `find_in_children::<T>` | `find_child`, `parent`, `children`; пути и `*_in_children` — нет | |
| `set_parent(_keep_world)` | `set_parent(p, keep_world)` | |
| `set_active`, `is_active` | `set_active`, `active`, `active_in_hierarchy` | |
| `ctx.instantiate/_at/_under/_with`, `duplicate` | `instantiate` (2 формы), `instantiate_under`, `duplicate` | масштаб/поворот префаба — из префаба; `Quat` заменён углами |
| `scene.spawn` / `build` | `spawn(name)` + `add_component` | цепочки `build()` нет |
| `destroy`, `destroy_after`, `destroy_self` | `destroy`, `destroy_after` | |
| `send_message`, `after`, `every`, `send_after`, `cancel_timer`, `cancel_my_timers` | `send_message`, `send_after`, `send_every`, `Timer::cancel`, `cancel_timers` | |
| `call_after(\|world\| …)` (замыкание) | — | замыкания через границу не ходят: используйте сообщение самому себе |
| `Scheduler::{limit, unscaled, remaining, is_pending}` | — | только через Rust |
| `Tween::{field, position, scale, rotation, euler}`, `ease`, `yoyo`, `loops`, `delay`, `unscaled`, `on_complete` | `Object::tween(path, value, TweenParams)` | позиция/масштаб — через `"Transform.position"`; `Sequence`, `.from()` — нет |
| `cancel_tween`, `cancel_tweens_of` | `Tween::cancel`, `cancel_tweens` | |
| `ctx.load_scene/_additive/unload/reload/scene_name`, `dont_destroy_on_load` | те же | `SceneSource`, события `SceneLoaded` — нет |
| `time.{delta, unscaled_delta, elapsed, frame, fixed_delta, time_scale}` | `delta_time … set_time_scale`, `time_info()` | |
| `input.{axis, button*, key*, mouse*, scroll, touches, text_input, screen_size}` | `input::…` | клавиши по имени |
| геймпад: `gamepad_*`, `left_stick`, `any_*` | `input::gamepad_*`, `left_stick`, `right_stick`; `pad = -1` — любой | имена, список геймпадов и их названий — нет |
| привязки, `rebind_button`, `last_pressed_binding` | — | только Rust |
| `Physics3D::raycast/overlap_sphere/apply_impulse*/add_force/radial_impulse/mass/is_sleeping` | `physics::…` (сервисы) | суставы (`*_joint`) — нет |
| `Physics2D::raycast` | `physics::raycast2d` | `overlap_circle` — по сцене (форма: коробка/круг/капсула), не Rapier |
| `Rigidbody.velocity`, `CharacterController` | `physics::velocity…`, `character_*` (поля) | |
| `Audio::{play, play_at, play_ui, play_with, play_music, stop_music, bus_*, pause_all…}` | `audio::…` (сервисы) | `add_clip`, синтезатор, шины-настройки микшера — Rust |
| `ctx.ui().{clicked, toggled, value_changed, text_changed, submitted, pointer_over_ui, wants_keyboard}` | `ui::…` (сервисы) | `hovered()/focused()` — нет |
| `Text::set`, `ProgressBar.value`, `ui_set_visible` | `ui::set_text/set_value/set_visible` (поля) | `ui_*` конструкторы — нет |
| `SaveData::{int, set_int, bool, float, string, vec3, has_key, delete_key, save}` | `save::get_*`, `set`, `has`, `erase`, `flush` | `open/with_backend` — Rust |
| `Random::{f32, range, int_range_inclusive, chance, seed, in_unit_circle, on_unit_sphere}` | `rng::…` | `pick`, `shuffle`, `gaussian`, `weighted_index` — нет (пишутся на C++) |
| `ctx.quit()`, `set_time_scale` | `quit()`, `set_time_scale` | |
| `ctx.resource_or_default::<T>()`, `events::<E>()`, `emit` | — | типизированные ресурсы/события Rust через границу не ходят — используйте сервисы (§13) или сообщения |
| `StateMachine`, `Timer`, `Cooldown`, `mathf` | — | пишутся на C++ (`<cmath>`); `Vec3`, `Vec2` с арифметикой есть |
| `log::info!` | `log_info`, `log_infof` | |

---

## 16. Чего из C++ нет и почему

* **Типизированный доступ к Rust-структурам и ресурсам** (`ctx.get::<Health>()`, `resource::<T>()`,
  `events::<E>()`): у C++ нет типов Rust, раскладка структур не стабильна. Взамен — поля по именам,
  сообщения и сервисы (§13).
* **Замыкания и корутины** (`call_after(|w| …)`, `Sequence`): через C ABI замыкания не ходят.
  Используйте `send_after` + `on_message` или несколько твинов с `delay`.
* **Выход из столкновения / триггера, залипание (`on_collision_stay`)**: в Rust-трейте `Component`
  таких хуков нет — движок их не вызывает.
* **Суставы и `Physics3D::*_joint`, настройка тел** (масса, слои): суставы пока не выведены в
  сервисы; тела настраиваются полями компонентов.
* **2D-импульсы** `Rigidbody2D`: в `engine-physics` их нет — есть `add_velocity2d` (Δv без учёта массы).
  `overlap_circle` в 2D считается по `Collider2D` сцены, а не Rapier.
* **Создание виджетов интерфейса из кода** (`ui_button` и т. п.), сцены на лету, реестр типов
  (`app.register::<T>()`), подсистемы, рендер (камеры, частицы, спрайтовые анимации как API) —
  это настройка приложения, а не логика кадра; делается на Rust или полями компонентов
  (`self.set("SpriteAnimator", "clip", …)`).
* **Привязки ввода и переназначение**, `SaveData::open`, `Audio::add_clip`, `SceneSource` — настройка
  игры в `main`, на Rust.
* **Другие потоки.** API однопоточное.
* **Веб (wasm).** Динамические библиотеки на вебе не загружаются; статический плагин
  (`builtin_gameplay`) зависит от того, соберётся ли C++ под цель — см. заметки платформы.

---

## 17. C++ в своей игре

Игре из `games/<имя>/` не нужен ни CMake, ни DLL: её собственные `*.cpp` собираются в
`build.rs` и **статически линкуются в крейт игры**. Пример — «Шаровой завод»
(`games/marble`): игра целиком на Rust-скриптах, а один небольшой C++-компонент `CppSpinner`
в `cpp/` собирается по желанию (cargo-фича игры `cpp-example`).

Три шага.

**1. `Cargo.toml` игры** — сборочная зависимость и `build`:

```toml
[package]
name = "my_game"
build = "build.rs"

[build-dependencies]
engine-cpp-build = { path = "../../crates/engine-cpp-build" }
```

**2. `build.rs`** — одна строка. Все `*.cpp` каталога (с подкаталогами) собираются
**одной единицей трансляции**; `rerun-if-changed` ставится сам.

```rust
fn main() {
    engine_cpp_build::compile("cpp");        // каталог — относительно Cargo.toml игры
}
```

**3. `src/lib.rs`** — подключить скрипты в `build()`:

```rust
pub fn build() -> Result<App, EngineError> {
    let mut app = engine::app();
    engine::add_static_cpp(&mut app, engine::cpp_scripts!())?;   // компоненты — в реестре типов
    // … дальше как обычно: сцены и код могут ссылаться на C++-компоненты по имени
    Ok(app)
}
```

Скрипт — тот же код, что в §1, но точку входа плагина **писать не нужно**: её добавляет
сборка (`ENGINE_PLUGIN_AUTO_ENTRY(engine_game_<пакет>_entry, "<пакет>")`).

```cpp
// games/my_game/cpp/Spinner.cpp
#include "engine_component.hpp"

struct GameSpinner : engine::Component {
    float speed_deg = 90.0f;
    void on_update(engine::Object self, float dt) override {
        auto r = self.rotation();
        self.set_rotation({r.x, r.y + speed_deg * dt, r.z});
    }
};
ENGINE_FIELDS(GameSpinner, ENGINE_FIELD(GameSpinner, speed_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(GameSpinner);
```

Из Rust такой компонент создаётся по имени и настраивается полями — так, например, цеха
«Шарового завода» строятся кодом:

```rust
let mut c = app.world.registry.create("GameSpinner")?;      // Box<dyn Component>
c.set_field("speed_deg", &Value::Float(45.0))?;
app.world.scene.get_mut(id).unwrap().add_boxed(c);
```

### Что делает `engine_cpp_build::compile`

* Собирает `*.cpp` в одну единицу: `#include "engine_component.hpp"`, все файлы по
  алфавиту, `ENGINE_PLUGIN_AUTO_ENTRY(...)`. Поэтому **статические объекты и
  свободные функции всех файлов делят одну область имён**: общий код кладите в `.hpp`
  с `#pragma once` (каталог `cpp/` — в путях поиска), вспомогательные функции делайте
  `inline` или в своём `namespace`, а имена типов компонентов — уникальными (в том числе
  относительно `plugins/cpp-gameplay`: если игра собирает ещё и `builtin_gameplay`, его
  компоненты `Spinner`, `Bobber`, `Mover`, `Pickup`… — те же глобальные имена).
* Компилирует через `cc` как C++17 (`/EHsc /utf-8` у MSVC, `-Wno-invalid-offsetof` у
  GCC/Clang) с путём к `engine-cpp-bridge/include`. Проверено с MSVC (хост этой машины) и
  clang из Android NDK: `cargo ndk -t arm64-v8a check -p <игра>`.
* Пишет в `OUT_DIR/engine_cpp_scripts.rs` выражение, которое раскрывает
  `engine::cpp_scripts!()` (тип `engine::cpp::StaticScripts`), и включает для крейта игры
  `cfg(engine_game_cpp)`.
* На `wasm32` (веб) C++ **не компилируется**: `engine_cpp_build::compile` — пустая операция,
  `cpp_scripts!()` раскрывается в «скриптов нет», а `add_static_cpp` возвращает
  `EngineError::Plugin(PluginError::StaticUnavailable)` с объяснением. Игре, которая
  должна работать и в браузере, нужна ветка без C++ (`#[cfg(engine_game_cpp)]`).

### Требования к крейту игры

* `#![forbid(unsafe_code)]` в крейте игры **нельзя**: в раскрытии `cpp_scripts!()` есть один
  `unsafe`-вызов (`#[allow(unsafe_code)]` в нём стоит, `deny` не мешает; `forbid` запрещает
  `allow`). Макрос `engine::game_main!` и так генерирует `#[no_mangle]` для Android.
* В `build.rs` — только `engine_cpp_build::compile(..)`; вызов `Build::new("cpp").include("cpp/third_party").define("NAME", Some("1")).compile()`
  добавляет каталоги поиска и макросы.
* Горячей перезагрузки у статических скриптов нет (для неё — `add_cpp_plugins` и CMake
  из §2). Значения полей и поведение остаются теми же, поэтому скрипт можно сначала
  отлаживать как DLL, а потом переложить в `cpp/` игры.

### API

| | |
|---|---|
| `engine_cpp_build::compile(dir)` | собрать `dir/**/*.cpp`; остановить сборку игры понятной ошибкой |
| `engine_cpp_build::Build::new(dir).include(..).define(..).try_compile()` | то же с настройкой, возвращает `Result<Built, BuildError>` |
| `engine::cpp_scripts!()` | выражение типа `engine::cpp::StaticScripts` (только в крейте с `build.rs` выше) |
| `engine::add_static_cpp(&mut app, scripts)` | подключить скрипты: компоненты — в реестр, подсистема — в приложение |
| `CppPlugins::add_static_scripts(scripts)` | то же на уровне `CppPlugins` (если нужны и динамические плагины) |

Скрипты игры видят те же сервисы (`physics::`, `audio::`, `ui::`, §9–§13), что и плагин
движка; игра может добавить свои (`engine::cpp_services::register`, §13). Пример
целиком — `games/marble/cpp/` и `games/marble/build.rs` (за фичей `cpp-example`), тест —
`games/marble/tests/cpp_example.rs`.

