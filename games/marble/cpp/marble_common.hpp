// Общие мелочи механизмов «Шарового завода». Все файлы cpp/ собираются одной единицей
// трансляции (engine-cpp-build), поэтому заголовок защищён от повторного включения.
#pragma once

#include "engine_component.hpp"

#include <algorithm>
#include <cmath>
#include <string>

namespace marble {

constexpr float kPi = 3.14159265358979f;

inline float clampf(float v, float lo, float hi) { return v < lo ? lo : (v > hi ? hi : v); }
inline float lerpf(float a, float b, float t) { return a + (b - a) * t; }
inline float smooth(float t) { t = clampf(t, 0.0f, 1.0f); return t * t * (3.0f - 2.0f * t); }
// Дробная часть: 2.3 -> 0.3, -0.2 -> 0.8.
inline float frac(float v) { return v - std::floor(v); }

// Шар игрока помечен тегом «Ball».
inline bool is_ball(const engine::Object& o) { return o && o.alive() && o.tag() == "Ball"; }

// Объект «Game» — менеджер игры на стороне Rust; ему механизмы шлют сообщения
// («gear», «checkpoint», «finish», «squashed», «shake»…).
inline void tell_game(const char* message, const engine::Value& value = engine::Value()) {
    const engine::Object game = engine::find_by_name("Game");
    if (game) game.send_message(message, value);
}

// Шар среди тел, которые задевает сфера.
inline engine::Object find_ball_near(engine::Vec3 center, float radius) {
    for (const engine::Object& o : engine::physics::overlap_sphere(center, radius, 16)) {
        if (is_ball(o)) return o;
    }
    return engine::Object{};
}

// Положение точки `p` в осях объекта: x — вправо, y — вверх, z — вперёд (−Z объекта).
inline engine::Vec3 to_local_axes(const engine::Object& self, engine::Vec3 p) {
    const engine::Vec3 rel = p - self.world_position();
    return {rel.dot(self.right()), rel.dot(self.up()), rel.dot(self.forward())};
}

}  // namespace marble
