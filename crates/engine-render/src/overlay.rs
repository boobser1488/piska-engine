//! Экранный слой — шов между рендером и игровым интерфейсом.
//!
//! Рендер ничего не знает об интерфейсе: он берёт из `world.resources` готовый
//! список треугольников [`Overlay`] (его каждый кадр собирает `engine-ui` или любой
//! другой код) и рисует его поверх кадра — *после* пост-обработки (без блума и
//! тонмаппинга), в пикселях цели, с альфа-смешиванием. Подряд идущие треугольники с
//! одной текстурой и одной областью отсечения рисуются одним вызовом.
//!
//! Слой рисуется в окне игры, в виде Game редактора и в [`Renderer::capture`]; вид
//! Scene редактора его не показывает (см. [`CameraView::overlay`]).
//!
//! [`Renderer::capture`]: crate::Renderer::capture
//! [`CameraView::overlay`]: crate::CameraView::overlay

use std::ops::Range;

use engine_assets::Guid;
use engine_core::math::Vec2;

/// Вершина экранного слоя. Координаты — пиксели, (0, 0) — левый верхний угол.
///
/// Помимо текстуры и цвета вершина может описывать скруглённый прямоугольник с
/// рамкой: шейдер считает расстояние до его края (SDF) и сглаживает край сам, без
/// MSAA. Так одним и тем же конвейером рисуются панели, кнопки, тени и глифы текста.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayVertex {
    /// Позиция в пикселях списка (см. [`Overlay::size`]).
    pub position: [f32; 2],
    pub uv: [f32; 2],
    /// Цвет заливки (линейный, альфа не домножена); умножается на текстуру.
    pub color: [f32; 4],
    /// Цвет рамки фигуры.
    pub border_color: [f32; 4],
    /// Фигура: полуширина, полувысота, радиус скругления, толщина рамки (пиксели).
    /// Полуширина `<= 0` — фигуры нет, рисуется просто текстура × цвет.
    pub shape: [f32; 4],
    /// `[x, y]` — позиция вершины относительно центра фигуры (без поворота),
    /// `[2]` — мягкость края в пикселях (размытая тень), `[3]` — не используется.
    pub local: [f32; 4],
}

/// Какая текстура у отрезка треугольников.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayTexture {
    /// Картинка из [`Overlay::images`] с этим `id` (например, атлас шрифта).
    Image(u32),
    /// Текстура из базы ассетов (как у спрайта).
    Asset(Guid),
}

/// Отрезок списка индексов, который рисуется одним вызовом.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayBatch {
    pub texture: OverlayTexture,
    /// Диапазон в [`Overlay::indices`].
    pub indices: Range<u32>,
    /// Область отсечения `[x, y, ширина, высота]` в пикселях списка; `None` — без отсечения.
    pub clip: Option<[f32; 4]>,
}

/// Картинка, которую рисующий код создаёт сам (атлас глифов). Рендер загружает её
/// на GPU и обновляет, когда растёт `version`.
#[derive(Clone, Debug, Default)]
pub struct OverlayImage {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` байт, без гамма-коррекции.
    pub rgba: Vec<u8>,
    /// Растёт при каждом изменении пикселей или размера.
    pub version: u64,
    /// Что поменялось с версии `version - 1`: `[x, y, ширина, высота]`. `None` — всё.
    /// Рендер, пропустивший версии, загружает картинку целиком.
    pub dirty: Option<[u32; 4]>,
}

/// Список отрисовки экранного слоя на этот кадр. Лежит в `world.resources`.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    /// Размер экрана в пикселях, под который собран список (обычно
    /// `input.screen_size()`). Если цель другого размера, рендер растягивает слой.
    /// Ноль — список уже в пикселях цели.
    pub size: Vec2,
    pub vertices: Vec<OverlayVertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<OverlayBatch>,
    pub images: Vec<OverlayImage>,
}

impl Overlay {
    /// Очистить геометрию, сохранив выделенную память и картинки.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.batches.clear();
    }

    /// Добавить четырёхугольник (вершины по кругу) с текстурой и отсечением;
    /// склеивается с предыдущим отрезком, если текстура и отсечение те же.
    pub fn push_quad(&mut self, texture: OverlayTexture, clip: Option<[f32; 4]>, quad: [OverlayVertex; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&quad);
        let start = self.indices.len() as u32;
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        let end = start + 6;
        match self.batches.last_mut() {
            Some(batch) if batch.texture == texture && batch.clip == clip && batch.indices.end == start => {
                batch.indices.end = end;
            }
            _ => self.batches.push(OverlayBatch {
                texture,
                indices: start..end,
                clip,
            }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_with_same_texture_and_clip_share_a_batch() {
        let mut overlay = Overlay::default();
        let quad = [OverlayVertex::default(); 4];
        overlay.push_quad(OverlayTexture::Image(0), None, quad);
        overlay.push_quad(OverlayTexture::Image(0), None, quad);
        overlay.push_quad(OverlayTexture::Image(0), Some([0.0, 0.0, 5.0, 5.0]), quad);
        overlay.push_quad(OverlayTexture::Asset(Guid(7)), Some([0.0, 0.0, 5.0, 5.0]), quad);
        assert_eq!(overlay.batches.len(), 3);
        assert_eq!(overlay.batches[0].indices, 0..12);
        assert_eq!(overlay.vertices.len(), 16);
        assert_eq!(overlay.indices[6..12], [4, 5, 6, 4, 6, 7]);
    }
}
