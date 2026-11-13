// LightFlicker — скрипт на источнике света: яркость дрожит, как у огня или
// неисправной лампы. Повесьте его на объект с компонентом Light.

#include "engine_component.hpp"

#include <cmath>

struct LightFlicker : engine::Component {
    // Доля яркости, на которую свет может просесть (0 — не мерцает, 1 — гаснет полностью).
    float amount = 0.35f;
    // Как быстро меняется яркость.
    float speed = 9.0f;
    // Яркость, вокруг которой идёт мерцание; запоминается при старте.
    float base_intensity = 0.0f;

    void on_start(engine::Object self) override {
        // Поле другого компонента (он на Rust) читается по именам.
        self.get("Light", "intensity", base_intensity);
    }

    void on_update(engine::Object self, float) override {
        const float t = engine::time_seconds() * speed;
        // Сумма несоизмеримых синусов выглядит как шум, но не требует состояния.
        const float noise = 0.5f + 0.25f * std::sin(t) + 0.15f * std::sin(t * 2.31f + 1.7f)
                            + 0.10f * std::sin(t * 5.13f + 0.4f);
        self.set("Light", "intensity", base_intensity * (1.0f - amount * noise));
    }
};
ENGINE_FIELDS(LightFlicker,
              ENGINE_FIELD(LightFlicker, amount, ENGINE_FIELD_F32),
              ENGINE_FIELD(LightFlicker, speed, ENGINE_FIELD_F32));
ENGINE_COMPONENT(LightFlicker);
