// Игровая логика на C++ — удобная обёртка над C ABI из engine_api.h (ABI v3).
//
// Компонент — обычный класс с хуками, как `impl Component` в Rust:
//
//     struct Spinner : engine::Component {
//         float speed_deg = 90.0f;
//         void on_update(engine::Object self, float dt) override {
//             auto r = self.rotation();
//             self.set_rotation({r.x, r.y + speed_deg * dt, r.z});
//         }
//     };
//     ENGINE_FIELDS(Spinner, ENGINE_FIELD(Spinner, speed_deg, ENGINE_FIELD_F32));
//     ENGINE_COMPONENT(Spinner);
//
// Каждый .cpp в plugins/cpp-gameplay/scripts/ — отдельный скрипт; точка входа плагина
// (gameplay.cpp) собирает все компоненты сама. Полное руководство — docs/api/cpp.md.
//
// Правила, которые держит этот заголовок:
//  * через границу идут только C-типы; исключения C++ гасятся в переходниках хуков
//    (в лог идёт «C++ exception in Type::hook: …»), в Rust они не попадают;
//  * строки — UTF-8, `const char*` с нулём на конце (нулевой указатель = пустая строка);
//  * функции мира работают только изнутри хуков (on_start, on_update, …). В конструкторе
//    компонента, в статических инициализаторах и в on_load мира нет: вызов вернёт нейтральное
//    значение и запишет ошибку в лог — инициализацию переносите в on_start;
//  * устаревший (уничтоженный) объект и объект «нет объекта» безопасны: ошибка в логе и
//    нейтральный результат. `Object::alive()` проверяет, жив ли объект;
//  * `destroy` откладывается до конца кадра, поэтому уничтожить себя в on_update можно.

#ifndef ENGINE_COMPONENT_HPP
#define ENGINE_COMPONENT_HPP

#include "engine_api.h"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstdarg>
#include <exception>
#include <initializer_list>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

#if defined(_WIN32)
#  define ENGINE_EXPORT __declspec(dllexport)
#else
#  define ENGINE_EXPORT __attribute__((visibility("default")))
#endif

#if defined(__GNUC__) || defined(__clang__)
#  define ENGINE_PRINTF_LIKE(fmt, args) __attribute__((format(printf, fmt, args)))
#else
#  define ENGINE_PRINTF_LIKE(fmt, args)
#endif

namespace engine {

// Сервисы движка. Заполняется при загрузке плагина (ENGINE_PLUGIN делает это сам).
inline const EngineApi* api = nullptr;

namespace detail {
[[noreturn]] inline void no_api() {
    std::fputs("engine: the C++ API was used before the plugin was loaded (static initializer?). "
               "Move the call into a component hook such as on_start.\n", stderr);
    std::abort();
}
}  // namespace detail

// Таблица сервисов; до загрузки плагина обращаться к ней нельзя.
inline const EngineApi& A() noexcept {
    if (!api) detail::no_api();
    return *api;
}

// ─────────────────────────────────────────────────────────────────────────────
// Математика
// ─────────────────────────────────────────────────────────────────────────────

struct Vec2 {
    float x = 0.0f, y = 0.0f;
    constexpr Vec2() = default;
    constexpr Vec2(float x_, float y_) : x(x_), y(y_) {}
    Vec2 operator+(Vec2 o) const { return {x + o.x, y + o.y}; }
    Vec2 operator-(Vec2 o) const { return {x - o.x, y - o.y}; }
    Vec2 operator*(float k) const { return {x * k, y * k}; }
    Vec2 operator/(float k) const { return {x / k, y / k}; }
    Vec2 operator-() const { return {-x, -y}; }
    float length() const { return std::sqrt(x * x + y * y); }
    float dot(Vec2 o) const { return x * o.x + y * o.y; }
    // Единичный вектор; у нулевого — нулевой.
    Vec2 normalized() const { const float l = length(); return l > 1e-9f ? *this / l : Vec2{}; }
};

struct Vec3 {
    float x = 0.0f, y = 0.0f, z = 0.0f;
    constexpr Vec3() = default;
    constexpr Vec3(float x_, float y_, float z_) : x(x_), y(y_), z(z_) {}
    constexpr Vec3(const EngineVec3& v) : x(v.x), y(v.y), z(v.z) {}
    constexpr operator EngineVec3() const { return EngineVec3{x, y, z}; }
    Vec3 operator+(Vec3 o) const { return {x + o.x, y + o.y, z + o.z}; }
    Vec3 operator-(Vec3 o) const { return {x - o.x, y - o.y, z - o.z}; }
    Vec3 operator*(float k) const { return {x * k, y * k, z * k}; }
    Vec3 operator/(float k) const { return {x / k, y / k, z / k}; }
    Vec3 operator-() const { return {-x, -y, -z}; }
    Vec3& operator+=(Vec3 o) { x += o.x; y += o.y; z += o.z; return *this; }
    Vec3& operator-=(Vec3 o) { x -= o.x; y -= o.y; z -= o.z; return *this; }
    Vec3& operator*=(float k) { x *= k; y *= k; z *= k; return *this; }
    float length() const { return std::sqrt(x * x + y * y + z * z); }
    float dot(Vec3 o) const { return x * o.x + y * o.y + z * o.z; }
    Vec3 cross(Vec3 o) const { return {y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x}; }
    Vec3 normalized() const { const float l = length(); return l > 1e-9f ? *this / l : Vec3{}; }
    float distance(Vec3 o) const { return (*this - o).length(); }
    static Vec3 lerp(Vec3 a, Vec3 b, float t) { return a + (b - a) * t; }
    // x и y как плоский вектор.
    Vec2 xy() const { return {x, y}; }
};
inline Vec3 operator*(float k, Vec3 v) { return v * k; }
inline Vec2 operator*(float k, Vec2 v) { return v * k; }

// Линейный RGBA, как `Color` в движке.
struct Color {
    float r = 1.0f, g = 1.0f, b = 1.0f, a = 1.0f;
    constexpr Color() = default;
    constexpr Color(float r_, float g_, float b_, float a_ = 1.0f) : r(r_), g(g_), b(b_), a(a_) {}
};

// Кватернион (x, y, z, w).
struct Quat {
    float x = 0.0f, y = 0.0f, z = 0.0f, w = 1.0f;
    constexpr Quat() = default;
    constexpr Quat(float x_, float y_, float z_, float w_) : x(x_), y(y_), z(z_), w(w_) {}
};

// ─────────────────────────────────────────────────────────────────────────────
// Лог
// ─────────────────────────────────────────────────────────────────────────────

inline void log_info(const char* message) noexcept { if (api) api->log(2, message ? message : ""); }
inline void log_warn(const char* message) noexcept { if (api) api->log(1, message ? message : ""); }
inline void log_error(const char* message) noexcept { if (api) api->log(0, message ? message : ""); }
inline void log_info(const std::string& message) noexcept { log_info(message.c_str()); }
inline void log_warn(const std::string& message) noexcept { log_warn(message.c_str()); }
inline void log_error(const std::string& message) noexcept { log_error(message.c_str()); }

// printf-стиль: log_infof("score = %d", score);
inline void log_infof(const char* fmt, ...) noexcept ENGINE_PRINTF_LIKE(1, 2);
inline void log_warnf(const char* fmt, ...) noexcept ENGINE_PRINTF_LIKE(1, 2);
inline void log_errorf(const char* fmt, ...) noexcept ENGINE_PRINTF_LIKE(1, 2);

namespace detail {
inline void vlog(std::uint32_t level, const char* fmt, va_list args) noexcept {
    char buffer[512];
    std::vsnprintf(buffer, sizeof buffer, fmt ? fmt : "", args);
    if (api) api->log(level, buffer);
}
}  // namespace detail

inline void log_infof(const char* fmt, ...) noexcept {
    va_list args; va_start(args, fmt); detail::vlog(2, fmt, args); va_end(args);
}
inline void log_warnf(const char* fmt, ...) noexcept {
    va_list args; va_start(args, fmt); detail::vlog(1, fmt, args); va_end(args);
}
inline void log_errorf(const char* fmt, ...) noexcept {
    va_list args; va_start(args, fmt); detail::vlog(0, fmt, args); va_end(args);
}

// ─────────────────────────────────────────────────────────────────────────────
// Value — динамическое значение: поле компонента, аргумент сообщения, ключ сохранения
// ─────────────────────────────────────────────────────────────────────────────

struct Object;

class Value {
public:
    enum class Kind : std::uint32_t {
        Nil = ENGINE_VALUE_NIL, Bool = ENGINE_VALUE_BOOL, Int = ENGINE_VALUE_INT,
        Float = ENGINE_VALUE_FLOAT, Str = ENGINE_VALUE_STR, Vec2 = ENGINE_VALUE_VEC2,
        Vec3 = ENGINE_VALUE_VEC3, Quat = ENGINE_VALUE_QUAT, Color = ENGINE_VALUE_COLOR,
        Enum = ENGINE_VALUE_ENUM, Asset = ENGINE_VALUE_ASSET, Object = ENGINE_VALUE_OBJECT,
    };

    Value() = default;
    Value(bool v) : kind_(Kind::Bool), i_(v ? 1 : 0) {}
    Value(int v) : kind_(Kind::Int), i_(v) {}
    Value(long long v) : kind_(Kind::Int), i_(v) {}
    Value(float v) : kind_(Kind::Float) { f_[0] = v; }
    Value(double v) : kind_(Kind::Float) { f_[0] = static_cast<float>(v); }
    Value(const char* v) : kind_(Kind::Str), s_(v ? v : "") {}
    Value(std::string v) : kind_(Kind::Str), s_(std::move(v)) {}
    Value(Vec2 v) : kind_(Kind::Vec2) { f_[0] = v.x; f_[1] = v.y; }
    Value(Vec3 v) : kind_(Kind::Vec3) { f_[0] = v.x; f_[1] = v.y; f_[2] = v.z; }
    Value(Quat v) : kind_(Kind::Quat) { f_[0] = v.x; f_[1] = v.y; f_[2] = v.z; f_[3] = v.w; }
    Value(Color v) : kind_(Kind::Color) { f_[0] = v.r; f_[1] = v.g; f_[2] = v.b; f_[3] = v.a; }
    inline Value(Object v);  // объект (определён ниже)

    // Вариант перечисления по имени и ассет по GUID — отдельными конструкторами:
    // по типу их от строки не отличить.
    static Value enum_name(const char* name) { Value v; v.kind_ = Kind::Enum; v.s_ = name ? name : ""; return v; }
    static Value asset(const char* guid) { Value v; v.kind_ = Kind::Asset; v.s_ = guid ? guid : ""; return v; }

    Kind kind() const noexcept { return kind_; }
    bool is_nil() const noexcept { return kind_ == Kind::Nil; }

    // Приведения: если значение другого вида, возвращается `fallback`.
    float to_float(float fallback = 0.0f) const noexcept {
        switch (kind_) {
            case Kind::Float: return f_[0];
            case Kind::Int: case Kind::Bool: return static_cast<float>(i_);
            default: return fallback;
        }
    }
    int to_int(int fallback = 0) const noexcept {
        switch (kind_) {
            case Kind::Int: case Kind::Bool: return static_cast<int>(i_);
            case Kind::Float: return static_cast<int>(std::lround(f_[0]));
            default: return fallback;
        }
    }
    std::int64_t to_int64(std::int64_t fallback = 0) const noexcept {
        return (kind_ == Kind::Int || kind_ == Kind::Bool || kind_ == Kind::Object) ? i_ : fallback;
    }
    bool to_bool(bool fallback = false) const noexcept {
        switch (kind_) {
            case Kind::Bool: case Kind::Int: return i_ != 0;
            case Kind::Float: return f_[0] != 0.0f;
            default: return fallback;
        }
    }
    // Текст строки, перечисления или ассета; у остальных видов — `fallback`.
    std::string to_string(const std::string& fallback = std::string()) const {
        return (kind_ == Kind::Str || kind_ == Kind::Enum || kind_ == Kind::Asset) ? s_ : fallback;
    }
    Vec2 to_vec2() const noexcept { return (kind_ == Kind::Vec2 || kind_ == Kind::Vec3) ? Vec2{f_[0], f_[1]} : Vec2{}; }
    Vec3 to_vec3() const noexcept {
        if (kind_ == Kind::Vec3 || kind_ == Kind::Color) return Vec3{f_[0], f_[1], f_[2]};
        if (kind_ == Kind::Vec2) return Vec3{f_[0], f_[1], 0.0f};
        return Vec3{};
    }
    Quat to_quat() const noexcept { return kind_ == Kind::Quat ? Quat{f_[0], f_[1], f_[2], f_[3]} : Quat{}; }
    Color to_color() const noexcept {
        if (kind_ == Kind::Color) return Color{f_[0], f_[1], f_[2], f_[3]};
        if (kind_ == Kind::Vec3) return Color{f_[0], f_[1], f_[2], 1.0f};
        return Color{};
    }
    inline Object to_object() const noexcept;  // объект или «нет объекта»

    // Значение для ABI. Строка внутри указывает в этот Value: он должен пережить вызов.
    EngineValue pack() const noexcept {
        EngineValue v{};
        v.kind = static_cast<std::uint32_t>(kind_);
        v.i = i_;
        for (int n = 0; n < 4; ++n) v.f[n] = f_[n];
        v.s = (kind_ == Kind::Str || kind_ == Kind::Enum || kind_ == Kind::Asset) ? s_.c_str() : nullptr;
        return v;
    }
    // Скопировать значение из ABI (строка копируется сразу).
    static Value from(const EngineValue& e) {
        Value v;
        v.kind_ = static_cast<Kind>(e.kind);
        v.i_ = e.i;
        for (int n = 0; n < 4; ++n) v.f_[n] = e.f[n];
        if (e.s) v.s_ = e.s;
        return v;
    }

private:
    Kind kind_ = Kind::Nil;
    std::int64_t i_ = 0;
    float f_[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    std::string s_;
};

namespace detail {
// Прочитать строку через функцию «(буфер, ёмкость) → полная длина».
template <class F>
std::string read_string(F&& fill) {
    char small[128];
    const std::uint32_t n = fill(small, static_cast<std::uint32_t>(sizeof small));
    if (n < sizeof small) return std::string(small, n);
    std::string big(static_cast<std::size_t>(n) + 1, '\0');
    fill(&big[0], n + 1);
    big.resize(n);
    return big;
}
inline const char* s(const char* text) noexcept { return text ? text : ""; }
}  // namespace detail

// ─────────────────────────────────────────────────────────────────────────────
// Таймеры и твины
// ─────────────────────────────────────────────────────────────────────────────

// Таймер, запущенный send_after / send_every / destroy_after.
struct Timer {
    std::uint64_t id = 0;
    explicit operator bool() const noexcept { return id != 0; }
    // true, если таймер ещё был активен.
    bool cancel() const noexcept { return id != 0 && A().cancel_timer(id) != 0; }
};

// Твин поля, запущенный Object::tween.
struct Tween {
    std::uint64_t id = 0;
    explicit operator bool() const noexcept { return id != 0; }
    bool cancel() const noexcept { return id != 0 && A().tween_cancel(id) != 0; }
};

// Параметры твина. Кривые: "Linear", "OutQuad", "InOutCubic", "OutBack", "OutBounce"…
// (полный список — docs/api/gameplay.md, раздел «Твины»).
struct TweenParams {
    float duration = 1.0f;
    float delay = 0.0f;
    const char* ease = "Linear";
    std::uint32_t loops = 1;       // 0 — бесконечно
    bool yoyo = false;             // каждый второй проход идёт назад
    bool unscaled = false;         // реальное время: идёт и на паузе
    const char* on_complete = nullptr;  // сообщение по завершению…
    inline TweenParams() = default;
    inline explicit TweenParams(float seconds, const char* curve = "Linear") : duration(seconds), ease(curve) {}
};

// ─────────────────────────────────────────────────────────────────────────────
// Object — объект сцены
// ─────────────────────────────────────────────────────────────────────────────

// Лёгкое значение: копируйте свободно, храните между кадрами и проверяйте `alive()` —
// объект могли уничтожить. Через `explicit operator bool` проверяется «объект найден».
struct Object {
    EngineObject id{0, 0};

    Object() = default;
    explicit Object(EngineObject o) : id(o) {}

    explicit operator bool() const noexcept { return id.generation != 0; }
    bool operator==(const Object& o) const noexcept { return id.index == o.id.index && id.generation == o.id.generation; }
    bool operator!=(const Object& o) const noexcept { return !(*this == o); }
    // Число-ключ, пригодное для std::map / std::unordered_map.
    std::uint64_t key() const noexcept { return (static_cast<std::uint64_t>(id.generation) << 32) | id.index; }

    // Не уничтожен ли объект (не пишет в лог ни при каких условиях).
    bool alive() const noexcept { return A().is_alive(id) != 0; }

    // ── Transform ──
    Vec3 position() const noexcept { return A().get_position(id); }
    void set_position(Vec3 p) const noexcept { A().set_position(id, p); }
    Vec3 world_position() const noexcept { return A().get_world_position(id); }
    void set_world_position(Vec3 p) const noexcept { A().set_world_position(id, p); }
    void translate(Vec3 delta) const noexcept { set_position(position() + delta); }
    // Углы Эйлера в градусах, как в инспекторе.
    Vec3 rotation() const noexcept { return A().get_rotation(id); }
    void set_rotation(Vec3 euler_deg) const noexcept { A().set_rotation(id, euler_deg); }
    Vec3 world_rotation() const noexcept { return A().get_world_rotation(id); }
    void set_world_rotation(Vec3 euler_deg) const noexcept { A().set_world_rotation(id, euler_deg); }
    Vec3 scale() const noexcept { return A().get_scale(id); }
    void set_scale(Vec3 s) const noexcept { A().set_scale(id, s); }
    // Оси объекта в мировых координатах (вперёд — это −Z).
    Vec3 right() const noexcept { return A().get_direction(id, 0); }
    Vec3 up() const noexcept { return A().get_direction(id, 1); }
    Vec3 forward() const noexcept { return A().get_direction(id, 2); }
    // Повернуть «вперёд» на мировую точку.
    void look_at(Vec3 target, Vec3 up_axis = Vec3{0.0f, 1.0f, 0.0f}) const noexcept { A().look_at(id, target, up_axis); }

    // ── иерархия и состояние ──
    Object parent() const noexcept { return Object{A().get_parent(id)}; }
    // `new_parent` «нет объекта» — вынести в корень. keep_world сохраняет мировую позу.
    bool set_parent(Object new_parent, bool keep_world = false) const noexcept {
        return A().set_parent(id, new_parent.id, keep_world ? 1 : 0) != 0;
    }
    std::uint32_t child_count() const noexcept { return A().child_count(id); }
    Object child(std::uint32_t index) const noexcept { return Object{A().get_child(id, index)}; }
    std::vector<Object> children() const {
        std::vector<Object> out;
        const std::uint32_t n = child_count();
        for (std::uint32_t i = 0; i < n; ++i) out.push_back(child(i));
        return out;
    }
    Object find_child(const char* name) const noexcept { return Object{A().find_child(id, detail::s(name))}; }
    void set_active(bool active) const noexcept { A().set_active(id, active ? 1 : 0); }
    // Собственный флаг объекта.
    bool active() const noexcept { return A().get_active(id, 0) != 0; }
    // Активен ли объект с учётом всех предков.
    bool active_in_hierarchy() const noexcept { return A().get_active(id, 1) != 0; }
    std::string name() const {
        return detail::read_string([&](char* b, std::uint32_t c) { return A().get_name(id, b, c); });
    }
    void set_name(const char* name) const noexcept { A().set_name(id, detail::s(name)); }
    std::string tag() const {
        return detail::read_string([&](char* b, std::uint32_t c) { return A().get_tag(id, b, c); });
    }
    void set_tag(const char* tag) const noexcept { A().set_tag(id, detail::s(tag)); }
    std::uint32_t layer() const noexcept { return A().get_layer(id); }
    void set_layer(std::uint32_t layer) const noexcept { A().set_layer(id, layer); }
    // Уничтожить в конце кадра (с детьми). Безопасно звать на себе внутри любого хука.
    void destroy() const noexcept { A().destroy(id); }
    Timer destroy_after(float seconds) const noexcept { return Timer{A().destroy_after(id, seconds)}; }
    // Копия объекта с потомками.
    Object duplicate() const noexcept { return Object{A().duplicate(id)}; }
    // Не уничтожать при смене сцены.
    void dont_destroy_on_load() const noexcept { A().dont_destroy_on_load(id); }

    // ── компоненты ──
    // Добавить компонент по имени типа (Rust или C++): add_component("Rigidbody2D").
    bool add_component(const char* type) const noexcept { return A().add_component(id, detail::s(type)) != 0; }
    bool remove_component(const char* type) const noexcept { return A().remove_component(id, detail::s(type)) != 0; }
    // Свой собственный компонент на время его хука из объекта «вынут»: has_component по
    // имени собственного типа внутри его хука даёт false.
    bool has_component(const char* type) const noexcept { return A().has_component(id, detail::s(type)) != 0; }

    // ── поля любых компонентов (Rust или C++) по именам ──
    // Значение поля; Nil, если компонента или поля нет (причина — в логе).
    inline Value get(const char* component, const char* field) const;
    inline bool set(const char* component, const char* field, const Value& value) const;
    // Типизированное чтение. Возвращает false, если поле не нашлось или другого вида.
    inline bool get(const char* component, const char* field, float& out) const;
    inline bool get(const char* component, const char* field, int& out) const;
    inline bool get(const char* component, const char* field, bool& out) const;
    inline bool get(const char* component, const char* field, Vec2& out) const;
    inline bool get(const char* component, const char* field, Vec3& out) const;
    inline bool get(const char* component, const char* field, Color& out) const;
    inline bool get(const char* component, const char* field, Quat& out) const;
    inline bool get(const char* component, const char* field, std::string& out) const;

    // ── сообщения ──
    // Всем компонентам объекта придёт on_message (в конце текущей фазы кадра).
    void send_message(const char* name, float value = 0.0f) const noexcept { A().send_message(id, detail::s(name), value); }
    inline void send_message(const char* name, const Value& value) const;
    // Сообщение через `delay` секунд / каждые `interval` секунд. Таймер можно отменить.
    inline Timer send_after(float delay, const char* name, const Value& value = Value()) const;
    inline Timer send_every(float interval, const char* name, const Value& value = Value()) const;
    // Отменить все таймеры объекта; вернуть их число.
    std::uint32_t cancel_timers() const noexcept { return A().cancel_timers_of(id); }

    // ── твины ──
    // Плавно менять поле: obj.tween("SpriteRenderer.color", engine::Color{1,0,0,1}, engine::TweenParams(0.3f));
    // Путь — "Компонент.поле" или "Компонент.поле.канал" ("Transform.position.y").
    inline Tween tween(const char* path, const Value& to, const TweenParams& params = TweenParams()) const;
    std::uint32_t cancel_tweens() const noexcept { return A().tween_cancel_of(id); }
};

inline Value::Value(Object v) : kind_(Kind::Object), i_(static_cast<std::int64_t>(v.key())) {}
inline Object Value::to_object() const noexcept {
    if (kind_ != Kind::Object && kind_ != Kind::Int) return Object{};
    const std::uint64_t bits = static_cast<std::uint64_t>(i_);
    return Object{EngineObject{static_cast<std::uint32_t>(bits), static_cast<std::uint32_t>(bits >> 32)}};
}

inline Value Object::get(const char* component, const char* field) const {
    EngineValue out{};
    if (!A().get_field(id, detail::s(component), detail::s(field), &out)) return Value();
    return Value::from(out);
}
inline bool Object::set(const char* component, const char* field, const Value& value) const {
    const EngineValue packed = value.pack();
    return A().set_field(id, detail::s(component), detail::s(field), &packed) != 0;
}
inline bool Object::get(const char* c, const char* f, float& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Float && v.kind() != Value::Kind::Int && v.kind() != Value::Kind::Bool) return false;
    out = v.to_float(); return true;
}
inline bool Object::get(const char* c, const char* f, int& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Float && v.kind() != Value::Kind::Int && v.kind() != Value::Kind::Bool) return false;
    out = v.to_int(); return true;
}
inline bool Object::get(const char* c, const char* f, bool& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Float && v.kind() != Value::Kind::Int && v.kind() != Value::Kind::Bool) return false;
    out = v.to_bool(); return true;
}
inline bool Object::get(const char* c, const char* f, Vec2& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Vec2 && v.kind() != Value::Kind::Vec3) return false;
    out = v.to_vec2(); return true;
}
inline bool Object::get(const char* c, const char* f, Vec3& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Vec3 && v.kind() != Value::Kind::Vec2 && v.kind() != Value::Kind::Color) return false;
    out = v.to_vec3(); return true;
}
inline bool Object::get(const char* c, const char* f, Color& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Color) return false;
    out = v.to_color(); return true;
}
inline bool Object::get(const char* c, const char* f, Quat& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Quat) return false;
    out = v.to_quat(); return true;
}
inline bool Object::get(const char* c, const char* f, std::string& out) const {
    const Value v = get(c, f);
    if (v.kind() != Value::Kind::Str && v.kind() != Value::Kind::Enum && v.kind() != Value::Kind::Asset) return false;
    out = v.to_string(); return true;
}
inline void Object::send_message(const char* name, const Value& value) const {
    const EngineValue packed = value.pack();
    A().send_message_value(id, detail::s(name), &packed);
}
inline Timer Object::send_after(float delay, const char* name, const Value& value) const {
    const EngineValue packed = value.pack();
    return Timer{A().send_after(id, delay, detail::s(name), &packed)};
}
inline Timer Object::send_every(float interval, const char* name, const Value& value) const {
    const EngineValue packed = value.pack();
    return Timer{A().send_every(id, interval, detail::s(name), &packed)};
}
inline Tween Object::tween(const char* path, const Value& to, const TweenParams& p) const {
    const EngineValue packed = to.pack();
    EngineTweenParams params{};
    params.duration = p.duration;
    params.delay = p.delay;
    params.ease = p.ease;
    params.loops = p.loops;
    params.yoyo = p.yoyo ? 1 : 0;
    params.unscaled = p.unscaled ? 1 : 0;
    params.on_complete = p.on_complete;
    params.on_complete_target = id;
    return Tween{A().tween_field(id, detail::s(path), &packed, &params)};
}

// ─────────────────────────────────────────────────────────────────────────────
// Время
// ─────────────────────────────────────────────────────────────────────────────

inline EngineTime time_info() noexcept { EngineTime t{}; A().time_info(&t); return t; }
// Секунд за кадр с учётом time_scale (в хуках это же приходит параметром dt).
inline float delta_time() noexcept { return time_info().delta; }
inline float unscaled_delta_time() noexcept { return time_info().unscaled_delta; }
inline float fixed_delta_time() noexcept { return time_info().fixed_delta; }
// Игровое время с запуска (идёт с учётом time_scale) и реальное.
inline float time_seconds() noexcept { return A().time_seconds(); }
inline double elapsed_time() noexcept { return time_info().elapsed; }
inline double unscaled_elapsed_time() noexcept { return time_info().unscaled_elapsed; }
inline std::uint64_t frame_count() noexcept { return time_info().frame; }
inline float time_scale() noexcept { return time_info().time_scale; }
// 0 — пауза, 1 — норма, 0.3 — замедление. Действует со следующего кадра.
inline void set_time_scale(float scale) noexcept { A().time_set_scale(scale); }

// ─────────────────────────────────────────────────────────────────────────────
// Ввод. Семантика фиксированного шага та же, что в Rust: внутри on_fixed_update
// «нажато в этом кадре» значит «нажато с прошлого фиксированного шага».
// ─────────────────────────────────────────────────────────────────────────────

namespace input {

// Действия (именованные оси и кнопки: "Horizontal", "Vertical", "Jump", "Fire1", "Submit", "Cancel").
inline float axis(const char* name) noexcept { return A().input_axis(detail::s(name)); }
inline bool button(const char* name) noexcept { return A().input_button(detail::s(name)) != 0; }
inline bool button_down(const char* name) noexcept { return A().input_button_down(detail::s(name)) != 0; }
inline bool button_up(const char* name) noexcept { return A().input_button_up(detail::s(name)) != 0; }
inline float button_value(const char* name) noexcept { return A().input_button_value(detail::s(name)); }

// Клавиши по имени: "Space", "A", "Digit1" (или "1"), "Left", "LShift", "Escape", "F5"…
inline bool key(const char* name) noexcept { return A().input_key(detail::s(name), 0) != 0; }
inline bool key_down(const char* name) noexcept { return A().input_key(detail::s(name), 1) != 0; }
inline bool key_up(const char* name) noexcept { return A().input_key(detail::s(name), 2) != 0; }

// Мышь: кнопки 0 — левая, 1 — правая, 2 — средняя. Позиция в пикселях окна, (0, 0) — левый верхний угол.
inline bool mouse_button(int b) noexcept { return A().input_mouse_button(static_cast<std::uint32_t>(b), 0) != 0; }
inline bool mouse_button_down(int b) noexcept { return A().input_mouse_button(static_cast<std::uint32_t>(b), 1) != 0; }
inline bool mouse_button_up(int b) noexcept { return A().input_mouse_button(static_cast<std::uint32_t>(b), 2) != 0; }
inline Vec2 mouse_position() noexcept { const Vec3 v = A().input_vector(0); return {v.x, v.y}; }
inline Vec2 mouse_delta() noexcept { const Vec3 v = A().input_vector(1); return {v.x, v.y}; }
inline Vec2 scroll_delta() noexcept { const Vec3 v = A().input_vector(2); return {v.x, v.y}; }
inline Vec2 screen_size() noexcept { const Vec3 v = A().input_vector(3); return {v.x, v.y}; }

// Геймпад. `pad` — номер от 0 до gamepad_count()−1 или −1 («любой»).
// Кнопки: "South" "East" "West" "North" "LeftBumper" "RightBumper" "LeftTrigger" "RightTrigger"
// "Select" "Start" "Mode" "LeftStick" "RightStick" "DPadUp" "DPadDown" "DPadLeft" "DPadRight".
// Оси: "LeftStickX" "LeftStickY" "RightStickX" "RightStickY" "LeftTrigger" "RightTrigger".
inline int gamepad_count() noexcept { return static_cast<int>(A().input_gamepad_count()); }
inline bool gamepad_button(const char* name, int pad = -1) noexcept { return A().input_gamepad_button(pad, detail::s(name), 0) != 0; }
inline bool gamepad_button_down(const char* name, int pad = -1) noexcept { return A().input_gamepad_button(pad, detail::s(name), 1) != 0; }
inline bool gamepad_button_up(const char* name, int pad = -1) noexcept { return A().input_gamepad_button(pad, detail::s(name), 2) != 0; }
inline float gamepad_axis(const char* name, int pad = -1) noexcept { return A().input_gamepad_axis(pad, detail::s(name)); }
inline Vec2 left_stick(int pad = -1, float dead_zone = 0.2f) noexcept { const Vec3 v = A().input_gamepad_stick(pad, 0, dead_zone); return {v.x, v.y}; }
inline Vec2 right_stick(int pad = -1, float dead_zone = 0.2f) noexcept { const Vec3 v = A().input_gamepad_stick(pad, 1, dead_zone); return {v.x, v.y}; }

// Касания экрана.
struct Touch {
    std::uint64_t id = 0;
    Vec2 position, delta;
    enum class Phase { Began = 0, Moved = 1, Ended = 2 } phase = Phase::Moved;
};
inline int touch_count() noexcept { return static_cast<int>(A().input_touch_count()); }
inline std::vector<Touch> touches() {
    std::vector<Touch> out;
    const std::uint32_t n = A().input_touch_count();
    for (std::uint32_t i = 0; i < n; ++i) {
        EngineTouch t{};
        if (!A().input_touch(i, &t)) break;
        out.push_back(Touch{t.id, {t.x, t.y}, {t.dx, t.dy}, static_cast<Touch::Phase>(t.phase)});
    }
    return out;
}
// Текст, набранный в этом кадре (с учётом раскладки).
inline std::string text() {
    return detail::read_string([](char* b, std::uint32_t c) { return A().input_text(b, c); });
}

}  // namespace input

// Короткие формы прежнего API.
inline float axis(const char* name) noexcept { return input::axis(name); }
inline bool button(const char* name) noexcept { return input::button(name); }
inline bool button_down(const char* name) noexcept { return input::button_down(name); }

// ─────────────────────────────────────────────────────────────────────────────
// Сцена: поиск, создание, префабы, смена сцены
// ─────────────────────────────────────────────────────────────────────────────

inline Object find_by_tag(const char* tag) noexcept { return Object{A().find_by_tag(detail::s(tag))}; }
inline Object find_by_name(const char* name) noexcept { return Object{A().find_by_name(detail::s(name))}; }

namespace detail {
template <class F>
std::vector<Object> collect_objects(F&& fill) {
    std::vector<EngineObject> raw(64);
    std::uint32_t n = fill(raw.data(), static_cast<std::uint32_t>(raw.size()));
    if (n > raw.size()) {
        raw.resize(n);
        n = fill(raw.data(), static_cast<std::uint32_t>(raw.size()));
    }
    std::vector<Object> out;
    for (std::uint32_t i = 0; i < n && i < raw.size(); ++i) out.push_back(Object{raw[i]});
    return out;
}
}  // namespace detail

// Все объекты с тегом (включая неактивные).
inline std::vector<Object> find_all_by_tag(const char* tag) {
    return detail::collect_objects([&](EngineObject* o, std::uint32_t c) { return A().find_all_by_tag(detail::s(tag), o, c); });
}
// Все объекты, у которых есть компонент с таким именем типа (кроме компонента, чей хук идёт прямо сейчас).
inline std::vector<Object> find_with_component(const char* type) {
    return detail::collect_objects([&](EngineObject* o, std::uint32_t c) { return A().find_with_component(detail::s(type), o, c); });
}
// Создать пустой объект в корне сцены.
inline Object spawn(const char* name = "GameObject") noexcept { return Object{A().spawn(detail::s(name))}; }
// Экземпляр префаба в начале координат (путь относительно каталога ассетов или GUID).
inline Object instantiate(const char* prefab) noexcept { return Object{A().instantiate(detail::s(prefab))}; }
// Экземпляр префаба с позой. Без `parent` поза мировая, с ним — локальная.
inline Object instantiate(const char* prefab, Vec3 position, Vec3 euler_deg = Vec3{}, Object parent = Object{}) noexcept {
    return Object{A().instantiate_at(detail::s(prefab), position, euler_deg, parent.id)};
}
// Экземпляр префаба ребёнком `parent`, в его начале координат.
inline Object instantiate_under(const char* prefab, Object parent) noexcept {
    return Object{A().instantiate_at(detail::s(prefab), Vec3{}, Vec3{}, parent.id)};
}

// Смена сцены выполняется в конце кадра.
inline void load_scene(const char* name) noexcept { A().load_scene(detail::s(name)); }
inline void load_scene_additive(const char* name) noexcept { A().load_scene_additive(detail::s(name)); }
inline void unload_scene(const char* name) noexcept { A().unload_scene(detail::s(name)); }
inline void reload_scene() noexcept { A().reload_scene(); }
inline std::string scene_name() {
    return detail::read_string([](char* b, std::uint32_t c) { return A().scene_name(b, c); });
}
// Закрыть игру (в редакторе — выйти из Play).
inline void quit() noexcept { A().quit(); }

// ─────────────────────────────────────────────────────────────────────────────
// Сохранения (аналог PlayerPrefs): ключ → значение
// ─────────────────────────────────────────────────────────────────────────────

namespace save {
inline bool has(const char* key) noexcept { return A().save_has(detail::s(key)) != 0; }
inline Value get(const char* key) {
    EngineValue out{};
    if (!A().save_get(detail::s(key), &out)) return Value();
    return Value::from(out);
}
inline void set(const char* key, const Value& value) {
    const EngineValue packed = value.pack();
    A().save_set(detail::s(key), &packed);
}
inline float get_float(const char* key, float fallback = 0.0f) { const Value v = get(key); return v.is_nil() ? fallback : v.to_float(fallback); }
inline int get_int(const char* key, int fallback = 0) { const Value v = get(key); return v.is_nil() ? fallback : v.to_int(fallback); }
inline bool get_bool(const char* key, bool fallback = false) { const Value v = get(key); return v.is_nil() ? fallback : v.to_bool(fallback); }
inline std::string get_string(const char* key, const std::string& fallback = std::string()) {
    const Value v = get(key); return v.kind() == Value::Kind::Str ? v.to_string() : fallback;
}
inline bool erase(const char* key) noexcept { return A().save_delete(detail::s(key)) != 0; }
// Записать сохранения на диск (автосохранения нет: зовите на контрольных точках).
inline bool flush() noexcept { return A().save_flush() != 0; }
}  // namespace save

// ─────────────────────────────────────────────────────────────────────────────
// Случайные числа (детерминированный генератор движка)
// ─────────────────────────────────────────────────────────────────────────────

namespace rng {
inline float value() noexcept { return A().random_f32(); }                       // [0, 1)
inline float range(float lo, float hi) noexcept { return A().random_range(lo, hi); }  // [lo, hi)
inline int integer(int lo, int hi) noexcept { return A().random_int(lo, hi); }   // [lo, hi], обе границы входят
inline bool chance(float probability) noexcept { return A().random_chance(probability) != 0; }
inline void seed(std::uint64_t s) noexcept { A().random_seed(s); }
inline Vec2 in_unit_circle() noexcept { const Vec3 v = A().random_in_unit_circle(); return {v.x, v.y}; }
inline Vec3 on_unit_sphere() noexcept { return A().random_on_unit_sphere(); }
}  // namespace rng

// ─────────────────────────────────────────────────────────────────────────────
// Сервисы: физика, звук, интерфейс и всё, что зарегистрировала игра на стороне Rust
// ─────────────────────────────────────────────────────────────────────────────

// Установлен ли сервис (вне хуков — false).
inline bool service_available(const char* name) noexcept { return A().service_available(detail::s(name)) != 0; }

// Вызвать сервис по имени. Возвращает результаты; пустой список — «нет результата»
// (нет попадания, сервис не установлен: причина в логе). `status` (необязательно) — одна
// из констант ENGINE_SERVICE_*.
inline std::vector<Value> call_service(const char* name, std::initializer_list<Value> args = {},
                                       std::uint32_t max_results = 8, int* status = nullptr) {
    std::vector<EngineValue> packed;
    packed.reserve(args.size());
    for (const Value& v : args) packed.push_back(v.pack());
    std::vector<EngineValue> raw(max_results ? max_results : 1);
    std::uint32_t count = 0;
    const int code = A().service_call(detail::s(name), packed.data(), static_cast<std::uint32_t>(packed.size()),
                                      raw.data(), static_cast<std::uint32_t>(raw.size()), &count);
    if (status) *status = code;
    std::vector<Value> out;
    if (code == ENGINE_SERVICE_OK)
        for (std::uint32_t i = 0; i < count && i < raw.size(); ++i) out.push_back(Value::from(raw[i]));
    return out;
}

namespace detail {
inline std::vector<Object> objects_from(const std::vector<Value>& values) {
    std::vector<Object> out;
    for (const Value& v : values) out.push_back(v.to_object());
    return out;
}
inline float first_float(const std::vector<Value>& v, float fallback = 0.0f) { return v.empty() ? fallback : v[0].to_float(fallback); }
inline bool first_bool(const std::vector<Value>& v, bool fallback = false) { return v.empty() ? fallback : v[0].to_bool(fallback); }
}  // namespace detail

// ── Физика ──────────────────────────────────────────────────────────────────

namespace physics {

// Результат луча. `if (hit)` — было ли попадание.
struct RaycastHit {
    bool hit = false;
    Object object;
    Vec3 point, normal;
    float distance = 0.0f;
    explicit operator bool() const noexcept { return hit; }
};

// Луч в 3D. Триггеры игнорируются; `ignore` — обычно сам стреляющий. Видит мир на момент
// последнего шага физики.
inline RaycastHit raycast(Vec3 origin, Vec3 direction, float max_distance = 1000.0f, Object ignore = Object{}) {
    const auto r = call_service("physics3d.raycast", {origin, direction, max_distance, ignore ? Value(ignore) : Value()}, 4);
    RaycastHit hit;
    if (r.size() == 4) hit = RaycastHit{true, r[0].to_object(), r[1].to_vec3(), r[2].to_vec3(), r[3].to_float()};
    return hit;
}
// Луч в 2D (мировые координаты XY).
inline RaycastHit raycast2d(Vec2 origin, Vec2 direction, float max_distance = 1000.0f, Object ignore = Object{}) {
    const auto r = call_service("physics2d.raycast", {origin, direction, max_distance, ignore ? Value(ignore) : Value()}, 4);
    RaycastHit hit;
    if (r.size() == 4) hit = RaycastHit{true, r[0].to_object(), r[1].to_vec3(), r[2].to_vec3(), r[3].to_float()};
    return hit;
}
// Объекты, чьи коллайдеры задевают шар / круг (не больше `max_results`).
inline std::vector<Object> overlap_sphere(Vec3 center, float radius, int max_results = 64) {
    return detail::objects_from(call_service("physics3d.overlap_sphere", {center, radius, max_results},
                                             static_cast<std::uint32_t>(max_results)));
}
inline std::vector<Object> overlap_circle(Vec2 center, float radius, int max_results = 64) {
    return detail::objects_from(call_service("physics2d.overlap_circle", {center, radius, max_results},
                                             static_cast<std::uint32_t>(max_results)));
}

// Силы и импульсы (3D). Импульс — мгновенное изменение скорости на импульс/масса.
inline void apply_impulse(Object body, Vec3 impulse) { call_service("physics3d.apply_impulse", {body, impulse}, 0); }
inline void apply_impulse_at(Object body, Vec3 impulse, Vec3 world_point) { call_service("physics3d.apply_impulse_at", {body, impulse, world_point}, 0); }
inline void apply_torque_impulse(Object body, Vec3 torque) { call_service("physics3d.apply_torque_impulse", {body, torque}, 0); }
// Сила на один шаг физики: зовите каждый on_fixed_update, пока она действует.
inline void add_force(Object body, Vec3 force) { call_service("physics3d.add_force", {body, force}, 0); }
// Взрыв: динамические тела в радиусе получают скорость до `speed` от центра. Возвращает задетые.
inline std::vector<Object> radial_impulse(Vec3 center, float radius, float speed) {
    return detail::objects_from(call_service("physics3d.radial_impulse", {center, radius, speed}, 64));
}
inline float mass(Object body) { return detail::first_float(call_service("physics3d.mass", {body}, 1)); }
inline bool is_sleeping(Object body) { return detail::first_bool(call_service("physics3d.is_sleeping", {body}, 1)); }
// Гравитация мира.
inline Vec3 gravity() { const auto r = call_service("physics3d.gravity", {}, 1); return r.empty() ? Vec3{} : r[0].to_vec3(); }
inline void set_gravity(Vec3 g) { call_service("physics3d.set_gravity", {g}, 0); }
inline Vec2 gravity2d() { const auto r = call_service("physics2d.gravity", {}, 1); return r.empty() ? Vec2{} : r[0].to_vec2(); }
inline void set_gravity2d(Vec2 g) { call_service("physics2d.set_gravity", {g}, 0); }
// 2D: у Rigidbody2D нет импульсов — мгновенное изменение скорости (Δv, масса не учитывается).
inline void add_velocity2d(Object body, Vec2 delta_v) { call_service("physics2d.velocity_change", {body, delta_v}, 0); }

// Скорость тел — это поля Rigidbody / Rigidbody2D, поэтому они читаются и пишутся без сервисов.
inline Vec3 velocity(Object body) { Vec3 v; body.get("Rigidbody", "velocity", v); return v; }
inline void set_velocity(Object body, Vec3 v) { body.set("Rigidbody", "velocity", v); }
inline Vec3 angular_velocity(Object body) { Vec3 v; body.get("Rigidbody", "angular_velocity", v); return v; }
inline void set_angular_velocity(Object body, Vec3 v) { body.set("Rigidbody", "angular_velocity", v); }
inline Vec2 velocity2d(Object body) { Vec2 v; body.get("Rigidbody2D", "velocity", v); return v; }
inline void set_velocity2d(Object body, Vec2 v) { body.set("Rigidbody2D", "velocity", v); }

// Контроллер персонажа (3D): скорость в м/с задаёте вы, гравитацию и шаги по ступенькам делает движок.
inline void character_move(Object body, Vec3 velocity_ms) { body.set("CharacterController", "velocity", velocity_ms); }
inline Vec3 character_velocity(Object body) { Vec3 v; body.get("CharacterController", "velocity", v); return v; }
inline bool character_grounded(Object body) { bool g = false; body.get("CharacterController", "grounded", g); return g; }

}  // namespace physics

// ── Звук ────────────────────────────────────────────────────────────────────

namespace audio {

// Голос — запущенный звук; нулевой — «клип не найден».
struct Voice {
    std::int64_t id = 0;
    explicit operator bool() const noexcept { return id != 0; }
    bool playing() const { return id != 0 && detail::first_bool(call_service("audio.is_playing", {Value(static_cast<long long>(id))}, 1)); }
    void stop() const { if (id) call_service("audio.stop", {Value(static_cast<long long>(id))}, 0); }
    void set_volume(float v) const { if (id) call_service("audio.set_volume", {Value(static_cast<long long>(id)), v}, 0); }
    void set_pitch(float p) const { if (id) call_service("audio.set_pitch", {Value(static_cast<long long>(id)), p}, 0); }
    void fade_out(float seconds) const { if (id) call_service("audio.fade_out", {Value(static_cast<long long>(id)), seconds}, 0); }
};

// Параметры запуска. `bus`: "Master", "Music", "Sfx" (по умолчанию), "Ui".
struct PlayParams {
    float volume = 1.0f;
    float pitch = 1.0f;       // 2.0 — октавой выше и вдвое быстрее
    float pan = 0.0f;         // −1 (слева) … +1 (справа)
    bool looping = false;
    const char* bus = "Sfx";
    float fade_in = 0.0f;
    bool spatial = false;     // пространственный звук в точке `position`
    Vec3 position;
    float min_distance = 1.0f, max_distance = 30.0f;
};

// Запустить клип по имени (зарегистрирован в Audio::add_clip) или по GUID ассета.
inline Voice play(const char* clip, const PlayParams& p) {
    const auto r = call_service("audio.play", {clip, p.volume, p.pitch, p.pan, p.looping, p.bus, p.spatial, p.position,
                                               p.min_distance, p.max_distance, p.fade_in}, 1);
    return Voice{r.empty() ? 0 : r[0].to_int64()};
}
inline Voice play(const char* clip, float volume = 1.0f, float pitch = 1.0f) {
    PlayParams p; p.volume = volume; p.pitch = pitch; return play(clip, p);
}
// Звук в точке мира: громкость и панорама считаются относительно AudioListener.
inline Voice play_at(const char* clip, Vec3 position, float volume = 1.0f) {
    PlayParams p; p.volume = volume; p.spatial = true; p.position = position; return play(clip, p);
}
// Звук интерфейса (шина "Ui").
inline Voice play_ui(const char* clip, float volume = 1.0f) {
    PlayParams p; p.volume = volume; p.bus = "Ui"; return play(clip, p);
}
// Музыка по кругу с плавной сменой (предыдущая затихает за `crossfade` секунд).
inline Voice play_music(const char* clip, float crossfade = 0.5f) {
    const auto r = call_service("audio.play_music", {clip, crossfade}, 1);
    return Voice{r.empty() ? 0 : r[0].to_int64()};
}
inline void stop_music(float fade = 0.5f) { call_service("audio.stop_music", {fade}, 0); }
inline bool is_music_playing() { return detail::first_bool(call_service("audio.is_music_playing", {}, 1)); }
// Сколько голосов звучит сейчас.
inline int voice_count() { const auto r = call_service("audio.voice_count", {}, 1); return r.empty() ? 0 : r[0].to_int(); }
inline void stop_all() { call_service("audio.stop_all", {}, 0); }
inline void pause_all() { call_service("audio.pause_all", {}, 0); }
inline void resume_all() { call_service("audio.resume_all", {}, 0); }
inline void set_bus_volume(const char* bus, float volume) { call_service("audio.set_bus_volume", {bus, volume}, 0); }
inline float bus_volume(const char* bus) { return detail::first_float(call_service("audio.bus_volume", {bus}, 1), 1.0f); }
inline void set_bus_muted(const char* bus, bool muted) { call_service("audio.set_bus_muted", {bus, muted}, 0); }
// Запустить / остановить компонент AudioSource объекта.
inline void play_source(Object source) { call_service("audio.play_source", {source}, 0); }
inline void stop_source(Object source) { call_service("audio.stop_source", {source}, 0); }

}  // namespace audio

// ── Интерфейс ───────────────────────────────────────────────────────────────

namespace ui {

// События интерфейса этого кадра (имя виджета = его поле `message`, по умолчанию — имя объекта).
// Появляются в начале кадра, до on_update, и живут до начала следующего.
inline bool clicked(const char* widget) { return detail::first_bool(call_service("ui.clicked", {widget}, 1)); }
inline bool toggled(const char* widget, bool& state) {
    const auto r = call_service("ui.toggled", {widget}, 1);
    if (r.empty()) return false;
    state = r[0].to_bool(); return true;
}
inline bool value_changed(const char* widget, float& value) {
    const auto r = call_service("ui.value_changed", {widget}, 1);
    if (r.empty()) return false;
    value = r[0].to_float(); return true;
}
inline bool text_changed(const char* widget, std::string& text) {
    const auto r = call_service("ui.text_changed", {widget}, 1);
    if (r.empty()) return false;
    text = r[0].to_string(); return true;
}
inline bool submitted(const char* widget, std::string& text) {
    const auto r = call_service("ui.submitted", {widget}, 1);
    if (r.empty()) return false;
    text = r[0].to_string(); return true;
}
// Мышь над интерфейсом (клик по нему — не выстрел) / игрок печатает в поле ввода.
inline bool pointer_over_ui() { return detail::first_bool(call_service("ui.pointer_over_ui", {}, 1)); }
inline bool wants_keyboard() { return detail::first_bool(call_service("ui.wants_keyboard", {}, 1)); }

// Изменение виджетов — это обычные поля компонентов, поэтому сервисов не нужно.
// Виджет ищется по имени объекта.
inline void set_text(Object widget, const char* text) { widget.set("Text", "text", Value(text)); }
inline bool set_text(const char* widget_name, const char* text) {
    const Object o = find_by_name(widget_name);
    if (!o) return false;
    set_text(o, text);
    return true;
}
inline std::string text(Object widget) { std::string s; widget.get("Text", "text", s); return s; }
// Значение ползунка (Slider) или полосы (ProgressBar, 0…1).
inline bool set_value(const char* widget_name, float value) {
    const Object o = find_by_name(widget_name);
    if (!o) return false;
    return o.set("ProgressBar", "value", value) || o.set("Slider", "value", value);
}
inline void set_visible(Object widget, bool visible) { widget.set_active(visible); }
inline bool set_visible(const char* widget_name, bool visible) {
    const Object o = find_by_name(widget_name);
    if (!o) return false;
    o.set_active(visible);
    return true;
}
inline void set_interactable(Object widget, bool on) {
    widget.set("Button", "interactable", on) || widget.set("Toggle", "interactable", on) ||
        widget.set("Slider", "interactable", on) || widget.set("InputField", "interactable", on);
}

}  // namespace ui

// ─────────────────────────────────────────────────────────────────────────────
// Компонент
// ─────────────────────────────────────────────────────────────────────────────

// Базовый класс компонента. Переопределяйте только нужные хуки; каждый получает объект,
// на котором висит компонент. Порядок кадра: on_start (один раз) → on_fixed_update (0…N раз)
// → on_update → on_late_update → отложенное уничтожение (on_destroy).
struct Component {
    virtual ~Component() = default;
    virtual void on_start(Object self) { (void)self; }
    virtual void on_update(Object self, float dt) { (void)self; (void)dt; }
    virtual void on_fixed_update(Object self, float dt) { (void)self; (void)dt; }
    // После всех on_update: камеры и слежение.
    virtual void on_late_update(Object self, float dt) { (void)self; (void)dt; }
    // Началось столкновение с твёрдым коллайдером `other` (2D и 3D).
    virtual void on_collision_enter(Object self, Object other) { (void)self; (void)other; }
    // `other` вошёл в триггер (или мы вошли в его триггер) (2D и 3D).
    virtual void on_trigger_enter(Object self, Object other) { (void)self; (void)other; }
    // Перед удалением объекта (в конце кадра, в котором вызван destroy).
    virtual void on_destroy(Object self) { (void)self; }
    // Сообщение: send_message, таймеры send_after/send_every, on_complete твина и действия
    // интерфейса (Button — Nil, Toggle — Bool, Slider — Float, InputField — Str).
    virtual void on_message(Object self, const char* name, const Value& value) { (void)self; (void)name; (void)value; }
};

// Список полей типа T; специализируется макросом ENGINE_FIELDS.
template <class T>
struct Fields {
    static const EngineField* data() noexcept { return nullptr; }
    static std::uint32_t count() noexcept { return 0; }
};

namespace detail {

template <class T>
struct TypeNameOf {
    static const char*& get() noexcept { static const char* name = "?"; return name; }
};

// Сколько исключений из хуков одного типа попадает в лог.
inline bool exception_budget(const char* type) noexcept {
    static std::uint32_t counts[64] = {};
    static const char* names[64] = {};
    for (std::uint32_t i = 0; i < 64; ++i) {
        if (names[i] == type || names[i] == nullptr) {
            names[i] = type;
            return ++counts[i] <= 5;
        }
    }
    return true;
}

inline void report_exception(const char* type, const char* hook, const char* what) noexcept {
    if (!exception_budget(type)) return;
    log_errorf("C++ exception in %s::%s: %s (the hook is skipped; the game keeps running)", type, hook, what);
}

// Вызвать хук так, чтобы исключение не вышло за границу.
template <class T, class F>
void guard(const char* hook, F&& body) noexcept {
    try {
        body();
    } catch (const std::exception& e) {
        report_exception(TypeNameOf<T>::get(), hook, e.what());
    } catch (...) {
        report_exception(TypeNameOf<T>::get(), hook, "unknown exception");
    }
}

// Значение поля ↔ ABI. Вид поля — аргумент шаблона (ENGINE_FIELD_*).
template <std::uint32_t Kind, class V>
void pack_field(const V& v, EngineValue& out) {
    out = EngineValue{};
    if constexpr (Kind == ENGINE_FIELD_F32) { out.kind = ENGINE_VALUE_FLOAT; out.f[0] = static_cast<float>(v); }
    else if constexpr (Kind == ENGINE_FIELD_I32) { out.kind = ENGINE_VALUE_INT; out.i = static_cast<std::int64_t>(v); }
    else if constexpr (Kind == ENGINE_FIELD_BOOL) { out.kind = ENGINE_VALUE_BOOL; out.i = v ? 1 : 0; }
    else if constexpr (Kind == ENGINE_FIELD_VEC2) { out.kind = ENGINE_VALUE_VEC2; out.f[0] = v.x; out.f[1] = v.y; }
    else if constexpr (Kind == ENGINE_FIELD_VEC3) { out.kind = ENGINE_VALUE_VEC3; out.f[0] = v.x; out.f[1] = v.y; out.f[2] = v.z; }
    else if constexpr (Kind == ENGINE_FIELD_COLOR) { out.kind = ENGINE_VALUE_COLOR; out.f[0] = v.r; out.f[1] = v.g; out.f[2] = v.b; out.f[3] = v.a; }
    else if constexpr (Kind == ENGINE_FIELD_QUAT) { out.kind = ENGINE_VALUE_QUAT; out.f[0] = v.x; out.f[1] = v.y; out.f[2] = v.z; out.f[3] = v.w; }
    else if constexpr (Kind == ENGINE_FIELD_STRING) { out.kind = ENGINE_VALUE_STR; out.s = v.c_str(); }
    else if constexpr (Kind == ENGINE_FIELD_ASSET) { out.kind = ENGINE_VALUE_ASSET; out.s = v.c_str(); }
    else if constexpr (Kind == ENGINE_FIELD_ENUM) { out.kind = ENGINE_VALUE_INT; out.i = static_cast<std::int64_t>(v); }
}

// false — значение не подходит полю.
template <std::uint32_t Kind, class V>
bool unpack_field(V& v, const EngineValue& in) {
    if constexpr (Kind == ENGINE_FIELD_F32) {
        if (in.kind == ENGINE_VALUE_FLOAT) { v = static_cast<V>(in.f[0]); return true; }
        if (in.kind == ENGINE_VALUE_INT) { v = static_cast<V>(in.i); return true; }
        return false;
    } else if constexpr (Kind == ENGINE_FIELD_I32) {
        if (in.kind != ENGINE_VALUE_INT) return false;
        v = static_cast<V>(in.i); return true;
    } else if constexpr (Kind == ENGINE_FIELD_BOOL) {
        if (in.kind != ENGINE_VALUE_BOOL) return false;
        v = in.i != 0; return true;
    } else if constexpr (Kind == ENGINE_FIELD_VEC2) {
        if (in.kind != ENGINE_VALUE_VEC2) return false;
        v.x = in.f[0]; v.y = in.f[1]; return true;
    } else if constexpr (Kind == ENGINE_FIELD_VEC3) {
        if (in.kind != ENGINE_VALUE_VEC3) return false;
        v.x = in.f[0]; v.y = in.f[1]; v.z = in.f[2]; return true;
    } else if constexpr (Kind == ENGINE_FIELD_COLOR) {
        if (in.kind != ENGINE_VALUE_COLOR) return false;
        v.r = in.f[0]; v.g = in.f[1]; v.b = in.f[2]; v.a = in.f[3]; return true;
    } else if constexpr (Kind == ENGINE_FIELD_QUAT) {
        if (in.kind != ENGINE_VALUE_QUAT) return false;
        v.x = in.f[0]; v.y = in.f[1]; v.z = in.f[2]; v.w = in.f[3]; return true;
    } else if constexpr (Kind == ENGINE_FIELD_STRING) {
        if (in.kind != ENGINE_VALUE_STR) return false;
        v = in.s ? in.s : ""; return true;
    } else if constexpr (Kind == ENGINE_FIELD_ASSET) {
        if (in.kind != ENGINE_VALUE_ASSET) return false;
        v = in.s ? in.s : ""; return true;
    } else {
        if (in.kind != ENGINE_VALUE_INT) return false;
        v = static_cast<V>(in.i); return true;
    }
}

template <class T, auto M, std::uint32_t Kind>
void field_get(const void* self, EngineValue* out) noexcept {
    try {
        pack_field<Kind>(static_cast<const T*>(self)->*M, *out);
    } catch (...) {
        *out = EngineValue{};
    }
}

template <class T, auto M, std::uint32_t Kind>
int field_set(void* self, const EngineValue* value) noexcept {
    try {
        return unpack_field<Kind>(static_cast<T*>(self)->*M, *value) ? 0 : 1;
    } catch (...) {
        return 2;
    }
}

}  // namespace detail

// Описание типа для движка: создание, уничтожение и переходники к методам класса.
// Исключения гасятся на границе — в движок они уйти не должны.
template <class T>
EngineComponentType component(const char* name) noexcept {
    detail::TypeNameOf<T>::get() = name;
    EngineComponentType type{};
    type.name = name;
    type.create = []() noexcept -> void* {
        try {
            return static_cast<void*>(new T());
        } catch (...) {
            log_errorf("C++ exception in the constructor of %s: the component is not created", detail::TypeNameOf<T>::get());
            return nullptr;
        }
    };
    type.destroy = [](void* self) noexcept { delete static_cast<T*>(self); };
    type.on_start = [](void* self, EngineObject object) noexcept {
        detail::guard<T>("on_start", [&] { static_cast<T*>(self)->on_start(Object{object}); });
    };
    type.on_update = [](void* self, EngineObject object, float dt) noexcept {
        detail::guard<T>("on_update", [&] { static_cast<T*>(self)->on_update(Object{object}, dt); });
    };
    type.on_fixed_update = [](void* self, EngineObject object, float dt) noexcept {
        detail::guard<T>("on_fixed_update", [&] { static_cast<T*>(self)->on_fixed_update(Object{object}, dt); });
    };
    type.on_late_update = [](void* self, EngineObject object, float dt) noexcept {
        detail::guard<T>("on_late_update", [&] { static_cast<T*>(self)->on_late_update(Object{object}, dt); });
    };
    type.on_collision_enter = [](void* self, EngineObject object, EngineObject other) noexcept {
        detail::guard<T>("on_collision_enter", [&] { static_cast<T*>(self)->on_collision_enter(Object{object}, Object{other}); });
    };
    type.on_trigger_enter = [](void* self, EngineObject object, EngineObject other) noexcept {
        detail::guard<T>("on_trigger_enter", [&] { static_cast<T*>(self)->on_trigger_enter(Object{object}, Object{other}); });
    };
    type.on_destroy = [](void* self, EngineObject object) noexcept {
        detail::guard<T>("on_destroy", [&] { static_cast<T*>(self)->on_destroy(Object{object}); });
    };
    type.on_message = [](void* self, EngineObject object, const char* message, const EngineValue* value) noexcept {
        detail::guard<T>("on_message", [&] {
            static_cast<T*>(self)->on_message(Object{object}, message ? message : "", value ? Value::from(*value) : Value());
        });
    };
    type.fields = Fields<T>::data();
    type.field_count = Fields<T>::count();
    return type;
}

// Компоненты, объявленные макросом ENGINE_COMPONENT в любом файле плагина.
inline std::vector<EngineComponentType>& registered_components() {
    static std::vector<EngineComponentType> types;
    return types;
}

template <class T>
struct AutoRegister {
    explicit AutoRegister(const char* name) { registered_components().push_back(component<T>(name)); }
};

}  // namespace engine

// Одно поле: имя члена и его вид (ENGINE_FIELD_F32, _I32, _BOOL, _VEC2, _VEC3, _COLOR, _QUAT,
// _STRING, _ASSET; для перечислений — ENGINE_ENUM_FIELD). Члены публичные.
//   float / int32_t / bool / engine::Vec2 / engine::Vec3 / engine::Color / engine::Quat / std::string
#define ENGINE_FIELD(Type, member, kind)                                                           \
    EngineField{#member, kind, nullptr, 0, nullptr,                                                \
                &engine::detail::field_get<Type, &Type::member, kind>,                             \
                &engine::detail::field_set<Type, &Type::member, kind>}

// Ссылка на ассет: член — std::string с GUID; `asset_kind` — "Texture", "Prefab", "AudioClip"…
#define ENGINE_ASSET_FIELD(Type, member, asset_kind)                                               \
    EngineField{#member, ENGINE_FIELD_ASSET, nullptr, 0, asset_kind,                               \
                &engine::detail::field_get<Type, &Type::member, ENGINE_FIELD_ASSET>,               \
                &engine::detail::field_set<Type, &Type::member, ENGINE_FIELD_ASSET>}

// Перечисление: член — `enum` / `enum class` (значения с нуля подряд), далее имена вариантов:
//   ENGINE_ENUM_FIELD(Guard, mode, "Patrol", "Chase", "Attack")
#define ENGINE_ENUM_FIELD(Type, member, ...)                                                       \
    [] {                                                                                           \
        static const char* const names[] = {__VA_ARGS__};                                          \
        return EngineField{#member, ENGINE_FIELD_ENUM, names,                                      \
                           static_cast<std::uint32_t>(sizeof(names) / sizeof(names[0])), nullptr,  \
                           &engine::detail::field_get<Type, &Type::member, ENGINE_FIELD_ENUM>,     \
                           &engine::detail::field_set<Type, &Type::member, ENGINE_FIELD_ENUM>};    \
    }()

// Поля типа, видимые движку: ENGINE_FIELDS(Spinner, ENGINE_FIELD(Spinner, speed, ENGINE_FIELD_F32));
// Они видны в инспекторе редактора и сохраняются в сцену.
#define ENGINE_FIELDS(Type, ...)                                                     \
    template <>                                                                      \
    struct engine::Fields<Type> {                                                    \
        static const EngineField* data() noexcept {                                  \
            static const EngineField fields[] = {__VA_ARGS__};                       \
            return fields;                                                           \
        }                                                                            \
        static std::uint32_t count() noexcept {                                      \
            static const EngineField fields[] = {__VA_ARGS__};                       \
            return static_cast<std::uint32_t>(sizeof(fields) / sizeof(fields[0]));   \
        }                                                                            \
    }

// Точка входа плагина, собирающая все компоненты из ENGINE_COMPONENT.
#define ENGINE_PLUGIN_AUTO_ENTRY(entry, plugin_name)                                 \
    extern "C" ENGINE_EXPORT const EnginePlugin* entry(void) {                       \
        static const EnginePlugin plugin = [] {                                      \
            EnginePlugin p{};                                                        \
            p.abi_version = ENGINE_ABI_VERSION;                                      \
            p.name = plugin_name;                                                    \
            p.on_load = [](const EngineApi* host) noexcept -> int {                  \
                if (!host || host->abi_version != ENGINE_ABI_VERSION) return 1;      \
                engine::api = host;                                                  \
                return 0;                                                            \
            };                                                                       \
            p.components = engine::registered_components().data();                   \
            p.component_count =                                                      \
                static_cast<std::uint32_t>(engine::registered_components().size());  \
            return p;                                                                \
        }();                                                                         \
        return &plugin;                                                              \
    }

// Описание плагина с явным списком компонентов и именем `entry` точки входа.
#define ENGINE_PLUGIN_ENTRY(entry, plugin_name, ...)                                 \
    extern "C" ENGINE_EXPORT const EnginePlugin* entry(void) {                       \
        static const EngineComponentType components[] = {__VA_ARGS__};               \
        static const EnginePlugin plugin = [] {                                      \
            EnginePlugin p{};                                                        \
            p.abi_version = ENGINE_ABI_VERSION;                                      \
            p.name = plugin_name;                                                    \
            p.on_load = [](const EngineApi* host) noexcept -> int {                  \
                if (!host || host->abi_version != ENGINE_ABI_VERSION) return 1;      \
                engine::api = host;                                                  \
                return 0;                                                            \
            };                                                                       \
            p.components = components;                                               \
            p.component_count =                                                      \
                static_cast<std::uint32_t>(sizeof(components) / sizeof(components[0])); \
            return p;                                                                \
        }();                                                                         \
        return &plugin;                                                              \
    }

// Динамический плагин: движок ищет в библиотеке символ `engine_plugin_entry`.
#define ENGINE_PLUGIN(plugin_name, ...) \
    ENGINE_PLUGIN_ENTRY(engine_plugin_entry, plugin_name, __VA_ARGS__)

// Сделать класс компонентом плагина (после ENGINE_FIELDS). Имя в сценах = имя класса.
#define ENGINE_COMPONENT(Type) static const engine::AutoRegister<Type> engine_register_##Type{#Type}

#endif  // ENGINE_COMPONENT_HPP
