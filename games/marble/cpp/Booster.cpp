// Ускоритель: плита-триггер толкает шар вдоль «вперёд» (−Z узла) до скорости speed.
// Односторонний: шар, едущий уже быстрее, не тормозится.
#include "marble_common.hpp"

struct Booster : engine::Component {
    float speed = 13.0f;       // целевая скорость вдоль плиты, м/с
    float pop = 1.2f;          // небольшой подскок, м/с — чтобы не цепляться за стыки

    float cooldown = 0.0f;

    void on_update(engine::Object, float dt) override { cooldown = std::max(0.0f, cooldown - dt); }

    void on_trigger_enter(engine::Object self, engine::Object other) override {
        if (cooldown > 0.0f || !marble::is_ball(other)) return;
        cooldown = 0.5f;
        const engine::Vec3 along = self.forward();
        const float have = engine::physics::velocity(other).dot(along);
        const float mass = engine::physics::mass(other);
        const float delta = std::max(speed - have, 0.0f);
        engine::physics::apply_impulse(other, (along * delta + engine::Vec3{0.0f, pop, 0.0f}) * mass);
        engine::audio::play_at("boost", self.world_position(), 0.9f);
        marble::tell_game("shake", 0.1f);
        const engine::Object arrows = self.find_child("Arrows");
        if (arrows) {
            arrows.set("MeshRenderer", "emissive", engine::Color{3.0f, 2.4f, 0.4f, 1.0f});
            arrows.tween("MeshRenderer.emissive", engine::Color{0.6f, 0.45f, 0.05f, 1.0f}, engine::TweenParams(0.6f, "OutQuad"));
        }
    }
};
ENGINE_FIELDS(Booster,
              ENGINE_FIELD(Booster, speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(Booster, pop, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Booster);
