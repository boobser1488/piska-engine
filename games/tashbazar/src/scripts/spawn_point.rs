use engine::prelude::*;

reflect_enum! {
    /// Сторона: атакующие (север карты) или защитники (юг).
    pub enum Team { Attackers, Defenders }
}

component! {
    /// Точка появления игрока. Позиция объекта — у ног, взгляд — вдоль его оси −Z.
    /// Добавить точку: перетащите `prefabs/markers/spawn_*.prefab` в сцену.
    pub struct SpawnPoint {
        pub team: Team = Team::Attackers,
        /// Порядок в команде (0 — первая).
        pub index: i32 = 0,
    }
}

impl Component for SpawnPoint {
    /// Только данные: точки ищет менеджер игры.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Точки команды `team` в порядке `index`: (объект, позиция, yaw).
pub fn spawns(scene: &Scene, team: Team) -> Vec<(GameObjectId, Vec3, f32)> {
    let mut found: Vec<(i32, GameObjectId)> = scene
        .iter_active_with::<SpawnPoint>()
        .filter(|(_, s)| s.team == team)
        .map(|(id, s)| (s.index, id))
        .collect();
    found.sort_by_key(|(index, _)| *index);
    found
        .into_iter()
        .map(|(_, id)| (id, scene.world_position(id), crate::game::yaw_of(scene.world_rotation(id))))
        .collect()
}
