/* C-интерфейс C++-модуля шума (cpp/noise.cpp).
 *
 * Это «C++ → Rust» половина границы: Rust-объявления в src/ffi.rs повторяют
 * этот файл, безопасные обёртки — в src/noise.rs.
 *
 * Контракт всех функций: не бросают исключений, не сохраняют переданные
 * указатели, потокобезопасны (нет глобального состояния).
 */
#ifndef ENGINE_NOISE_H
#define ENGINE_NOISE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Фрактальный value-шум (fBm) в точке (x, y). Возвращает значение в [0, 1].
 * octaves: 1..16. */
float eng_noise_fbm2(float x, float y, uint32_t octaves, uint32_t seed);

/* Заполняет RGBA8-буфер картой шума (оттенки серого, альфа = 255).
 * out   — буфер на len байт; требуется len == width * height * 4.
 * scale — размер «пятен» в пикселях, > 0.
 * Возвращает 0 при успехе, 1 — неверные аргументы, 2 — внутренняя ошибка. */
int32_t eng_noise_fill_rgba(uint8_t* out, size_t len, uint32_t width,
                            uint32_t height, float scale, uint32_t seed);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* ENGINE_NOISE_H */
