/* Piska Engine — C ABI для плагинов. */

#ifndef ENGINE_API_H
#define ENGINE_API_H

/* Сгенерировано cbindgen из crates/engine-cpp-bridge/src/abi.rs. Не редактировать вручную. */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Версия ABI. Плагин с другой версией не загружается: его нужно пересобрать.
 */
#define ENGINE_ABI_VERSION 3

/**
 * Значение: ничего.
 */
#define ENGINE_VALUE_NIL 0

/**
 * Значение: `bool` в `i` (0 или 1).
 */
#define ENGINE_VALUE_BOOL 1

/**
 * Значение: целое в `i`.
 */
#define ENGINE_VALUE_INT 2

/**
 * Значение: `float` в `f[0]`.
 */
#define ENGINE_VALUE_FLOAT 3

/**
 * Значение: строка UTF-8 в `s`.
 */
#define ENGINE_VALUE_STR 4

/**
 * Значение: два `float` в `f[0..2]`.
 */
#define ENGINE_VALUE_VEC2 5

/**
 * Значение: три `float` в `f[0..3]`.
 */
#define ENGINE_VALUE_VEC3 6

/**
 * Значение: кватернион (x, y, z, w) в `f[0..4]`.
 */
#define ENGINE_VALUE_QUAT 7

/**
 * Значение: линейный RGBA в `f[0..4]`.
 */
#define ENGINE_VALUE_COLOR 8

/**
 * Значение: вариант перечисления по имени в `s`.
 */
#define ENGINE_VALUE_ENUM 9

/**
 * Значение: ассет по GUID в `s` (пустая строка — «не назначен»).
 */
#define ENGINE_VALUE_ASSET 10

/**
 * Значение: объект; `i` — `generation << 32 | index`.
 */
#define ENGINE_VALUE_OBJECT 11

/**
 * Тип поля компонента: `float`.
 */
#define ENGINE_FIELD_F32 0

/**
 * Тип поля компонента: `int32_t`.
 */
#define ENGINE_FIELD_I32 1

/**
 * Тип поля компонента: `bool`.
 */
#define ENGINE_FIELD_BOOL 2

/**
 * Тип поля компонента: два `float` ([`EngineValue`] VEC2).
 */
#define ENGINE_FIELD_VEC2 3

/**
 * Тип поля компонента: три `float` ([`EngineVec3`]).
 */
#define ENGINE_FIELD_VEC3 4

/**
 * Тип поля компонента: четыре `float` — линейный RGBA.
 */
#define ENGINE_FIELD_COLOR 5

/**
 * Тип поля компонента: строка (`std::string`).
 */
#define ENGINE_FIELD_STRING 6

/**
 * Тип поля компонента: перечисление (`enum` / `enum class`), в инспекторе — список по именам.
 */
#define ENGINE_FIELD_ENUM 7

/**
 * Тип поля компонента: ссылка на ассет (GUID в `std::string`).
 */
#define ENGINE_FIELD_ASSET 8

/**
 * Тип поля компонента: кватернион из четырёх `float` (x, y, z, w).
 */
#define ENGINE_FIELD_QUAT 9

/**
 * Результат `service_call`: успех.
 */
#define ENGINE_SERVICE_OK 0

/**
 * Результат `service_call`: сервиса с таким именем нет (его не установила игра/фасад).
 */
#define ENGINE_SERVICE_MISSING 1

/**
 * Результат `service_call`: сервис отказался от аргументов (подробности в логе).
 */
#define ENGINE_SERVICE_BAD_ARGS 2

/**
 * Результат `service_call`: вызов вне хука компонента, мира нет.
 */
#define ENGINE_SERVICE_NO_WORLD 3

/**
 * Объект сцены: индекс и поколение. `generation == 0` — «нет объекта».
 * Устаревший (уничтоженный) объект движок распознаёт по поколению.
 */
typedef struct {
  uint32_t index;
  uint32_t generation;
} EngineObject;

typedef struct {
  float x;
  float y;
  float z;
} EngineVec3;

/**
 * Касание экрана.
 */
typedef struct {
  uint64_t id;
  float x;
  float y;
  float dx;
  float dy;
  /**
   * 0 — началось в этом кадре, 1 — идёт, 2 — закончилось в этом кадре.
   */
  uint32_t phase;
} EngineTouch;

/**
 * Время кадра (см. `engine_core::world::Time`).
 */
typedef struct {
  /**
   * Секунд за кадр с учётом `time_scale`.
   */
  float delta;
  /**
   * Секунд за кадр без `time_scale`.
   */
  float unscaled_delta;
  float fixed_delta;
  float time_scale;
  /**
   * Игровое время с запуска.
   */
  double elapsed;
  /**
   * Реальное время с запуска.
   */
  double unscaled_elapsed;
  uint64_t frame;
} EngineTime;

/**
 * Динамическое значение: поле компонента, аргумент сообщения, ключ сохранения,
 * аргумент или результат сервиса. Нужные члены определяются по `kind`.
 */
typedef struct {
  /**
   * Одна из констант `ENGINE_VALUE_*`.
   */
  uint32_t kind;
  int64_t i;
  float f[4];
  /**
   * Строка для `STR`, `ENUM`, `ASSET`; иначе NULL.
   */
  const char *s;
} EngineValue;

/**
 * Параметры твина поля.
 */
typedef struct {
  float duration;
  float delay;
  /**
   * Имя кривой (`"Linear"`, `"OutBack"`…); NULL — `Linear`.
   */
  const char *ease;
  /**
   * Число проходов; 0 — бесконечно.
   */
  uint32_t loops;
  int32_t yoyo;
  /**
   * Идти по реальному времени (на паузе тоже).
   */
  int32_t unscaled;
  /**
   * Сообщение, которое получит `on_complete_target` по завершении; NULL — без сообщения.
   */
  const char *on_complete;
  EngineObject on_complete_target;
} EngineTweenParams;

/**
 * Сервисы движка, доступные плагину. Передаётся в `on_load`, живёт до конца процесса.
 *
 * Функции, работающие с миром (все, кроме `log`),
 * можно вызывать только изнутри хуков компонента; вне хука они ничего не делают, пишут
 * ошибку в лог и возвращают нейтральные значения (0, пустую строку, «нет объекта»).
 * Устаревший объект (уже уничтоженный) даёт то же: ошибка в логе и нейтральное значение.
 * `destroy` откладывается до конца кадра, поэтому «уничтожить себя в `on_update`» безопасно.
 */
typedef struct {
  uint32_t abi_version;
  /**
   * Записать строку в лог движка. `level`: 0 — error, 1 — warn, 2 — info.
   */
  void (*log)(uint32_t level, const char *message);
  /**
   * Жив ли объект (не уничтожен). 1 — да, 0 — нет. Не пишет в лог.
   */
  int32_t (*is_alive)(EngineObject object);
  /**
   * Локальная позиция объекта.
   */
  EngineVec3 (*get_position)(EngineObject object);
  void (*set_position)(EngineObject object, EngineVec3 position);
  /**
   * Локальный поворот: углы Эйлера в градусах, как в инспекторе.
   */
  EngineVec3 (*get_rotation)(EngineObject object);
  void (*set_rotation)(EngineObject object, EngineVec3 euler_deg);
  EngineVec3 (*get_scale)(EngineObject object);
  void (*set_scale)(EngineObject object, EngineVec3 scale);
  /**
   * Мировая позиция (с учётом родителей).
   */
  EngineVec3 (*get_world_position)(EngineObject object);
  void (*set_world_position)(EngineObject object, EngineVec3 position);
  /**
   * Мировой поворот: углы Эйлера в градусах.
   */
  EngineVec3 (*get_world_rotation)(EngineObject object);
  void (*set_world_rotation)(EngineObject object, EngineVec3 euler_deg);
  /**
   * Мировое направление оси объекта: 0 — вправо, 1 — вверх, 2 — вперёд (−Z).
   */
  EngineVec3 (*get_direction)(EngineObject object, uint32_t axis);
  /**
   * Повернуть «вперёд» на мировую точку `target` (Unity: `LookAt`).
   */
  void (*look_at)(EngineObject object, EngineVec3 target, EngineVec3 up);
  /**
   * Ось ввода: −1…1 (`"Horizontal"`, `"Vertical"`).
   */
  float (*input_axis)(const char *name);
  /**
   * Кнопка удерживается (`"Jump"`, `"Fire1"`).
   */
  int32_t (*input_button)(const char *name);
  /**
   * Кнопка нажата в этом кадре (в `on_fixed_update` — с прошлого шага).
   */
  int32_t (*input_button_down)(const char *name);
  /**
   * Кнопка отпущена в этом кадре (в `on_fixed_update` — с прошлого шага).
   */
  int32_t (*input_button_up)(const char *name);
  /**
   * Значение кнопки 0…1 (у триггера геймпада — степень нажатия).
   */
  float (*input_button_value)(const char *name);
  /**
   * Клавиша по имени (`"Space"`, `"A"`, `"Digit1"`, `"LShift"`…). `mode`: 0 удерживается,
   * 1 нажата, 2 отпущена (как у кнопок: в `on_fixed_update` — с прошлого шага).
   */
  int32_t (*input_key)(const char *name, uint32_t mode);
  /**
   * Кнопка мыши: 0 левая, 1 правая, 2 средняя. `mode` как у `input_key`.
   */
  int32_t (*input_mouse_button)(uint32_t button, uint32_t mode);
  /**
   * Вектор ввода (xy; z = 0): 0 — позиция мыши, 1 — смещение мыши, 2 — колесо, 3 — размер окна.
   */
  EngineVec3 (*input_vector)(uint32_t kind);
  /**
   * Число подключённых геймпадов.
   */
  uint32_t (*input_gamepad_count)(void);
  /**
   * Кнопка геймпада по имени (`"South"`, `"DPadUp"`…). `pad` — номер от 0 до
   * `input_gamepad_count()−1`, либо −1 — любой. `mode` как у `input_key`.
   */
  int32_t (*input_gamepad_button)(int32_t pad, const char *name, uint32_t mode);
  /**
   * Ось геймпада по имени (`"LeftStickX"`, `"RightTrigger"`…), сырое значение.
   */
  float (*input_gamepad_axis)(int32_t pad, const char *name);
  /**
   * Стик с круговой мёртвой зоной: `which` 0 — левый, 1 — правый. Возвращает xy.
   */
  EngineVec3 (*input_gamepad_stick)(int32_t pad, uint32_t which, float dead_zone);
  /**
   * Число касаний экрана.
   */
  uint32_t (*input_touch_count)(void);
  /**
   * Касание по номеру. 1 — есть, 0 — номер вне диапазона.
   */
  int32_t (*input_touch)(uint32_t index, EngineTouch *out);
  /**
   * Текст, набранный в этом кадре (с учётом раскладки); возвращает длину.
   */
  uint32_t (*input_text)(char *buf, uint32_t cap);
  /**
   * Секунд с начала игры.
   */
  float (*time_seconds)(void);
  void (*time_info)(EngineTime *out);
  /**
   * 0 — пауза, 1 — норма. Действует со следующего кадра.
   */
  void (*time_set_scale)(float scale);
  /**
   * Создать пустой объект в корне сцены.
   */
  EngineObject (*spawn)(const char *name);
  /**
   * Первый объект с таким тегом или «нет объекта».
   */
  EngineObject (*find_by_tag)(const char *tag);
  EngineObject (*find_by_name)(const char *name);
  /**
   * Записать в `out` до `cap` объектов с тегом; вернуть полное число найденных.
   */
  uint32_t (*find_all_by_tag)(const char *tag, EngineObject *out, uint32_t cap);
  /**
   * Записать в `out` до `cap` объектов с компонентом `component`; вернуть полное число.
   */
  uint32_t (*find_with_component)(const char *component, EngineObject *out, uint32_t cap);
  /**
   * Потомок по имени (прямой ребёнок).
   */
  EngineObject (*find_child)(EngineObject parent, const char *name);
  /**
   * Уничтожить объект в конце кадра (вместе с детьми).
   */
  void (*destroy)(EngineObject object);
  /**
   * Уничтожить через `delay` секунд; возвращает таймер для `cancel_timer`.
   */
  uint64_t (*destroy_after)(EngineObject object, float delay);
  /**
   * Создать экземпляр префаба в начале координат (путь относительно каталога ассетов или GUID).
   */
  EngineObject (*instantiate)(const char *prefab);
  /**
   * Экземпляр префаба с позой: без `parent` она мировая, с ним — локальная.
   */
  EngineObject (*instantiate_at)(const char *prefab,
                                 EngineVec3 position,
                                 EngineVec3 euler_deg,
                                 EngineObject parent);
  /**
   * Копия живого объекта с потомками.
   */
  EngineObject (*duplicate)(EngineObject object);
  EngineObject (*get_parent)(EngineObject object);
  /**
   * Сменить родителя; `parent` «нет объекта» — вынести в корень. 1 — успех.
   */
  int32_t (*set_parent)(EngineObject object, EngineObject parent, int32_t keep_world);
  uint32_t (*child_count)(EngineObject object);
  EngineObject (*get_child)(EngineObject object, uint32_t index);
  void (*set_active)(EngineObject object, int32_t active);
  /**
   * `in_hierarchy` = 0 — собственный флаг, 1 — с учётом предков.
   */
  int32_t (*get_active)(EngineObject object, int32_t in_hierarchy);
  /**
   * Имя, тег: записать в буфер, вернуть полную длину (без нуля).
   */
  uint32_t (*get_name)(EngineObject object, char *buf, uint32_t cap);
  void (*set_name)(EngineObject object, const char *name);
  uint32_t (*get_tag)(EngineObject object, char *buf, uint32_t cap);
  void (*set_tag)(EngineObject object, const char *tag);
  uint32_t (*get_layer)(EngineObject object);
  void (*set_layer)(EngineObject object, uint32_t layer);
  /**
   * Не уничтожать при смене сцены.
   */
  void (*dont_destroy_on_load)(EngineObject object);
  /**
   * Добавить компонент по имени типа (Rust или C++). 1 — успех; неизвестный тип — ошибка в логе.
   */
  int32_t (*add_component)(EngineObject object, const char *component);
  int32_t (*remove_component)(EngineObject object, const char *component);
  int32_t (*has_component)(EngineObject object, const char *component);
  /**
   * Прочитать поле `component.field` любого компонента любого типа. 1 — успех.
   */
  int32_t (*get_field)(EngineObject object,
                       const char *component,
                       const char *field,
                       EngineValue *out);
  /**
   * Записать поле; число приводится к типу поля (`Int` → `Float` и т. д.). 1 — успех.
   */
  int32_t (*set_field)(EngineObject object,
                       const char *component,
                       const char *field,
                       const EngineValue *value);
  /**
   * Отправить сообщение компонентам объекта (`on_message`) с числом.
   */
  void (*send_message)(EngineObject object, const char *name, float value);
  /**
   * То же с произвольным значением (NULL — пустое).
   */
  void (*send_message_value)(EngineObject object, const char *name, const EngineValue *value);
  /**
   * Сообщение через `delay` секунд. Возвращает таймер (0 — не удалось).
   */
  uint64_t (*send_after)(EngineObject object,
                         float delay,
                         const char *name,
                         const EngineValue *value);
  /**
   * Сообщение каждые `interval` секунд (первое — через `interval`).
   */
  uint64_t (*send_every)(EngineObject object,
                         float interval,
                         const char *name,
                         const EngineValue *value);
  /**
   * Отменить таймер. 1 — был активен.
   */
  int32_t (*cancel_timer)(uint64_t timer);
  /**
   * Отменить все таймеры, адресованные объекту; вернуть их число.
   */
  uint32_t (*cancel_timers_of)(EngineObject object);
  /**
   * Твин поля по пути `"Компонент.поле"` или `"Компонент.поле.канал"`. Возвращает
   * твин (0 — не удалось); неверный путь выясняется в первом кадре и пишется в лог.
   */
  uint64_t (*tween_field)(EngineObject object,
                          const char *path,
                          const EngineValue *to,
                          const EngineTweenParams *params);
  int32_t (*tween_cancel)(uint64_t tween);
  uint32_t (*tween_cancel_of)(EngineObject object);
  /**
   * Заменить сцену в конце кадра.
   */
  void (*load_scene)(const char *name);
  void (*load_scene_additive)(const char *name);
  void (*unload_scene)(const char *name);
  void (*reload_scene)(void);
  uint32_t (*scene_name)(char *buf, uint32_t cap);
  /**
   * Закрыть игру (в редакторе — выйти из Play).
   */
  void (*quit)(void);
  /**
   * Значение ключа. 1 — ключ есть.
   */
  int32_t (*save_get)(const char *key, EngineValue *out);
  /**
   * Записать значение в памяти (на диск — `save_flush`). 1 — успех.
   */
  int32_t (*save_set)(const char *key, const EngineValue *value);
  int32_t (*save_has)(const char *key);
  int32_t (*save_delete)(const char *key);
  /**
   * Записать сохранения на диск. 1 — успех (ошибка — в логе).
   */
  int32_t (*save_flush)(void);
  /**
   * Число из [0, 1).
   */
  float (*random_f32)(void);
  float (*random_range)(float min, float max);
  /**
   * Целое из [min, max], обе границы входят.
   */
  int32_t (*random_int)(int32_t min, int32_t max);
  int32_t (*random_chance)(float probability);
  void (*random_seed)(uint64_t seed);
  /**
   * Точка внутри единичного круга (xy; z = 0).
   */
  EngineVec3 (*random_in_unit_circle)(void);
  /**
   * Точка на единичной сфере.
   */
  EngineVec3 (*random_on_unit_sphere)(void);
  /**
   * Установлен ли сервис `name` (например `"physics3d.raycast"`). Вне хука — 0.
   */
  int32_t (*service_available)(const char *name);
  /**
   * Вызвать сервис, который игра или фасад движка зарегистрировали под этим именем.
   * Аргументы — `args[0..arg_count]`; результаты записываются в `out[0..*out_count]`
   * (не больше `out_cap`). Возвращает одну из констант `ENGINE_SERVICE_*`.
   */
  int32_t (*service_call)(const char *name,
                          const EngineValue *args,
                          uint32_t arg_count,
                          EngineValue *out,
                          uint32_t out_cap,
                          uint32_t *out_count);
} EngineApi;

/**
 * Поле компонента: движок читает и пишет его через `get` / `set` — так поле попадает
 * в инспектор редактора и в файл сцены. Функции генерирует `ENGINE_FIELD(...)`.
 */
typedef struct {
  /**
   * Имя поля (статическая строка).
   */
  const char *name;
  /**
   * Одна из констант `ENGINE_FIELD_*`.
   */
  uint32_t kind;
  /**
   * Для `ENGINE_FIELD_ENUM`: имена вариантов (статический массив) и их число.
   */
  const char *const *variants;
  uint32_t variant_count;
  /**
   * Для `ENGINE_FIELD_ASSET`: тип ассета (`"Texture"`, `"Prefab"`, `"AudioClip"`…) — по нему
   * инспектор фильтрует выбор; NULL — любой.
   */
  const char *asset_kind;
  /**
   * Прочитать поле в `out`. Строки в `out->s` живут до следующего изменения поля.
   */
  void (*get)(const void *self_, EngineValue *out);
  /**
   * Записать значение. 0 — успех, не ноль — значение не подходит полю.
   */
  int32_t (*set)(void *self_, const EngineValue *value);
} EngineField;

/**
 * Игровой компонент на C++ — то же, что `impl Component` в Rust.
 * Любой хук может быть NULL. `self_` — указатель, который вернул `create`.
 *
 * Хуки вызываются из движка; внутри хука доступен весь [`EngineApi`]. Вне хуков
 * (в конструкторе компонента, в статических инициализаторах, в `on_load`) функции
 * мира возвращают нейтральные значения и пишут ошибку в лог.
 */
typedef struct {
  /**
   * Имя типа (статическая строка); под ним компонент виден в сценах и в редакторе.
   */
  const char *name;
  /**
   * Создать экземпляр со значениями полей по умолчанию.
   */
  void *(*create)(void);
  /**
   * Уничтожить экземпляр, созданный `create`.
   */
  void (*destroy)(void *self_);
  void (*on_start)(void *self_, EngineObject object);
  void (*on_update)(void *self_, EngineObject object, float dt);
  void (*on_fixed_update)(void *self_, EngineObject object, float dt);
  void (*on_late_update)(void *self_, EngineObject object, float dt);
  void (*on_collision_enter)(void *self_, EngineObject object, EngineObject other);
  void (*on_trigger_enter)(void *self_, EngineObject object, EngineObject other);
  void (*on_destroy)(void *self_, EngineObject object);
  /**
   * Сообщение (`send_message`, таймеры, кнопки интерфейса). `value` живёт только на время вызова.
   */
  void (*on_message)(void *self_, EngineObject object, const char *name, const EngineValue *value);
  /**
   * Поля, видимые движку (массив из `field_count` элементов; может быть NULL при нуле).
   */
  const EngineField *fields;
  uint32_t field_count;
} EngineComponentType;

/**
 * Описание плагина. Плагин экспортирует функцию
 * `const EnginePlugin* engine_plugin_entry(void)`, возвращающую указатель
 * на статический экземпляр этой структуры. Любой колбэк может быть NULL.
 */
typedef struct {
  uint32_t abi_version;
  /**
   * Имя плагина (статическая строка).
   */
  const char *name;
  /**
   * Вызывается один раз после загрузки. Не ноль — отказ от загрузки.
   * Мир здесь недоступен: инициализируйте компоненты в их `on_start`.
   */
  int32_t (*on_load)(const EngineApi *api);
  /**
   * Вызывается на каждом фиксированном шаге симуляции (мир доступен).
   */
  void (*on_fixed_update)(float dt);
  /**
   * Обработать RGBA8-изображение на месте (CPU пост-эффект).
   * `pixels` указывает на `width * height * 4` байт.
   */
  void (*process_rgba)(uint8_t *pixels, uint32_t width, uint32_t height);
  /**
   * Вызывается перед выгрузкой библиотеки.
   */
  void (*on_unload)(void);
  /**
   * Игровые компоненты плагина (массив из `component_count` элементов).
   */
  const EngineComponentType *components;
  uint32_t component_count;
} EnginePlugin;

#endif  /* ENGINE_API_H */
