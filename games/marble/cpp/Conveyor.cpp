// Конвейер: тянет всё, что лежит на ленте, вдоль «вперёд» узла (−Z). Узел не масштабируется;
// лента (коллайдер) и планки — его дети. Скорость поддерживается силой: тело разгоняется
// до speed и не больше; против хода идти можно, но трудно.
#include "marble_common.hpp"

struct Conveyor : engine::Component {
    float speed = 4.0f;                              // м/с вдоль ленты; минус — задом наперёд
    float grip = 12.0f;                              // как быстро тело подтягивается к скорости ленты
    engine::Vec3 size{4.0f, 0.4f, 10.0f};            // габариты ленты: ширина, толщина, длина
    float slat_spacing = 1.2f;                       // расстояние между бегущими планками

    float scroll = 0.0f;

    void on_fixed_update(engine::Object self, float) override {
        const engine::Vec3 center = self.world_position();
        const float reach = 0.5f * std::sqrt(size.x * size.x + size.z * size.z) + 1.5f;
        const engine::Vec3 along = self.forward();
        for (const engine::Object& body : engine::physics::overlap_sphere(center, reach, 24)) {
            if (!body.alive() || !body.has_component("Rigidbody")) continue;
            const engine::Vec3 local = marble::to_local_axes(self, body.world_position());
            if (std::fabs(local.x) > size.x * 0.5f + 0.15f || std::fabs(local.z) > size.z * 0.5f + 0.15f) continue;
            const float above = local.y - size.y * 0.5f;
            if (above < -0.2f || above > 1.4f) continue;
            const float mass = engine::physics::mass(body);
            const float v_along = engine::physics::velocity(body).dot(along);
            const float wanted = speed - v_along;
            // Ускорение ленты ограничено (9 м/с²) — меньше ускорения шара (17): против ленты идти можно.
            const float force = marble::clampf(wanted * grip, -9.0f, 9.0f) * mass;
            engine::physics::add_force(body, along * force);
        }
    }

    // Планки бегут по ленте и переходят с конца в начало: видно, куда и как быстро она везёт.
    void on_update(engine::Object self, float dt) override {
        scroll += speed * dt;
        const std::uint32_t n = self.child_count();
        const float half = size.z * 0.5f;
        std::uint32_t slat_index = 0;
        for (std::uint32_t i = 0; i < n; ++i) {
            const engine::Object slat = self.child(i);
            if (slat.name() != "Slat") continue;
            const float u = marble::frac((scroll + static_cast<float>(slat_index) * slat_spacing) / std::max(size.z, 0.1f));
            engine::Vec3 p = slat.position();
            p.z = half - u * size.z;
            slat.set_position(p);
            ++slat_index;
        }
    }
};
ENGINE_FIELDS(Conveyor,
              ENGINE_FIELD(Conveyor, speed, ENGINE_FIELD_F32),
              ENGINE_FIELD(Conveyor, grip, ENGINE_FIELD_F32),
              ENGINE_FIELD(Conveyor, size, ENGINE_FIELD_VEC3),
              ENGINE_FIELD(Conveyor, slat_spacing, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Conveyor);
