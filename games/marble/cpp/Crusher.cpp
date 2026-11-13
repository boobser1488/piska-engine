// Пресс: поршень ждёт наверху, быстро бьёт вниз, держит и медленно поднимается. Шар под
// опускающимся поршнем раздавлен — игра вернёт его на контрольную точку («squashed»).
// Устройство: узел (без масштаба) — верхняя точка хода; дети «Head» (кинематический
// коллайдер с хэзард-текстурой) и «Shaft» (штанга, только вид).
#include "marble_common.hpp"

struct Crusher : engine::Component {
    float stroke = 3.0f;      // ход поршня вниз, м
    float period = 3.6f;      // полный цикл, с
    float wait_up = 1.6f;     // стоит наверху, с
    float slam_time = 0.14f;  // удар вниз, с
    float hold_down = 0.5f;   // держит внизу, с
    float phase = 0.0f;       // сдвиг цикла, 0..1

    engine::Object head, shaft;
    float clock = 0.0f;
    float head_half = 0.5f;   // половина толщины головы
    float shaft_half = 1.0f;  // половина длины штанги
    bool squashed_this_cycle = false;
    bool thudded = false;
    float last_s = 0.0f;

    void on_start(engine::Object self) override {
        head = self.find_child("Head");
        shaft = self.find_child("Shaft");
        if (head) head_half = head.scale().y * 0.5f;
        if (shaft) shaft_half = shaft.scale().y * 0.5f;
        apply(0.0f);
    }

    // Глубина хода 0 (вверху) .. 1 (внизу) и признак «бьёт» (голова ещё идёт вниз или внизу).
    float depth(float& danger) const {
        const float cycle = std::max(period, wait_up + slam_time + hold_down + 0.2f);
        float u = std::fmod(clock + phase * cycle, cycle);
        if (u < 0.0f) u += cycle;
        danger = 0.0f;
        if (u < wait_up) return 0.0f;
        u -= wait_up;
        if (u < slam_time) { danger = 1.0f; return marble::smooth(u / slam_time); }
        u -= slam_time;
        if (u < hold_down) { danger = 1.0f; return 1.0f; }
        u -= hold_down;
        const float rise = std::max(cycle - wait_up - slam_time - hold_down, 0.1f);
        return 1.0f - marble::smooth(u / rise);
    }

    void apply(float s) {
        last_s = s;
        const float y = -stroke * s;
        if (head) head.set_position({0.0f, y, 0.0f});
        if (shaft) shaft.set_position({0.0f, y + head_half + shaft_half, 0.0f});
    }

    void on_update(engine::Object self, float dt) override {
        clock += dt;
        float danger = 0.0f;
        const float s = depth(danger);
        apply(s);
        if (s < 0.05f) { squashed_this_cycle = false; thudded = false; }
        if (s > 0.97f && !thudded) {
            thudded = true;
            engine::audio::play_at("thud", self.world_position() - engine::Vec3{0.0f, stroke, 0.0f}, 1.0f);
            marble::tell_game("shake", 0.28f);
        }
    }

    // Шар под головой, пока та идёт вниз или стоит внизу, раздавлен.
    void on_fixed_update(engine::Object self, float) override {
        if (!head || squashed_this_cycle || last_s < 0.35f) return;
        float danger = 0.0f;
        depth(danger);
        if (danger < 0.5f) return;
        const engine::Vec3 center = head.world_position();
        const engine::Vec3 half = head.scale() * 0.5f;
        const engine::Object ball = marble::find_ball_near(center - engine::Vec3{0.0f, half.y + 0.4f, 0.0f}, half.x + half.z + 1.0f);
        if (!ball) return;
        // Положение шара в осях головы (она повёрнута вместе с узлом).
        const engine::Vec3 d = marble::to_local_axes(head, ball.world_position());
        const float bottom = -half.y;
        if (std::fabs(d.x) < half.x + 0.3f && std::fabs(d.z) < half.z + 0.3f && d.y + 0.5f > bottom - 0.1f && d.y < half.y) {
            squashed_this_cycle = true;
            engine::audio::play_at("thud", center, 1.0f);
            marble::tell_game("squashed");
        }
        (void)self;
    }
};
ENGINE_FIELDS(Crusher,
              ENGINE_FIELD(Crusher, stroke, ENGINE_FIELD_F32),
              ENGINE_FIELD(Crusher, period, ENGINE_FIELD_F32),
              ENGINE_FIELD(Crusher, wait_up, ENGINE_FIELD_F32),
              ENGINE_FIELD(Crusher, slam_time, ENGINE_FIELD_F32),
              ENGINE_FIELD(Crusher, hold_down, ENGINE_FIELD_F32),
              ENGINE_FIELD(Crusher, phase, ENGINE_FIELD_F32));
ENGINE_COMPONENT(Crusher);
