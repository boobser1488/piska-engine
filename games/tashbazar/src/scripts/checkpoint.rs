use engine::prelude::*;

component! {
    /// Отметка полосы препятствий. Зона отметки — ящик `size` над позицией объекта
    /// (позиция — у пола, где встаёт игрок после падения). Порядок отметок задаёт
    /// `MovementCourse::checkpoints`.
    pub struct Checkpoint {
        /// Подпись в HUD.
        pub title: String = String::new(),
        /// Полный размер зоны, м (по высоте — вверх от позиции).
        pub size: Vec3 = Vec3::new(3.0, 2.5, 3.0),
        /// Куда смотреть после возврата на отметку, градусы (0 — вдоль −Z).
        pub respawn_yaw_deg: f32 = 0.0,
        /// Светящийся маяк отметки (подсвечивается, когда отметка следующая).
        pub beacon: ObjectRef = ObjectRef::NONE,
    }
}

impl Component for Checkpoint {
    /// Только данные: отметки проверяет `MovementCourse`.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Checkpoint {
    /// Ноги игрока `feet` внутри зоны отметки с основанием `base`.
    pub fn contains(&self, base: Vec3, feet: Vec3) -> bool {
        let half = self.size.abs() * 0.5;
        let d = feet - base;
        d.x.abs() <= half.x && d.z.abs() <= half.z && d.y >= -0.3 && d.y <= self.size.y.abs()
    }
}
