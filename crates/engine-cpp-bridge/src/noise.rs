//! Безопасные обёртки над C++-модулем шума (`cpp/noise.cpp`).
//!
//! Пример того, как выглядит *любой* C++-модуль в движке: C++ считает, Rust
//! проверяет инварианты на входе и превращает коды возврата в `Result`.

#[cfg(engine_static_cpp)]
use crate::ffi;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NoiseError {
    #[error(
        "buffer has {actual} bytes, but a {width}x{height} RGBA image needs {expected}. \
         Allocate `vec![0u8; width * height * 4]`"
    )]
    BufferSize {
        actual: usize,
        expected: usize,
        width: u32,
        height: u32,
    },
    #[error("C++ noise module failed with code {0}")]
    Native(i32),
}

/// Фрактальный шум (fBm) в точке. Результат в диапазоне 0..=1.
#[cfg(engine_static_cpp)]
pub fn fbm2(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    // SAFETY: функция чистая, принимает только значения (без указателей),
    // помечена `noexcept` и не имеет предусловий — см. engine_noise.h.
    unsafe { ffi::eng_noise_fbm2(x, y, octaves.clamp(1, 16), seed) }
}

/// Заполнить RGBA8-буфер картой шума в оттенках серого.
/// `scale` — размер «пятен» в пикселях.
#[cfg(engine_static_cpp)]
pub fn fill_rgba(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    seed: u32,
) -> Result<(), NoiseError> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() != expected {
        return Err(NoiseError::BufferSize {
            actual: pixels.len(),
            expected,
            width,
            height,
        });
    }
    // SAFETY: `pixels` — валидный эксклюзивный срез ровно на `len` байт, и мы
    // только что проверили, что `len == width * height * 4`. C++ пишет не более
    // `len` байт (проверяет сам и возвращает код ошибки) и не сохраняет указатель.
    let code = unsafe {
        ffi::eng_noise_fill_rgba(
            pixels.as_mut_ptr(),
            pixels.len(),
            width,
            height,
            scale.max(1e-3),
            seed,
        )
    };
    match code {
        0 => Ok(()),
        other => Err(NoiseError::Native(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fbm_is_deterministic_and_normalized() {
        let a = fbm2(1.5, 2.5, 5, 42);
        assert_eq!(a, fbm2(1.5, 2.5, 5, 42));
        assert_ne!(a, fbm2(1.5, 2.5, 5, 43));
        for i in 0..200 {
            let v = fbm2(i as f32 * 0.37, i as f32 * 0.11, 6, 7);
            assert!((0.0..=1.0).contains(&v), "fbm out of range: {v}");
        }
    }

    #[test]
    fn fill_rgba_writes_opaque_grayscale() {
        let mut pixels = vec![0u8; 16 * 8 * 4];
        fill_rgba(&mut pixels, 16, 8, 4.0, 1).unwrap();
        assert!(pixels.chunks(4).all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255));
        assert!(pixels.chunks(4).any(|p| p[0] != pixels[0]), "noise must not be flat");
    }

    #[test]
    fn wrong_buffer_size_is_rejected_before_crossing_the_boundary() {
        let mut pixels = vec![0u8; 10];
        let err = fill_rgba(&mut pixels, 16, 8, 4.0, 1).unwrap_err();
        assert_eq!(
            err,
            NoiseError::BufferSize { actual: 10, expected: 512, width: 16, height: 8 }
        );
    }
}
