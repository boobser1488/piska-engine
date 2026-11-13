// Хрупкая плита: пока на ней стоит шар, она трескается (дрожит), потом проваливается,
// а через несколько секунд возвращается на место. Кинематическое тело с коллайдером.
// Шар ищем сами (overlap_sphere над плитой): событие касания кинематики ненадёжно, а
// такой опрос работает и для медленно подкатившегося шара.
#include "marble_common.hpp"

struct CollapsingTile : engine::Component {
    float delay = 0.7f;        // сколько плита держится под шаром, прежде чем провалиться, с
    float respawn = 5.0f;      // через сколько вернуться, с
    float shake = 0.04f;       // амплитуда дрожи, м

    enum class State { Idle, Warn, Fall, Gone };
    State state = State::Idle;
    float timer = 0.0f;
    float drop = 0.0f;         // насколько уже упала
    float fall_speed = 0.0f;
    engine::Vec3 origin;

    void on_start(engine::Object self) override { origin = self.position(); }

    bool ball_on_top(engine::Object self) const {
        const engine::Vec3 half = self.scale() * 0.5f;
        const engine::Vec3 center = self.world_position();
        const engine::Object ball = marble::find_ball_near(center + engine::Vec3{0.0f, half.y + 0.5f, 0.0f}, half.x + half.z + 1.2f);
        if (!ball) return false;
        const engine::Vec3 d = marble::to_local_axes(self, ball.world_position());
        return std::fabs(d.x) <= half.x + 0.25f && std::fabs(d.z) <= half.z + 0.25f && d.y > half.y && d.y < half.y + 1.3f;
    }

    void on_fixed_update(engine::Object self, float) override {
        if (state == State::Idle && ball_on_top(self)) {
            state = State::Warn;
            timer = 0.0f;
            engine::audio::play_at("crack", self.world_position(), 0.9f);
        }
    }

    void on_update(engine::Object self, float dt) override {
        switch (state) {
            case State::Idle:
                break;
            case State::Warn: {
                timer += dt;
                const float k = shake * (0.4f + 0.6f * timer / std::max(delay, 0.05f));
                self.set_position(origin + engine::Vec3{engine::rng::range(-k, k), 0.0f, engine::rng::range(-k, k)});
                if (timer >= delay) {
                    state = State::Fall;
                    timer = 0.0f;
                    drop = 0.0f;
                    fall_speed = 0.0f;
                    engine::audio::play_at("crack", self.world_position(), 1.0f);
                }
                break;
            }
            case State::Fall:
                fall_speed += 22.0f * dt;
                drop += fall_speed * dt;
                self.set_position(origin - engine::Vec3{0.0f, drop, 0.0f});
                if (drop > 60.0f) { state = State::Gone; timer = 0.0f; }
                break;
            case State::Gone:
                timer += dt;
                if (timer >= respawn) {
                    state = State::Idle;
                    self.set_position(origin);
                }
                break;
        }
    }
};
ENGINE_FIELDS(CollapsingTile,
              ENGINE_FIELD(CollapsingTile, delay, ENGINE_FIELD_F32),
              ENGINE_FIELD(CollapsingTile, respawn, ENGINE_FIELD_F32),
              ENGINE_FIELD(CollapsingTile, shake, ENGINE_FIELD_F32));
ENGINE_COMPONENT(CollapsingTile);
