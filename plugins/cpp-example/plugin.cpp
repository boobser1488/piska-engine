// Пример C++-плагина движка: CPU пост-эффект «раскраска карты высот + виньетка».
//
// Плагин — обычная динамическая библиотека. Единственное, что он знает о движке, —
// сгенерированный cbindgen заголовок engine_api.h (C ABI): никаких Rust-типов,
// классов и исключений через границу.

#include "engine_api.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>

#if defined(_WIN32)
#  define PLUGIN_EXPORT __declspec(dllexport)
#else
#  define PLUGIN_EXPORT __attribute__((visibility("default")))
#endif

namespace {

const EngineApi* g_engine = nullptr;
float g_time = 0.0f;

struct Rgb {
    float r, g, b;
};

Rgb mix(Rgb a, Rgb b, float t) noexcept {
    return {a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t};
}

// Палитра «карта высот»: глубина → мелководье → песок → трава → скалы → снег.
Rgb height_to_color(float h) noexcept {
    static const struct { float at; Rgb color; } stops[] = {
        {0.00f, {0.05f, 0.12f, 0.30f}},
        {0.40f, {0.10f, 0.35f, 0.60f}},
        {0.48f, {0.85f, 0.78f, 0.55f}},
        {0.58f, {0.25f, 0.55f, 0.25f}},
        {0.75f, {0.45f, 0.40f, 0.35f}},
        {1.00f, {0.97f, 0.97f, 1.00f}},
    };
    for (size_t i = 1; i < sizeof(stops) / sizeof(stops[0]); ++i) {
        if (h <= stops[i].at) {
            const float t = (h - stops[i - 1].at) / (stops[i].at - stops[i - 1].at);
            return mix(stops[i - 1].color, stops[i].color, std::clamp(t, 0.0f, 1.0f));
        }
    }
    return stops[5].color;
}

uint8_t to_byte(float v) noexcept {
    return static_cast<uint8_t>(std::lround(std::clamp(v, 0.0f, 1.0f) * 255.0f));
}

}  // namespace

// ── FFI: колбэки, которые вызывает движок (Rust) ────────────────────────────
// Все — с C-линковкой и без исключений наружу.
extern "C" {

static int32_t on_load(const EngineApi* api) {
    if (api == nullptr || api->abi_version != ENGINE_ABI_VERSION) return 1;
    g_engine = api;
    g_engine->log(2, "vignette plugin: hello from C++");
    return 0;
}

static void on_fixed_update(float dt) { g_time += dt; }

// pixels: width * height * 4 байт RGBA8 (гарантирует Rust-обёртка Plugin::process_rgba).
static void process_rgba(uint8_t* pixels, uint32_t width, uint32_t height) {
    if (pixels == nullptr || width == 0 || height == 0) return;
    for (uint32_t y = 0; y < height; ++y) {
        for (uint32_t x = 0; x < width; ++x) {
            uint8_t* p = pixels + (static_cast<size_t>(y) * width + x) * 4u;
            Rgb color = height_to_color(static_cast<float>(p[0]) / 255.0f);

            // Виньетка: плавное затемнение к краям кадра.
            const float u = (static_cast<float>(x) + 0.5f) / static_cast<float>(width) - 0.5f;
            const float v = (static_cast<float>(y) + 0.5f) / static_cast<float>(height) - 0.5f;
            const float falloff = std::clamp(1.0f - 1.6f * (u * u + v * v), 0.0f, 1.0f);
            const float shade = 0.25f + 0.75f * falloff * falloff;

            p[0] = to_byte(color.r * shade);
            p[1] = to_byte(color.g * shade);
            p[2] = to_byte(color.b * shade);
        }
    }
}

static void on_unload(void) {
    if (g_engine != nullptr) {
        char line[96];
        std::snprintf(line, sizeof(line), "vignette plugin: bye, simulated %.3f s", g_time);
        g_engine->log(2, line);
    }
    g_engine = nullptr;
}

static const EnginePlugin PLUGIN = {
    ENGINE_ABI_VERSION,
    "piska-vignette",
    on_load,
    on_fixed_update,
    process_rgba,
    on_unload,
    nullptr,  // components: у этого плагина игровых компонентов нет
    0,
};

// Точка входа — единственный экспортируемый символ библиотеки.
PLUGIN_EXPORT const EnginePlugin* engine_plugin_entry(void) { return &PLUGIN; }

}  // extern "C"
