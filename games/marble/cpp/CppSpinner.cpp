// Необязательный пример смешения языков: один C++-компонент в игре на Rust.
// Собирается только с возможностью `cpp-example` (cargo test -p marble --features cpp-example);
// в сценах игры не используется. Подробно — docs/api/cpp.md, раздел «C++ в своей игре».
#include "engine_component.hpp"

struct CppSpinner : engine::Component {
    float speed_deg = 90.0f;   // градусов в секунду вокруг вертикальной оси

    void on_update(engine::Object self, float dt) override {
        const engine::Vec3 r = self.rotation();
        self.set_rotation({r.x, r.y + speed_deg * dt, r.z});
    }
};
ENGINE_FIELDS(CppSpinner, ENGINE_FIELD(CppSpinner, speed_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(CppSpinner);
