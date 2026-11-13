// Шестерёнка: вращается и покачивается; шар, коснувшийся её триггера, забирает её.
// Золотая (одна на цех) — отдельное сообщение, игра считает её отдельно.
#include "marble_common.hpp"

struct GearPickup : engine::Component {
    bool golden = false;       // золотая шестерня цеха
    float spin_deg = 120.0f;   // вращение, град/с
    float bob_height = 0.12f;  // покачивание, м

    engine::Vec3 origin;
    float clock = 0.0f;
    bool taken = false;

    void on_start(engine::Object self) override {
        origin = self.position();
        clock = engine::rng::range(0.0f, 6.28f);
    }

    void on_update(engine::Object self, float dt) override {
        clock += dt;
        const engine::Vec3 r = self.rotation();
        self.set_rotation({r.x, std::fmod(r.y + spin_deg * dt, 360.0f), r.z});
        self.set_position(origin + engine::Vec3{0.0f, std::sin(clock * 2.2f) * bob_height, 0.0f});
    }

    void on_trigger_enter(engine::Object self, engine::Object other) override {
        if (taken || !marble::is_ball(other)) return;
        taken = true;
        marble::tell_game(golden ? "golden_gear" : "gear", self.world_position());
        self.destroy();
    }
};
ENGINE_FIELDS(GearPickup,
              ENGINE_FIELD(GearPickup, golden, ENGINE_FIELD_BOOL),
              ENGINE_FIELD(GearPickup, spin_deg, ENGINE_FIELD_F32),
              ENGINE_FIELD(GearPickup, bob_height, ENGINE_FIELD_F32));
ENGINE_COMPONENT(GearPickup);
