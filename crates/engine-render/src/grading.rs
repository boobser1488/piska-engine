//! Цветокоррекция кадра (Unity: Color Adjustments + White Balance + Split Toning):
//! необязательный компонент [`ColorGrading`] на любом объекте сцены. Без него кадр
//! сводится как раньше. Применяется в пост-обработке (`post.wgsl::fs_composite`): баланс
//! белого — до тонмаппинга, контраст, насыщенность и оттенки теней / светов — после.
//!
//! Действует на вид из камеры сцены (игра, окно Game, снимки); виды с явными
//! `CameraView::post` (превью материалов в редакторе) его не получают.

use engine_core::prelude::*;

component! {
    /// Цветокоррекция кадра: баланс белого, контраст, насыщенность, оттенки теней и светов.
    pub struct ColorGrading {
        /// Включена. `false` — кадр как без компонента.
        pub enabled: bool = true,
        /// Баланс белого: + теплее (жёлто-оранжевый), − холоднее (голубой). −1..1.
        pub temperature: f32 = 0.0,
        /// Оттенок: + пурпурнее, − зеленее. −1..1.
        pub tint: f32 = 0.0,
        /// Контраст вокруг среднего серого: 1 — без изменений, больше — контрастнее.
        pub contrast: f32 = 1.0,
        /// Насыщенность: 0 — чёрно-белый кадр, 1 — без изменений.
        pub saturation: f32 = 1.0,
        /// Цвет, подмешиваемый в тени (тонирование). Чёрный — без изменений.
        pub shadows: Color = Color::BLACK,
        /// Множитель светов. Белый — без изменений.
        pub highlights: Color = Color::WHITE,
    }
}

impl Component for ColorGrading {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Параметры для шейдера сведения (четыре `vec4`, см. `PostParams` в post.wgsl).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GradingParams {
    /// rgb — множитель баланса белого, w — 1, если коррекция включена.
    pub balance: [f32; 4],
    /// x — контраст, y — насыщенность.
    pub tone: [f32; 4],
    /// rgb — цвет теней.
    pub lift: [f32; 4],
    /// rgb — множитель светов.
    pub gain: [f32; 4],
}

impl GradingParams {
    /// Без коррекции (шейдер её пропускает).
    pub const OFF: GradingParams = GradingParams {
        balance: [1.0, 1.0, 1.0, 0.0],
        tone: [1.0, 1.0, 0.0, 0.0],
        lift: [0.0; 4],
        gain: [1.0, 1.0, 1.0, 0.0],
    };
}

impl ColorGrading {
    /// Тёплый «закат»: теплее, чуть контрастнее и насыщеннее.
    pub fn warm() -> Self {
        Self { temperature: 0.25, contrast: 1.08, saturation: 1.1, ..Self::default() }
    }

    /// Множитель баланса белого для `temperature` и `tint` (средняя яркость сохраняется).
    pub fn balance(&self) -> [f32; 3] {
        let t = finite(self.temperature, 0.0).clamp(-1.0, 1.0);
        let g = finite(self.tint, 0.0).clamp(-1.0, 1.0);
        let rgb = [1.0 + 0.32 * t + 0.08 * g, 1.0 - 0.14 * g, 1.0 - 0.36 * t + 0.08 * g];
        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        [rgb[0] / luma, rgb[1] / luma, rgb[2] / luma]
    }

    pub(crate) fn params(&self) -> GradingParams {
        if !self.enabled {
            return GradingParams::OFF;
        }
        let [r, g, b] = self.balance();
        let color = |c: Color, d: f32| [finite(c.r, d).max(0.0), finite(c.g, d).max(0.0), finite(c.b, d).max(0.0), 0.0];
        GradingParams {
            balance: [r, g, b, 1.0],
            tone: [finite(self.contrast, 1.0).clamp(0.0, 4.0), finite(self.saturation, 1.0).clamp(0.0, 4.0), 0.0, 0.0],
            lift: color(self.shadows, 0.0),
            gain: color(self.highlights, 1.0),
        }
    }

    /// Параметры первой активной коррекции сцены (или «без коррекции»).
    pub(crate) fn scene_params(scene: &Scene) -> GradingParams {
        let mut found = None;
        if scene.count_with::<ColorGrading>() > 0 {
            scene.visit_active_with::<ColorGrading>(|_, grading| {
                if found.is_none() {
                    found = Some(grading.params());
                }
            });
        }
        found.unwrap_or(GradingParams::OFF)
    }
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_grading_changes_nothing() {
        let p = ColorGrading::default().params();
        for (a, b) in p.balance[..3].iter().zip([1.0, 1.0, 1.0]) {
            assert!((a - b).abs() < 1e-5);
        }
        assert_eq!(p.tone[..2], [1.0, 1.0]);
        assert_eq!(ColorGrading { enabled: false, ..ColorGrading::warm() }.params(), GradingParams::OFF);
    }

    #[test]
    fn warm_balance_is_redder_than_blue_and_keeps_luminance() {
        let [r, g, b] = ColorGrading::warm().balance();
        assert!(r > 1.0 && b < 1.0);
        assert!((0.2126 * r + 0.7152 * g + 0.0722 * b - 1.0).abs() < 1e-5);
        let [r, _, b] = ColorGrading { temperature: -0.5, ..Default::default() }.balance();
        assert!(b > r);
    }

    #[test]
    fn broken_values_fall_back() {
        let p = ColorGrading { contrast: f32::NAN, temperature: f32::INFINITY, ..Default::default() }.params();
        assert_eq!(p.tone[0], 1.0);
        assert!(p.balance.iter().all(|v| v.is_finite()));
    }
}
