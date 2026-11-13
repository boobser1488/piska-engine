//! Коллайдеры рельефа: для каждого активного `Terrain` подсистема создаёт в
//! [`Physics3D`] плитки `HeightField` — по одной на чанк, с теми же высотами, что у
//! ближнего LOD меша (и у `terrain_height`).
//!
//! Как в образце, коллайдеры подгружаются вокруг тех, кому они нужны: чанки под
//! динамическими телами и персонажами (и их соседи) создаются сразу, перед шагом
//! физики; остальные — в фоне, по чуть-чуть за шаг, пока не будет покрыт весь рельеф
//! (тогда лучи попадают в рельеф везде). Скульпт во время игры перестраивает только
//! изменённые чанки.

use std::collections::HashMap;

use engine_assets::AssetDatabase;
use engine_core::prelude::*;
use engine_physics::{BodyType, CharacterController, HeightfieldTile, Physics3D, Rigidbody};
use engine_render::{Terrain, TerrainSampler};

/// Сколько миллисекунд за шаг физики тратить на фоновые плитки.
const BACKGROUND_BUDGET_MS: f64 = 1.5;
/// Трение рельефа.
const FRICTION: f32 = 0.8;

struct State {
    shape: u64,
    origin: Vec3,
    layer: u32,
    /// Плитка → подпись правок, с которой она построена.
    tiles: HashMap<(i32, i32), u64>,
    /// Следующий чанк для фонового обхода.
    cursor: i32,
}

/// Подсистема коллайдеров рельефа (её подключает [`crate::app`]).
#[derive(Default)]
pub struct TerrainPhysicsSystem {
    states: HashMap<GameObjectId, State>,
}

impl Subsystem for TerrainPhysicsSystem {
    fn name(&self) -> &'static str {
        "terrain-physics"
    }

    fn before_fixed_update(&mut self, world: &mut World) {
        self.sync(world, BACKGROUND_BUDGET_MS);
    }
}

fn build_tile(physics: &mut Physics3D, id: GameObjectId, state: &mut State, sampler: &TerrainSampler, key: (i32, i32)) {
    let layout = sampler.layout();
    let heights = sampler.chunk_heights(key.0, key.1);
    physics.set_heightfield_tile(
        id,
        key,
        HeightfieldTile {
            corner: state.origin + Vec3::new(key.0 as f32 * layout.chunk_size, 0.0, key.1 as f32 * layout.chunk_size),
            cell: layout.spacing(),
            samples: (layout.resolution + 1) as usize,
            heights: &heights,
            friction: FRICTION,
            layer: state.layer,
        },
    );
    state.tiles.insert(key, sampler.chunk_signature(key.0, key.1, 0));
}

impl TerrainPhysicsSystem {
    /// Привести коллайдеры рельефов в соответствие со сценой. `budget_ms` — сколько
    /// времени можно потратить на фоновые плитки (`f64::INFINITY` — построить все).
    pub fn sync(&mut self, world: &mut World, budget_ms: f64) {
        let World { scene, resources, .. } = world;
        let Some(mut physics) = resources.remove::<Physics3D>() else {
            return;
        };
        let assets = resources.get::<AssetDatabase>();

        // Кому нужна земля под ногами: динамические тела и персонажи.
        let mut interest: Vec<Vec3> = Vec::new();
        scene.visit_active_with::<Rigidbody>(|id, body| {
            if body.body_type == BodyType::Dynamic {
                interest.push(scene.world_matrix(id).w_axis.truncate());
            }
        });
        scene.visit_active_with::<CharacterController>(|id, _| {
            interest.push(scene.world_matrix(id).w_axis.truncate());
        });

        let mut terrains: Vec<(GameObjectId, Vec3, u32, TerrainSampler)> = Vec::new();
        scene.visit_active_with::<Terrain>(|id, terrain| {
            let layer = scene.get(id).map_or(0, |o| o.layer);
            terrains.push((id, scene.world_matrix(id).w_axis.truncate(), layer, terrain.sampler(assets)));
        });

        // Рельефы, которых больше нет.
        let alive: Vec<GameObjectId> = terrains.iter().map(|t| t.0).collect();
        self.states.retain(|id, _| alive.contains(id));
        for owner in physics.heightfield_owners() {
            if !alive.contains(&owner) {
                physics.remove_heightfields(owner);
            }
        }

        let clock = engine_core::profile::Stopwatch::start();
        for (id, origin, layer, sampler) in terrains {
            let layout = sampler.layout();
            let state = self.states.entry(id).or_insert_with(|| State {
                shape: sampler.shape_hash(),
                origin,
                layer,
                tiles: HashMap::new(),
                cursor: 0,
            });
            if state.shape != sampler.shape_hash() || state.origin != origin || state.layer != layer {
                physics.remove_heightfields(id);
                *state = State { shape: sampler.shape_hash(), origin, layer, tiles: HashMap::new(), cursor: 0 };
            }

            // Изменённые правками плитки — заново.
            let stale: Vec<(i32, i32)> = state
                .tiles
                .iter()
                .filter(|(key, signature)| sampler.chunk_signature(key.0, key.1, 0) != **signature)
                .map(|(key, _)| *key)
                .collect();
            for key in stale {
                build_tile(&mut physics, id, state, &sampler, key);
            }

            // Под телами — сразу (с соседями: тело может перекатиться через границу).
            let size = layout.chunk_size;
            for point in &interest {
                let local = *point - origin;
                let (cx, cz) = ((local.x / size).floor() as i32, (local.z / size).floor() as i32);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let key = (cx + dx, cz + dz);
                        if key.0 < 0 || key.1 < 0 || key.0 >= layout.chunks || key.1 >= layout.chunks {
                            continue;
                        }
                        if !state.tiles.contains_key(&key) {
                            build_tile(&mut physics, id, state, &sampler, key);
                        }
                    }
                }
            }

            // Остальное — в фоне, пока есть время.
            let total = layout.chunks * layout.chunks;
            while state.cursor < total && (clock.ms() as f64) < budget_ms {
                let key = (state.cursor % layout.chunks, state.cursor / layout.chunks);
                state.cursor += 1;
                if !state.tiles.contains_key(&key) {
                    build_tile(&mut physics, id, state, &sampler, key);
                }
            }
        }
        resources.insert(physics);
    }

    /// Сколько плиток-коллайдеров у рельефа `id`.
    pub fn tile_count(&self, id: GameObjectId) -> usize {
        self.states.get(&id).map_or(0, |s| s.tiles.len())
    }
}

/// Сервисы для C++ (`engine::call_service`): `terrain.height` (x, z) → `[высота]` или `[]`
/// (под точкой нет рельефа), `terrain.normal` (x, z) → `[Vec3]` или `[]`.
#[cfg(feature = "cpp")]
pub fn install_services(world: &mut World) {
    fn float(args: &[Value], i: usize) -> f32 {
        match args.get(i) {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(n)) => *n as f32,
            _ => 0.0,
        }
    }
    crate::cpp_services::register(world, "terrain.height", |world, a| {
        Ok(engine_render::terrain_height(world, float(a, 0), float(a, 1)).map_or(Vec::new(), |h| vec![Value::Float(h)]))
    });
    crate::cpp_services::register(world, "terrain.normal", |world, a| {
        Ok(engine_render::terrain_normal(world, float(a, 0), float(a, 1))
            .map_or(Vec::new(), |n| vec![Value::Vec3(n.x, n.y, n.z)]))
    });
}
