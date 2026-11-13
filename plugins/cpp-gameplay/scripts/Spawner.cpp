// Spawner — раз в interval секунд создаёт экземпляр префаба рядом с собой, но не больше
// max_alive одновременно живых. Создаваемым объектам ставится тег spawn_tag.

#include "engine_component.hpp"

struct Spawner : engine::Component {
    // Путь префаба относительно каталога ассетов (или GUID).
    std::string prefab;
    float interval = 2.0f;
    std::int32_t max_alive = 3;
    // Разброс по X вокруг спавнера.
    float spread = 1.0f;
    std::string spawn_tag = "Spawned";
    std::int32_t spawned = 0;

    void on_start(engine::Object self) override {
        // Таймер — это сообщение самому себе; он умирает вместе с объектом.
        self.send_every(interval, "Spawn");
    }

    void on_message(engine::Object self, const char* name, const engine::Value&) override {
        if (std::string(name) != "Spawn" || prefab.empty()) return;
        if (static_cast<std::int32_t>(engine::find_all_by_tag(spawn_tag.c_str()).size()) >= max_alive) return;
        engine::Vec3 at = self.world_position();
        at.x += engine::rng::range(-spread, spread);
        const engine::Object made = engine::instantiate(prefab.c_str(), at);
        if (!made) return;  // префаб не нашёлся: причина уже в логе
        made.set_tag(spawn_tag.c_str());
        spawned += 1;
    }
};
ENGINE_FIELDS(Spawner,
              ENGINE_FIELD(Spawner, prefab, ENGINE_FIELD_STRING),
              ENGINE_FIELD(Spawner, interval, ENGINE_FIELD_F32),
              ENGINE_FIELD(Spawner, max_alive, ENGINE_FIELD_I32),
              ENGINE_FIELD(Spawner, spread, ENGINE_FIELD_F32),
              ENGINE_FIELD(Spawner, spawn_tag, ENGINE_FIELD_STRING),
              ENGINE_FIELD(Spawner, spawned, ENGINE_FIELD_I32));
ENGINE_COMPONENT(Spawner);
