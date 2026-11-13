//! Коллайдеры-карты высот (Rapier `HeightField`) — для рельефа. Физика не знает,
//! откуда высоты: плитки задаёт владелец (подсистема рельефа в фасаде `engine`
//! берёт их из компонента `Terrain`). Плитка — квадратная сетка отсчётов; клетки
//! режутся диагональю из (x1, z0) в (x0, z1), как и меш рельефа.

use engine_core::prelude::*;
use rapier3d::na;
use rapier3d::prelude as rp;

use super::Physics3D;

/// Плитка карты высот в мировых координатах.
#[derive(Clone, Copy, Debug)]
pub struct HeightfieldTile<'a> {
    /// Угол плитки с минимальными x и z; `y` — нулевая высота.
    pub corner: Vec3,
    /// Шаг сетки, м.
    pub cell: f32,
    /// Отсчётов на сторону (клеток — на один меньше).
    pub samples: usize,
    /// `samples × samples` высот, строка — вдоль x, строки идут по z.
    pub heights: &'a [f32],
    pub friction: f32,
    /// Слой столкновений (как `GameObject::layer`).
    pub layer: u32,
}

impl Physics3D {
    /// Создать или заменить плитку `tile` карты высот объекта `owner`. Лучи, попавшие в
    /// неё, возвращают `owner`. Запросы видят плитку сразу, симуляция — со следующего шага.
    pub fn set_heightfield_tile(&mut self, owner: GameObjectId, tile: (i32, i32), desc: HeightfieldTile) {
        self.remove_heightfield_tile(owner, tile);
        let n = desc.samples.max(2);
        if desc.heights.len() < n * n {
            log::error!("heightfield tile {tile:?}: {} heights for {n}×{n} samples", desc.heights.len());
            return;
        }
        // Rapier: строки матрицы — вдоль z, столбцы — вдоль x; плитка с центром в translation.
        let heights = na::DMatrix::from_fn(n, n, |row, col| desc.heights[row * n + col]);
        let extent = desc.cell * (n - 1) as f32;
        let center = desc.corner + Vec3::new(extent * 0.5, 0.0, extent * 0.5);
        // Матрица слоёв из настроек проекта; по умолчанию — со всеми слоями.
        let groups = self.groups_for(desc.layer, u32::MAX);
        let collider = rp::ColliderBuilder::heightfield(heights, na::Vector3::new(extent, 1.0, extent))
            .translation(na::Vector3::new(center.x, center.y, center.z))
            .friction(desc.friction)
            .collision_groups(groups)
            .build();
        let handle = self.colliders.insert(collider);
        self.by_collider.insert(handle, owner);
        self.heightfields.insert((owner, tile.0, tile.1), handle);
        self.query.update(&self.colliders);
    }

    /// Убрать плитку. `false` — её не было.
    pub fn remove_heightfield_tile(&mut self, owner: GameObjectId, tile: (i32, i32)) -> bool {
        let Some(handle) = self.heightfields.remove(&(owner, tile.0, tile.1)) else {
            return false;
        };
        self.by_collider.remove(&handle);
        self.colliders.remove(handle, &mut self.islands, &mut self.bodies, true);
        self.query.update(&self.colliders);
        true
    }

    /// Убрать все плитки объекта.
    pub fn remove_heightfields(&mut self, owner: GameObjectId) {
        let tiles: Vec<(i32, i32)> = self.heightfields.keys().filter(|k| k.0 == owner).map(|k| (k.1, k.2)).collect();
        for tile in tiles {
            self.remove_heightfield_tile(owner, tile);
        }
    }

    /// Плитки объекта, которые сейчас есть в симуляции.
    pub fn heightfield_tiles(&self, owner: GameObjectId) -> Vec<(i32, i32)> {
        let mut tiles: Vec<(i32, i32)> = self.heightfields.keys().filter(|k| k.0 == owner).map(|k| (k.1, k.2)).collect();
        tiles.sort_unstable();
        tiles
    }

    /// Владельцы плиток карт высот (для уборки удалённых объектов).
    pub fn heightfield_owners(&self) -> Vec<GameObjectId> {
        let mut owners: Vec<GameObjectId> = self.heightfields.keys().map(|k| k.0).collect();
        owners.sort_unstable();
        owners.dedup();
        owners
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_the_tile_between_samples() {
        let mut physics = Physics3D::default();
        let owner = {
            let mut scene = Scene::default();
            scene.spawn("Ground")
        };
        // Наклонная плоскость y = x / 4 на клетке 2 м.
        let n = 5;
        let heights: Vec<f32> = (0..n * n).map(|i| (i % n) as f32 * 2.0 / 4.0).collect();
        physics.set_heightfield_tile(
            owner,
            (0, 0),
            HeightfieldTile { corner: Vec3::new(10.0, 1.0, 20.0), cell: 2.0, samples: n, heights: &heights, friction: 0.5, layer: 0 },
        );
        let hit = physics.raycast(Vec3::new(13.0, 50.0, 23.0), Vec3::NEG_Y, 100.0, None).expect("hits");
        assert_eq!(hit.object, owner);
        assert!((hit.point.y - (1.0 + 3.0 / 4.0)).abs() < 1e-3, "{:?}", hit.point);
        assert!(physics.raycast(Vec3::new(30.0, 50.0, 23.0), Vec3::NEG_Y, 100.0, None).is_none());
        assert_eq!(physics.heightfield_tiles(owner), vec![(0, 0)]);
        assert!(physics.remove_heightfield_tile(owner, (0, 0)));
        assert!(physics.raycast(Vec3::new(13.0, 50.0, 23.0), Vec3::NEG_Y, 100.0, None).is_none());
    }
}
