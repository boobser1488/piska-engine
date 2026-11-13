// Бампер, как в пинболе: отталкивает шар от центра, подпрыгивает и вспыхивает.
#include "marble_common.hpp"

struct Bumper : engine::Component {
    float kick = 9.0f;                              // скорость шара от бампера после удара, м/с
    float lift_up = 1.5f;                           // добавка вверх, м/с
    engine::Color glow{4.0f, 1.4f, 0.2f, 1.0f};     // цвет вспышки (ярче 1 — даёт блум)

    float cooldown = 0.0f;
    engine::Vec3 rest_scale{1.0f, 1.0f, 1.0f};

    void on_start(engine::Object self) override { rest_scale = self.scale(); }

    void on_update(engine::Object, float dt) override { cooldown = std::max(0.0f, cooldown - dt); }

    void on_collision_enter(engine::Object self, engine::Object other) override {
        if (!marble::is_ball(other) || cooldown > 0.0f) return;
        cooldown = 0.15f;
        engine::Vec3 away = other.world_position() - self.world_position();
        away.y = 0.0f;
        away = away.normalized();
        // Гасим скорость «в бампер» и выдаём новую вдоль нормали: шар не липнет и не гаснет.
        const float into = engine::physics::velocity(other).dot(away);
        const float mass = engine::physics::mass(other);
        const float delta = std::max(kick - into, 0.0f);
        engine::physics::apply_impulse(other, (away * delta + engine::Vec3{0.0f, lift_up, 0.0f}) * mass);
        engine::audio::play_at("bumper", self.world_position(), 0.9f);

        // Пружинка: сжался и вернулся; вспышка гаснет.
        engine::TweenParams squash(0.1f, "OutQuad");
        squash.yoyo = true;
        squash.loops = 2;
        self.tween("Transform.scale", rest_scale * 1.18f, squash);
        self.set("MeshRenderer", "emissive", glow);
        self.tween("MeshRenderer.emissive", engine::Color{0.15f, 0.05f, 0.0f, 1.0f}, engine::TweenParams(0.3f, "OutQuad"));
        marble::tell_game("shake", 0.12f);
    }
};
ENGINE_FIELDS(Bumper,
              ENGINE_FIELD(Bumper, kick, ENGINE_FIELD_F32),
              ENGINE_FIELD(Bumper, lift_up, ENGINE_FIELD_F32),
              ENGINE_FIELD(Bumper, glow, ENGINE_FIELD_COLOR));
ENGINE_COMPONENT(Bumper);
