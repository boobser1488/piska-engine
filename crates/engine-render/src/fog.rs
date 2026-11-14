//! Туман — подсказка расстояния, а не молочная пелена.
//!
//! Плотность задаёт `Environment::fog_density` (0 — тумана нет). Форму — необязательный
//! компонент [`Fog`] на любом объекте сцены; без него действуют те же значения по
//! умолчанию: до `start` метров от камеры тумана нет совсем (ближние объекты не
//! выцветают), выше `height` он редеет по экспоненте (`height_falloff` на метр), цвет —
//! цвет неба у горизонта в направлении взгляда. `Fog { enabled: false, .. }` выключает
//! туман сцены целиком.
//!
//! Шейдеру параметры идут в `Globals.counts.yzw` (см. `common.wgsl::apply_fog`).

use engine_core::prelude::*;

component! {
    /// Форма тумана сцены: начало, высота и спад с высотой. Плотность — в `Environment`.
    pub struct Fog {
        /// Туман включён. Выключить туман сцены целиком — `false`.
        pub enabled: bool = true,
        /// До этого расстояния от камеры (м) тумана нет.
        pub start: f32 = 25.0,
        /// Высота (мировая y), на которой плотность равна `Environment::fog_density`.
        pub height: f32 = 0.0,
        /// Насколько туман редеет с высотой: плотность × e^(−спад × (y − height)).
        /// 0 — одинаково на любой высоте.
        pub height_falloff: f32 = 0.004,
    }
}

impl Component for Fog {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Fog {
    /// Туман выключен.
    pub fn off() -> Self {
        Self { enabled: false, ..Self::default() }
    }

    /// Параметры для шейдера: (включён, начало, спад, высота).
    pub(crate) fn shader_params(scene: &Scene) -> (bool, [f32; 3]) {
        let mut fog = None;
        if scene.count_with::<Fog>() > 0 {
            scene.visit_active_with::<Fog>(|_, f| {
                if fog.is_none() {
                    fog = Some(f.clone());
                }
            });
        }
        let fog = fog.unwrap_or_default();
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        (
            fog.enabled,
            [finite(fog.start, 25.0).max(0.0), finite(fog.height_falloff, 0.0).clamp(0.0, 1.0), finite(fog.height, 0.0)],
        )
    }
}
