//! Значок окна: по умолчанию — значок движка, зашитый в бинарник; игра может подставить
//! свой PNG ([`WindowConfig::with_icon_png`](crate::WindowConfig::with_icon_png)).
//!
//! winit умеет значок окна на Windows и X11. На macOS окно значка не имеет (его берёт
//! бандл приложения), на Wayland, в браузере и на Android winit значок игнорирует —
//! там (кроме Wayland, где вызов безвреден) декодирование пропускается.

use winit::window::WindowAttributes;

/// Значок движка, PNG 256 px (`assets/branding/icon.png`, рисует `tools/gen_icon.py`).
pub const ENGINE_ICON_PNG: &[u8] = include_bytes!("../../../assets/branding/icon.png");
/// Тот же значок, PNG 32 px с подогнанными под пиксели краями — для заголовка окна.
pub const ENGINE_ICON_SMALL_PNG: &[u8] = include_bytes!("../../../assets/branding/icon-32.png");

/// Расшифровать PNG в RGBA8: `(пиксели, ширина, высота)`.
pub fn decode_icon(png: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((image.into_raw(), width, height))
}

/// Вернуть `attributes` со значком окна. `custom` — PNG игры (`None` — значок движка).
/// Нечитаемый PNG не мешает запуску: пишется предупреждение и берётся значок движка.
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "macos")))]
pub fn with_window_icon(attributes: WindowAttributes, custom: Option<&[u8]>) -> WindowAttributes {
    use winit::window::Icon;

    // Два размера: маленький (заголовок, 16–32 px) и большой (панель задач, Alt-Tab).
    let (small, big) = match custom {
        Some(png) => match custom_pair(png) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("the window icon is not a readable PNG ({e}); using the engine icon");
                engine_pair()
            }
        },
        None => engine_pair(),
    };
    let icon = |(rgba, w, h): (Vec<u8>, u32, u32)| Icon::from_rgba(rgba, w, h).ok();
    let attributes = attributes.with_window_icon(icon(small));
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attributes.with_taskbar_icon(icon(big))
    };
    #[cfg(not(windows))]
    let _ = big;
    attributes
}

/// На платформах без значка окна — `attributes` как есть.
#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "macos"))]
pub fn with_window_icon(attributes: WindowAttributes, _custom: Option<&[u8]>) -> WindowAttributes {
    attributes
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "macos")))]
type Rgba = (Vec<u8>, u32, u32);

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "macos")))]
fn engine_pair() -> (Rgba, Rgba) {
    // Зашитые PNG проверяет тест; на случай порчи — пустой значок не создаётся.
    let small = decode_icon(ENGINE_ICON_SMALL_PNG).unwrap_or_else(|_| (vec![0; 4], 1, 1));
    let big = decode_icon(ENGINE_ICON_PNG).unwrap_or_else(|_| (vec![0; 4], 1, 1));
    (small, big)
}

/// Свой PNG игры → 32 px и не больше 256 px (Lanczos), чтобы не отдавать ОС огромные битмапы.
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "macos")))]
fn custom_pair(png: &[u8]) -> Result<(Rgba, Rgba), String> {
    use image::imageops::{resize, FilterType};
    let (rgba, w, h) = decode_icon(png)?;
    let source = image::RgbaImage::from_raw(w, h, rgba).ok_or("bad pixel buffer")?;
    let scaled = |size: u32| -> Rgba {
        let (fw, fh) = if w >= h {
            (size, (size * h / w).max(1))
        } else {
            ((size * w / h).max(1), size)
        };
        let out = resize(&source, fw, fh, FilterType::Lanczos3);
        let (ow, oh) = out.dimensions();
        (out.into_raw(), ow, oh)
    };
    Ok((scaled(32), scaled(w.max(h).min(256))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_icons_decode_to_the_expected_sizes() {
        let (rgba, w, h) = decode_icon(ENGINE_ICON_PNG).unwrap();
        assert_eq!((w, h), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
        // Скруглённый угол прозрачен, середина плитки — нет.
        assert_eq!(rgba[3], 0);
        assert_eq!(rgba[(128 * 256 + 20) * 4 + 3], 255);
        let (_, w, h) = decode_icon(ENGINE_ICON_SMALL_PNG).unwrap();
        assert_eq!((w, h), (32, 32));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_icon(b"not a png").is_err());
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "macos")))]
    #[test]
    fn window_attributes_get_an_icon() {
        let attributes = with_window_icon(WindowAttributes::default(), None);
        assert!(attributes.window_icon.is_some());
        // Свой PNG и мусор: оба дают значок (мусор — запасной, движка).
        let custom = with_window_icon(WindowAttributes::default(), Some(ENGINE_ICON_PNG));
        assert!(custom.window_icon.is_some());
        let broken = with_window_icon(WindowAttributes::default(), Some(b"junk"));
        assert!(broken.window_icon.is_some());
    }
}
