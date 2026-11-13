// Patroller — враг, который ходит туда-сюда и разворачивается у стены или обрыва. Две
// проверки лучами: вперёд (стена) и вниз перед собой (есть ли земля). Коснулся игрока —
// шлёт ему сообщение "Hit". Нужны Rigidbody2D и Collider2D.

#include "engine_component.hpp"

struct Patroller : engine::Component {
    float speed = 2.0f;
    // +1 — вправо, −1 — влево.
    float direction = 1.0f;
    // Насколько вперёд от центра смотрят лучи.
    float reach = 0.7f;
    std::int32_t turns = 0;

    void on_fixed_update(engine::Object self, float) override {
        const engine::Vec3 p = self.world_position();
        const engine::Vec2 origin{p.x, p.y};
        const bool wall = static_cast<bool>(
            engine::physics::raycast2d(origin, {direction, 0.0f}, reach, self));
        const bool ground = static_cast<bool>(
            engine::physics::raycast2d({p.x + direction * reach, p.y}, {0.0f, -1.0f}, 1.5f, self));
        if (wall || !ground) {
            direction = -direction;
            turns += 1;
        }
        engine::Vec2 v = engine::physics::velocity2d(self);
        v.x = direction * speed;
        engine::physics::set_velocity2d(self, v);
    }

    void on_collision_enter(engine::Object, engine::Object other) override {
        if (other.alive() && other.tag() == "Player") other.send_message("Hit");
    }
};
ENGINE_FIELDS(Patroller,
              ENGINE_FIELD(Patroller, speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(Patroller, direction, ENGINE_FIELD_F32),
              ENGINE_FIELD(Patroller, reach, ENGINE_FIELD_F32),
              ENGINE_FIELD(Patroller, turns, ENGINE_FIELD_I32));
ENGINE_COMPONENT(Patroller);
