#include "engine_component.hpp"

struct Probe : engine::Component {
    float speed_deg = 90.0f;
    void on_update(engine::Object self, float dt) override {
        auto r = self.rotation();
        self.set_rotation({r.x, r.y + speed_deg * dt, r.z});
    }
};
ENGINE_FIELDS(Probe, ENGINE_FIELD(Probe, speed_deg, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Probe);
