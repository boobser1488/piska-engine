//! Вода с эффектами: необязательный компонент [`WaterEffects`] на объекте с [`Water`].
//!
//! Без компонента вода рисуется как раньше (water.wgsl). С ним (water_fx.wgsl):
//!
//! * мелкие волны — две бегущие карты нормалей (процедурный шум) поверх крупных волн;
//! * блик солнца: узкий и яркий, размером с диск солнца `Sky` (или по умолчанию);
//! * преломление: дно и камни под водой видны с искажением волнами, цвет зависит от
//!   толщины воды — у берега `shallow_color`, глубже — `Water::deep_color`;
//! * пена там, где вода касается геометрии (берег, камни, сваи);
//! * отражения: небо всегда; на пресете High — отражения сцены по экрану (лучом по
//!   буферу глубины, вне кадра — небо);
//! * `stylized` — плоский стиль для низкополигональных сцен: гранёные волны, цвет полосами,
//!   чёткая полоса пены, без отражений сцены.
//!
//! Преломление, цвет по глубине, пена и отражения сцены читают кадр и буфер глубины под
//! водой — в браузере на WebGL2 многовыборочную глубину читать нельзя, там остаются волны,
//! блик и плоский стиль. Пресет (`GraphicsSettings::water_detail`): Low — волны и блик,
//! Medium — + преломление, глубина, пена, High — + отражения сцены.

use engine_core::prelude::*;

component! {
    /// Эффекты воды для компонента `Water` на этом же объекте.
    pub struct WaterEffects {
        /// Включено. `false` — вода как без компонента.
        pub enabled: bool = true,
        /// Цвет мелкой воды у берега (тонкий слой над дном).
        pub shallow_color: Color = Color::rgb(0.1, 0.45, 0.42),
        /// Прозрачность: на сколько метров толщины дно ещё видно.
        pub clarity: f32 = 1.5,
        /// Сила искажения дна волнами.
        pub refraction: f32 = 0.35,
        /// Сила пены у берега и предметов (0 — без пены).
        pub foam: f32 = 0.8,
        /// Ширина полосы пены, м (по высоте воды над дном).
        pub foam_width: f32 = 0.18,
        /// Сила мелких волн (карты нормалей).
        pub ripples: f32 = 0.6,
        /// Размер мелких волн, м.
        pub ripple_scale: f32 = 3.0,
        /// Сила блика солнца.
        pub glint: f32 = 1.0,
        /// Отражения сцены (пресет High): доля поверх отражения неба.
        pub reflections: f32 = 1.0,
        /// Плоский стиль для низкополигональных сцен.
        pub stylized: bool = false,
    }
}

impl Component for WaterEffects {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Параметры воды с эффектами для шейдера (см. `WaterFx` в water_fx.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct WaterFxUniform {
    /// x, уровень, z, половина стороны (0 — море за камерой).
    pub center: [f32; 4],
    /// rgb — цвет глубины, a — непрозрачность сверху.
    pub deep: [f32; 4],
    /// x — время, y — высота волн, z — скорость, w — 0.
    pub wave: [f32; 4],
    /// rgb — мелкая вода, a — прозрачность (м).
    pub shallow: [f32; 4],
    /// x — пена, y — ширина пены, z — сила мелких волн, w — их размер.
    pub foam: [f32; 4],
    /// x — преломление, y — блик, z — отражения сцены, w — плоский стиль.
    pub look: [f32; 4],
    /// x — детализация (0 — волны и блик, 1 — + глубина и пена, 2 — + отражения).
    pub detail: [f32; 4],
}

impl WaterFxUniform {
    pub(crate) fn new(base: [[f32; 4]; 3], fx: &WaterEffects, detail: u32) -> Self {
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let s = fx.shallow_color;
        Self {
            center: base[0],
            deep: base[1],
            wave: base[2],
            shallow: [s.r, s.g, s.b, finite(fx.clarity, 2.5).max(0.05)],
            foam: [
                finite(fx.foam, 0.8).clamp(0.0, 2.0),
                finite(fx.foam_width, 0.18).max(0.01),
                finite(fx.ripples, 0.6).clamp(0.0, 2.0),
                finite(fx.ripple_scale, 3.0).max(0.1),
            ],
            look: [
                finite(fx.refraction, 0.35).clamp(0.0, 3.0),
                finite(fx.glint, 1.0).clamp(0.0, 4.0),
                finite(fx.reflections, 1.0).clamp(0.0, 1.0),
                if fx.stylized { 1.0 } else { 0.0 },
            ],
            detail: [detail.min(2) as f32, 0.0, 0.0, 0.0],
        }
    }
}
