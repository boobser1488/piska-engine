//! Палитра набора витрины: одна текстура 256 × 256 из квадратов 16 × 16 по одному цвету.
//! Каждая модель берёт цвет грани из этой картинки (UV — центр квадрата), поэтому все
//! модели делят один материал и одну группу привязок.

/// Сторона квадрата цвета, пикселей.
pub const SWATCH: u32 = 16;
/// Квадратов в строке (и строк).
pub const GRID: u32 = 16;

macro_rules! palette {
    ($($name:ident = $hex:literal,)*) => {
        /// Цвет палитры (номер квадрата — порядок в списке).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        #[allow(dead_code)] // не каждый цвет палитры уже занят моделью
        pub enum C { $($name,)* }
        /// Имя и sRGB-цвет каждого квадрата.
        pub const COLORS: &[(&str, u32)] = &[$((stringify!($name), $hex),)*];
    };
}

palette! {
    // зелень
    GrassLight = 0x9cc65a, Grass = 0x76ad4c, GrassDark = 0x4f8a3c, Leaf = 0x5e9b45,
    LeafDark = 0x3d7038, Pine = 0x2f6e4f, PineDark = 0x244f3e, Snow = 0xe9f0f2,
    Olive = 0x8a9a48, Moss = 0x6d8f45,
    // осень и цветение
    Autumn = 0xe0893a, AutumnRed = 0xc4553d, AutumnGold = 0xe9bf4f, Blossom = 0xf0a4b5,
    // кора
    Bark = 0x7a5235, BarkDark = 0x553826, Birch = 0xebe5d3, BirchMark = 0x3a3430,
    PalmBark = 0xa98a5f, PalmLeaf = 0x6fb04a,
    // камень
    Rock = 0x8e8a83, RockDark = 0x67645f, RockLight = 0xb6b0a4, Cliff = 0x9b8e7c,
    CliffDark = 0x766a5b,
    // земля
    Sand = 0xe8d39e, SandWet = 0xc9ad7a, Dirt = 0x8a6a4b, DirtDark = 0x5f4835,
    Path = 0xb89670, Seabed = 0x5f8c84,
    // цветы и грибы
    FlowerWhite = 0xf6f2e6, FlowerPurple = 0x9a72d0, FlowerYellow = 0xf2cf4a,
    FlowerBlue = 0x6593e2, FlowerPink = 0xe87aa0, MushRed = 0xd2453b, MushSpot = 0xf6efdf,
    MushStem = 0xeee2c9, MushBrown = 0xa8744a, MushGlow = 0x7ff0c8,
    // облака
    Cloud = 0xf8f6f2, CloudShade = 0xdde2ea,
    // постройки
    Plaster = 0xf0e3c8, PlasterWarm = 0xe9c9a0, Timber = 0x6a4a33, RoofRed = 0xb9503a,
    RoofRedDark = 0x8f3a2c, RoofSlate = 0x55627a, RoofThatch = 0xcfa85e, RoofTeal = 0x3f8088,
    Stone = 0xa79f92, StoneDark = 0x7d766c,
    // дерево
    Wood = 0xa8784b, WoodLight = 0xcc9d66, WoodDark = 0x6d4c32, Plank = 0x8f8574,
    Rope = 0xc8b07a,
    // светящееся (материал glow)
    Window = 0xffc764, Lamp = 0xffb04a, Ember = 0xff6a28, Flame = 0xffd27a,
    Crystal = 0x6fe6ff, CrystalViolet = 0xb68cff, Screen = 0x63f0d8, Eye = 0xffe08a,
    // ткань
    ClothRed = 0xc94a3d, ClothWhite = 0xf1ece2, ClothBlue = 0x3f6fa8, ClothYellow = 0xe8b74a,
    ClothGreen = 0x5b8c55, Canvas = 0xd9c9a3, Sack = 0xc2a878,
    // металл и краска
    Iron = 0x4b4f56, Steel = 0x9ba4ae, Brass = 0xcaa24b, Copper = 0xc47a4a,
    RedPaint = 0xc23b33, YellowPaint = 0xf0b43c, Teal = 0x2f8f8a,
    // маяк
    LightRed = 0xc63d36, LightWhite = 0xf3f0e9,
    // звери
    Fox = 0xe2792f, FoxLight = 0xf4ede0, FoxDark = 0x3b2c26, Gull = 0xf4f4f0,
    GullGrey = 0x8b919a, Beak = 0xf0a830, Owl = 0x9a7a5a, OwlLight = 0xd9c3a0,
    Black = 0x202125, White = 0xf6f6f2,
    // мастерская
    TableTop = 0x9c6b43, Floor = 0xb98b5c, FloorDark = 0x9a7049, WallBlue = 0x5b7896,
    WallBlueDark = 0x48627c, Plinth = 0x2e333b, PlinthEdge = 0x3b414b, Pot = 0xb5603f,
    Paper = 0xeee8d6, Book1 = 0x8e3b3b, Book2 = 0x3b5e8e, Book3 = 0x4f7a43,
    // рельеф острова и поляны
    Shore = 0xd9c48c, Meadow = 0x86b552, MeadowDark = 0x6a9a44, Forest = 0x3f6e3a,
    ForestDark = 0x2f5631,
}

impl C {
    /// Центр квадрата в UV картинки (v вниз, как в текстуре).
    pub fn uv(self) -> (f32, f32) {
        let i = self as u32;
        (
            ((i % GRID) as f32 + 0.5) / GRID as f32,
            ((i / GRID) as f32 + 0.5) / GRID as f32,
        )
    }

    /// sRGB-цвет (0..255).
    #[cfg(test)]
    pub fn rgb(self) -> [u8; 3] {
        let hex = COLORS[self as usize].1;
        [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
    }
}

/// Картинка палитры (RGBA8, 256 × 256): квадраты цветов, пустые — нейтральный серый.
pub fn image() -> image::RgbaImage {
    let side = SWATCH * GRID;
    image::RgbaImage::from_fn(side, side, |x, y| {
        let index = (y / SWATCH * GRID + x / SWATCH) as usize;
        match COLORS.get(index) {
            Some(&(_, hex)) => image::Rgba([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255]),
            None => image::Rgba([128, 128, 128, 255]),
        }
    })
}

/// Палитра в PNG (одинаковые байты при каждом запуске).
pub fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image())
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encoding a PNG into memory cannot fail");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_fits_the_texture() {
        assert!(COLORS.len() <= (GRID * GRID) as usize);
        let image = image();
        let (u, v) = C::Fox.uv();
        let px = image.get_pixel((u * 256.0) as u32, (v * 256.0) as u32);
        assert_eq!([px[0], px[1], px[2]], C::Fox.rgb());
    }
}
