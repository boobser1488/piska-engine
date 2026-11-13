//! Значок приложения (Android, Linux, веб, Windows): свой значок игры или значок движка.
//!
//! Свой значок — PNG: `icon = "sprites/icon.png"` в `[package.metadata.piska]` или файл
//! `assets/icon.png`. Без него берётся значок движка (`assets/branding/`, рисует
//! `tools/gen_icon.py`).

use image::{imageops::FilterType, ImageFormat, RgbaImage};

use crate::project::Project;
use crate::{Error, Result};

/// Значок движка: крупный (масштабируется вниз) и маленький с подогнанными под пиксели краями.
const ENGINE_ICON_512: &[u8] = include_bytes!("../../../assets/branding/icon-512.png");
const ENGINE_ICON_32: &[u8] = include_bytes!("../../../assets/branding/icon-32.png");

/// PNG значка размером `size`×`size`: свой значок игры (масштабируется) или значок движка.
pub fn app_icon(project: &Project, size: u32) -> Result<Vec<u8>> {
    let image = match project.icon_png()? {
        Some(bytes) => image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .map_err(|e| Error(format!("the game icon is not a readable PNG: {e}")))?
            .resize_exact(size, size, FilterType::Lanczos3)
            .to_rgba8(),
        None => default_icon(size),
    };
    encode(&image)
}

pub fn encode(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error(format!("cannot encode the icon: {e}")))?;
    Ok(out.into_inner())
}

/// Значок движка нужного размера.
pub fn default_icon(size: u32) -> RgbaImage {
    let (bytes, native) = if size <= 32 { (ENGINE_ICON_32, 32) } else { (ENGINE_ICON_512, 512) };
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .expect("the embedded engine icon is a valid PNG")
        .to_rgba8();
    if size == native {
        image
    } else {
        image::imageops::resize(&image, size, size, FilterType::Lanczos3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_is_the_engine_mark() {
        let icon = default_icon(192);
        assert_eq!(icon.dimensions(), (192, 192));
        assert_eq!(icon.get_pixel(0, 0)[3], 0, "rounded corner is transparent");
        let tile = icon.get_pixel(96, 12);
        assert_eq!(tile[3], 255, "inside is opaque");
        assert!(tile[2] > tile[0], "the tile is blue: {tile:?}");
        let letter = icon.get_pixel(70, 110);
        assert!(letter[0] > 240 && letter[1] > 240 && letter[2] > 240, "the P stem is white: {letter:?}");
        let png = encode(&icon).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    #[test]
    fn small_sizes_use_the_hand_tuned_image() {
        assert_eq!(default_icon(32), default_icon(32));
        assert_eq!(default_icon(16).dimensions(), (16, 16));
        assert_eq!(default_icon(512).dimensions(), (512, 512));
    }
}
