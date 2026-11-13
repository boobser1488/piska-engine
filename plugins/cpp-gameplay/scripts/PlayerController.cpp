// PlayerController — управление персонажем в 2D: бег по оси Horizontal, прыжок по кнопке Jump.
// Нужны Rigidbody2D и Collider2D на том же объекте. Возвращает игрока на старт, если он упал
// ниже fall_limit или получил сообщение "Hit" (его шлёт Patroller).

#include "engine_component.hpp"

struct PlayerController : engine::Component {
    float speed = 6.0f;
    float jump_speed = 11.0f;
    // Ниже этой высоты игрок считается упавшим.
    float fall_limit = -30.0f;
    // Имя звука прыжка (Audio::add_clip); пусто — без звука.
    std::string jump_sound = "jump";
    engine::Vec3 spawn_point;
    std::int32_t jumps = 0;

    void on_start(engine::Object self) override { spawn_point = self.position(); }

    // Ввод читается прямо в on_fixed_update: «нажато» там значит «нажато с прошлого шага»,
    // поэтому короткое нажатие не теряется.
    void on_fixed_update(engine::Object self, float) override {
        if (self.position().y < fall_limit) {
            respawn(self);
            return;
        }
        engine::Vec2 v = engine::physics::velocity2d(self);
        v.x = engine::input::axis("Horizontal") * speed;
        if (engine::input::button_down("Jump") && grounded(self)) {
            v.y = jump_speed;
            jumps += 1;
            if (!jump_sound.empty()) engine::audio::play(jump_sound.c_str());
        }
        engine::physics::set_velocity2d(self, v);
    }

    void on_message(engine::Object self, const char* name, const engine::Value&) override {
        if (std::string(name) == "Hit") respawn(self);
    }

private:
    void respawn(engine::Object self) {
        self.set_position(spawn_point);
        engine::physics::set_velocity2d(self, engine::Vec2{});
    }

    // Стоим ли на чём-то твёрдом: короткий луч вниз от центра.
    bool grounded(engine::Object self) const {
        if (engine::physics::velocity2d(self).y > 0.5f) return false;
        engine::Vec2 half{0.5f, 0.5f};
        engine::Vec2 size;
        if (self.get("Collider2D", "size", size)) half = size * 0.5f;
        const engine::Vec3 p = self.world_position();
        return static_cast<bool>(engine::physics::raycast2d({p.x, p.y}, {0.0f, -1.0f}, half.y + 0.08f, self));
    }
};
ENGINE_FIELDS(PlayerController,
              ENGINE_FIELD(PlayerController, speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(PlayerController, jump_speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(PlayerController, fall_limit, ENGINE_FIELD_F32),
              ENGINE_FIELD(PlayerController, jump_sound, ENGINE_FIELD_STRING),
              ENGINE_FIELD(PlayerController, spawn_point, ENGINE_FIELD_VEC3),
              ENGINE_FIELD(PlayerController, jumps, ENGINE_FIELD_I32));
ENGINE_COMPONENT(PlayerController);
