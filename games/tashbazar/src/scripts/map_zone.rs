use engine::prelude::*;

component! {
    /// Именованная часть карты («позывной»): HUD пишет, где стоит игрок. Зона — ящик
    /// размера `size` с центром в позиции объекта (поворот не учитывается).
    pub struct MapZone {
        /// Название на экране.
        pub title: String = String::new(),
        /// Полный размер зоны, м.
        pub size: Vec3 = Vec3::new(10.0, 8.0, 10.0),
        /// Это точка задания (площадка A или B).
        pub site: bool = false,
    }
}

impl Component for MapZone {
    /// Только данные: зону ищет HUD.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl MapZone {
    /// Точка внутри зоны с центром `center`.
    pub fn contains(&self, center: Vec3, point: Vec3) -> bool {
        let d = (point - center).abs();
        let half = self.size.abs() * 0.5;
        d.x <= half.x && d.y <= half.y && d.z <= half.z
    }
}

/// Название зоны, где стоит `point` (из нескольких — самая маленькая).
pub fn zone_at(scene: &Scene, point: Vec3) -> Option<(GameObjectId, String)> {
    let mut best: Option<(f32, GameObjectId, String)> = None;
    scene.visit_active_with::<MapZone>(|id, zone| {
        if zone.contains(scene.world_position(id), point) {
            let volume = zone.size.x * zone.size.y * zone.size.z;
            if best.as_ref().is_none_or(|(v, ..)| volume < *v) {
                best = Some((volume, id, zone.title.clone()));
            }
        }
    });
    best.map(|(_, id, title)| (id, title))
}
