//! Типы C ABI плагинов. Единственный источник правды: из этого файла `cbindgen`
//! генерирует `include/engine_api.h`, который включают C++-плагины.
//!
//! Правила ABI:
//! * только `#[repr(C)]`-структуры, целые/float фиксированного размера и указатели;
//! * строки — `const char*` с завершающим нулём, кодировка UTF-8; строки, которые движок
//!   отдаёт плагину в `EngineValue.s`, живут до следующих 256 строк (копируйте сразу),
//!   строки в буфер плагина отдаются парой `(char* buf, uint32_t cap)` и возвращают
//!   нужную длину без нуля;
//! * исключения C++ и паники Rust границу не пересекают;
//! * любое изменение структур → увеличить [`ENGINE_ABI_VERSION`].

use std::os::raw::{c_char, c_void};

/// Версия ABI. Плагин с другой версией не загружается: его нужно пересобрать.
pub const ENGINE_ABI_VERSION: u32 = 3;

/// Объект сцены: индекс и поколение. `generation == 0` — «нет объекта».
/// Устаревший (уничтоженный) объект движок распознаёт по поколению.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineObject {
    pub index: u32,
    pub generation: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

// ----------------------------------------------------------------------
// Динамическое значение
// ----------------------------------------------------------------------

/// Значение: ничего.
pub const ENGINE_VALUE_NIL: u32 = 0;
/// Значение: `bool` в `i` (0 или 1).
pub const ENGINE_VALUE_BOOL: u32 = 1;
/// Значение: целое в `i`.
pub const ENGINE_VALUE_INT: u32 = 2;
/// Значение: `float` в `f[0]`.
pub const ENGINE_VALUE_FLOAT: u32 = 3;
/// Значение: строка UTF-8 в `s`.
pub const ENGINE_VALUE_STR: u32 = 4;
/// Значение: два `float` в `f[0..2]`.
pub const ENGINE_VALUE_VEC2: u32 = 5;
/// Значение: три `float` в `f[0..3]`.
pub const ENGINE_VALUE_VEC3: u32 = 6;
/// Значение: кватернион (x, y, z, w) в `f[0..4]`.
pub const ENGINE_VALUE_QUAT: u32 = 7;
/// Значение: линейный RGBA в `f[0..4]`.
pub const ENGINE_VALUE_COLOR: u32 = 8;
/// Значение: вариант перечисления по имени в `s`.
pub const ENGINE_VALUE_ENUM: u32 = 9;
/// Значение: ассет по GUID в `s` (пустая строка — «не назначен»).
pub const ENGINE_VALUE_ASSET: u32 = 10;
/// Значение: объект; `i` — `generation << 32 | index`.
pub const ENGINE_VALUE_OBJECT: u32 = 11;

/// Динамическое значение: поле компонента, аргумент сообщения, ключ сохранения,
/// аргумент или результат сервиса. Нужные члены определяются по `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EngineValue {
    /// Одна из констант `ENGINE_VALUE_*`.
    pub kind: u32,
    pub i: i64,
    pub f: [f32; 4],
    /// Строка для `STR`, `ENUM`, `ASSET`; иначе NULL.
    pub s: *const c_char,
}

// ----------------------------------------------------------------------
// Поля компонента
// ----------------------------------------------------------------------

/// Тип поля компонента: `float`.
pub const ENGINE_FIELD_F32: u32 = 0;
/// Тип поля компонента: `int32_t`.
pub const ENGINE_FIELD_I32: u32 = 1;
/// Тип поля компонента: `bool`.
pub const ENGINE_FIELD_BOOL: u32 = 2;
/// Тип поля компонента: два `float` ([`EngineValue`] VEC2).
pub const ENGINE_FIELD_VEC2: u32 = 3;
/// Тип поля компонента: три `float` ([`EngineVec3`]).
pub const ENGINE_FIELD_VEC3: u32 = 4;
/// Тип поля компонента: четыре `float` — линейный RGBA.
pub const ENGINE_FIELD_COLOR: u32 = 5;
/// Тип поля компонента: строка (`std::string`).
pub const ENGINE_FIELD_STRING: u32 = 6;
/// Тип поля компонента: перечисление (`enum` / `enum class`), в инспекторе — список по именам.
pub const ENGINE_FIELD_ENUM: u32 = 7;
/// Тип поля компонента: ссылка на ассет (GUID в `std::string`).
pub const ENGINE_FIELD_ASSET: u32 = 8;
/// Тип поля компонента: кватернион из четырёх `float` (x, y, z, w).
pub const ENGINE_FIELD_QUAT: u32 = 9;

/// Поле компонента: движок читает и пишет его через `get` / `set` — так поле попадает
/// в инспектор редактора и в файл сцены. Функции генерирует `ENGINE_FIELD(...)`.
#[repr(C)]
pub struct EngineField {
    /// Имя поля (статическая строка).
    pub name: *const c_char,
    /// Одна из констант `ENGINE_FIELD_*`.
    pub kind: u32,
    /// Для `ENGINE_FIELD_ENUM`: имена вариантов (статический массив) и их число.
    pub variants: *const *const c_char,
    pub variant_count: u32,
    /// Для `ENGINE_FIELD_ASSET`: тип ассета (`"Texture"`, `"Prefab"`, `"AudioClip"`…) — по нему
    /// инспектор фильтрует выбор; NULL — любой.
    pub asset_kind: *const c_char,
    /// Прочитать поле в `out`. Строки в `out->s` живут до следующего изменения поля.
    pub get: Option<unsafe extern "C" fn(self_: *const c_void, out: *mut EngineValue)>,
    /// Записать значение. 0 — успех, не ноль — значение не подходит полю.
    pub set: Option<unsafe extern "C" fn(self_: *mut c_void, value: *const EngineValue) -> i32>,
}

/// Игровой компонент на C++ — то же, что `impl Component` в Rust.
/// Любой хук может быть NULL. `self_` — указатель, который вернул `create`.
///
/// Хуки вызываются из движка; внутри хука доступен весь [`EngineApi`]. Вне хуков
/// (в конструкторе компонента, в статических инициализаторах, в `on_load`) функции
/// мира возвращают нейтральные значения и пишут ошибку в лог.
#[repr(C)]
pub struct EngineComponentType {
    /// Имя типа (статическая строка); под ним компонент виден в сценах и в редакторе.
    pub name: *const c_char,
    /// Создать экземпляр со значениями полей по умолчанию.
    pub create: Option<unsafe extern "C" fn() -> *mut c_void>,
    /// Уничтожить экземпляр, созданный `create`.
    pub destroy: Option<unsafe extern "C" fn(self_: *mut c_void)>,
    pub on_start: Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject)>,
    pub on_update: Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject, dt: f32)>,
    pub on_fixed_update: Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject, dt: f32)>,
    pub on_late_update: Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject, dt: f32)>,
    pub on_collision_enter:
        Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject, other: EngineObject)>,
    pub on_trigger_enter:
        Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject, other: EngineObject)>,
    pub on_destroy: Option<unsafe extern "C" fn(self_: *mut c_void, object: EngineObject)>,
    /// Сообщение (`send_message`, таймеры, кнопки интерфейса). `value` живёт только на время вызова.
    pub on_message: Option<
        unsafe extern "C" fn(
            self_: *mut c_void,
            object: EngineObject,
            name: *const c_char,
            value: *const EngineValue,
        ),
    >,
    /// Поля, видимые движку (массив из `field_count` элементов; может быть NULL при нуле).
    pub fields: *const EngineField,
    pub field_count: u32,
}

// ----------------------------------------------------------------------
// Вспомогательные структуры API
// ----------------------------------------------------------------------

/// Время кадра (см. `engine_core::world::Time`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineTime {
    /// Секунд за кадр с учётом `time_scale`.
    pub delta: f32,
    /// Секунд за кадр без `time_scale`.
    pub unscaled_delta: f32,
    pub fixed_delta: f32,
    pub time_scale: f32,
    /// Игровое время с запуска.
    pub elapsed: f64,
    /// Реальное время с запуска.
    pub unscaled_elapsed: f64,
    pub frame: u64,
}

/// Касание экрана.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineTouch {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    /// 0 — началось в этом кадре, 1 — идёт, 2 — закончилось в этом кадре.
    pub phase: u32,
}

/// Параметры твина поля.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EngineTweenParams {
    pub duration: f32,
    pub delay: f32,
    /// Имя кривой (`"Linear"`, `"OutBack"`…); NULL — `Linear`.
    pub ease: *const c_char,
    /// Число проходов; 0 — бесконечно.
    pub loops: u32,
    pub yoyo: i32,
    /// Идти по реальному времени (на паузе тоже).
    pub unscaled: i32,
    /// Сообщение, которое получит `on_complete_target` по завершении; NULL — без сообщения.
    pub on_complete: *const c_char,
    pub on_complete_target: EngineObject,
}

/// Результат `service_call`: успех.
pub const ENGINE_SERVICE_OK: i32 = 0;
/// Результат `service_call`: сервиса с таким именем нет (его не установила игра/фасад).
pub const ENGINE_SERVICE_MISSING: i32 = 1;
/// Результат `service_call`: сервис отказался от аргументов (подробности в логе).
pub const ENGINE_SERVICE_BAD_ARGS: i32 = 2;
/// Результат `service_call`: вызов вне хука компонента, мира нет.
pub const ENGINE_SERVICE_NO_WORLD: i32 = 3;

/// Сервисы движка, доступные плагину. Передаётся в `on_load`, живёт до конца процесса.
///
/// Функции, работающие с миром (все, кроме `log`),
/// можно вызывать только изнутри хуков компонента; вне хука они ничего не делают, пишут
/// ошибку в лог и возвращают нейтральные значения (0, пустую строку, «нет объекта»).
/// Устаревший объект (уже уничтоженный) даёт то же: ошибка в логе и нейтральное значение.
/// `destroy` откладывается до конца кадра, поэтому «уничтожить себя в `on_update`» безопасно.
#[repr(C)]
pub struct EngineApi {
    pub abi_version: u32,
    /// Записать строку в лог движка. `level`: 0 — error, 1 — warn, 2 — info.
    pub log: unsafe extern "C" fn(level: u32, message: *const c_char),

    // ── объект и Transform ────────────────────────────────────────────────
    /// Жив ли объект (не уничтожен). 1 — да, 0 — нет. Не пишет в лог.
    pub is_alive: unsafe extern "C" fn(object: EngineObject) -> i32,
    /// Локальная позиция объекта.
    pub get_position: unsafe extern "C" fn(object: EngineObject) -> EngineVec3,
    pub set_position: unsafe extern "C" fn(object: EngineObject, position: EngineVec3),
    /// Локальный поворот: углы Эйлера в градусах, как в инспекторе.
    pub get_rotation: unsafe extern "C" fn(object: EngineObject) -> EngineVec3,
    pub set_rotation: unsafe extern "C" fn(object: EngineObject, euler_deg: EngineVec3),
    pub get_scale: unsafe extern "C" fn(object: EngineObject) -> EngineVec3,
    pub set_scale: unsafe extern "C" fn(object: EngineObject, scale: EngineVec3),
    /// Мировая позиция (с учётом родителей).
    pub get_world_position: unsafe extern "C" fn(object: EngineObject) -> EngineVec3,
    pub set_world_position: unsafe extern "C" fn(object: EngineObject, position: EngineVec3),
    /// Мировой поворот: углы Эйлера в градусах.
    pub get_world_rotation: unsafe extern "C" fn(object: EngineObject) -> EngineVec3,
    pub set_world_rotation: unsafe extern "C" fn(object: EngineObject, euler_deg: EngineVec3),
    /// Мировое направление оси объекта: 0 — вправо, 1 — вверх, 2 — вперёд (−Z).
    pub get_direction: unsafe extern "C" fn(object: EngineObject, axis: u32) -> EngineVec3,
    /// Повернуть «вперёд» на мировую точку `target` (Unity: `LookAt`).
    pub look_at: unsafe extern "C" fn(object: EngineObject, target: EngineVec3, up: EngineVec3),

    // ── ввод ──────────────────────────────────────────────────────────────
    /// Ось ввода: −1…1 (`"Horizontal"`, `"Vertical"`).
    pub input_axis: unsafe extern "C" fn(name: *const c_char) -> f32,
    /// Кнопка удерживается (`"Jump"`, `"Fire1"`).
    pub input_button: unsafe extern "C" fn(name: *const c_char) -> i32,
    /// Кнопка нажата в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub input_button_down: unsafe extern "C" fn(name: *const c_char) -> i32,
    /// Кнопка отпущена в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub input_button_up: unsafe extern "C" fn(name: *const c_char) -> i32,
    /// Значение кнопки 0…1 (у триггера геймпада — степень нажатия).
    pub input_button_value: unsafe extern "C" fn(name: *const c_char) -> f32,
    /// Клавиша по имени (`"Space"`, `"A"`, `"Digit1"`, `"LShift"`…). `mode`: 0 удерживается,
    /// 1 нажата, 2 отпущена (как у кнопок: в `on_fixed_update` — с прошлого шага).
    pub input_key: unsafe extern "C" fn(name: *const c_char, mode: u32) -> i32,
    /// Кнопка мыши: 0 левая, 1 правая, 2 средняя. `mode` как у `input_key`.
    pub input_mouse_button: unsafe extern "C" fn(button: u32, mode: u32) -> i32,
    /// Вектор ввода (xy; z = 0): 0 — позиция мыши, 1 — смещение мыши, 2 — колесо, 3 — размер окна.
    pub input_vector: unsafe extern "C" fn(kind: u32) -> EngineVec3,
    /// Число подключённых геймпадов.
    pub input_gamepad_count: unsafe extern "C" fn() -> u32,
    /// Кнопка геймпада по имени (`"South"`, `"DPadUp"`…). `pad` — номер от 0 до
    /// `input_gamepad_count()−1`, либо −1 — любой. `mode` как у `input_key`.
    pub input_gamepad_button: unsafe extern "C" fn(pad: i32, name: *const c_char, mode: u32) -> i32,
    /// Ось геймпада по имени (`"LeftStickX"`, `"RightTrigger"`…), сырое значение.
    pub input_gamepad_axis: unsafe extern "C" fn(pad: i32, name: *const c_char) -> f32,
    /// Стик с круговой мёртвой зоной: `which` 0 — левый, 1 — правый. Возвращает xy.
    pub input_gamepad_stick: unsafe extern "C" fn(pad: i32, which: u32, dead_zone: f32) -> EngineVec3,
    /// Число касаний экрана.
    pub input_touch_count: unsafe extern "C" fn() -> u32,
    /// Касание по номеру. 1 — есть, 0 — номер вне диапазона.
    pub input_touch: unsafe extern "C" fn(index: u32, out: *mut EngineTouch) -> i32,
    /// Текст, набранный в этом кадре (с учётом раскладки); возвращает длину.
    pub input_text: unsafe extern "C" fn(buf: *mut c_char, cap: u32) -> u32,

    // ── время ─────────────────────────────────────────────────────────────
    /// Секунд с начала игры.
    pub time_seconds: unsafe extern "C" fn() -> f32,
    pub time_info: unsafe extern "C" fn(out: *mut EngineTime),
    /// 0 — пауза, 1 — норма. Действует со следующего кадра.
    pub time_set_scale: unsafe extern "C" fn(scale: f32),

    // ── объекты и иерархия ────────────────────────────────────────────────
    /// Создать пустой объект в корне сцены.
    pub spawn: unsafe extern "C" fn(name: *const c_char) -> EngineObject,
    /// Первый объект с таким тегом или «нет объекта».
    pub find_by_tag: unsafe extern "C" fn(tag: *const c_char) -> EngineObject,
    pub find_by_name: unsafe extern "C" fn(name: *const c_char) -> EngineObject,
    /// Записать в `out` до `cap` объектов с тегом; вернуть полное число найденных.
    pub find_all_by_tag: unsafe extern "C" fn(tag: *const c_char, out: *mut EngineObject, cap: u32) -> u32,
    /// Записать в `out` до `cap` объектов с компонентом `component`; вернуть полное число.
    pub find_with_component:
        unsafe extern "C" fn(component: *const c_char, out: *mut EngineObject, cap: u32) -> u32,
    /// Потомок по имени (прямой ребёнок).
    pub find_child: unsafe extern "C" fn(parent: EngineObject, name: *const c_char) -> EngineObject,
    /// Уничтожить объект в конце кадра (вместе с детьми).
    pub destroy: unsafe extern "C" fn(object: EngineObject),
    /// Уничтожить через `delay` секунд; возвращает таймер для `cancel_timer`.
    pub destroy_after: unsafe extern "C" fn(object: EngineObject, delay: f32) -> u64,
    /// Создать экземпляр префаба в начале координат (путь относительно каталога ассетов или GUID).
    pub instantiate: unsafe extern "C" fn(prefab: *const c_char) -> EngineObject,
    /// Экземпляр префаба с позой: без `parent` она мировая, с ним — локальная.
    pub instantiate_at: unsafe extern "C" fn(
        prefab: *const c_char,
        position: EngineVec3,
        euler_deg: EngineVec3,
        parent: EngineObject,
    ) -> EngineObject,
    /// Копия живого объекта с потомками.
    pub duplicate: unsafe extern "C" fn(object: EngineObject) -> EngineObject,
    pub get_parent: unsafe extern "C" fn(object: EngineObject) -> EngineObject,
    /// Сменить родителя; `parent` «нет объекта» — вынести в корень. 1 — успех.
    pub set_parent:
        unsafe extern "C" fn(object: EngineObject, parent: EngineObject, keep_world: i32) -> i32,
    pub child_count: unsafe extern "C" fn(object: EngineObject) -> u32,
    pub get_child: unsafe extern "C" fn(object: EngineObject, index: u32) -> EngineObject,
    pub set_active: unsafe extern "C" fn(object: EngineObject, active: i32),
    /// `in_hierarchy` = 0 — собственный флаг, 1 — с учётом предков.
    pub get_active: unsafe extern "C" fn(object: EngineObject, in_hierarchy: i32) -> i32,
    /// Имя, тег: записать в буфер, вернуть полную длину (без нуля).
    pub get_name: unsafe extern "C" fn(object: EngineObject, buf: *mut c_char, cap: u32) -> u32,
    pub set_name: unsafe extern "C" fn(object: EngineObject, name: *const c_char),
    pub get_tag: unsafe extern "C" fn(object: EngineObject, buf: *mut c_char, cap: u32) -> u32,
    pub set_tag: unsafe extern "C" fn(object: EngineObject, tag: *const c_char),
    pub get_layer: unsafe extern "C" fn(object: EngineObject) -> u32,
    pub set_layer: unsafe extern "C" fn(object: EngineObject, layer: u32),
    /// Не уничтожать при смене сцены.
    pub dont_destroy_on_load: unsafe extern "C" fn(object: EngineObject),

    // ── компоненты и поля ─────────────────────────────────────────────────
    /// Добавить компонент по имени типа (Rust или C++). 1 — успех; неизвестный тип — ошибка в логе.
    pub add_component: unsafe extern "C" fn(object: EngineObject, component: *const c_char) -> i32,
    pub remove_component: unsafe extern "C" fn(object: EngineObject, component: *const c_char) -> i32,
    pub has_component: unsafe extern "C" fn(object: EngineObject, component: *const c_char) -> i32,
    /// Прочитать поле `component.field` любого компонента любого типа. 1 — успех.
    pub get_field: unsafe extern "C" fn(
        object: EngineObject,
        component: *const c_char,
        field: *const c_char,
        out: *mut EngineValue,
    ) -> i32,
    /// Записать поле; число приводится к типу поля (`Int` → `Float` и т. д.). 1 — успех.
    pub set_field: unsafe extern "C" fn(
        object: EngineObject,
        component: *const c_char,
        field: *const c_char,
        value: *const EngineValue,
    ) -> i32,

    // ── сообщения, таймеры, твины ─────────────────────────────────────────
    /// Отправить сообщение компонентам объекта (`on_message`) с числом.
    pub send_message: unsafe extern "C" fn(object: EngineObject, name: *const c_char, value: f32),
    /// То же с произвольным значением (NULL — пустое).
    pub send_message_value:
        unsafe extern "C" fn(object: EngineObject, name: *const c_char, value: *const EngineValue),
    /// Сообщение через `delay` секунд. Возвращает таймер (0 — не удалось).
    pub send_after: unsafe extern "C" fn(
        object: EngineObject,
        delay: f32,
        name: *const c_char,
        value: *const EngineValue,
    ) -> u64,
    /// Сообщение каждые `interval` секунд (первое — через `interval`).
    pub send_every: unsafe extern "C" fn(
        object: EngineObject,
        interval: f32,
        name: *const c_char,
        value: *const EngineValue,
    ) -> u64,
    /// Отменить таймер. 1 — был активен.
    pub cancel_timer: unsafe extern "C" fn(timer: u64) -> i32,
    /// Отменить все таймеры, адресованные объекту; вернуть их число.
    pub cancel_timers_of: unsafe extern "C" fn(object: EngineObject) -> u32,
    /// Твин поля по пути `"Компонент.поле"` или `"Компонент.поле.канал"`. Возвращает
    /// твин (0 — не удалось); неверный путь выясняется в первом кадре и пишется в лог.
    pub tween_field: unsafe extern "C" fn(
        object: EngineObject,
        path: *const c_char,
        to: *const EngineValue,
        params: *const EngineTweenParams,
    ) -> u64,
    pub tween_cancel: unsafe extern "C" fn(tween: u64) -> i32,
    pub tween_cancel_of: unsafe extern "C" fn(object: EngineObject) -> u32,

    // ── сцены и выход ─────────────────────────────────────────────────────
    /// Заменить сцену в конце кадра.
    pub load_scene: unsafe extern "C" fn(name: *const c_char),
    pub load_scene_additive: unsafe extern "C" fn(name: *const c_char),
    pub unload_scene: unsafe extern "C" fn(name: *const c_char),
    pub reload_scene: unsafe extern "C" fn(),
    pub scene_name: unsafe extern "C" fn(buf: *mut c_char, cap: u32) -> u32,
    /// Закрыть игру (в редакторе — выйти из Play).
    pub quit: unsafe extern "C" fn(),

    // ── сохранения ────────────────────────────────────────────────────────
    /// Значение ключа. 1 — ключ есть.
    pub save_get: unsafe extern "C" fn(key: *const c_char, out: *mut EngineValue) -> i32,
    /// Записать значение в памяти (на диск — `save_flush`). 1 — успех.
    pub save_set: unsafe extern "C" fn(key: *const c_char, value: *const EngineValue) -> i32,
    pub save_has: unsafe extern "C" fn(key: *const c_char) -> i32,
    pub save_delete: unsafe extern "C" fn(key: *const c_char) -> i32,
    /// Записать сохранения на диск. 1 — успех (ошибка — в логе).
    pub save_flush: unsafe extern "C" fn() -> i32,

    // ── случайные числа ───────────────────────────────────────────────────
    /// Число из [0, 1).
    pub random_f32: unsafe extern "C" fn() -> f32,
    pub random_range: unsafe extern "C" fn(min: f32, max: f32) -> f32,
    /// Целое из [min, max], обе границы входят.
    pub random_int: unsafe extern "C" fn(min: i32, max: i32) -> i32,
    pub random_chance: unsafe extern "C" fn(probability: f32) -> i32,
    pub random_seed: unsafe extern "C" fn(seed: u64),
    /// Точка внутри единичного круга (xy; z = 0).
    pub random_in_unit_circle: unsafe extern "C" fn() -> EngineVec3,
    /// Точка на единичной сфере.
    pub random_on_unit_sphere: unsafe extern "C" fn() -> EngineVec3,

    // ── сервисы (физика, звук, интерфейс…) ────────────────────────────────
    /// Установлен ли сервис `name` (например `"physics3d.raycast"`). Вне хука — 0.
    pub service_available: unsafe extern "C" fn(name: *const c_char) -> i32,
    /// Вызвать сервис, который игра или фасад движка зарегистрировали под этим именем.
    /// Аргументы — `args[0..arg_count]`; результаты записываются в `out[0..*out_count]`
    /// (не больше `out_cap`). Возвращает одну из констант `ENGINE_SERVICE_*`.
    pub service_call: unsafe extern "C" fn(
        name: *const c_char,
        args: *const EngineValue,
        arg_count: u32,
        out: *mut EngineValue,
        out_cap: u32,
        out_count: *mut u32,
    ) -> i32,
}

/// Описание плагина. Плагин экспортирует функцию
/// `const EnginePlugin* engine_plugin_entry(void)`, возвращающую указатель
/// на статический экземпляр этой структуры. Любой колбэк может быть NULL.
#[repr(C)]
pub struct EnginePlugin {
    pub abi_version: u32,
    /// Имя плагина (статическая строка).
    pub name: *const c_char,
    /// Вызывается один раз после загрузки. Не ноль — отказ от загрузки.
    /// Мир здесь недоступен: инициализируйте компоненты в их `on_start`.
    pub on_load: Option<unsafe extern "C" fn(api: *const EngineApi) -> i32>,
    /// Вызывается на каждом фиксированном шаге симуляции (мир доступен).
    pub on_fixed_update: Option<unsafe extern "C" fn(dt: f32)>,
    /// Обработать RGBA8-изображение на месте (CPU пост-эффект).
    /// `pixels` указывает на `width * height * 4` байт.
    pub process_rgba: Option<unsafe extern "C" fn(pixels: *mut u8, width: u32, height: u32)>,
    /// Вызывается перед выгрузкой библиотеки.
    pub on_unload: Option<unsafe extern "C" fn()>,
    /// Игровые компоненты плагина (массив из `component_count` элементов).
    pub components: *const EngineComponentType,
    pub component_count: u32,
}

/// Сигнатура точки входа плагина — символ `engine_plugin_entry`.
pub type EnginePluginEntry = unsafe extern "C" fn() -> *const EnginePlugin;
