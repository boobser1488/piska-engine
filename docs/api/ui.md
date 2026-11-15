# Игровой интерфейс (`engine-ui`)

HUD, главное меню, пауза, настройки — в стиле Unity uGUI. Интерфейс состоит из обычных
объектов сцены с компонентами, поэтому он сохраняется в `.scene`, правится в инспекторе
и создаётся из кода короткими конструкторами.

Всё нужное есть в `use engine::prelude::*;` — `engine::app()` уже подключает интерфейс
(`engine_ui::install`). Пример целиком: `crates/engine/examples/ui_demo.rs`
(`cargo run --example ui_demo`).

**Интерфейс в файле сцены.** HUD и экраны удобнее собирать в редакторе (GameObject → UI) и
хранить в `.scene`, как любые объекты: тогда скрипт только меняет тексты и показывает
панели — `ctx.ui_text("Score", …)`, `ctx.ui_show("GameOverPanel")`, `ctx.clicked("restart")`.
Всё, что строят конструкторы `scene.ui_*` ниже, сохраняется в сцену и загружается обратно
без потерь (тест `ui_round_trips_through_ron_scenes` в `crates/engine/tests/ui.rs`). Пример — `games/template/assets/scenes/main.scene`
(объект `HUD`): поля, которых нет в файле, берут значения по умолчанию, поэтому запись
короткая:

```ron
(
    name: "Score",
    components: [
        (kind: "RectTransform", fields: { "size": Vec2(320.0, 42.0) }),
        (kind: "Text", fields: { "text": Str("Score: 0"), "size": Float(30.0), "bold": Bool(true) }),
    ],
),
```

## Быстрый старт

```rust
use engine::prelude::*;

component! {
    /// Реагирует на кнопки главного меню.
    pub struct MainMenu {}
}

impl Component for MainMenu {
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        match message.name.as_str() {
            "play" => log::info!("start the game"),
            "settings" => log::info!("open settings"),
            _ => {}
        }
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.register::<MainMenu>();
    app.spawn("Camera").with(Camera::default());

    let scene = &mut app.world.scene;
    let canvas = scene.ui_canvas().with(MainMenu::default()).id();
    let menu = scene.ui_panel(canvas).anchor(Anchor::Center).width(420.0)
        .column(12.0).padding(32.0).fit().id();
    scene.ui_text(menu, "Главное меню").font_size(40.0).bold().align(TextAlign::Center);
    scene.ui_button(menu, "play", "Играть").primary();
    scene.ui_button(menu, "settings", "Настройки");

    engine::run(app, WindowConfig::titled("Menu"))
}
```

Кнопка `"play"` по клику шлёт сообщение `play` себе и **всем предкам** — поэтому скрипт на
холсте (или на панели меню) ловит клики всех своих кнопок в одном `on_message`.

## Координаты и холст

* Начало координат — **левый верхний угол**, ось Y направлена **вниз** (как у
  `input.mouse_position()`).
* Единицы — «пиксели холста». `Canvas` переводит их в пиксели экрана:
  * `ScaleMode::ScaleWithScreenSize` (по умолчанию): масштаб = экран /
    `reference_resolution` (по умолчанию 1280×720), смешанный между шириной и высотой по
    `match_width_or_height` (0 — по ширине, 1 — по высоте, 0.5 — поровну, как в Unity);
  * `ScaleMode::ConstantPixelSize`: `scale_factor` пикселей на единицу.
* Текст растеризуется под итоговый размер в пикселях, поэтому чёткий при любом масштабе.
* Холсты рисуются по возрастанию `Canvas::sort_order`, внутри холста — в порядке иерархии
  (позже в иерархии — выше на экране). Верхний холст первым получает клики.
* `Canvas::keyboard_navigation` (по умолчанию `true`): Tab/стрелки/Enter управляют фокусом.
  Геймпад делает то же: крестовина или левый стик (порог 0.5, одно отклонение — один шаг,
  при удержании шаги повторяются), South — как Enter, East — как Escape.
  Для HUD выключайте (`.no_navigation()`), иначе стрелки, которыми ходит игрок, будут
  переводить фокус на кнопку «Пауза».
* `Transform` объектов интерфейса не используется — только `RectTransform`.

## `RectTransform` — прямоугольник с якорями

| поле | по умолчанию | смысл |
|---|---|---|
| `anchor_min: Vec2` | (0.5, 0.5) | левый верхний якорь в долях родителя |
| `anchor_max: Vec2` | (0.5, 0.5) | правый нижний якорь |
| `pivot: Vec2` | (0.5, 0.5) | точка опоры элемента (доли его размера) |
| `position: Vec2` | (0, 0) | смещение опоры от якоря |
| `size: Vec2` | (160, 40) | размер; при разведённых якорях — добавка к растянутой области |

Формула как в Unity: `размер = (якорь_max − якорь_min) × родитель + size`.
Пресеты — `Anchor::{TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom,
BottomRight, StretchTop, StretchBottom, StretchLeft, StretchRight, Stretch}`:

```rust
RectTransform::anchored(Anchor::TopRight, Vec2::new(-24.0, 24.0), Vec2::new(120.0, 48.0)); // угол с отступом 24
RectTransform::stretch();                       // на весь родитель
RectTransform::inset(10.0, 20.0, 10.0, 20.0);   // на весь родитель с отступами (лево, верх, право, низ)
RectTransform::anchored(Anchor::StretchBottom, Vec2::new(0.0, -16.0), Vec2::new(-36.0, 14.0)); // полоса внизу, по 18 от краёв
```

Объект без `RectTransform` под холстом занимает весь прямоугольник родителя.

## `LayoutGroup` — столбик или строка

Раскладывает активных детей подряд. Размер ребёнка вдоль направления берётся из его
`RectTransform.size` (его якоря и `position` в группе не используются).

| поле | по умолчанию | |
|---|---|---|
| `direction` | `Vertical` | или `Horizontal` |
| `spacing` | 8 | промежуток |
| `padding_left/right/top/bottom` | 0 | внутренние отступы |
| `child_alignment` | `TopLeft` | куда прижать детей, если места больше |
| `stretch_children` | `true` | растянуть детей поперёк (ширину в столбце) |
| `expand_children` | `false` | делить свободное место (или нехватку) поровну: дети заполняют группу точно |
| `fit_content` | `false` | подогнать свой размер под детей (Content Size Fitter) |

`fit_content` меняет размер только по осям, где якоря элемента не разведены. Скрытые
(`active = false`) дети места не занимают.

## Виджеты

Все цвета — `Color` (линейные; дизайнерские значения задавайте `Color::hex(0x4f8cff)`).
Значения по умолчанию взяты из темы `engine_ui::theme` (тёмные панели, синий акцент).

* **`Image`** — `color`, `texture: AssetHandle<Texture>` (пусто — сплошной цвет),
  `corner_radius`, `border_width`, `border_color`, `shadow_size` (мягкая тень, 0 — нет),
  `shadow_color`, `shadow_offset`, `slice_border` (9-slice: рамка текстуры в её пикселях,
  рисуется тем же числом единиц холста), `raycast_target` (ловит ли мышь, по умолчанию да).
  `Image::panel()` — панель в стиле темы, `Image::colored(c)`.
* **`Text`** — `text`, `font: AssetHandle<Font>` (пусто — встроенный Ubuntu Light: латиница
  и кириллица; символы, которых нет в шрифте, берутся из встроенного Hack), `size` (кегль,
  как font-size в CSS), `color`, `align: TextAlign::{Left, Center, Right}`,
  `vertical_align: VerticalAlign::{Top, Middle, Bottom}`, `wrap` (перенос по словам,
  по умолчанию да), `bold` (синтетический полужирный), `line_spacing`, `outline_width` +
  `outline_color` (обводка для читаемости поверх игры), `shadow_offset` + `shadow_color`.
  Текст мышь не ловит. Меняйте строку через `text.set("…")` — раскладка пересчитывается
  только при изменении.
* **`Button`** — красит `Image` своего объекта цветом состояния: `normal_color`,
  `hover_color`, `pressed_color`, `disabled_color` (с плавным переходом); `interactable`;
  `message` (пусто — имя объекта). Клик — нажать и отпустить над той же кнопкой, или Enter
  (South на геймпаде) в фокусе. `Button::primary("play")` — акцентная.
* **`Toggle`** — `is_on`, `style: ToggleStyle::{Checkbox, Switch}`, `off_color`, `on_color`,
  `check_color`, `interactable`, `message`. Рисуется у левого края своего прямоугольника,
  подпись — дочерний `Text` (клик по подписи тоже переключает).
* **`Slider`** — `value`, `min`, `max`, `whole_numbers`, `interactable`, `track_color`,
  `fill_color`, `handle_color`, `message`. Мышь тянет ручку; колесо над ползунком, стрелки
  ←/→ в фокусе, а на геймпаде — крестовина или стик ←/→ — шаг (1 или 1/20 диапазона).
* **`ProgressBar`** — `value` (0..=1), `background_color`, `fill_color`, `corner_radius`,
  `direction: FillDirection::{LeftToRight, RightToLeft, BottomToTop, TopToBottom}`, `inset`.
  Только отображение.
* **`InputField`** — однострочное поле: `text`, `placeholder`, `max_length` (0 — без
  ограничения), `font_size`, `text_color`, `placeholder_color`, `background_color`,
  `border_color`, `focus_color`, `corner_radius`, `padding`, `interactable`, `message`.
  Клик — начать ввод (на геймпаде: South в фокусе); Backspace и Delete (с автоповтором),
  ←/→ (с автоповтором), Home / End — в начало / конец, Enter — «отправить», Escape или
  клик мимо — закончить ввод (на геймпаде — East). Текст берётся из `input.text_input()`
  (с учётом раскладки).
* **`RectMask`** — обрезает всё, что дети рисуют за его прямоугольником (и клики там).
  `padding` сжимает область.
* **`CanvasGroup`** — `alpha` (прозрачность ветки), `interactable`, `blocks_raycasts`.

## Как игра узнаёт о действиях

### Сообщения (`on_message`)

Каждое действие виджета шлёт `Message` **виджету и всем его предкам**:

| виджет | имя | значение |
|---|---|---|
| `Button` | `message` | `Value::Nil` |
| `Toggle` | `message` | `Value::Bool(новое состояние)` |
| `Slider` | `message` | `Value::Float(новое значение)` |
| `InputField`, текст изменился | `message` | `Value::Str(текст)` |
| `InputField`, Enter | `"<message>:submit"` | `Value::Str(текст)` |

Если поле `message` пустое, именем служит имя объекта (`ui_button` задаёт оба).

```rust
fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
    match (message.name.as_str(), &message.value) {
        ("resume", _) => ctx.world.time.time_scale = 1.0,
        ("volume", Value::Float(v)) => self.volume = *v,
        ("fullscreen", Value::Bool(on)) => self.fullscreen = *on,
        ("player_name:submit", Value::Str(name)) => self.name = name.clone(),
        _ => {}
    }
}
```

### Опрос (`ctx.ui()`)

`ctx.ui()` (и `world.ui()`) возвращает `&UiEvents` — состояние интерфейса **этого кадра**.
События появляются в начале кадра, до `on_update` скриптов, и живут до начала следующего.

```rust
fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
    if ctx.ui().clicked("pause") { /* … */ }
    if let Some(v) = ctx.ui().value_changed("volume") { self.volume = v; }
    if let Some(on) = ctx.ui().toggled("vsync") { /* … */ }
    if let Some(text) = ctx.ui().submitted("chat") { /* … */ }

    // Клик по интерфейсу — не выстрел.
    if ctx.input().mouse_button_down(MouseButton::Left) && !ctx.ui().pointer_over_ui() {
        self.shoot(ctx);
    }
    // Игрок печатает в поле — не ходим по WASD.
    if ctx.ui().wants_keyboard() { return; }
}
```

Методы `UiEvents`: `events() -> &[UiEvent]`, `clicked(name)`, `clicked_id(id)`,
`toggled(name) -> Option<bool>`, `value_changed(name) -> Option<f32>`,
`text_changed(name) -> Option<&str>`, `submitted(name) -> Option<&str>`,
`pointer_over_ui()`, `wants_keyboard()`, `hovered()`, `focused()`.
`UiEvent { source: GameObjectId, name: String, kind: UiEventKind }`,
`UiEventKind::{Click, Toggle(bool), Value(f32), TextChanged(String), Submit(String)}`.

### Изменение интерфейса из скрипта

```rust
// Обновить счёт (строка пересобирается только при изменении).
if let Some(id) = ctx.scene().find_by_name("Score") {
    if let Some(text) = ctx.scene_mut().get_component_mut::<Text>(id) {
        text.set(&format!("Счёт: {}", self.score));
    }
}
// Полоса здоровья.
if let Some(bar) = ctx.scene_mut().get_component_mut::<ProgressBar>(self.health_bar) {
    bar.value = self.health / self.max_health;
}
// Показать / скрыть экран.
ctx.scene_mut().ui_set_visible(pause_menu, true);
```

Пауза: `ctx.world.time.time_scale = 0.0` — интерфейс продолжает работать (анимации
кнопок идут по реальному времени).

## Конструкторы (`UiScene`)

Методы трейта `UiScene` есть у `Scene` (`app.world.scene`, `ctx.scene_mut()`). Каждый
создаёт объект с `RectTransform` и возвращает `UiNode` — цепочку настроек; `.id()`
завершает её.

| конструктор | что создаёт | размер по умолчанию |
|---|---|---|
| `ui_canvas()` | корневой объект `Canvas` | экран |
| `ui_node(parent, name)` | пустой прямоугольник | 160×40 |
| `ui_panel(parent)` | `Image::panel()` | 420×320 |
| `ui_column(parent, spacing)` / `ui_row(parent, spacing)` | прозрачная раскладка | 320×240 / 320×48 |
| `ui_text(parent, "…")` | `Text` (кегль 24) | 320×34 |
| `ui_image(parent, color)` | `Image` | 100×100 |
| `ui_button(parent, message, label)` | `Image` + `Button` + дочерний `Text` | 260×52 |
| `ui_toggle(parent, message, label, on)` | `Toggle` + подпись | 260×36 |
| `ui_slider(parent, message, min, max, value)` | `Slider` | 260×32 |
| `ui_progress(parent, value)` | `ProgressBar` | 260×14 |
| `ui_input(parent, message, placeholder)` | `InputField` | 300×48 |
| `ui_edit(id)` | настроить существующий объект | — |
| `ui_set_visible(id, bool)` | показать/скрыть (`active`) | — |

Методы `UiNode`:

* прямоугольник: `.anchor(Anchor)`, `.pos(x, y)`, `.size(w, h)`, `.width(w)`, `.height(h)`,
  `.stretch()`, `.inset(l, t, r, b)`, `.margin(m)`;
* раскладка: `.column(spacing)`, `.row(spacing)`, `.padding(p)`, `.child_align(Alignment)`,
  `.no_stretch()`, `.expand()`, `.fit()`, `.mask()`;
* вид: `.color(c)` (у `Image`, иначе у `Text`, иначе заполнение `ProgressBar`),
  `.radius(r)`, `.border(w, c)`, `.shadow(size)`;
* текст (свой или подпись кнопки/переключателя): `.font_size(s)` (у одиночного текста
  заодно ставит высоту под одну строку — `.height()` после неё переопределяет),
  `.bold()`, `.text_color(c)`, `.align(TextAlign)`, `.valign(VerticalAlign)`,
  `.outline(w, c)`, `.text_shadow(x, y)`, `.no_wrap()`, `.label("…")`;
* виджеты: `.primary()`, `.interactable(bool)`, `.switch()`, `.whole_numbers()`,
  `.max_length(n)`, `.value("…")` (текст поля ввода);
* холст: `.sort_order(i)`, `.reference_resolution(w, h)`, `.no_navigation()`;
* общее: `.name("…")`, `.hidden()`, `.with(component)`, `.edit::<T>(|c| …)`, `.id()`.

### HUD

```rust
let hud = scene.ui_canvas().no_navigation().id();
let health = scene.ui_panel(hud).anchor(Anchor::TopLeft).pos(24.0, 24.0).size(320.0, 74.0).id();
scene.ui_text(health, "Здоровье").anchor(Anchor::TopLeft).pos(18.0, 10.0).font_size(17.0);
scene.ui_progress(health, 1.0).name("HealthBar")
    .anchor(Anchor::StretchBottom).pos(0.0, -16.0).size(-36.0, 14.0);
scene.ui_text(hud, "Счёт: 0").name("Score").anchor(Anchor::Top).pos(0.0, 22.0).size(400.0, 48.0)
    .font_size(34.0).bold().align(TextAlign::Center).outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.8));
scene.ui_button(hud, "pause", "Пауза").anchor(Anchor::TopRight).pos(-24.0, 24.0).size(132.0, 48.0);
```

### Затемнение под меню паузы

```rust
let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.0, 0.55)).name("Pause").stretch().hidden().id();
let menu = scene.ui_panel(dim).anchor(Anchor::Center).width(380.0).column(12.0).padding(32.0).fit().id();
scene.ui_button(menu, "resume", "Продолжить").primary();
```

Затемнение — `Image` с `raycast_target = true`, поэтому клики до игры и нижних холстов не
доходят.

## Сцены (RON)

Все компоненты регистрируются `engine::app()` и сохраняются как любые другие:

```ron
(
    name: "Play",
    components: [
        (kind: "RectTransform", fields: { "size": Vec2(260.0, 52.0) }),
        (kind: "Image", fields: { "corner_radius": Float(10.0) }),
        (kind: "Button", fields: { "message": Str("play") }),
    ],
    children: [
        (
            name: "Label",
            components: [
                (kind: "RectTransform", fields: { "anchor_min": Vec2(0.0, 0.0), "anchor_max": Vec2(1.0, 1.0), "size": Vec2(0.0, 0.0) }),
                (kind: "Text", fields: { "text": Str("Играть"), "align": Enum("Center"), "bold": Bool(true) }),
            ],
        ),
    ],
)
```

Поля, которых нет в файле, берут значения по умолчанию. Свой шрифт: положите `.ttf`/`.otf`
в каталог ассетов (импортёр `Font`) и укажите `"font": Asset("<guid>")` у `Text`.

## Отрисовка

* Интерфейс рисуется **после** пост-обработки (без блума и тонмаппинга), прямо в пикселях
  цели, с альфа-смешиванием, в окне игры, в виде Game редактора и в
  `Renderer::capture` / `ENGINE_SCREENSHOT`. Вид Scene редактора его не показывает
  (`CameraView::overlay`).
* Фигуры (скругления, рамки, тени) считаются в шейдере по расстоянию до края — края гладкие
  без MSAA. Панели и текст лежат в одной текстуре (атлас глифов + белый квадрат), поэтому
  меню обычно рисуется одним вызовом; новый вызов нужен только при смене текстуры
  (`Image.texture`) или области отсечения (`RectMask`, поле ввода).
* Шов рендер ↔ интерфейс — ресурс `engine::render::Overlay` (треугольники в пикселях +
  отрезки по текстурам). Его можно заполнять и своим кодом, если интерфейс не нужен.

## Ограничения

* Поворот и масштаб `Transform` к интерфейсу не применяются.
* Клавиши редактирования поля — Backspace, Delete, ←, →, Home, End, Enter, Escape, Tab;
  выделения и буфера обмена нет.
* Текст слева направо, без сложной типографики (лигатур, арабского/индийских письменностей).
* Геймпад управляет только навигацией и кнопками: мышь на геймпад не переводится, поле
  ввода набирается клавиатурой (South лишь открывает ввод).
