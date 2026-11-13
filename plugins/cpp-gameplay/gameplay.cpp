// Игровая логика на C++: компоненты наравне с компонентами на Rust.
//
// Этот файл компилируется двумя способами:
//  * как динамический плагин (CMakeLists.txt рядом, вместе со скриптами из scripts/) —
//    движок подхватывает пересборку на лету, не закрывая игру и редактор;
//  * статически, внутрь крейта engine-cpp-bridge (его build.rs) — для тестов и для
//    игр, которым плагины не нужны.

#include "engine_component.hpp"

#include <cmath>

// Вращает объект вокруг вертикальной оси.
struct Spinner : engine::Component {
    float speed_deg = 90.0f;

    void on_update(engine::Object self, float dt) override {
        auto rotation = self.rotation();
        rotation.y = std::fmod(rotation.y + speed_deg * dt, 360.0f);
        self.set_rotation(rotation);
    }
};
ENGINE_FIELDS(Spinner, ENGINE_FIELD(Spinner, speed_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Spinner);

// Плавно качает объект вокруг точки, где он стоял при старте.
struct Bobber : engine::Component {
    EngineVec3 amplitude{0.0f, 0.5f, 0.0f};
    float period = 2.0f;
    bool enabled = true;
    EngineVec3 origin{0.0f, 0.0f, 0.0f};

    void on_start(engine::Object self) override { origin = self.position(); }

    void on_update(engine::Object self, float) override {
        if (!enabled || period <= 0.0f) {
            return;
        }
        const float phase = std::sin(engine::time_seconds() * 6.2831853f / period);
        self.set_position({origin.x + amplitude.x * phase,
                           origin.y + amplitude.y * phase,
                           origin.z + amplitude.z * phase});
    }
};
ENGINE_FIELDS(Bobber,
              ENGINE_FIELD(Bobber, amplitude, ENGINE_FIELD_VEC3),
              ENGINE_FIELD(Bobber, period, ENGINE_FIELD_F32),
              ENGINE_FIELD(Bobber, enabled, ENGINE_FIELD_BOOL));
ENGINE_COMPONENT(Bobber);

// Двигает объект по осям ввода; считает, сколько раз нажали «Jump».
struct Mover : engine::Component {
    float speed = 4.0f;
    std::int32_t jumps = 0;

    void on_fixed_update(engine::Object self, float dt) override {
        auto position = self.position();
        position.x += engine::axis("Horizontal") * speed * dt;
        position.z -= engine::axis("Vertical") * speed * dt;
        self.set_position(position);
        if (engine::button_down("Jump")) {
            jumps += 1;
            // Сообщение получат все компоненты объекта — и на Rust, и на C++.
            self.send_message("Jumped", static_cast<float>(jumps));
        }
    }
};
ENGINE_FIELDS(Mover,
              ENGINE_FIELD(Mover, speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(Mover, jumps, ENGINE_FIELD_I32));
ENGINE_COMPONENT(Mover);

#ifndef ENGINE_GAMEPLAY_ENTRY
#  define ENGINE_GAMEPLAY_ENTRY engine_plugin_entry
#endif

// Точка входа собирает все компоненты плагина: и эти, и скрипты из папки scripts/.
ENGINE_PLUGIN_AUTO_ENTRY(ENGINE_GAMEPLAY_ENTRY, "piska_gameplay")
