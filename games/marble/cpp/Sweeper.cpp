// Вращающаяся «метла»: узел крутится вокруг вертикальной оси, плечи — его дети с
// кинематическими коллайдерами. swing_deg > 0 — не вертится по кругу, а качается.
#include "marble_common.hpp"

struct Sweeper : engine::Component {
    float speed_deg = 50.0f;   // градусов в секунду (при качании — скорость фазы)
    float swing_deg = 0.0f;    // 0 — полный оборот; иначе размах ± в градусах
    float phase_deg = 0.0f;    // начальный угол

    engine::Vec3 base;         // начальный поворот узла (запоминается в on_start)
    float angle = 0.0f;

    void on_start(engine::Object self) override {
        base = self.rotation();
        angle = phase_deg;
        apply(self);
    }

    void apply(engine::Object self) const {
        const float a = swing_deg > 0.0f ? swing_deg * std::sin(angle * marble::kPi / 180.0f) : angle;
        self.set_rotation({base.x, base.y + a, base.z});
    }

    void on_update(engine::Object self, float dt) override {
        angle += speed_deg * dt;
        if (angle > 36000.0f) angle -= 36000.0f;
        apply(self);
    }
};
ENGINE_FIELDS(Sweeper,
              ENGINE_FIELD(Sweeper, speed_deg, ENGINE_FIELD_F32),
              ENGINE_FIELD(Sweeper, swing_deg, ENGINE_FIELD_F32),
              ENGINE_FIELD(Sweeper, phase_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Sweeper);

// Плечо метлы: звонкий удар, когда оно задевает шар. Висит на детях узла Sweeper.
struct SweeperArm : engine::Component {
    float volume = 0.8f;
    float cooldown = 0.0f;

    void on_update(engine::Object, float dt) override { cooldown = std::max(0.0f, cooldown - dt); }

    void on_collision_enter(engine::Object, engine::Object other) override {
        if (!marble::is_ball(other) || cooldown > 0.0f) return;
        cooldown = 0.25f;
        engine::audio::play_at("clang", other.world_position(), volume);
        marble::tell_game("shake", 0.1f);
    }
};
ENGINE_FIELDS(SweeperArm, ENGINE_FIELD(SweeperArm, volume, ENGINE_FIELD_F32));
ENGINE_COMPONENT(SweeperArm);
