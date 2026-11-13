//! Текст: шрифты, динамический атлас глифов, перенос строк и раскладка.
//!
//! Глифы растеризуются под итоговый размер в пикселях (кегль × масштаб холста) и
//! кладутся в общий RGBA-атлас (белый цвет, альфа = покрытие). В том же атласе лежит
//! белый квадрат для сплошных фигур — поэтому панели и текст рисуются одной текстурой
//! и обычно одним вызовом. Раскладка текста кэшируется и пересчитывается только при
//! изменении строки или параметров.

use std::collections::HashMap;

use ab_glyph::{Font as _, FontArc, GlyphId, PxScale, ScaleFont};
use engine_assets::{AssetDatabase, AssetHandle, Font, Guid};
use engine_core::math::Vec2;
use engine_render::OverlayImage;

use crate::components::{TextAlign, VerticalAlign};

/// Номер встроенного шрифта (Ubuntu Light).
pub(crate) const DEFAULT_FONT: usize = 0;
/// Запасной встроенный шрифт (Hack): символы, которых нет в основном.
const FALLBACK_FONT: usize = 1;
/// Id атласа в [`Overlay::images`](engine_render::Overlay::images).
pub(crate) const ATLAS_ID: u32 = 0;
const ATLAS_START: u32 = 512;
const ATLAS_MAX: u32 = 4096;
/// Белый квадрат в углу атласа (для сплошных фигур).
const WHITE_SIZE: u32 = 4;
/// Сколько субпиксельных сдвигов по X храним для каждого глифа.
const SUBPIXEL_STEPS: f32 = 3.0;

// ----------------------------------------------------------------------
// Шрифты
// ----------------------------------------------------------------------

pub(crate) struct FontLibrary {
    fonts: Vec<FontArc>,
    by_guid: HashMap<Guid, usize>,
}

impl FontLibrary {
    pub fn new() -> Self {
        let builtin = |bytes: &'static [u8]| {
            FontArc::try_from_slice(bytes).expect("built-in fonts from epaint_default_fonts are valid")
        };
        Self {
            fonts: vec![
                builtin(epaint_default_fonts::UBUNTU_LIGHT),
                builtin(epaint_default_fonts::HACK_REGULAR),
            ],
            by_guid: HashMap::new(),
        }
    }

    /// Номер шрифта для ссылки из компонента. Ненайденный шрифт — один раз в лог,
    /// дальше встроенный.
    pub fn resolve(&mut self, handle: AssetHandle<Font>, assets: Option<&AssetDatabase>) -> usize {
        if handle.is_none() {
            return DEFAULT_FONT;
        }
        let guid = handle.guid();
        if let Some(&index) = self.by_guid.get(&guid) {
            return index;
        }
        let loaded = (|| {
            let assets = assets.ok_or_else(|| {
                "no AssetDatabase in world.resources; mount the assets directory first".to_string()
            })?;
            // Байты — из базы ассетов: так шрифт читается и из каталога, и из пака.
            let name = assets
                .relative_path_of(guid)
                .ok_or_else(|| "no asset with this GUID — was the .meta file deleted?".to_string())?
                .to_string();
            let bytes = assets.read(guid).map_err(|e| format!("cannot read `{name}`: {e}"))?;
            FontArc::try_from_vec(bytes.into_owned())
                .map_err(|_| format!("`{name}` is not a TrueType/OpenType font"))
        })();
        let index = match loaded {
            Ok(font) => {
                self.fonts.push(font);
                self.fonts.len() - 1
            }
            Err(reason) => {
                log::error!("font {guid}: {reason}; using the built-in font");
                DEFAULT_FONT
            }
        };
        self.by_guid.insert(guid, index);
        index
    }

    /// Глиф символа: из шрифта `font`, а если его там нет — из встроенных.
    fn glyph(&self, font: usize, c: char) -> (usize, GlyphId) {
        let id = self.fonts[font].glyph_id(c);
        if id.0 != 0 || c.is_control() {
            return (font, id);
        }
        for fallback in [DEFAULT_FONT, FALLBACK_FONT] {
            if fallback != font {
                let id = self.fonts[fallback].glyph_id(c);
                if id.0 != 0 {
                    return (fallback, id);
                }
            }
        }
        (font, id)
    }

    /// Масштаб ab_glyph для кегля `size_px` (em, как font-size в CSS).
    fn scale(&self, font: usize, size_px: f32) -> PxScale {
        let font = &self.fonts[font];
        let em = font.units_per_em().unwrap_or(1000.0);
        PxScale::from(size_px * font.height_unscaled() / em)
    }
}

// ----------------------------------------------------------------------
// Атлас
// ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u16,
    glyph: u16,
    /// Кегль × 4 (четверть пикселя).
    size: u32,
    bold: bool,
    subpixel: u8,
}

/// Глиф в атласе.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GlyphEntry {
    /// Левый верхний угол в атласе (пиксели).
    pub origin: [u32; 2],
    pub size: [u32; 2],
    /// Смещение картинки от точки пера на базовой линии.
    pub offset: Vec2,
}

struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

/// Раскладчик атласа «полками». Сами пиксели живут в [`OverlayImage`] (её и рисует рендер).
pub(crate) struct Atlas {
    size: u32,
    shelves: Vec<Shelf>,
    next_y: u32,
    entries: Vec<GlyphEntry>,
    map: HashMap<GlyphKey, u32>,
    /// Растёт при полной очистке: кэши раскладки текста становятся недействительны.
    pub generation: u32,
    dirty: Option<[u32; 4]>,
    resized: bool,
    scratch: Vec<u8>,
}

impl Atlas {
    pub fn new() -> Self {
        Self {
            size: ATLAS_START,
            shelves: Vec::new(),
            next_y: WHITE_SIZE + 2,
            entries: Vec::new(),
            map: HashMap::new(),
            generation: 0,
            dirty: None,
            resized: true,
            scratch: Vec::new(),
        }
    }

    /// Подготовить картинку атласа (размер, белый квадрат).
    pub fn init_image(&mut self, image: &mut OverlayImage) {
        image.id = ATLAS_ID;
        if image.width != self.size || image.rgba.len() != (self.size * self.size * 4) as usize {
            image.width = self.size;
            image.height = self.size;
            image.rgba = vec![0; (self.size * self.size * 4) as usize];
            self.paint_white(image);
            self.resized = true;
        }
    }

    fn paint_white(&self, image: &mut OverlayImage) {
        for y in 0..WHITE_SIZE {
            for x in 0..WHITE_SIZE {
                let i = ((y * self.size + x) * 4) as usize;
                image.rgba[i..i + 4].copy_from_slice(&[255; 4]);
            }
        }
    }

    /// Текстурные координаты середины белого квадрата.
    pub fn white_uv(&self) -> [f32; 2] {
        let c = WHITE_SIZE as f32 * 0.5 / self.size as f32;
        [c, c]
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn entry(&self, index: u32) -> &GlyphEntry {
        &self.entries[index as usize]
    }

    /// Конец кадра: сообщить рендеру, что атлас изменился.
    pub fn publish(&mut self, image: &mut OverlayImage) {
        if self.resized {
            image.version += 1;
            image.dirty = None;
        } else if let Some(rect) = self.dirty {
            image.version += 1;
            image.dirty = Some(rect);
        }
        self.resized = false;
        self.dirty = None;
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.dirty = Some(match self.dirty {
            None => [x, y, w, h],
            Some([dx, dy, dw, dh]) => {
                let x0 = dx.min(x);
                let y0 = dy.min(y);
                let x1 = (dx + dw).max(x + w);
                let y1 = (dy + dh).max(y + h);
                [x0, y0, x1 - x0, y1 - y0]
            }
        });
    }

    /// Найти место под картинку `w × h` (с полем в 1 пиксель).
    fn allocate(&mut self, w: u32, h: u32, image: &mut OverlayImage) -> Option<[u32; 2]> {
        let (pw, ph) = (w + 1, h + 1);
        loop {
            if let Some(shelf) = self
                .shelves
                .iter_mut()
                .find(|s| s.height >= ph && s.height <= ph + ph / 3 + 2 && s.x + pw <= self.size)
            {
                let at = [shelf.x, shelf.y];
                shelf.x += pw;
                return Some(at);
            }
            if self.next_y + ph <= self.size && pw <= self.size {
                self.shelves.push(Shelf {
                    y: self.next_y,
                    height: ph,
                    x: pw,
                });
                let at = [0, self.next_y];
                self.next_y += ph;
                return Some(at);
            }
            if self.size >= ATLAS_MAX || pw > ATLAS_MAX {
                return None;
            }
            self.grow(image);
        }
    }

    /// Удвоить сторону атласа, сохранив уже лежащие глифы.
    fn grow(&mut self, image: &mut OverlayImage) {
        let old = self.size;
        let new = old * 2;
        let mut pixels = vec![0u8; (new * new * 4) as usize];
        for y in 0..old {
            let src = (y * old * 4) as usize;
            let dst = (y * new * 4) as usize;
            pixels[dst..dst + (old * 4) as usize].copy_from_slice(&image.rgba[src..src + (old * 4) as usize]);
        }
        self.size = new;
        image.width = new;
        image.height = new;
        image.rgba = pixels;
        self.resized = true;
    }

    /// Всё очистить: атлас переполнен глифами давно не нужных размеров.
    fn reset(&mut self, image: &mut OverlayImage) {
        self.shelves.clear();
        self.entries.clear();
        self.map.clear();
        self.next_y = WHITE_SIZE + 2;
        self.generation += 1;
        image.rgba.fill(0);
        self.paint_white(image);
        self.resized = true;
        log::debug!("ui glyph atlas is full; cleared it");
    }

    /// Номер глифа в атласе; растеризует его при первом запросе.
    fn glyph(
        &mut self,
        fonts: &FontLibrary,
        key: GlyphKey,
        image: &mut OverlayImage,
    ) -> u32 {
        if let Some(&index) = self.map.get(&key) {
            return index;
        }
        let font = &fonts.fonts[key.font as usize];
        let size_px = key.size as f32 / 4.0;
        let scale = fonts.scale(key.font as usize, size_px);
        let glyph = GlyphId(key.glyph).with_scale_and_position(
            scale,
            ab_glyph::point(key.subpixel as f32 / SUBPIXEL_STEPS, 0.0),
        );
        let mut entry = GlyphEntry::default();
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let w = bounds.width() as u32;
            let h = bounds.height() as u32;
            let bold = if key.bold { bold_extra(size_px) } else { 0.0 };
            let extra = bold.ceil() as u32;
            let total_w = w + extra;
            if total_w > 0 && h > 0 {
                self.scratch.clear();
                self.scratch.resize((total_w * h) as usize, 0);
                let scratch = &mut self.scratch;
                outline.draw(|x, y, coverage| {
                    let i = (y * total_w + x) as usize;
                    if let Some(px) = scratch.get_mut(i) {
                        *px = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                });
                if extra > 0 {
                    embolden(&mut self.scratch, total_w, h, bold);
                }
                let at = match self.allocate(total_w, h, image) {
                    Some(at) => at,
                    None => {
                        self.reset(image);
                        self.allocate(total_w, h, image).unwrap_or_default()
                    }
                };
                for row in 0..h {
                    for col in 0..total_w {
                        let a = self.scratch[(row * total_w + col) as usize];
                        let i = (((at[1] + row) * self.size + at[0] + col) * 4) as usize;
                        image.rgba[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
                    }
                }
                self.mark_dirty(at[0], at[1], total_w, h);
                entry = GlyphEntry {
                    origin: at,
                    size: [total_w, h],
                    offset: Vec2::new(bounds.min.x - key.subpixel as f32 / SUBPIXEL_STEPS, bounds.min.y),
                };
            }
        }
        let index = self.entries.len() as u32;
        self.entries.push(entry);
        self.map.insert(key, index);
        index
    }
}

/// На сколько пикселей «толстеет» полужирный глиф.
fn bold_extra(size_px: f32) -> f32 {
    (size_px / 20.0).clamp(0.6, 2.5)
}

/// Синтетический полужирный: растянуть покрытие вправо на `amount` пикселей.
fn embolden(pixels: &mut [u8], width: u32, height: u32, amount: f32) {
    let steps = amount.ceil() as u32;
    for row in 0..height as usize {
        let line = &mut pixels[row * width as usize..(row + 1) * width as usize];
        for x in (0..width as usize).rev() {
            let mut best = line[x] as f32;
            for k in 1..=steps as usize {
                if x >= k {
                    let weight = (amount - (k - 1) as f32).clamp(0.0, 1.0);
                    best = best.max(line[x - k] as f32 * weight);
                }
            }
            line[x] = best.round() as u8;
        }
    }
}

// ----------------------------------------------------------------------
// Перенос строк
// ----------------------------------------------------------------------

/// Разбить текст на строки (диапазоны байтов). `max_width = None` — переносы только
/// по `\n`. Слово длиннее строки режется посимвольно. Пробелы в конце строки остаются
/// в диапазоне (при выравнивании они не учитываются).
pub(crate) fn wrap_lines(
    text: &str,
    max_width: Option<f32>,
    mut advance: impl FnMut(char) -> f32,
    out: &mut Vec<(usize, usize)>,
) {
    out.clear();
    let mut paragraph_start = 0;
    for paragraph in text.split('\n') {
        let base = paragraph_start;
        paragraph_start += paragraph.len() + 1;
        let mut line_start = 0;
        let mut x = 0.0;
        let mut last_break: Option<(usize, f32)> = None;
        for (i, c) in paragraph.char_indices() {
            let w = advance(c);
            if c == ' ' {
                x += w;
                last_break = Some((i + 1, x));
                continue;
            }
            if let Some(max) = max_width {
                if x + w > max + 0.01 && i > line_start {
                    match last_break {
                        Some((at, at_x)) if at > line_start => {
                            out.push((base + line_start, base + at));
                            x -= at_x;
                            line_start = at;
                        }
                        _ => {
                            out.push((base + line_start, base + i));
                            x = 0.0;
                            line_start = i;
                        }
                    }
                    last_break = None;
                }
            }
            x += w;
        }
        out.push((base + line_start, base + paragraph.len()));
    }
}

// ----------------------------------------------------------------------
// Раскладка
// ----------------------------------------------------------------------

/// Всё, от чего зависит раскладка текста (кроме самой строки).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextParams {
    pub font: usize,
    /// Кегль в пикселях.
    pub size_px: f32,
    pub bold: bool,
    pub align: TextAlign,
    pub vertical_align: VerticalAlign,
    pub wrap: bool,
    pub line_spacing: f32,
    /// Размер прямоугольника в пикселях.
    pub box_size: Vec2,
}

/// Глиф, поставленный на место: картинка из атласа и её левый верхний угол
/// относительно левого верхнего угла прямоугольника текста.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedGlyph {
    pub entry: u32,
    pub position: Vec2,
}

/// Кэш раскладки одного текста.
#[derive(Default)]
pub(crate) struct TextLayout {
    text: String,
    params: Option<TextParams>,
    generation: u32,
    pub glyphs: Vec<PlacedGlyph>,
    /// X каждой границы символа первой строки (для каретки поля ввода): `chars + 1` значений.
    pub carets: Vec<f32>,
    /// Размер занятого текстом блока.
    pub size: Vec2,
    /// Верх и высота первой строки (для каретки).
    pub line_top: f32,
    pub line_height: f32,
    /// Последний кадр, когда текст рисовался (для чистки кэша).
    pub seen: u64,
}

/// Общий движок текста: шрифты + атлас + рабочие буферы.
pub(crate) struct TextEngine {
    pub fonts: FontLibrary,
    pub atlas: Atlas,
    lines: Vec<(usize, usize)>,
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            fonts: FontLibrary::new(),
            atlas: Atlas::new(),
            lines: Vec::new(),
        }
    }

    /// Обновить раскладку, если строка, параметры или атлас поменялись.
    pub fn layout(&mut self, cache: &mut TextLayout, text: &str, params: TextParams, image: &mut OverlayImage) {
        if cache.params == Some(params) && cache.generation == self.atlas.generation && cache.text == text {
            return;
        }
        // Атлас мог очиститься посреди раскладки: тогда раскладываем заново.
        for _ in 0..2 {
            let generation = self.atlas.generation;
            self.build(cache, text, params, image);
            if generation == self.atlas.generation {
                break;
            }
        }
        cache.text.clear();
        cache.text.push_str(text);
        cache.params = Some(params);
        cache.generation = self.atlas.generation;
    }

    fn build(&mut self, cache: &mut TextLayout, text: &str, params: TextParams, image: &mut OverlayImage) {
        cache.glyphs.clear();
        cache.carets.clear();
        let size_px = params.size_px.max(1.0);
        let size_q = (size_px * 4.0).round() as u32;
        let size_px = size_q as f32 / 4.0;
        let bold = if params.bold { bold_extra(size_px) } else { 0.0 };
        let fonts = &self.fonts;
        let primary = fonts.fonts[params.font].as_scaled(fonts.scale(params.font, size_px));
        let ascent = primary.ascent();
        let descent = primary.descent();
        let line_height = ((ascent - descent + primary.line_gap()) * params.line_spacing.max(0.1)).round();

        let advance = |c: char| {
            let (font, id) = fonts.glyph(params.font, c);
            fonts.fonts[font].as_scaled(fonts.scale(font, size_px)).h_advance(id) + bold
        };
        let max_width = (params.wrap && params.box_size.x > 0.0).then_some(params.box_size.x);
        wrap_lines(text, max_width, advance, &mut self.lines);

        let line_count = self.lines.len().max(1);
        let block_height = line_height * (line_count - 1) as f32 + (ascent - descent);
        let valign = match params.vertical_align {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Middle => 0.5,
            VerticalAlign::Bottom => 1.0,
        };
        let halign = match params.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
        };
        let top = ((params.box_size.y - block_height) * valign).round();
        let mut widest: f32 = 0.0;

        for (line_index, &(start, end)) in self.lines.iter().enumerate() {
            let line = &text[start..end];
            // Ширина строки без хвостовых пробелов — для выравнивания.
            let mut x = 0.0;
            let mut width = 0.0;
            let mut prev: Option<(usize, GlyphId)> = None;
            for c in line.chars() {
                let (font, id) = fonts.glyph(params.font, c);
                let scaled = fonts.fonts[font].as_scaled(fonts.scale(font, size_px));
                if let Some((pf, pid)) = prev {
                    if pf == font {
                        x += scaled.kern(pid, id);
                    }
                }
                x += scaled.h_advance(id) + bold;
                if c != ' ' {
                    width = x;
                }
                prev = Some((font, id));
            }
            widest = widest.max(width);
            let left = ((params.box_size.x - width) * halign).round();
            let baseline = top + ascent.round() + line_height * line_index as f32;

            let mut x = 0.0;
            let mut prev: Option<(usize, GlyphId)> = None;
            if line_index == 0 {
                cache.carets.push(left);
            }
            for c in line.chars() {
                let (font, id) = fonts.glyph(params.font, c);
                let scaled = fonts.fonts[font].as_scaled(fonts.scale(font, size_px));
                if let Some((pf, pid)) = prev {
                    if pf == font {
                        x += scaled.kern(pid, id);
                    }
                }
                let pen = left + x;
                let whole = pen.floor();
                let subpixel = (((pen - whole) * SUBPIXEL_STEPS) as u8).min(SUBPIXEL_STEPS as u8 - 1);
                if !c.is_whitespace() {
                    let key = GlyphKey {
                        font: font as u16,
                        glyph: id.0,
                        size: size_q,
                        bold: params.bold,
                        subpixel,
                    };
                    let entry = self.atlas.glyph(fonts, key, image);
                    let placed = self.atlas.entry(entry);
                    if placed.size[0] > 0 {
                        cache.glyphs.push(PlacedGlyph {
                            entry,
                            position: Vec2::new(whole + placed.offset.x.round(), baseline + placed.offset.y.round()),
                        });
                    }
                }
                x += scaled.h_advance(id) + bold;
                prev = Some((font, id));
                if line_index == 0 {
                    cache.carets.push(left + x);
                }
            }
        }
        if cache.carets.is_empty() {
            let left = (params.box_size.x * halign).round();
            cache.carets.push(left);
        }
        cache.size = Vec2::new(widest, block_height);
        cache.line_top = top;
        cache.line_height = (ascent - descent).round();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_asset_loads_from_a_pack() {
        use engine_assets::AssetPack;
        let guid = Guid(0xF0_17);
        let mut builder = AssetPack::builder();
        builder.add("fonts/hack.ttf", epaint_default_fonts::HACK_REGULAR.to_vec());
        builder.add(
            "fonts/hack.ttf.meta",
            format!("(
    guid: \"{:032x}\",
    importer: \"Font\",
    settings: {{}},
)
", guid.0).into_bytes(),
        );
        let database = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
        assert!(!database.path_of(guid).unwrap().exists(), "no file on disk: bytes come from the pack");
        let mut library = FontLibrary::new();
        let index = library.resolve(AssetHandle::from_guid(guid), Some(&database));
        assert_ne!(index, DEFAULT_FONT, "the packed font is loaded, not the built-in fallback");
        assert_eq!(library.resolve(AssetHandle::from_guid(guid), Some(&database)), index, "cached");
    }

    fn lines(text: &str, width: Option<f32>) -> Vec<&str> {
        let mut out = Vec::new();
        wrap_lines(text, width, |_| 1.0, &mut out);
        out.iter().map(|&(s, e)| &text[s..e]).collect()
    }

    #[test]
    fn wrap_breaks_at_spaces_newlines_and_inside_long_words() {
        assert_eq!(lines("hello world foo", Some(11.0)), ["hello world ", "foo"]);
        assert_eq!(lines("hello world foo", Some(8.0)), ["hello ", "world ", "foo"]);
        assert_eq!(lines("one\ntwo three", None), ["one", "two three"]);
        assert_eq!(lines("abcdefghij", Some(4.0)), ["abcd", "efgh", "ij"]);
        assert_eq!(lines("", Some(4.0)), [""]);
        assert_eq!(lines("a\n\nb", Some(4.0)), ["a", "", "b"]);
        // Кириллица: границы по байтам не рвут символы.
        assert_eq!(lines("Привет мир", Some(7.0)), ["Привет ", "мир"]);
    }

    #[test]
    fn builtin_font_covers_latin_and_cyrillic() {
        let fonts = FontLibrary::new();
        for c in "AZaz09Ёё ЖжЯяЩщ!?".chars().filter(|c| *c != ' ') {
            let (font, id) = fonts.glyph(DEFAULT_FONT, c);
            assert_eq!(font, DEFAULT_FONT, "{c}");
            assert_ne!(id.0, 0, "{c}");
        }
    }

    fn params(box_size: Vec2, align: TextAlign) -> TextParams {
        TextParams {
            font: DEFAULT_FONT,
            size_px: 20.0,
            bold: false,
            align,
            vertical_align: VerticalAlign::Top,
            wrap: true,
            line_spacing: 1.0,
            box_size,
        }
    }

    #[test]
    fn layout_wraps_aligns_and_is_cached() {
        let mut engine = TextEngine::new();
        let mut image = OverlayImage::default();
        engine.atlas.init_image(&mut image);
        let mut cache = TextLayout::default();
        let text = "Новая игра and more words to wrap";
        engine.layout(&mut cache, text, params(Vec2::new(120.0, 200.0), TextAlign::Left), &mut image);
        assert!(cache.size.x <= 120.0 + 0.5, "{:?}", cache.size);
        assert!(cache.size.y > 40.0, "wrapped onto several lines: {:?}", cache.size);
        let glyphs = cache.glyphs.len();
        assert_eq!(glyphs, text.chars().filter(|c| !c.is_whitespace()).count());
        assert!(cache.glyphs.iter().all(|g| g.position.x >= 0.0 && g.position.x < 120.0));

        // Тот же текст — раскладка не пересчитывается и атлас не трогается.
        engine.atlas.publish(&mut image);
        let version = image.version;
        engine.layout(&mut cache, text, params(Vec2::new(120.0, 200.0), TextAlign::Left), &mut image);
        engine.atlas.publish(&mut image);
        assert_eq!(image.version, version);

        // Выравнивание по правому краю сдвигает строку вправо.
        let mut right = TextLayout::default();
        engine.layout(&mut right, "Hi", params(Vec2::new(300.0, 50.0), TextAlign::Right), &mut image);
        let last = right.glyphs.last().unwrap();
        let entry = engine.atlas.entry(last.entry);
        assert!(last.position.x + entry.size[0] as f32 > 280.0);
        assert_eq!(right.carets.len(), 3);
    }

    #[test]
    fn glyph_pixels_land_in_the_atlas_and_bold_is_wider() {
        let mut engine = TextEngine::new();
        let mut image = OverlayImage::default();
        engine.atlas.init_image(&mut image);
        let mut regular = TextLayout::default();
        let mut bold = TextLayout::default();
        engine.layout(&mut regular, "Ж", params(Vec2::new(100.0, 40.0), TextAlign::Left), &mut image);
        engine.layout(
            &mut bold,
            "Ж",
            TextParams {
                bold: true,
                ..params(Vec2::new(100.0, 40.0), TextAlign::Left)
            },
            &mut image,
        );
        let r = *engine.atlas.entry(regular.glyphs[0].entry);
        let b = *engine.atlas.entry(bold.glyphs[0].entry);
        assert!(b.size[0] > r.size[0]);
        let covered = (0..r.size[1])
            .flat_map(|y| (0..r.size[0]).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let i = (((r.origin[1] + y) * engine.atlas.size() + r.origin[0] + x) * 4 + 3) as usize;
                image.rgba[i] > 128
            })
            .count();
        assert!(covered > 20, "{covered}");
    }

    #[test]
    fn wrap_handles_extreme_widths() {
        // Ни одна ширина не даёт пустых строк в середине и не теряет символов.
        let text = "The quick brown fox";
        for width in [0.5, 1.0, 3.0, 100.0] {
            let got = lines(text, Some(width));
            assert_eq!(got.concat(), text);
            assert!(got.iter().all(|l| !l.is_empty()));
        }
    }
}
