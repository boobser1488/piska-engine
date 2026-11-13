// Площадка отгрузки: шар, въехавший в триггер, завершает цех. Маяк (ребёнок «Beacon»)
// вращается и мигает.
#include "marble_common.hpp"

struct FinishPad : engine::Component {
    float beacon_deg = 180.0f;   // скорость вращения маяка, град/с

    bool done = false;
    float clock = 0.0f;

    void on_update(engine::Object self, float dt) override {
        clock += dt;
        const engine::Object beacon = self.find_child("Beacon");
        if (!beacon) return;
        const engine::Vec3 r = beacon.rotation();
        beacon.set_rotation({r.x, std::fmod(r.y + beacon_deg * dt, 360.0f), r.z});
        const float k = 1.6f + 1.4f * (0.5f + 0.5f * std::sin(clock * 5.0f));
        beacon.set("MeshRenderer", "emissive", engine::Color{0.2f * k, 1.0f * k, 0.5f * k, 1.0f});
    }

    void on_trigger_enter(engine::Object self, engine::Object other) override {
        if (done || !marble::is_ball(other)) return;
        done = true;
        engine::audio::play_at("chime", self.world_position(), 1.0f);
        marble::tell_game("finish");
    }
};
ENGINE_FIELDS(FinishPad, ENGINE_FIELD(FinishPad, beacon_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(FinishPad);
