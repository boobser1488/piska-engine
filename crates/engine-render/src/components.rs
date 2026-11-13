//! Компоненты рендера: чистые данные, без ссылок на GPU.

use engine_assets::{AssetHandle, Material, Mesh, Texture};
use engine_core::prelude::*;

use crate::primitives::MeshShape;

component! {
    /// Рисует прямоугольный спрайт в плоскости XY объекта.
    pub struct SpriteRenderer {
        /// Цвет-множитель. Без текстуры — просто цвет прямоугольника.
        pub color: Color = Color::WHITE,
        /// Размер в мировых единицах (до масштаба Transform'а).
        pub size: Vec2 = Vec2::ONE,
        pub texture: AssetHandle<Texture> = AssetHandle::none(),
        /// Слой сортировки: больший рисуется поверх.
        pub sorting_layer: i32 = 0,
        /// Порядок внутри слоя.
        pub order: i32 = 0,
        pub flip_x: bool = false,
        pub flip_y: bool = false,
        /// Точка опоры в долях спрайта: (0.5, 0.5) — центр, (0.5, 0) — середина нижней
        /// кромки («ноги» персонажа). Объект стоит в этой точке.
        pub pivot: Vec2 = Vec2::new(0.5, 0.5),
        /// Смещение источника в текстуре в долях (0..1, от левого верхнего угла).
        pub uv_offset: Vec2 = Vec2::ZERO,
        /// Размер источника в текстуре в долях: (1, 1) — вся картинка.
        pub uv_size: Vec2 = Vec2::ONE,
        /// Столбцов в листе спрайтов (сетка внутри источника). 1 — лист не используется.
        pub sheet_columns: u32 = 1,
        /// Строк в листе спрайтов.
        pub sheet_rows: u32 = 1,
        /// Номер кадра листа: слева направо, затем сверху вниз, с нуля.
        pub frame: u32 = 0,
    }
}

impl Component for SpriteRenderer {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl SpriteRenderer {
    /// Лист спрайтов: сетка `columns × rows` одинаковых кадров.
    pub fn sheet(texture: AssetHandle<Texture>, columns: u32, rows: u32) -> Self {
        Self {
            texture,
            sheet_columns: columns.max(1),
            sheet_rows: rows.max(1),
            ..Self::default()
        }
    }

    /// Показать кадр листа (с нуля).
    pub fn with_frame(mut self, frame: u32) -> Self {
        self.frame = frame;
        self
    }

    /// Источник — прямоугольник в пикселях текстуры (`x`, `y` от левого верхнего угла).
    /// Нужны размеры текстуры: `tex_w`, `tex_h`.
    pub fn with_source_pixels(mut self, x: f32, y: f32, w: f32, h: f32, tex_w: f32, tex_h: f32) -> Self {
        self.uv_offset = Vec2::new(x / tex_w, y / tex_h);
        self.uv_size = Vec2::new(w / tex_w, h / tex_h);
        self
    }

    pub fn with_pivot(mut self, x: f32, y: f32) -> Self {
        self.pivot = Vec2::new(x, y);
        self
    }

    /// Всего кадров в листе.
    pub fn frame_count(&self) -> u32 {
        self.sheet_columns.max(1) * self.sheet_rows.max(1)
    }

    pub fn colored(color: Color) -> Self {
        Self {
            color,
            ..Self::default()
        }
    }

    pub fn textured(texture: AssetHandle<Texture>) -> Self {
        Self {
            texture,
            ..Self::default()
        }
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Vec2::new(width, height);
        self
    }

    pub fn with_order(mut self, sorting_layer: i32, order: i32) -> Self {
        self.sorting_layer = sorting_layer;
        self.order = order;
        self
    }
}

reflect_enum! {
    pub enum Projection { Orthographic, Perspective }
}

reflect_enum! {
    /// Чем заполнять кадр позади объектов.
    pub enum Background { SolidColor, Sky }
}

component! {
    /// Камера. Рисует та активная камера, у которой наибольший `priority`.
    pub struct Camera {
        pub projection: Projection = Projection::Orthographic,
        /// Половина высоты видимой области в мировых единицах (для ортографической).
        pub ortho_size: f32 = 5.0,
        /// Вертикальный угол обзора в градусах (для перспективной).
        pub fov_deg: f32 = 60.0,
        pub near: f32 = 0.1,
        pub far: f32 = 1000.0,
        pub clear_color: Color = Color::hex(0x1b2433),
        /// `Sky` — процедурное небо с солнцем вместо заливки `clear_color`.
        pub background: Background = Background::SolidColor,
        pub priority: i32 = 0,
        /// Битовая маска слоёв GameObject'ов, которые видит камера.
        pub culling_mask: u32 = u32::MAX,
    }
}

impl Component for Camera {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Camera {
    pub fn orthographic(ortho_size: f32) -> Self {
        Self {
            ortho_size,
            ..Self::default()
        }
    }

    /// Перспективная камера для 3D — сразу с небом.
    pub fn perspective(fov_deg: f32) -> Self {
        Self {
            projection: Projection::Perspective,
            fov_deg,
            background: Background::Sky,
            ..Self::default()
        }
    }

    /// Матрица «мир → clip-пространство» для камеры с мировой матрицей `world`.
    pub fn view_projection(&self, world: Mat4, aspect: f32) -> Mat4 {
        // Масштаб объекта-камеры не должен влиять на картинку.
        let (_, rotation, position) = world.to_scale_rotation_translation();
        let view = Mat4::from_rotation_translation(rotation, position).inverse();
        let projection = match self.projection {
            Projection::Orthographic => {
                let half_h = self.ortho_size.max(1e-4);
                let half_w = half_h * aspect;
                // Симметричный диапазон глубины: 2D-камера может стоять в z = 0.
                Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, -self.far, self.far)
            }
            Projection::Perspective => Mat4::perspective_rh(
                self.fov_deg.to_radians(),
                aspect,
                self.near.max(1e-4),
                self.far,
            ),
        };
        projection * view
    }
}

component! {
    /// Рисует 3D-меш с PBR-материалом (metallic-roughness): ассетами `.material` по
    /// частям меша (`materials`) или встроенным материалом (поля от `color` до `grid`).
    pub struct MeshRenderer {
        /// Встроенная форма. Не используется, если назначен `mesh`.
        pub shape: MeshShape = MeshShape::Cube,
        /// Меш из файла `.obj`, `.gltf` или `.glb`.
        pub mesh: AssetHandle<Mesh> = AssetHandle::none(),
        /// Базовый цвет; умножается на текстуру.
        pub color: Color = Color::hex(0xc8c8c8),
        pub texture: AssetHandle<Texture> = AssetHandle::none(),
        /// 0 — диэлектрик (пластик, дерево), 1 — металл.
        pub metallic: f32 = 0.0,
        /// 0 — зеркало, 1 — матовая поверхность.
        pub roughness: f32 = 0.6,
        /// Собственное свечение (добавляется поверх освещения; ярче единицы — даёт блум).
        pub emissive: Color = Color::BLACK,
        /// Шаг повторения текстуры в метрах: она проецируется в мировых координатах,
        /// и стены любого размера получают одинаковый масштаб. 0 — UV меша.
        pub tiling: f32 = 0.0,
        /// Сетка в метрах поверх материала — для прототипов уровней.
        pub grid: bool = false,
        pub cast_shadows: bool = true,
        /// Материалы (ассеты `.material`) по частям меша: первый — первой части и т. д.;
        /// частям без своего слота достаётся последний. Пусто — встроенный материал выше.
        pub materials: Vec<AssetHandle<Material>> = Vec::new(),
        /// Без `materials` брать материалы из файла модели (glTF, `.mtl`), а не встроенный.
        pub model_materials: bool = false,
        /// Оттенок именно этого объекта: умножается на базовый цвет материала. Не мешает
        /// рисовать объекты с одним материалом одной пачкой.
        pub tint: Color = Color::WHITE,
    }
}

impl Component for MeshRenderer {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl MeshRenderer {
    pub fn shape(shape: MeshShape) -> Self {
        Self {
            shape,
            ..Self::default()
        }
    }

    pub fn cube() -> Self {
        Self::shape(MeshShape::Cube)
    }

    pub fn sphere() -> Self {
        Self::shape(MeshShape::Sphere)
    }

    pub fn plane() -> Self {
        Self::shape(MeshShape::Plane)
    }

    pub fn cylinder() -> Self {
        Self::shape(MeshShape::Cylinder)
    }

    /// Меш из файла: `MeshRenderer::asset(assets.handle("models/ship.glb")?)`.
    pub fn asset(mesh: AssetHandle<Mesh>) -> Self {
        Self {
            mesh,
            ..Self::default()
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Материал одной строкой: `.with_material(1.0, 0.25)` — полированный металл.
    pub fn with_material(mut self, metallic: f32, roughness: f32) -> Self {
        self.metallic = metallic;
        self.roughness = roughness;
        self
    }

    pub fn with_emissive(mut self, emissive: Color) -> Self {
        self.emissive = emissive;
        self
    }

    /// Текстура с повторением каждые `tile` метров (0 — по UV меша).
    pub fn with_texture(mut self, texture: AssetHandle<Texture>, tile: f32) -> Self {
        self.texture = texture;
        self.tiling = tile;
        self
    }

    /// Прототипный материал: сетка в метрах, как в редакторах уровней.
    pub fn with_grid(mut self) -> Self {
        self.grid = true;
        self
    }

    pub fn without_shadows(mut self) -> Self {
        self.cast_shadows = false;
        self
    }

    /// Модель из файла со своими материалами (glTF, `.mtl`): каждая часть — своим.
    pub fn model(mesh: AssetHandle<Mesh>) -> Self {
        Self {
            mesh,
            model_materials: true,
            ..Self::default()
        }
    }

    /// Один материал-ассет на весь меш: `MeshRenderer::sphere().with_material_asset(gold)`.
    pub fn with_material_asset(mut self, material: AssetHandle<Material>) -> Self {
        self.materials = vec![material];
        self
    }

    /// Материалы по частям меша (порядок — как у частей модели).
    pub fn with_materials(mut self, materials: Vec<AssetHandle<Material>>) -> Self {
        self.materials = materials;
        self
    }

    /// Оттенок этого объекта поверх материала (пачки не разбиваются).
    pub fn with_tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    /// Перекрасить объект: с материалами-ассетами меняется оттенок `tint` (материал общий,
    /// другие объекты не затрагиваются), со встроенным материалом — `color`.
    pub fn set_color(&mut self, color: Color) {
        if self.uses_material_assets() {
            self.tint = color;
        } else {
            self.color = color;
        }
    }

    /// Рисуется ли объект материалами-ассетами или материалами модели, а не встроенным.
    pub fn uses_material_assets(&self) -> bool {
        !self.materials.is_empty() || (self.model_materials && !self.mesh.is_none())
    }
}

reflect_enum! {
    pub enum LightKind { Directional, Point }
}

component! {
    /// Источник света. Направленный светит вдоль оси «вперёд» объекта (−Z) и не
    /// зависит от позиции; точечный светит во все стороны из позиции объекта.
    pub struct Light {
        pub kind: LightKind = LightKind::Directional,
        /// Тёплый белый по умолчанию — цвет дневного солнца.
        pub color: Color = Color::rgb(1.0, 0.95, 0.88),
        pub intensity: f32 = 2.4,
        /// Дальность точечного источника: на этом расстоянии свет гаснет до нуля.
        pub range: f32 = 10.0,
        /// Отбрасывает ли тени (только первый направленный источник сцены).
        pub shadows: bool = true,
    }
}

impl Component for Light {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Light {
    /// Солнце. Поверните объект, чтобы задать направление лучей.
    pub fn directional(intensity: f32) -> Self {
        Self {
            intensity,
            ..Self::default()
        }
    }

    pub fn point(color: Color, intensity: f32, range: f32) -> Self {
        Self {
            kind: LightKind::Point,
            color,
            intensity,
            range,
            ..Self::default()
        }
    }
}

component! {
    /// Атмосфера и сведение кадра — одни на сцену (Unity: окно Lighting + Volume).
    /// Без этого компонента 3D-сцена использует значения по умолчанию, а 2D-сцена
    /// выводится без пост-обработки.
    pub struct Environment {
        /// Сила рассеянного света от неба.
        pub ambient: f32 = 0.35,
        /// Плотность тумана: даль растворяется в цвет неба у горизонта. 0 — без тумана.
        pub fog_density: f32 = 0.004,
        pub exposure: f32 = 1.0,
        /// Сила свечения вокруг ярких участков. 0 — без блума.
        pub bloom: f32 = 0.06,
        /// Затемнение к углам кадра.
        pub vignette: f32 = 0.18,
        /// Радиус области вокруг камеры, где солнце отбрасывает тени (в больших сценах).
        pub shadow_distance: f32 = 60.0,
    }
}

impl Component for Environment {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}
