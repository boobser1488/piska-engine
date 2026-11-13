// C++-модуль шума: пример «тяжёлого вычисления», вынесенного из Rust.
// Компилируется build.rs (крейт `cc`) и линкуется статически.
//
// Внутри — обычный современный C++. Наружу — только C ABI из engine_noise.h:
// никаких классов, исключений и STL-типов через границу.

#include "engine_noise.h"

#include <algorithm>
#include <cmath>

namespace {

// Целочисленный хеш узла решётки → [0, 1).
inline float hash2(int32_t x, int32_t y, uint32_t seed) noexcept {
    uint32_t h = static_cast<uint32_t>(x) * 0x8da6b343u
               ^ static_cast<uint32_t>(y) * 0xd8163841u
               ^ seed * 0xcb1ab31fu;
    h ^= h >> 13;
    h *= 0x5bd1e995u;
    h ^= h >> 15;
    return static_cast<float>(h & 0x00ffffffu) / static_cast<float>(0x01000000u);
}

inline float smooth(float t) noexcept { return t * t * (3.0f - 2.0f * t); }

inline float lerp(float a, float b, float t) noexcept { return a + (b - a) * t; }

// Value-шум с билинейной интерполяцией по сглаженным весам.
float value_noise(float x, float y, uint32_t seed) noexcept {
    const float fx = std::floor(x);
    const float fy = std::floor(y);
    const auto ix = static_cast<int32_t>(fx);
    const auto iy = static_cast<int32_t>(fy);
    const float tx = smooth(x - fx);
    const float ty = smooth(y - fy);
    const float a = lerp(hash2(ix, iy, seed), hash2(ix + 1, iy, seed), tx);
    const float b = lerp(hash2(ix, iy + 1, seed), hash2(ix + 1, iy + 1, seed), tx);
    return lerp(a, b, ty);
}

float fbm(float x, float y, uint32_t octaves, uint32_t seed) noexcept {
    float sum = 0.0f;
    float amplitude = 0.5f;
    float norm = 0.0f;
    for (uint32_t i = 0; i < octaves; ++i) {
        sum += amplitude * value_noise(x, y, seed + i * 101u);
        norm += amplitude;
        x *= 2.0f;
        y *= 2.0f;
        amplitude *= 0.5f;
    }
    return norm > 0.0f ? std::clamp(sum / norm, 0.0f, 1.0f) : 0.0f;
}

}  // namespace

// ── FFI: вызывается из Rust (src/ffi.rs) ────────────────────────────────────

extern "C" float eng_noise_fbm2(float x, float y, uint32_t octaves, uint32_t seed) {
    return fbm(x, y, std::clamp<uint32_t>(octaves, 1u, 16u), seed);
}

extern "C" int32_t eng_noise_fill_rgba(uint8_t* out, size_t len, uint32_t width,
                                       uint32_t height, float scale, uint32_t seed) {
    // Аргументы перепроверяем и здесь: граница не доверяет ни одной стороне.
    if (out == nullptr || !(scale > 0.0f)) return 1;
    if (len != static_cast<size_t>(width) * static_cast<size_t>(height) * 4u) return 1;

    try {
        for (uint32_t y = 0; y < height; ++y) {
            for (uint32_t x = 0; x < width; ++x) {
                const float v = fbm(static_cast<float>(x) / scale,
                                    static_cast<float>(y) / scale, 5u, seed);
                const auto g = static_cast<uint8_t>(std::lround(v * 255.0f));
                uint8_t* p = out + (static_cast<size_t>(y) * width + x) * 4u;
                p[0] = g;
                p[1] = g;
                p[2] = g;
                p[3] = 255;
            }
        }
    } catch (...) {
        // Исключение не должно пересечь границу FFI — это неопределённое поведение.
        return 2;
    }
    return 0;
}
