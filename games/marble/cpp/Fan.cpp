// Вентилятор: объёмная сила вдоль «вверх» узла. Тело внутри столба получает ускорение
// lift (больше g — поднимается, меньше — зависает) и мягко подтягивается к оси столба.
// Ротор — ребёнок с именем «Rotor», он крутится.
#include "marble_common.hpp"

struct Fan : engine::Component {
    float lift = 26.0f;        // ускорение у основания, м/с² (гравитация игры — 15)
    float falloff = 0.45f;     // на сколько ослабевает к верхнему краю, 0..1
    float height = 5.5f;       // высота столба, м
    float radius = 1.7f;       // радиус столба, м
    float spin_deg = 540.0f;   // скорость ротора, град/с

    void on_fixed_update(engine::Object self, float) override {
        const engine::Vec3 base = self.world_position();
        const engine::Vec3 up = self.up();
        const engine::Vec3 mid = base + up * (height * 0.5f);
        const float reach = std::sqrt(height * height * 0.25f + radius * radius) + 0.5f;
        for (const engine::Object& body : engine::physics::overlap_sphere(mid, reach, 24)) {
            if (!body.alive() || !body.has_component("Rigidbody")) continue;
            const engine::Vec3 rel = body.world_position() - base;
            const float h = rel.dot(up);
            if (h < -0.3f || h > height) continue;
            const engine::Vec3 lateral = rel - up * h;
            if (lateral.length() > radius) continue;
            const float mass = engine::physics::mass(body);
            const float strength = lift * (1.0f - falloff * marble::clampf(h / height, 0.0f, 1.0f));
            // Мягкая «пружина» к оси: шар не вылетает из столба сразу.
            const engine::Vec3 pull = lateral * (-2.5f);
            engine::physics::add_force(body, (up * strength + pull) * mass);
        }
    }

    void on_update(engine::Object self, float dt) override {
        const engine::Object rotor = self.find_child("Rotor");
        if (!rotor) return;
        const engine::Vec3 r = rotor.rotation();
        rotor.set_rotation({r.x, std::fmod(r.y + spin_deg * dt, 360.0f), r.z});
    }
};
ENGINE_FIELDS(Fan,
              ENGINE_FIELD(Fan, lift, ENGINE_FIELD_F32),
              ENGINE_FIELD(Fan, falloff, ENGINE_FIELD_F32),
              ENGINE_FIELD(Fan, height, ENGINE_FIELD_F32),
              ENGINE_FIELD(Fan, radius, ENGINE_FIELD_F32),
              ENGINE_FIELD(Fan, spin_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Fan);
