// Оформление цеха: мигающие лампы и медленно вращающиеся шестерни на заднем плане.
#include "marble_common.hpp"

// Лампа: яркость свечения (MeshRenderer.emissive) плавно пульсирует.
struct Blinker : engine::Component {
    engine::Color color{3.0f, 1.2f, 0.2f, 1.0f};   // цвет при полной яркости (ярче 1 — блум)
    float rate = 0.8f;                              // вспышек в секунду
    float low = 0.15f;                              // минимальная доля яркости
    float phase = 0.0f;                             // сдвиг, 0..1

    float clock = 0.0f;

    void on_update(engine::Object self, float dt) override {
        clock += dt;
        const float wave = 0.5f + 0.5f * std::sin((clock * rate + phase) * 2.0f * marble::kPi);
        const float k = marble::lerpf(low, 1.0f, wave);
        self.set("MeshRenderer", "emissive", engine::Color{color.r * k, color.g * k, color.b * k, 1.0f});
    }
};
ENGINE_FIELDS(Blinker,
              ENGINE_FIELD(Blinker, color, ENGINE_FIELD_COLOR),
              ENGINE_FIELD(Blinker, rate, ENGINE_FIELD_F32),
              ENGINE_FIELD(Blinker, low, ENGINE_FIELD_F32),
              ENGINE_FIELD(Blinker, phase, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Blinker);

// Шестерня-декорация: вращение вокруг одной из осей (0 — X, 1 — Y, 2 — Z).
struct Cog : engine::Component {
    float speed_deg = 12.0f;
    int axis = 2;

    engine::Vec3 base;

    void on_start(engine::Object self) override { base = self.rotation(); }

    void on_update(engine::Object self, float dt) override {
        const engine::Vec3 r = self.rotation();
        engine::Vec3 next = r;
        const float step = speed_deg * dt;
        if (axis == 0) next.x = std::fmod(r.x + step, 360.0f);
        else if (axis == 1) next.y = std::fmod(r.y + step, 360.0f);
        else next.z = std::fmod(r.z + step, 360.0f);
        self.set_rotation(next);
    }
};
ENGINE_FIELDS(Cog,
              ENGINE_FIELD(Cog, speed_deg, ENGINE_FIELD_F32),
              ENGINE_FIELD(Cog, axis, ENGINE_FIELD_I32));
ENGINE_COMPONENT(Cog);
