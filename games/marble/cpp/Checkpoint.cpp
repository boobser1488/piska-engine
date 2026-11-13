// Контрольная точка: триггер-ворота. Шар, проехавший сквозь них, запоминает место: после
// падения игра вернёт его сюда. Лампа (ребёнок «Lamp») меняет цвет с красного на зелёный.
#include "marble_common.hpp"

struct Checkpoint : engine::Component {
    int index = 1;             // порядковый номер на цехе (0 — старт)
    float yaw_deg = 0.0f;      // куда смотрит камера после возврата сюда

    bool reached = false;

    void on_start(engine::Object self) override { paint(self, false); }

    void paint(engine::Object self, bool on) const {
        const engine::Object lamp = self.find_child("Lamp");
        if (!lamp) return;
        const engine::Color c = on ? engine::Color{0.3f, 3.0f, 0.6f, 1.0f} : engine::Color{3.0f, 0.4f, 0.2f, 1.0f};
        lamp.set("MeshRenderer", "emissive", c);
    }

    void on_trigger_enter(engine::Object self, engine::Object other) override {
        if (reached || !marble::is_ball(other)) return;
        reached = true;
        paint(self, true);
        marble::tell_game("checkpoint_info", engine::Vec2{static_cast<float>(index), yaw_deg});
        marble::tell_game("checkpoint", self.world_position());
        engine::audio::play_at("chime", self.world_position(), 0.9f);
    }
};
ENGINE_FIELDS(Checkpoint,
              ENGINE_FIELD(Checkpoint, index, ENGINE_FIELD_I32),
              ENGINE_FIELD(Checkpoint, yaw_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Checkpoint);
