// Платформа ездит между начальной точкой и точкой «начало + travel».
// Кинематическое тело: шар на ней едет за счёт трения. Двигаем в on_update по времени —
// движение плавное при любой частоте кадров, а физика берёт последнюю позу на каждом шаге.
#include "marble_common.hpp"

struct MovingPlatform : engine::Component {
    engine::Vec3 travel{0.0f, 0.0f, -8.0f};  // смещение конечной точки от начальной, м
    float period = 6.0f;                      // полный цикл туда и обратно, с
    float pause = 0.8f;                       // стоянка в каждой крайней точке, с
    float phase = 0.0f;                       // сдвиг цикла, 0..1

    engine::Vec3 origin;                      // не поле: запоминается в on_start
    float clock = 0.0f;

    void on_start(engine::Object self) override {
        origin = self.position();
        apply(self);
    }

    // Положение вдоль пути, 0..1: стоим, едем, стоим, возвращаемся.
    float progress() const {
        const float cycle = std::max(period, 0.1f);
        const float half = cycle * 0.5f;
        const float hold = std::min(pause, half * 0.9f);
        float u = std::fmod(clock + phase * cycle, cycle);
        if (u < 0.0f) u += cycle;
        const bool back = u >= half;
        const float local = back ? u - half : u;
        const float run = marble::smooth((local - hold * 0.5f) / std::max(half - hold, 0.01f));
        return back ? 1.0f - run : run;
    }

    void apply(engine::Object self) const { self.set_position(origin + travel * progress()); }

    void on_update(engine::Object self, float dt) override {
        clock += dt;
        apply(self);
    }
};
ENGINE_FIELDS(MovingPlatform,
              ENGINE_FIELD(MovingPlatform, travel, ENGINE_FIELD_VEC3),
              ENGINE_FIELD(MovingPlatform, period, ENGINE_FIELD_F32),
              ENGINE_FIELD(MovingPlatform, pause, ENGINE_FIELD_F32),
              ENGINE_FIELD(MovingPlatform, phase, ENGINE_FIELD_F32));
ENGINE_COMPONENT(MovingPlatform);
