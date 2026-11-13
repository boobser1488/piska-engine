//! ╔══════════════════════════════════════════════════════════════════════╗
//! ║ ГРАНИЦА FFI: сырые объявления функций из `cpp/noise.cpp`.            ║
//! ║ Модуль приватный — наружу выходят только безопасные обёртки          ║
//! ║ из `noise.rs`. Контракт каждой функции — в `include/engine_noise.h`. ║
//! ╚══════════════════════════════════════════════════════════════════════╝
//!
//! Объявления написаны вручную и повторяют `engine_noise.h`. Когда C++-API
//! вырастет, этот файл будет генерировать `bindgen` из того же заголовка
//! (ему нужен libclang, поэтому в MVP он не обязателен).

extern "C" {
    /// `float eng_noise_fbm2(float x, float y, uint32_t octaves, uint32_t seed);`
    pub fn eng_noise_fbm2(x: f32, y: f32, octaves: u32, seed: u32) -> f32;

    /// `int32_t eng_noise_fill_rgba(uint8_t* out, size_t len, uint32_t width,
    ///                              uint32_t height, float scale, uint32_t seed);`
    pub fn eng_noise_fill_rgba(
        out: *mut u8,
        len: usize,
        width: u32,
        height: u32,
        scale: f32,
        seed: u32,
    ) -> i32;
}

extern "C" {
    /// `const EnginePlugin* engine_builtin_gameplay_entry(void);` — точка входа
    /// `plugins/cpp-gameplay/gameplay.cpp`, собранного статически (см. `build.rs`).
    pub fn engine_builtin_gameplay_entry() -> *const crate::abi::EnginePlugin;
}
