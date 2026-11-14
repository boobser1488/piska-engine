//! Лаборатория префабов: материалы (`assets/materials/*.material`) и префабы
//! (`assets/prefabs/**`). Каждый префаб собирается из объектов в пустой сцене и
//! записывается сериализатором движка — то же, что делает редактор по «Save as Prefab».

use std::error::Error;
use std::path::{Path, PathBuf};

use engine::assets::{Material, Mesh, Texture};
use engine::prelude::*;
use marble::scripts::{
    ball::Ball, blinker::Blinker, booster::Booster, bumper::Bumper, camera_rig::CameraRig, checkpoint::Checkpoint,
    cog::Cog, collapsing_tile::CollapsingTile, conveyor::Conveyor, crusher::Crusher, fan::Fan, finish_pad::FinishPad,
    game_manager::GameManager, gear_pickup::GearPickup, moving_platform::MovingPlatform, seesaw_rig::SeesawRig,
    sweeper::Sweeper, sweeper_arm::SweeperArm,
};

/// Ссылки на материалы игры.
#[derive(Clone)]
pub struct Mats {
    pub plate: AssetHandle<Material>,
    pub plate_dark: AssetHandle<Material>,
    pub hazard: AssetHandle<Material>,
    pub brushed: AssetHandle<Material>,
    pub rubber: AssetHandle<Material>,
    pub belt: AssetHandle<Material>,
    pub crate_: AssetHandle<Material>,
    pub steel: AssetHandle<Material>,
    pub band: AssetHandle<Material>,
    pub gold: AssetHandle<Material>,
    pub silver: AssetHandle<Material>,
    pub glow_amber: AssetHandle<Material>,
    pub glow_orange: AssetHandle<Material>,
    pub glow_red: AssetHandle<Material>,
    pub glow_cyan: AssetHandle<Material>,
    pub glow_green: AssetHandle<Material>,
    pub sea: AssetHandle<Material>,
    pub cloud: AssetHandle<Material>,
}

/// Записать материалы игры и вернуть ссылки.
pub fn write_materials(db: &mut AssetDatabase) -> Result<Mats, Box<dyn Error>> {
    let tex = |db: &AssetDatabase, path: &str| db.handle::<Texture>(path);
    let plate_tex = tex(db, "textures/plate.png")?;
    let hazard_tex = tex(db, "textures/hazard.png")?;
    let brushed_tex = tex(db, "textures/brushed.png")?;
    let crate_tex = tex(db, "textures/crate.png")?;
    let glow = |c: Color| Material::new().color(Color::hex(0x151a22)).roughness(0.5).emissive(c, 1.0);
    let mut save = |path: &str, m: Material| db.save_material(&format!("materials/{path}.material"), &m);
    Ok(Mats {
        plate: save("plate", Material::new().color(Color::hex(0xb8cdf2)).texture(plate_tex).world_tiling(2.0).metallic(0.3).roughness(0.6))?,
        plate_dark: save("plate_dark", Material::new().color(Color::hex(0x7e93b8)).texture(plate_tex).world_tiling(2.0).metallic(0.35).roughness(0.65))?,
        hazard: save("hazard", Material::new().color(Color::hex(0xf2f2f2)).texture(hazard_tex).world_tiling(1.5).metallic(0.1).roughness(0.7))?,
        brushed: save("brushed", Material::new().color(Color::hex(0xd6dce6)).texture(brushed_tex).world_tiling(2.0).metallic(0.85).roughness(0.38))?,
        rubber: save("rubber", Material::new().color(Color::hex(0xff6a1a)).roughness(0.85))?,
        belt: save("belt", Material::new().color(Color::hex(0x20242c)).roughness(0.9))?,
        crate_: save("crate", Material::new().texture(crate_tex).roughness(0.8))?,
        steel: save("steel", Material::new().color(Color::hex(0xc9d3e4)).metallic(0.9).roughness(0.25))?,
        band: save("ball_band", Material::new().color(Color::hex(0xff5a10)).metallic(0.1).roughness(0.45).emissive(Color::rgb(0.5, 0.1, 0.0), 1.0))?,
        gold: save("gold", Material::new().color(Color::hex(0xffc933)).metallic(1.0).roughness(0.25).emissive(Color::rgb(0.9, 0.55, 0.05), 1.0))?,
        silver: save("silver", Material::new().color(Color::hex(0xd8e0ee)).metallic(1.0).roughness(0.3).emissive(Color::rgb(0.05, 0.12, 0.22), 1.0))?,
        glow_amber: save("glow_amber", glow(Color::rgb(2.4, 1.6, 0.2)))?,
        glow_orange: save("glow_orange", glow(Color::rgb(3.0, 0.5, 0.15)))?,
        glow_red: save("glow_red", glow(Color::rgb(3.0, 0.3, 0.1)))?,
        glow_cyan: save("glow_cyan", glow(Color::rgb(0.2, 1.4, 2.6)))?,
        glow_green: save("glow_green", glow(Color::rgb(0.2, 2.2, 0.9)))?,
        sea: save("sea", Material::new().color(Color::hex(0x2c62b8)).roughness(1.0).emissive(Color::rgb(0.01, 0.04, 0.12), 1.0).shadows(false, true))?,
        cloud: save("cloud", Material::new().color(Color::hex(0xf4f8ff)).roughness(1.0).emissive(Color::rgb(0.10, 0.12, 0.16), 1.0).shadows(false, true))?,
    })
}

/// Сцена-«верстак», в которой собираются префабы.
pub struct Lab {
    pub scene: Scene,
    pub dir: PathBuf,
    pub mats: Mats,
    pub gear_mesh: AssetHandle<Mesh>,
    pub cog_mesh: AssetHandle<Mesh>,
    pub rotor_mesh: AssetHandle<Mesh>,
    pub saved: Vec<String>,
}

impl Lab {
    pub fn new(assets: &Path, db: &AssetDatabase, mats: Mats) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            scene: Scene::new("lab"),
            dir: assets.to_path_buf(),
            mats,
            gear_mesh: db.handle::<Mesh>("models/gear.obj")?,
            cog_mesh: db.handle::<Mesh>("models/cog.obj")?,
            rotor_mesh: db.handle::<Mesh>("models/fan_rotor.obj")?,
            saved: Vec::new(),
        })
    }

    /// Объект (корень или ребёнок) с позой.
    pub fn obj(&mut self, name: &str, parent: Option<GameObjectId>, pos: Vec3, rot: Quat, scale: Vec3) -> GameObjectId {
        let builder = self.scene.build(name).at3(pos.x, pos.y, pos.z).rotation(rot).scale(scale.x, scale.y, scale.z);
        match parent {
            Some(p) => builder.child_of(p).id(),
            None => builder.id(),
        }
    }

    pub fn add<T: Component>(&mut self, id: GameObjectId, component: T) {
        self.scene.get_mut(id).expect("lab object").add(component);
    }

    /// Меш из материала-ассета: `shape` — встроенная форма.
    pub fn mesh(&mut self, id: GameObjectId, shape: MeshShape, material: AssetHandle<Material>) {
        self.add(id, MeshRenderer::shape(shape).with_material_asset(material));
    }

    pub fn model(&mut self, id: GameObjectId, mesh: AssetHandle<Mesh>, material: AssetHandle<Material>) {
        self.add(id, MeshRenderer::asset(mesh).with_material_asset(material));
    }

    pub fn solid(&mut self, id: GameObjectId, friction: f32) {
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_material(friction, 0.0));
    }

    /// Записать объект как префаб `prefabs/<rel>.prefab` и убрать из верстака.
    pub fn save(&mut self, id: GameObjectId, rel: &str) -> Result<(), Box<dyn Error>> {
        let path = self.dir.join("prefabs").join(format!("{rel}.prefab"));
        std::fs::create_dir_all(path.parent().expect("prefab dir"))?;
        let prefab = Prefab::from_object(&self.scene, id).ok_or("prefab object is gone")?;
        prefab.save(&path)?;
        self.scene.destroy(id);
        self.saved.push(rel.to_string());
        Ok(())
    }

    // ------------------------------------------------------------------
    // Дорожка
    // ------------------------------------------------------------------

    pub fn track_blocks(&mut self) -> Result<(), Box<dyn Error>> {
        for (name, rel, mat, friction) in [
            ("Plate", "track/plate_block", self.mats.plate, 0.9),
            ("Dark Plate", "track/plate_dark_block", self.mats.plate_dark, 0.9),
            ("Hazard", "track/hazard_block", self.mats.hazard, 0.5),
        ] {
            let id = self.obj(name, None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            self.mesh(id, MeshShape::Cube, mat);
            self.solid(id, friction);
            self.save(id, rel)?;
        }
        // Светящаяся полоса без коллайдера: линия старта.
        let id = self.obj("Glow Strip", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.mesh(id, MeshShape::Cube, self.mats.glow_amber);
        self.save(id, "track/glow_strip")?;
        // Ящик — настоящее динамическое тело.
        let id = self.obj("Crate", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.mesh(id, MeshShape::Cube, self.mats.crate_);
        self.add(id, Rigidbody { angular_damping: 0.4, linear_damping: 0.05, ..Rigidbody::dynamic() });
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_density(0.45).with_material(0.55, 0.0));
        self.save(id, "track/crate")?;
        // Качели: доска на шарнире (SeesawRig сам вешает её на ось при старте).
        let id = self.obj("Seesaw", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(3.4, 0.35, 8.0));
        self.mesh(id, MeshShape::Cube, self.mats.plate);
        self.add(id, Rigidbody { angular_damping: 1.6, linear_damping: 0.1, ..Rigidbody::dynamic() });
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_density(0.8).with_material(0.9, 0.0));
        self.add(id, SeesawRig::default());
        let stripe = self.obj("Stripe", Some(id), Vec3::new(0.0, 0.51, 0.0), Quat::IDENTITY, Vec3::new(0.08, 0.03, 0.96));
        self.mesh(stripe, MeshShape::Cube, self.mats.glow_amber);
        self.save(id, "track/seesaw")?;
        // Ось качелей.
        let id = self.obj("Pivot", None, Vec3::ZERO, Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(0.6, 4.0, 0.6));
        self.mesh(id, MeshShape::Cylinder, self.mats.brushed);
        self.save(id, "track/pivot")?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Механизмы
    // ------------------------------------------------------------------

    pub fn mechanisms(&mut self) -> Result<(), Box<dyn Error>> {
        let m = self.mats.clone();

        // Ездящая платформа.
        let id = self.obj("Platform", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(5.2, 0.5, 4.0));
        self.mesh(id, MeshShape::Cube, m.plate);
        self.add(id, Rigidbody::kinematic());
        self.solid(id, 0.9);
        self.scene.get_mut(id).expect("platform").tag = "Mover".into();
        self.add(id, MovingPlatform::default());
        for (name, z) in [("Edge A", -0.46), ("Edge B", 0.46)] {
            let edge = self.obj(name, Some(id), Vec3::new(0.0, 0.52, z), Quat::IDENTITY, Vec3::new(0.98, 0.06, 0.08));
            self.mesh(edge, MeshShape::Cube, m.glow_amber);
        }
        self.save(id, "mech/moving_platform")?;

        // Метлы: одна палка и крест.
        for (rel, bars) in [("mech/sweeper", 1), ("mech/sweeper_cross", 2)] {
            let id = self.obj("Sweeper", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            self.add(id, Sweeper::default());
            let column = self.obj("Column", Some(id), Vec3::new(0.0, 0.55, 0.0), Quat::IDENTITY, Vec3::new(0.9, 1.1, 0.9));
            self.mesh(column, MeshShape::Cylinder, m.brushed);
            self.add(column, Collider::cylinder(0.5, 1.0));
            for k in 0..bars {
                let name = if k == 0 { "Arm".to_string() } else { format!("Arm {}", k + 1) };
                let angle = 90.0 * k as f32;
                let arm = self.obj(&name, Some(id), Vec3::new(0.0, 0.495, 0.0), Quat::from_rotation_y(angle.to_radians()), Vec3::new(8.0, 0.55, 0.45));
                self.mesh(arm, MeshShape::Cube, m.hazard);
                self.add(arm, Rigidbody::kinematic());
                self.solid(arm, 0.9);
                self.add(arm, SweeperArm::default());
                for (tip, x) in [("Tip A", -0.5), ("Tip B", 0.5)] {
                    let t = self.obj(tip, Some(arm), Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, Vec3::new(0.06, 1.15, 1.2));
                    self.mesh(t, MeshShape::Cube, m.glow_orange);
                }
            }
            self.save(id, rel)?;
        }

        // Конвейер (планки-«Slat» добавляет сцена) и планка.
        let id = self.obj("Conveyor", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Conveyor::default());
        let belt = self.obj("Belt", Some(id), Vec3::ZERO, Quat::IDENTITY, Vec3::new(4.0, 0.4, 10.0));
        self.mesh(belt, MeshShape::Cube, m.belt);
        self.solid(belt, 0.4);
        for (name, side) in [("Side L", -1.0), ("Side R", 1.0)] {
            let s = self.obj(name, Some(id), Vec3::new(side * 2.15, 0.05, 0.0), Quat::IDENTITY, Vec3::new(0.3, 0.5, 10.0));
            self.mesh(s, MeshShape::Cube, m.hazard);
            self.solid(s, 0.3);
        }
        self.save(id, "mech/conveyor")?;
        for (rel, mat) in [("mech/slat", m.glow_amber), ("mech/slat_reverse", m.glow_orange)] {
            let id = self.obj("Slat", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(3.9, 0.06, 0.28));
            self.mesh(id, MeshShape::Cube, mat);
            self.save(id, rel)?;
        }

        // Вентилятор: решётка, светящееся кольцо, ротор-модель, струя частиц.
        let id = self.obj("Fan", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Fan::default());
        let grille = self.obj("Grille", Some(id), Vec3::new(0.0, 0.04, 0.0), Quat::IDENTITY, Vec3::new(2.9, 0.08, 2.9));
        self.mesh(grille, MeshShape::Cylinder, m.brushed);
        let ring = self.obj("Ring", Some(id), Vec3::new(0.0, 0.1, 0.0), Quat::IDENTITY, Vec3::new(3.2, 0.06, 3.2));
        self.mesh(ring, MeshShape::Cylinder, m.glow_cyan);
        let rotor = self.obj("Rotor", Some(id), Vec3::new(0.0, 0.16, 0.0), Quat::IDENTITY, Vec3::new(2.6, 1.0, 2.6));
        self.model(rotor, self.rotor_mesh, m.silver);
        // Скрипт находит ротор по ссылке, а не по имени: ребёнка можно переименовать.
        let rotor_ref = ObjectRef::new(&self.scene, rotor);
        self.scene.get_component_mut::<Fan>(id).expect("fan").rotor = rotor_ref;
        let flow = self.obj("Flow", Some(id), Vec3::new(0.0, 0.2, 0.0), Quat::IDENTITY, Vec3::ONE);
        self.add(
            flow,
            ParticleSystem {
                emission_rate: 38.0,
                lifetime: Vec2::new(0.5, 0.8),
                start_speed: Vec2::new(5.0, 7.0),
                start_size: Vec2::new(0.06, 0.12),
                start_color: Color::rgba(0.55, 1.6, 2.4, 0.8),
                start_color_max: Color::rgba(0.3, 1.0, 2.0, 0.6),
                gravity: Vec3::ZERO,
                drag: 0.2,
                color_over_lifetime: vec![Color::WHITE.with_alpha(0.0), Color::WHITE, Color::WHITE.with_alpha(0.0)],
                shape: EmitterShape::Circle,
                shape_size: Vec3::new(1.4, 0.0, 0.0),
                cone_angle: 4.0,
                flat: false,
                blend: ParticleBlend::Additive,
                velocity_stretch: 0.12,
                max_particles: 120,
                seed: 11,
                ..ParticleSystem::default()
            },
        );
        self.save(id, "mech/fan")?;

        // Пресс: голова, штанга, корпус.
        let id = self.obj("Crusher", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Crusher::default());
        let head = self.obj("Head", Some(id), Vec3::ZERO, Quat::IDENTITY, Vec3::new(4.2, 1.0, 2.2));
        self.mesh(head, MeshShape::Cube, m.hazard);
        self.add(head, Rigidbody::kinematic());
        self.solid(head, 0.9);
        let lip = self.obj("Lip", Some(head), Vec3::new(0.0, -0.52, 0.0), Quat::IDENTITY, Vec3::new(0.94, 0.04, 0.94));
        self.mesh(lip, MeshShape::Cube, m.glow_red);
        let shaft = self.obj("Shaft", Some(id), Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY, Vec3::new(0.7, 9.0, 0.7));
        self.mesh(shaft, MeshShape::Cylinder, m.brushed);
        let (head_ref, shaft_ref) = (ObjectRef::new(&self.scene, head), ObjectRef::new(&self.scene, shaft));
        if let Some(crusher) = self.scene.get_component_mut::<Crusher>(id) {
            crusher.head = head_ref;
            crusher.shaft = shaft_ref;
        }
        let housing = self.obj("Housing", Some(id), Vec3::new(0.0, 10.0, 0.0), Quat::IDENTITY, Vec3::new(5.0, 1.2, 3.0));
        self.mesh(housing, MeshShape::Cube, m.plate_dark);
        self.save(id, "mech/crusher")?;

        // Бампер: колпак светится (его вспышку ведёт скрипт, поэтому у колпака свой цвет).
        let id = self.obj("Bumper", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.6, 0.9, 1.6));
        self.mesh(id, MeshShape::Cylinder, m.rubber);
        self.add(id, Collider::cylinder(0.5, 1.0).with_material(0.2, 0.9));
        self.add(id, Bumper::default());
        let cap = self.obj("Cap", Some(id), Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY, Vec3::new(0.7, 0.08, 0.7));
        self.add(cap, MeshRenderer::cylinder().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(3.0, 1.2, 0.2)));
        self.save(id, "mech/bumper")?;

        // Хрупкая плита.
        let id = self.obj("Tile", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(2.6, 0.45, 2.6));
        self.mesh(id, MeshShape::Cube, m.plate_dark);
        self.add(id, Rigidbody::kinematic());
        self.solid(id, 0.9);
        self.add(id, CollapsingTile::default());
        let frame = self.obj("Frame", Some(id), Vec3::new(0.0, 0.51, 0.0), Quat::IDENTITY, Vec3::new(0.96, 0.03, 0.96));
        self.mesh(frame, MeshShape::Cube, m.glow_amber);
        self.save(id, "mech/tile")?;

        // Ускоряющая плита.
        let id = self.obj("Booster", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Collider { offset: Vec3::new(0.0, 0.8, 0.0), ..Collider::cuboid(5.0, 1.6, 4.0).trigger() });
        self.add(id, Booster::default());
        let pad = self.obj("Pad", Some(id), Vec3::new(0.0, 0.03, 0.0), Quat::IDENTITY, Vec3::new(5.0, 0.06, 4.0));
        self.mesh(pad, MeshShape::Cube, m.hazard);
        let arrows = self.obj("Arrows", Some(id), Vec3::new(0.0, 0.08, 0.0), Quat::IDENTITY, Vec3::new(0.35, 0.02, 2.2));
        self.add(arrows, MeshRenderer::cube().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(0.6, 0.45, 0.05)));
        for (name, side) in [("Wing L", -1.0f32), ("Wing R", 1.0)] {
            let wing = self.obj(name, Some(arrows), Vec3::new(side * 1.0, 0.0, -0.32), Quat::from_rotation_y(side * 0.7), Vec3::new(1.5, 1.0, 0.3));
            self.add(wing, MeshRenderer::cube().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(0.6, 0.45, 0.05)));
        }
        self.save(id, "mech/booster")?;

        // Шестерёнки: обычная и золотая.
        for (rel, golden) in [("pickups/gear", false), ("pickups/gear_golden", true)] {
            let k = if golden { 1.35 } else { 1.0 };
            let id = self.obj(if golden { "Golden Gear" } else { "Gear" }, None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            self.add(id, Collider::sphere(if golden { 0.8 } else { 0.62 }).trigger());
            self.add(id, GearPickup { golden, ..GearPickup::default() });
            let body = self.obj("Model", Some(id), Vec3::ZERO, Quat::IDENTITY, Vec3::splat(k));
            self.model(body, self.gear_mesh, if golden { m.gold } else { m.silver });
            let hub = self.obj("Hub", Some(id), Vec3::ZERO, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), Vec3::new(0.24 * k, 0.2 * k, 0.24 * k));
            self.mesh(hub, MeshShape::Cylinder, if golden { m.glow_amber } else { m.glow_cyan });
            self.save(id, rel)?;
        }

        // Контрольные ворота: лампа красная, пока ворота не пройдены (её красит скрипт).
        let id = self.obj("Checkpoint", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Collider { offset: Vec3::new(0.0, 1.5, 0.0), ..Collider::cuboid(5.0, 3.0, 1.0).trigger() });
        self.add(id, Checkpoint::default());
        for (name, x) in [("Post L", -2.7), ("Post R", 2.7)] {
            let p = self.obj(name, Some(id), Vec3::new(x, 1.6, 0.0), Quat::IDENTITY, Vec3::new(0.3, 3.2, 0.3));
            self.mesh(p, MeshShape::Cylinder, m.brushed);
        }
        let bar = self.obj("Bar", Some(id), Vec3::new(0.0, 3.2, 0.0), Quat::IDENTITY, Vec3::new(5.8, 0.32, 0.32));
        self.mesh(bar, MeshShape::Cube, m.hazard);
        let lamp = self.obj("Lamp", Some(id), Vec3::new(0.0, 3.62, 0.0), Quat::IDENTITY, Vec3::splat(0.5));
        self.add(lamp, MeshRenderer::sphere().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(3.0, 0.4, 0.2)));
        self.save(id, "mech/checkpoint")?;

        // Площадка отгрузки.
        let id = self.obj("Finish", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Collider { offset: Vec3::new(0.0, 2.0, 0.0), ..Collider::cuboid(6.0, 4.0, 6.0).trigger() });
        self.add(id, FinishPad::default());
        let pad = self.obj("Pad", Some(id), Vec3::new(0.0, 0.04, 0.0), Quat::IDENTITY, Vec3::new(5.4, 0.08, 5.4));
        self.mesh(pad, MeshShape::Cylinder, m.glow_green);
        let ring = self.obj("Ring", Some(id), Vec3::new(0.0, 0.1, 0.0), Quat::IDENTITY, Vec3::new(3.7, 0.1, 3.7));
        self.mesh(ring, MeshShape::Cylinder, m.hazard);
        for (side, x) in [("L", -3.0), ("R", 3.0)] {
            let pylon = self.obj(&format!("Pylon {side}"), Some(id), Vec3::new(x, 1.8, 0.0), Quat::IDENTITY, Vec3::new(0.5, 3.6, 0.5));
            self.mesh(pylon, MeshShape::Cylinder, m.brushed);
            let cap = self.obj(&format!("Cap {side}"), Some(id), Vec3::new(x, 3.7, 0.0), Quat::IDENTITY, Vec3::splat(0.6));
            self.mesh(cap, MeshShape::Sphere, m.glow_green);
        }
        let beacon = self.obj("Beacon", Some(id), Vec3::new(0.0, 3.0, 0.0), Quat::IDENTITY, Vec3::splat(0.5));
        self.add(beacon, MeshRenderer::cube().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(0.3, 2.0, 0.8)));
        self.save(id, "mech/finish_pad")?;

        // Лампа-мигалка и столб с лампой.
        let id = self.obj("Lamp", None, Vec3::ZERO, Quat::IDENTITY, Vec3::splat(0.4));
        self.add(id, MeshRenderer::sphere().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(3.0, 1.2, 0.2)));
        self.add(id, Blinker::default());
        self.save(id, "mech/lamp")?;
        let id = self.obj("Lamp Post", None, Vec3::new(0.0, 1.1, 0.0), Quat::IDENTITY, Vec3::new(0.18, 2.2, 0.18));
        self.mesh(id, MeshShape::Cylinder, m.brushed);
        let lamp = self.obj("Lamp", Some(id), Vec3::new(0.0, 0.568, 0.0), Quat::IDENTITY, Vec3::new(2.2, 0.18, 2.2));
        self.add(lamp, MeshRenderer::sphere().with_color(Color::hex(0x151a22)).with_emissive(Color::rgb(3.0, 1.2, 0.2)));
        self.add(lamp, Blinker { rate: 0.5, ..Blinker::default() });
        self.save(id, "mech/lamp_post")?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Шар, декорации, эффекты, сцена-основа
    // ------------------------------------------------------------------

    pub fn ball_and_decor(&mut self) -> Result<(), Box<dyn Error>> {
        let m = self.mats.clone();
        for (rel, real) in [("ball", true), ("showcase_ball", false)] {
            let id = self.obj("Ball", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            self.mesh(id, MeshShape::Sphere, m.steel);
            self.add(id, Rigidbody { ccd: true, angular_damping: 0.05, linear_damping: 0.02, ..Rigidbody::dynamic() });
            self.add(id, Collider::sphere(0.5).with_material(0.8, 0.08).with_density(4.0));
            if real {
                self.scene.get_mut(id).expect("ball").tag = "Ball".into();
                self.add(id, Ball { frozen: true, ..Ball::default() });
            } else {
                self.scene.get_mut(id).expect("ball").name = "Showcase Ball".into();
            }
            // Две яркие полосы-кольца: по ним видно, как шар катится.
            for (name, rot) in [("Band A", Quat::IDENTITY), ("Band B", Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))] {
                let band = self.obj(name, Some(id), Vec3::ZERO, rot, Vec3::new(1.035, 0.24, 1.035));
                self.mesh(band, MeshShape::Cylinder, m.band);
            }
            self.save(id, rel)?;
        }

        // Декорации.
        let id = self.obj("Pillar", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(5.0, 60.0, 5.0));
        self.mesh(id, MeshShape::Cylinder, m.brushed);
        self.save(id, "decor/pillar")?;
        let id = self.obj("Cloud", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(25.0, 4.0, 25.0));
        self.mesh(id, MeshShape::Sphere, m.cloud);
        self.save(id, "decor/cloud")?;
        let id = self.obj("Cloud Sea", None, Vec3::ZERO, Quat::IDENTITY, Vec3::new(1600.0, 1.0, 1600.0));
        self.mesh(id, MeshShape::Plane, m.sea);
        self.save(id, "decor/cloud_sea")?;
        let id = self.obj("Cog", None, Vec3::ZERO, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), Vec3::splat(20.0));
        self.add(id, Cog::default());
        let model = self.obj("Model", Some(id), Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.model(model, self.cog_mesh, m.plate_dark);
        let hub = self.obj("Hub", Some(id), Vec3::ZERO, Quat::IDENTITY, Vec3::new(0.5, 0.12, 0.5));
        self.mesh(hub, MeshShape::Cylinder, m.brushed);
        self.save(id, "decor/cog")?;
        let id = self.obj("Smoke", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, ParticleSystem { emission_rate: 6.0, max_particles: 60, seed: 3, ..ParticleSystem::smoke().scaled(2.0) });
        self.save(id, "decor/smoke")?;

        // Эффекты: разовые, удаляют себя сами.
        for (rel, effect) in [
            ("fx/dust", ParticleSystem::dust().in_3d()),
            ("fx/sparkle", ParticleSystem::pickup_sparkle().in_3d()),
            ("fx/sparkle_big", ParticleSystem::pickup_sparkle().in_3d().scaled(2.0)),
            ("fx/explosion", ParticleSystem::explosion().in_3d().scaled(1.4)),
        ] {
            let id = self.obj("Effect", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            self.add(id, effect);
            self.save(id, rel)?;
        }

        // Основа сцены: камера, солнце, атмосфера, менеджер игры.
        let id = self.obj("Camera", None, Vec3::new(0.0, 8.0, 14.0), Quat::IDENTITY, Vec3::ONE);
        self.scene.get_mut(id).expect("camera").tag = "MainCamera".into();
        self.add(id, Camera { background: Background::Sky, far: 1800.0, ..Camera::perspective(62.0) });
        self.add(id, CameraRig::default());
        self.add(id, AudioListener::default());
        self.save(id, "rig/camera")?;
        let id = self.obj("Sun", None, Vec3::ZERO, Quat::from_euler(EulerRot::YXZ, 0.6, -0.62, 0.0), Vec3::ONE);
        self.add(id, Light { color: Color::rgb(1.0, 0.86, 0.68), ..Light::directional(4.6) });
        self.save(id, "rig/sun")?;
        let id = self.obj("Environment", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Environment { ambient: 0.32, fog_density: 0.0004, exposure: 1.0, bloom: 0.1, vignette: 0.2, shadow_distance: 80.0 });
        self.save(id, "rig/environment")?;
        let id = self.obj("Game", None, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        self.add(id, GameManager::default());
        self.save(id, "rig/game")?;
        Ok(())
    }
}
