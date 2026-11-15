//! Материалы (`assets/materials/*.material`) и префабы (`assets/prefabs/**`): блоки
//! архитектуры (куб 1 × 1 × 1 м с коллайдером — экземпляры растягиваются масштабом,
//! текстура ложится в мировых координатах и не растягивается), реквизит, отметки полосы,
//! маркеры спавнов, игрок, камера меню, солнце и атмосфера, менеджер игры.

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use engine::assets::{Material, Mesh, Texture};
use engine::prelude::*;
use tashbazar::scripts::{
    checkpoint::Checkpoint, game_manager::GameManager, lantern_flicker::LanternFlicker, menu_camera::MenuCamera,
    player_sounds::PlayerSounds, spawn_point::{SpawnPoint, Team},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Материалы по именам.
pub type Mats = HashMap<&'static str, AssetHandle<Material>>;

/// Записать материалы игры.
pub fn write_materials(db: &mut AssetDatabase) -> Result<Mats> {
    let tex = |db: &AssetDatabase, name: &str| db.handle::<Texture>(&format!("textures/{name}.png"));
    let sandstone = tex(db, "sandstone")?;
    let plaster = tex(db, "plaster")?;
    let paving = tex(db, "paving")?;
    let tiles = tex(db, "tiles")?;
    let sand = tex(db, "sand")?;
    let planks = tex(db, "planks")?;
    let metal = tex(db, "metal")?;
    let cloth = tex(db, "cloth")?;
    let cloth_blue = tex(db, "cloth_blue")?;
    let palette = tex(db, "palette")?;
    let flagstone = tex(db, "flagstone")?;
    let cobbles = tex(db, "cobbles")?;
    let louvre = tex(db, "louvre")?;
    let list: Vec<(&'static str, Material)> = vec![
        ("sandstone", Material::new().texture(sandstone).world_tiling(2.0).roughness(0.92)),
        ("sandstone_dark", Material::new().color(Color::rgb(0.66, 0.6, 0.54)).texture(sandstone).world_tiling(2.0).roughness(0.95)),
        ("plaster", Material::new().texture(plaster).world_tiling(4.0).roughness(0.9)),
        ("plaster_ochre", Material::new().color(Color::rgb(0.98, 0.78, 0.55)).texture(plaster).world_tiling(4.0).roughness(0.9)),
        ("plaster_rose", Material::new().color(Color::rgb(0.95, 0.72, 0.62)).texture(plaster).world_tiling(4.0).roughness(0.9)),
        ("roof", Material::new().color(Color::rgb(0.86, 0.8, 0.72)).texture(plaster).world_tiling(4.0).roughness(0.95)),
        ("plaster_blue", Material::new().color(Color::rgb(0.42, 0.62, 0.78)).texture(plaster).world_tiling(4.0).roughness(0.85)),
        ("paving", Material::new().texture(paving).world_tiling(4.0).roughness(0.95)),
        ("tiles", Material::new().texture(tiles).world_tiling(2.0).roughness(0.7)),
        ("sand", Material::new().texture(sand).world_tiling(4.0).roughness(1.0)),
        ("wood", Material::new().texture(planks).world_tiling(2.0).roughness(0.8)),
        ("wood_dark", Material::new().color(Color::rgb(0.55, 0.5, 0.46)).texture(planks).world_tiling(2.0).roughness(0.85)),
        ("metal", Material::new().texture(metal).world_tiling(2.0).metallic(0.45).roughness(0.55)),
        ("cloth_red", Material::new().texture(cloth).world_tiling(2.0).roughness(0.95).double_sided(true)),
        ("cloth_blue", Material::new().texture(cloth_blue).world_tiling(2.0).roughness(0.95).double_sided(true)),
        ("water", Material::new().color(Color::hex(0x2f6f7a)).roughness(0.06).metallic(0.1).shadows(false, true)),
        ("paint_attackers", Material::new().color(Color::hex(0xb8452c)).roughness(0.9).shadows(false, true)),
        ("paint_defenders", Material::new().color(Color::hex(0x2f6aa8)).roughness(0.9).shadows(false, true)),
        ("beacon_cyan", Material::new().color(Color::hex(0x10303a)).emissive(Color::rgb(0.2, 0.9, 1.2), 0.6).roughness(0.4).shadows(false, false)),
        ("beacon_gold", Material::new().color(Color::hex(0x3a2a08)).emissive(Color::rgb(1.4, 0.9, 0.2), 0.6).roughness(0.4).shadows(false, false)),
        ("lamp_glass", Material::new().color(Color::hex(0x3a2510)).emissive(Color::rgb(2.6, 1.6, 0.6), 1.0).roughness(0.3).shadows(false, false)),
        ("kit", Material::new().texture(palette).roughness(0.85)),
        ("iron", Material::new().color(Color::hex(0x2e2c2a)).metallic(0.7).roughness(0.45)),
        ("window_glass", Material::new().color(Color::hex(0x1f2a33)).roughness(0.25).metallic(0.2)),
        ("shutter_wood", Material::new().color(Color::rgb(0.45, 0.62, 0.8)).texture(planks).world_tiling(2.0).roughness(0.85)),
        ("crate", Material::new().texture(tex(db, "crate")?).roughness(0.9)),
        ("cart", Material::new().texture(tex(db, "cart")?).roughness(0.9)),
        ("shutter_louvre", Material::new().color(Color::rgb(0.5, 0.68, 0.86)).texture(louvre).roughness(0.8)),
        ("goods_red", Material::new().color(Color::hex(0xb83a26)).roughness(0.7)),
        ("goods_yellow", Material::new().color(Color::hex(0xd9a52a)).roughness(0.7)),
        ("goods_green", Material::new().color(Color::hex(0x6f8f2e)).roughness(0.7)),
        ("flagstone", Material::new().texture(flagstone).world_tiling(4.0).roughness(0.95)),
        ("cobbles", Material::new().texture(cobbles).world_tiling(2.0).roughness(0.95)),
    ];
    let mut mats = Mats::new();
    for (name, material) in list {
        let handle = db.save_material(&format!("materials/{name}.material"), &material)?;
        mats.insert(name, handle);
    }
    Ok(mats)
}

/// Сцена-«верстак», в которой собираются префабы.
pub struct Lab {
    pub scene: Scene,
    pub dir: PathBuf,
    pub mats: Mats,
    pub models: HashMap<&'static str, AssetHandle<Mesh>>,
    pub saved: Vec<String>,
    db: AssetDatabase,
}

/// Модели набора витрины (`assets/showcase/models`, скопированы в `assets/models/`).
pub const MODELS: [&str; 6] = ["palm", "barrel", "sack", "jar", "well", "plant_pot"];

/// Строительные блоки: (имя префаба, материал, с коллайдером).
pub const BLOCKS: [(&str, &str, bool); 22] = [
    ("sandstone", "sandstone", true),
    ("sandstone_dark", "sandstone_dark", true),
    ("plaster", "plaster", true),
    ("plaster_ochre", "plaster_ochre", true),
    ("plaster_rose", "plaster_rose", true),
    ("plaster_blue", "plaster_blue", true),
    ("paving", "paving", true),
    ("tiles", "tiles", true),
    ("flagstone", "flagstone", true),
    ("cobbles", "cobbles", true),
    ("sand", "sand", true),
    ("wood", "wood", true),
    ("wood_dark", "wood_dark", true),
    ("metal", "metal", true),
    ("water", "water", true),
    ("cloth_red", "cloth_red", false),
    ("cloth_blue", "cloth_blue", false),
    ("trim", "sandstone_dark", false),
    ("roof", "roof", false),
    ("shutter", "plaster_blue", false),
    ("paint_attackers", "paint_attackers", false),
    ("paint_defenders", "paint_defenders", false),
];

impl Lab {
    pub fn new(assets: &Path, db: &AssetDatabase, mats: Mats) -> Result<Self> {
        let mut models = HashMap::new();
        for name in MODELS {
            models.insert(name, db.handle::<Mesh>(&format!("models/{name}.obj"))?);
        }
        Ok(Self { scene: Scene::new("lab"), dir: assets.to_path_buf(), mats, models, saved: Vec::new(), db: db.clone() })
    }

    pub fn mat(&self, name: &str) -> AssetHandle<Material> {
        *self.mats.get(name).unwrap_or_else(|| panic!("material `{name}`"))
    }

    pub fn obj(&mut self, name: &str, parent: Option<GameObjectId>, pos: Vec3, scale: Vec3) -> GameObjectId {
        let builder = self.scene.build(name).at3(pos.x, pos.y, pos.z).scale(scale.x, scale.y, scale.z);
        match parent {
            Some(p) => builder.child_of(p).id(),
            None => builder.id(),
        }
    }

    pub fn add<T: Component>(&mut self, id: GameObjectId, component: T) {
        self.scene.get_mut(id).expect("lab object").add(component);
    }

    fn cube(&mut self, id: GameObjectId, material: &str) {
        let m = self.mat(material);
        self.add(id, MeshRenderer::cube().with_material_asset(m));
    }

    fn model(&mut self, id: GameObjectId, model: &str) {
        let mesh = self.models[model];
        let kit = self.mat("kit");
        self.add(id, MeshRenderer::asset(mesh).with_material_asset(kit));
    }

    /// Записать объект как префаб `prefabs/<rel>.prefab` и убрать из верстака.
    pub fn save(&mut self, id: GameObjectId, rel: &str) -> Result<()> {
        let path = self.dir.join("prefabs").join(format!("{rel}.prefab"));
        std::fs::create_dir_all(path.parent().ok_or("prefab dir")?)?;
        let prefab = Prefab::from_object(&self.scene, id).ok_or("prefab object is gone")?;
        prefab.save(&path)?;
        self.scene.destroy(id);
        self.saved.push(rel.to_string());
        Ok(())
    }

    /// Блоки: куб 1 м с материалом и (у твёрдых) коллайдером того же размера.
    pub fn blocks(&mut self) -> Result<()> {
        for (name, material, solid) in BLOCKS {
            let id = self.obj(name, None, Vec3::ZERO, Vec3::ONE);
            self.cube(id, material);
            if solid {
                self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_material(0.0, 0.0));
            }
            if matches!(name, "cloth_red" | "cloth_blue" | "paint_attackers" | "paint_defenders") {
                if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(id) {
                    r.cast_shadows = name.starts_with("cloth");
                }
            }
            self.save(id, &format!("blocks/{name}"))?;
        }
        Ok(())
    }

    /// Реквизит: ящики (прыжок 0.9 м и прыжок с приседом 1.25 м), бочка, пальма, лоток,
    /// телега, колодец, фонари, мелочи.
    pub fn props(&mut self) -> Result<()> {
        // Ящики — своя модель с атласом (`meshes.rs`): коллайдер ровно по видимому ящику.
        for (name, size) in super::meshes::CRATES {
            let root = self.obj(name, None, Vec3::ZERO, Vec3::ONE);
            self.add(root, Collider { size, offset: Vec3::Y * size.y * 0.5, ..Collider::default() });
            let body = self.obj("Body", Some(root), Vec3::ZERO, Vec3::ONE);
            let mesh = self.db.handle::<Mesh>(&format!("models/{name}.obj"))?;
            let m = self.mat("crate");
            self.add(body, MeshRenderer::asset(mesh).with_material_asset(m));
            self.save(root, &format!("props/{name}"))?;
        }

        // Модели набора витрины: коллайдер — по нижней части самой модели (ствол пальмы,
        // сруб колодца), а не «на глаз».
        for (name, below) in [("barrel", 2.0), ("palm", 1.6), ("well", 0.9), ("plant_pot", 1.0), ("jar", 1.0)] {
            let root = self.obj(name, None, Vec3::ZERO, Vec3::ONE);
            self.model(root, name);
            // Кувшин — обожжённая глина (в палитре набора он почти белый).
            if name == "jar" {
                if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(root) {
                    r.tint = Color::hex(0xc0704a);
                }
            }
            let (lo, hi) = self.model_bounds(name, below)?;
            if name != "jar" {
                self.add(root, Collider { size: hi - lo, offset: (lo + hi) * 0.5, ..Collider::default() });
            }
            self.save(root, &format!("props/{name}"))?;
        }
        // Мешок: у модели низ на 3.7 см ниже начала координат — поднимаем ребёнка.
        let sack = self.obj("sack", None, Vec3::ZERO, Vec3::ONE);
        let (lo, _) = self.model_bounds("sack", 2.0)?;
        let body = self.obj("Sack", Some(sack), Vec3::Y * -lo.y, Vec3::ONE);
        self.model(body, "sack");
        // Мешковина.
        if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(body) {
            r.tint = Color::hex(0xc8ae84);
        }
        self.save(sack, "props/sack")?;

        self.cart()?;
        self.stall()?;
        self.bench()?;
        self.lanterns()?;
        self.awning()?;
        self.drying_line()?;
        Ok(())
    }

    /// Габариты вершин модели ниже высоты `below` над её низом (локально): (min, max).
    fn model_bounds(&self, name: &str, below: f32) -> Result<(Vec3, Vec3)> {
        let mesh = self.db.load_mesh(self.models[name].guid())?;
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let floor = mesh.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        for p in &mesh.positions {
            if p[1] <= floor + below {
                lo = lo.min(Vec3::from_array(*p));
                hi = hi.max(Vec3::from_array(*p));
            }
        }
        Ok((lo, hi))
    }

    /// Деталь из примитива: центр, размер, поворот; `solid` — с коллайдером той же формы.
    #[allow(clippy::too_many_arguments)]
    fn part(&mut self, parent: GameObjectId, name: &str, shape: MeshShape, material: &str, center: Vec3, size: Vec3, rot: Quat, solid: bool) -> GameObjectId {
        let id = self
            .scene
            .build(name)
            .at3(center.x, center.y, center.z)
            .rotation(rot)
            .scale(size.x, size.y, size.z)
            .child_of(parent)
            .id();
        let m = self.mat(material);
        self.add(id, MeshRenderer::shape(shape).with_material_asset(m));
        if solid {
            let collider = match shape {
                MeshShape::Cylinder => Collider { shape: Shape3D::Cylinder, size: Vec3::ONE, ..Collider::default() },
                _ => Collider::cuboid(1.0, 1.0, 1.0),
            };
            self.add(id, collider);
        }
        id
    }

    fn cube_part(&mut self, parent: GameObjectId, name: &str, material: &str, center: Vec3, size: Vec3, solid: bool) -> GameObjectId {
        self.part(parent, name, MeshShape::Cube, material, center, size, Quat::IDENTITY, solid)
    }

    /// Телега: кузов с бортами на оси, два колеса стоят на земле, ось держат подушки,
    /// две оглобли от передка кузова опираются концами на землю. Корень — на земле под осью.
    fn cart(&mut self) -> Result<()> {
        // Модель со своим атласом (`meshes.rs`); коллайдеры — кузов с бортами и колёса.
        let root = self.obj("cart", None, Vec3::ZERO, Vec3::ONE);
        let mesh = self.db.handle::<Mesh>("models/cart.obj")?;
        let m = self.mat("cart");
        let body = self.obj("Body", Some(root), Vec3::ZERO, Vec3::ONE);
        self.add(body, MeshRenderer::asset(mesh).with_material_asset(m));
        self.add(root, Collider { size: Vec3::new(1.8, 0.39, 1.2), offset: Vec3::new(0.0, 0.825, 0.0), ..Collider::default() });
        for z in [-0.69f32, 0.69] {
            let wheel = self.obj("Wheel Collider", Some(root), Vec3::new(0.0, 0.45, z), Vec3::new(0.9, 0.08, 0.9));
            self.scene.get_mut(wheel).expect("wheel").transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            self.add(wheel, Collider { shape: Shape3D::Cylinder, size: Vec3::ONE, ..Collider::default() });
        }
        self.save(root, "props/cart")
    }

    /// Лоток рынка: прилавок, четыре стойки у углов, тент на стойках, лотки с товаром.
    /// Лицом к +Z, спиной к стене; корень — на земле у стены посередине.
    fn stall(&mut self) -> Result<()> {
        let root = self.obj("market_stall", None, Vec3::ZERO, Vec3::ONE);
        let (w, d, h) = (2.4, 0.9, 0.9);
        let back = 0.08;
        self.cube_part(root, "Counter", "wood", Vec3::new(0.0, h * 0.5, back + d * 0.5), Vec3::new(w, h, d), true);
        for (sx, front) in [(-1.0f32, false), (1.0, false), (-1.0, true), (1.0, true)] {
            let px = sx * (w * 0.5 + 0.04);
            let pz = if front { back + d + 0.04 } else { back - 0.04 };
            self.part(root, "Post", MeshShape::Cylinder, "wood_dark", Vec3::new(px, 1.1, pz), Vec3::new(0.08, 2.2, 0.08), Quat::IDENTITY, true);
        }
        self.cube_part(root, "Canopy", "cloth_red", Vec3::new(0.0, 2.22, back + d * 0.5 + 0.1), Vec3::new(w + 0.3, 0.04, d + 0.4), false);
        for (i, x) in [-0.7f32, 0.0, 0.7].iter().enumerate() {
            self.cube_part(root, "Tray", "wood_dark", Vec3::new(*x, h + 0.04, back + 0.45), Vec3::new(0.5, 0.08, 0.4), false);
            let goods = ["goods_red", "goods_yellow", "goods_green"][i];
            self.cube_part(root, "Goods", goods, Vec3::new(*x, h + 0.13, back + 0.45), Vec3::new(0.42, 0.1, 0.32), false);
        }
        self.save(root, "props/market_stall")
    }

    /// Скамья: сиденье на двух ножках, спинка стоит на сиденье.
    fn bench(&mut self) -> Result<()> {
        let root = self.obj("bench", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(root, "Seat", "wood", Vec3::new(0.0, 0.45, 0.0), Vec3::new(1.6, 0.06, 0.4), true);
        for x in [-0.7, 0.7] {
            self.cube_part(root, "Leg", "wood_dark", Vec3::new(x, 0.21, 0.0), Vec3::new(0.08, 0.42, 0.36), true);
        }
        self.cube_part(root, "Back", "wood", Vec3::new(0.0, 0.73, -0.175), Vec3::new(1.6, 0.5, 0.05), true);
        self.save(root, "props/bench")
    }

    /// Фонари: на столбе (опора, столб, кронштейн, крюк, колпак, стекло) и настенный
    /// (пластина на стене, кронштейн, колпак, стекло). У настенного корень — на поверхности
    /// стены, наружу +Z.
    fn lanterns(&mut self) -> Result<()> {
        let post = self.obj("lantern_post", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(post, "Base", "sandstone_dark", Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.32, 0.2, 0.32), true);
        self.part(post, "Pole", MeshShape::Cylinder, "iron", Vec3::new(0.0, 1.45, 0.0), Vec3::new(0.1, 2.5, 0.1), Quat::IDENTITY, true);
        self.cube_part(post, "Arm", "iron", Vec3::new(0.3, 2.65, 0.0), Vec3::new(0.5, 0.06, 0.06), false);
        self.cube_part(post, "Hook", "iron", Vec3::new(0.5, 2.6, 0.0), Vec3::new(0.03, 0.04, 0.03), false);
        self.cube_part(post, "Cap", "iron", Vec3::new(0.5, 2.565, 0.0), Vec3::new(0.22, 0.03, 0.22), false);
        self.cube_part(post, "Glass", "lamp_glass", Vec3::new(0.5, 2.43, 0.0), Vec3::new(0.17, 0.24, 0.17), false);
        let light = self.obj("Light", Some(post), Vec3::new(0.5, 2.3, 0.0), Vec3::ONE);
        self.add(light, Light { shadows: false, ..Light::point(Color::rgb(1.0, 0.68, 0.35), 2.0, 9.0) });
        self.add(light, LanternFlicker { intensity: 2.0, amount: 0.1, speed: 6.0, phase: 0.0 });
        self.save(post, "props/lantern_post")?;

        let wall = self.obj("wall_lantern", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(wall, "Plate", "iron", Vec3::new(0.0, 0.1, 0.01), Vec3::new(0.12, 0.16, 0.02), false);
        self.cube_part(wall, "Bracket", "iron", Vec3::new(0.0, 0.1, 0.17), Vec3::new(0.04, 0.04, 0.3), false);
        self.cube_part(wall, "Cap", "iron", Vec3::new(0.0, 0.065, 0.28), Vec3::new(0.2, 0.03, 0.2), false);
        self.cube_part(wall, "Glass", "lamp_glass", Vec3::new(0.0, -0.07, 0.28), Vec3::new(0.16, 0.24, 0.16), false);
        let light = self.obj("Light", Some(wall), Vec3::new(0.0, -0.1, 0.5), Vec3::ONE);
        self.add(light, Light { shadows: false, ..Light::point(Color::rgb(1.0, 0.66, 0.32), 1.6, 7.0) });
        self.add(light, LanternFlicker { intensity: 1.6, amount: 0.12, speed: 7.5, phase: 1.3 });
        self.save(wall, "props/wall_lantern")
    }

    /// Навес над дверью: рейка на стене, наклонное полотно от рейки, две стойки до земли.
    /// Корень — на земле у стены, наружу +Z; ширина 3 м, вылет 1.4 м.
    fn awning(&mut self) -> Result<()> {
        let root = self.obj("awning", None, Vec3::ZERO, Vec3::ONE);
        let (top, front, out) = (2.65, 2.3, 1.4);
        self.cube_part(root, "Batten", "wood_dark", Vec3::new(0.0, top, 0.04), Vec3::new(3.0, 0.08, 0.08), false);
        // Полотно лежит на рейке и на верхушках стоек.
        let a = Vec3::new(0.0, top + 0.055, 0.04);
        let b = Vec3::new(0.0, front + 0.015, out);
        let len = (b - a).length();
        let tilt = Quat::from_rotation_x(((a.y - b.y) / (b.z - a.z)).atan());
        self.part(root, "Cloth", MeshShape::Cube, "cloth_red", (a + b) * 0.5, Vec3::new(3.1, 0.03, len), tilt, false);
        for x in [-1.45, 1.45] {
            self.part(root, "Pole", MeshShape::Cylinder, "wood_dark", Vec3::new(x, front * 0.5, out), Vec3::new(0.07, front, 0.07), Quat::IDENTITY, true);
        }
        self.save(root, "props/awning")
    }

    /// Сушилка красильщиков: две стойки, верёвка между ними, ткань висит на верёвке.
    fn drying_line(&mut self) -> Result<()> {
        let root = self.obj("drying_line", None, Vec3::ZERO, Vec3::ONE);
        for x in [-1.6, 1.6] {
            self.part(root, "Pole", MeshShape::Cylinder, "wood_dark", Vec3::new(x, 1.25, 0.0), Vec3::new(0.08, 2.5, 0.08), Quat::IDENTITY, true);
        }
        self.cube_part(root, "Rope", "wood_dark", Vec3::new(0.0, 2.4, 0.0), Vec3::new(3.12, 0.025, 0.025), false);
        self.cube_part(root, "Cloth", "cloth_red", Vec3::new(-0.75, 1.9375, 0.0), Vec3::new(1.2, 0.9, 0.02), false);
        self.cube_part(root, "Cloth", "cloth_blue", Vec3::new(0.75, 2.0375, 0.0), Vec3::new(1.1, 0.7, 0.02), false);
        self.save(root, "props/drying_line")
    }

    /// Рама окна (проём `w` × `h`, низ проёма на `y0` над корнем): косяки, перемычка,
    /// подоконник, решётка.
    fn window_frame(&mut self, root: GameObjectId, w: f32, h: f32, y0: f32, bars: bool) {
        let jamb = 0.1;
        let hw = w * 0.5;
        self.cube_part(root, "Lintel", "sandstone_dark", Vec3::new(0.0, y0 + h + 0.06, 0.045), Vec3::new(w + 2.0 * jamb + 0.1, 0.12, 0.09), false);
        for x in [-(hw + jamb * 0.5), hw + jamb * 0.5] {
            self.cube_part(root, "Jamb", "sandstone_dark", Vec3::new(x, y0 + h * 0.5, 0.04), Vec3::new(jamb, h, 0.08), false);
        }
        self.cube_part(root, "Sill", "sandstone_dark", Vec3::new(0.0, y0 - 0.04, 0.07), Vec3::new(w + 2.0 * jamb + 0.16, 0.08, 0.14), false);
        if bars {
            for x in [-w / 6.0, w / 6.0] {
                self.part(root, "Bar", MeshShape::Cylinder, "iron", Vec3::new(x, y0 + h * 0.5, 0.05), Vec3::new(0.02, h, 0.02), Quat::IDENTITY, false);
            }
        }
    }

    /// Створка ставни в проёме окна 0.8 × 1.2: доска с жалюзи (текстура), две петли-полосы
    /// у наружного края. `side` — −1 левая, +1 правая; `open_deg` — 0 закрыта (лежит в
    /// проёме между косяками, не касаясь их), больше — повёрнута наружу на петле.
    fn shutter_leaf(&mut self, root: GameObjectId, side: f32, open_deg: f32) {
        let (w, h, t) = (0.395, 1.19, 0.03);
        // Петля — у наружного края створки, на её лицевой плоскости.
        let hinge = Vec3::new(side * 0.3975, 0.6, 0.042);
        let a = open_deg.to_radians();
        // Направление от петли к свободному краю: закрытая — к середине проёма.
        let dir = Vec3::new(-side * a.cos(), 0.0, a.sin());
        let normal = Vec3::new(side * a.sin(), 0.0, a.cos()); // лицевая сторона
        let rot = Quat::from_rotation_y(-side * a);
        let center = hinge + dir * (w * 0.5) - normal * (t * 0.5);
        let leaf = self.part(root, "Leaf", MeshShape::Cube, "shutter_louvre", center, Vec3::new(w, h, t), rot, false);
        let _ = leaf;
        for y in [0.15, 1.05] {
            let c = hinge + dir * 0.06 + normal * 0.003 + Vec3::new(0.0, y - 0.6, 0.0);
            self.part(root, "Hinge", MeshShape::Cube, "iron", c, Vec3::new(0.12, 0.03, 0.006), rot, false);
        }
    }

    /// Варианты окна — тот же проём 0.8 × 1.2 м, те же рама, подоконник и перемычка, что у
    /// открытого (корень — низ проёма на плоскости стены, наружу +Z): ставни закрыты (обе
    /// створки с жалюзи на весь проём), одна створка открыта, створки приоткрыты, заложено.
    fn window_variants(&mut self) -> Result<()> {
        let glass = |lab: &mut Self, root, w: f32, h: f32, y0: f32| {
            lab.cube_part(root, "Glass", "window_glass", Vec3::new(0.0, y0 + h * 0.5, 0.005), Vec3::new(w, h, 0.01), false);
        };
        let shut = self.obj("window", None, Vec3::ZERO, Vec3::ONE);
        glass(self, shut, 0.8, 1.2, 0.0);
        self.window_frame(shut, 0.8, 1.2, 0.0, false);
        self.shutter_leaf(shut, -1.0, 0.0);
        self.shutter_leaf(shut, 1.0, 0.0);
        self.save(shut, "facade/window_shut")?;

        let half = self.obj("window", None, Vec3::ZERO, Vec3::ONE);
        glass(self, half, 0.8, 1.2, 0.0);
        self.window_frame(half, 0.8, 1.2, 0.0, false);
        self.shutter_leaf(half, 1.0, 0.0);
        // Левая створка распахнута к стене (как у открытого окна).
        self.cube_part(half, "Shutter", "shutter_louvre", Vec3::new(-0.71, 0.6, 0.015), Vec3::new(0.42, 1.16, 0.03), false);
        self.save(half, "facade/window_half")?;

        let ajar = self.obj("window", None, Vec3::ZERO, Vec3::ONE);
        glass(self, ajar, 0.8, 1.2, 0.0);
        self.window_frame(ajar, 0.8, 1.2, 0.0, false);
        self.shutter_leaf(ajar, -1.0, 55.0);
        self.shutter_leaf(ajar, 1.0, 70.0);
        self.save(ajar, "facade/window_ajar")?;

        // Заложенный проём: кладка утоплена на 5 см за косяки, стекла нет.
        let blind = self.obj("window", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(blind, "Infill", "sandstone", Vec3::new(0.0, 0.6, 0.015), Vec3::new(0.8, 1.2, 0.03), false);
        self.window_frame(blind, 0.8, 1.2, 0.0, false);
        self.save(blind, "facade/window_blind")?;

        Ok(())
    }

    /// Окно и дверь фасада: корень на поверхности стены, наружу +Z. Окно: рама и подоконник
    /// выступают, стекло утоплено, ставни распахнуты к стене (корень — у подоконника, высота
    /// проёма 1.2 м). Дверь: коробка с перемычкой, полотно утоплено (2.2 м).
    pub fn facade_parts(&mut self) -> Result<()> {
        let window = self.obj("window", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(window, "Glass", "window_glass", Vec3::new(0.0, 0.6, 0.005), Vec3::new(0.8, 1.2, 0.01), false);
        self.cube_part(window, "Lintel", "sandstone_dark", Vec3::new(0.0, 1.26, 0.045), Vec3::new(1.1, 0.12, 0.09), false);
        for x in [-0.45, 0.45] {
            self.cube_part(window, "Jamb", "sandstone_dark", Vec3::new(x, 0.6, 0.04), Vec3::new(0.1, 1.2, 0.08), false);
        }
        self.cube_part(window, "Sill", "sandstone_dark", Vec3::new(0.0, -0.04, 0.07), Vec3::new(1.16, 0.08, 0.14), false);
        for x in [-0.71, 0.71] {
            self.cube_part(window, "Shutter", "shutter_louvre", Vec3::new(x, 0.6, 0.015), Vec3::new(0.42, 1.16, 0.03), false);
        }
        self.save(window, "facade/window")?;
        self.window_variants()?;

        let door = self.obj("door", None, Vec3::ZERO, Vec3::ONE);
        self.cube_part(door, "Leaf", "wood_dark", Vec3::new(0.0, 1.1, 0.01), Vec3::new(1.26, 2.2, 0.02), false);
        for x in [-0.7, 0.7] {
            self.cube_part(door, "Jamb", "sandstone_dark", Vec3::new(x, 1.15, 0.05), Vec3::new(0.14, 2.3, 0.1), true);
        }
        self.cube_part(door, "Lintel", "sandstone_dark", Vec3::new(0.0, 2.4, 0.06), Vec3::new(1.6, 0.2, 0.12), false);
        self.save(door, "facade/door")
    }

    /// Отметки полосы препятствий: зона + светящийся столбик-маяк (без коллайдера).
    pub fn course(&mut self) -> Result<()> {
        for (name, material, height) in [("checkpoint", "beacon_cyan", 2.2), ("finish", "beacon_gold", 3.0)] {
            let root = self.obj(name, None, Vec3::ZERO, Vec3::ONE);
            let beacon = self.obj("Beacon", Some(root), Vec3::Y * (height * 0.5), Vec3::new(0.18, height, 0.18));
            let m = self.mat(material);
            self.add(beacon, MeshRenderer::cylinder().with_material_asset(m));
            if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(beacon) {
                r.cast_shadows = false;
            }
            let ring = self.obj("Ring", Some(root), Vec3::Y * 0.02, Vec3::new(1.4, 0.03, 1.4));
            self.add(ring, MeshRenderer::cylinder().with_material_asset(m));
            if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(ring) {
                r.cast_shadows = false;
            }
            let reference = ObjectRef::new(&self.scene, beacon);
            self.add(root, Checkpoint { title: String::new(), size: Vec3::new(3.0, 2.5, 3.0), respawn_yaw_deg: 0.0, beacon: reference });
            self.save(root, &format!("course/{name}"))?;
        }
        Ok(())
    }

    /// Простые формы с коллайдером того же размера: цилиндр-бак и диск воды.
    pub fn shapes(&mut self) -> Result<()> {
        let tank = self.obj("tank", None, Vec3::ZERO, Vec3::ONE);
        let stone = self.mat("sandstone");
        self.add(tank, MeshRenderer::cylinder().with_material_asset(stone));
        self.add(tank, Collider { shape: Shape3D::Cylinder, size: Vec3::ONE, ..Collider::default() });
        self.save(tank, "shapes/tank")?;
        let disc = self.obj("water_disc", None, Vec3::ZERO, Vec3::ONE);
        let water = self.mat("water");
        self.add(disc, MeshRenderer::cylinder().with_material_asset(water));
        if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(disc) {
            r.cast_shadows = false;
        }
        self.save(disc, "shapes/water_disc")?;
        Ok(())
    }

    /// Маркеры спавнов: точка + краска на полу; зона карты (название места).
    pub fn markers(&mut self) -> Result<()> {
        let zone = self.obj("zone", None, Vec3::ZERO, Vec3::ONE);
        self.add(zone, tashbazar::scripts::map_zone::MapZone::default());
        self.save(zone, "markers/zone")?;
        for (name, team, paint) in [("spawn_attackers", Team::Attackers, "paint_attackers"), ("spawn_defenders", Team::Defenders, "paint_defenders")] {
            let root = self.obj(name, None, Vec3::ZERO, Vec3::ONE);
            self.add(root, SpawnPoint { team, index: 0 });
            let decal = self.obj("Paint", Some(root), Vec3::Y * 0.004, Vec3::new(0.9, 0.006, 0.9));
            self.cube(decal, paint);
            let arrow = self.obj("Arrow", Some(root), Vec3::new(0.0, 0.004, -0.75), Vec3::new(0.25, 0.006, 0.6));
            self.cube(arrow, paint);
            for child in [decal, arrow] {
                if let Some(r) = self.scene.get_component_mut::<MeshRenderer>(child) {
                    r.cast_shadows = false;
                }
            }
            self.save(root, &format!("markers/{name}"))?;
        }
        Ok(())
    }

    /// Игрок, камера меню, солнце, атмосфера, менеджер игры.
    pub fn rig(&mut self) -> Result<()> {
        let player = self.obj("Player", None, Vec3::ZERO, Vec3::ONE);
        let eyes = self.obj("Eyes", Some(player), Vec3::Y * engine::fps::units(64.0), Vec3::ONE);
        self.add(
            eyes,
            Camera { near: 0.04, far: 900.0, priority: 5, ..Camera::perspective(engine::fps::vertical_fov_deg(90.0)) },
        );
        self.add(eyes, AudioListener::default());
        let camera = ObjectRef::new(&self.scene, eyes);
        self.add(player, FpsController { camera, ..FpsController::default() });
        self.add(player, PlayerSounds::default());
        self.save(player, "rig/player")?;

        let menu = self.obj("Menu Camera", None, Vec3::new(0.0, 30.0, 50.0), Vec3::ONE);
        self.add(menu, Camera { near: 0.1, far: 900.0, priority: 10, ..Camera::perspective(52.0) });
        self.add(menu, MenuCamera::default());
        self.save(menu, "rig/menu_camera")?;

        let sun = self.obj("Sun", None, Vec3::new(0.0, 60.0, 0.0), Vec3::ONE);
        if let Some(object) = self.scene.get_mut(sun) {
            // Солнце высоко с юго-запада: короткие резкие тени, тёплый свет.
            object.transform.rotation = Quat::from_rotation_y(-35f32.to_radians()) * Quat::from_rotation_x(-52f32.to_radians());
        }
        self.add(sun, Light { color: Color::rgb(1.0, 0.9, 0.76), ..Light::directional(3.3) });
        self.save(sun, "rig/sun")?;

        let env = self.obj("Environment", None, Vec3::ZERO, Vec3::ONE);
        self.add(env, Environment { ambient: 0.5, fog_density: 0.0022, exposure: 1.0, bloom: 0.035, vignette: 0.16, shadow_distance: 70.0 });
        self.add(env, SkyColors { enabled: true, zenith: Color::rgb(0.16, 0.36, 0.74), horizon: Color::rgb(0.80, 0.78, 0.70), blend: 0.9 });
        self.add(env, Fog { enabled: true, start: 35.0, height: 0.0, height_falloff: 0.03 });
        // Новая графика движка (docs/api/graphics.md): каскадные мягкие тени — длинные улицы
        // в тени до конца, небо с солнцем и облаками, затенение щелей у стен и ящиков.
        // Пресет качества игрока по-прежнему ограничивает всё это (Low — без теней и AO).
        self.add(env, SoftShadows::default());
        self.add(env, Sky { clouds: 0.3, cloud_speed: 3.0, ..Sky::default() });
        self.add(env, AmbientOcclusion { intensity: 0.5, ..AmbientOcclusion::default() });
        self.add(
            env,
            ColorGrading {
                enabled: true,
                temperature: 0.12,
                tint: 0.0,
                contrast: 1.06,
                saturation: 0.96,
                shadows: Color::rgb(0.02, 0.025, 0.05),
                highlights: Color::rgb(1.0, 0.98, 0.94),
            },
        );
        self.save(env, "rig/environment")?;

        let game = self.obj("Game", None, Vec3::ZERO, Vec3::ONE);
        self.add(game, GameManager::default());
        self.save(game, "rig/game")?;
        Ok(())
    }
}
