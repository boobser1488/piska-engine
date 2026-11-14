//! Материалы на GPU.
//!
//! Материал делится на две части. То, что меняется от объекта к объекту без смены
//! текстур (цвет × оттенок, металл, шероховатость, свечение, тайлинг в мире), едет в
//! данных экземпляра — как у встроенного материала `MeshRenderer`. Текстуры (база,
//! metallic-roughness, нормали, свечение, AO) и общие параметры (UV-тайлинг, сила карты
//! нормалей и AO, порог выреза, флаги) лежат в группе привязок материала. Одинаковые
//! группы делятся: материалы с одинаковыми текстурами и общими параметрами рисуются
//! одной пачкой, сколько бы цветов у них ни было. Встроенный материал — та же группа
//! с одной базовой текстурой и параметрами «как раньше».
//!
//! Конвейеров четыре (непрозрачный / смешивание × отсечение обратных граней / без),
//! для теней — три (обычный, двусторонний, с вырезом по альфе).

use std::collections::{HashMap, HashSet};

use engine_assets::{AlphaMode, AssetDatabase, Guid, Material, Materials};
use engine_core::prelude::*;
use wgpu::util::DeviceExt;

use super::frame::{MeshBatch, MeshItem, RecentCache, WHOLE_MESH};
use super::{GpuMesh, GpuTexture, MeshKey, Renderer};
use crate::components::MeshRenderer;

/// Параметры материала в uniform-буфере группы (см. `MaterialParams` в mesh.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct MaterialUniform {
    uv: [f32; 4],
    factors: [f32; 4],
    flags: [f32; 4],
}

impl MaterialUniform {
    /// Встроенный материал `MeshRenderer`: всё как до появления ассетов.
    pub const LEGACY: Self = Self {
        uv: [1.0, 1.0, 0.0, 0.0],
        factors: [0.0, 1.0, 0.5, 0.0],
        flags: [0.0, 1.0, 0.0, 0.0],
    };

    pub fn of(material: &Material) -> Self {
        let has_normals = !material.normal_texture.is_none();
        Self {
            uv: [material.uv_scale.x, material.uv_scale.y, material.uv_offset.x, material.uv_offset.y],
            factors: [
                if has_normals { material.normal_strength.max(0.0) } else { 0.0 },
                if material.occlusion_texture.is_none() { 0.0 } else { material.occlusion_strength.clamp(0.0, 1.0) },
                material.alpha_cutoff,
                0.0,
            ],
            flags: [
                flag(material.unlit),
                flag(material.receive_shadows),
                flag(material.double_sided),
                match material.alpha_mode {
                    AlphaMode::Opaque => 0.0,
                    AlphaMode::Cutout => 1.0,
                    AlphaMode::Blend => 2.0,
                },
            ],
        }
    }

    fn bits(&self) -> [u32; 12] {
        bytemuck::cast(*self)
    }
}

fn flag(value: bool) -> f32 {
    if value {
        1.0
    } else {
        0.0
    }
}

/// Конвейер основного прохода: 0 — непрозрачный, 1 — непрозрачный двусторонний,
/// 2 — смешивание, 3 — смешивание двустороннее.
pub(super) const PASS_OPAQUE: u8 = 0;
pub(super) const PASS_BLEND: u8 = 2;
/// Конвейер теней: обычный, двусторонний, с вырезом по альфе (нужна группа материала).
pub(super) const SHADOW_OPAQUE: u8 = 0;
pub(super) const SHADOW_TWO_SIDED: u8 = 1;
pub(super) const SHADOW_CUTOUT: u8 = 2;

/// Чем рисовать часть меша: группа привязок, конвейеры и данные экземпляра.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Resolved {
    pub bind: u32,
    pub pass: u8,
    pub shadow: u8,
    pub base_color: [f32; 4],
    pub emissive: [f32; 4],
    pub params: [f32; 4],
    pub casts_shadows: bool,
}

/// Откуда материал: ассет `.material` (или материал из кода в [`Materials`]) либо
/// материал из файла модели (номер в её списке).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum MaterialSource {
    Asset(Guid),
    Model(Guid, u32),
}

/// Ключ группы привязок: текстуры слотов и параметры.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BindKey {
    textures: [Option<Guid>; 5],
    uniform: [u32; 12],
}

struct Cached {
    resolved: Resolved,
    /// Номер изменения в [`Materials`], с которого взят материал (`None` — из файла).
    revision: Option<u64>,
}

/// Конвейеры, раскладка и кэши материалов; живёт в [`Renderer`].
pub(super) struct MeshPass {
    pub layout: wgpu::BindGroupLayout,
    /// По номеру `pass` (см. `PASS_*`).
    pub pipelines: [wgpu::RenderPipeline; 4],
    pub shadow_two_sided: wgpu::RenderPipeline,
    pub shadow_cutout: wgpu::RenderPipeline,
    pub binds: Vec<wgpu::BindGroup>,
    bind_ids: HashMap<BindKey, u32>,
    materials: HashMap<MaterialSource, Cached>,
    /// Материалы файлов моделей (по GUID модели).
    model_materials: HashMap<Guid, Vec<Material>>,
    /// Текстуры с линейными данными (нормали, metallic-roughness, AO) — без перевода из sRGB.
    linear: HashMap<Guid, GpuTexture>,
    /// О неудачах пишем в лог один раз.
    failed: HashSet<MaterialSource>,
}

/// Атрибуты экземпляра для теней с вырезом: матрица и базовый цвет (его альфа).
/// Смещения — как в `MeshInstance`: базовый цвет идёт после матрицы модели (16 чисел)
/// и матрицы нормалей (12 чисел).
pub(super) const INSTANCE_CUTOUT_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    [column(3, 0), column(4, 4), column(5, 8), column(6, 12), column(10, 28)];

const fn column(location: u32, floats: u64) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: floats * 4, shader_location: location }
}

/// Касательные — отдельный вершинный буфер (слот 2): раскладка основных вершин не меняется.
pub(super) const TANGENT_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![13 => Float32x4];

/// Что нужно для сборки конвейеров мешей.
pub(super) struct PipelineInputs<'a> {
    pub device: &'a wgpu::Device,
    pub mesh_shader: &'a wgpu::ShaderModule,
    pub sky_shader: &'a wgpu::ShaderModule,
    pub lit_layout: &'a wgpu::BindGroupLayout,
    pub globals_layout: &'a wgpu::BindGroupLayout,
    pub vertex: wgpu::VertexBufferLayout<'a>,
    pub instance: wgpu::VertexBufferLayout<'a>,
    pub instance_model: wgpu::VertexBufferLayout<'a>,
    pub instance_cutout: wgpu::VertexBufferLayout<'a>,
    pub hdr_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub samples: u32,
}

impl MeshPass {
    pub fn new(inputs: PipelineInputs) -> Self {
        let device = inputs.device;
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(2),
                texture(3),
                texture(4),
                texture(5),
                texture(6),
            ],
        });
        let tangents = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TANGENT_ATTRIBUTES,
        };
        let lit = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[inputs.lit_layout, &layout],
            push_constant_ranges: &[],
        });
        let multisample = wgpu::MultisampleState { count: inputs.samples, ..wgpu::MultisampleState::default() };
        let main = |label: &'static str, blend: bool, two_sided: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&lit),
                vertex: wgpu::VertexState {
                    module: inputs.mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[inputs.vertex.clone(), inputs.instance.clone(), tangents.clone()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: inputs.mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: inputs.hdr_format,
                        blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!two_sided).then_some(wgpu::Face::Back),
                    ..wgpu::PrimitiveState::default()
                },
                // Полупрозрачные проверяют глубину, но не пишут её: позади видно всё.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: inputs.depth_format,
                    depth_write_enabled: !blend,
                    // Обратная глубина (depth.rs): ближе — больше.
                    depth_compare: wgpu::CompareFunction::Greater,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample,
                multiview: None,
                cache: None,
            })
        };
        let pipelines = [
            main("mesh pipeline", false, false),
            main("two-sided mesh pipeline", false, true),
            main("transparent mesh pipeline", true, false),
            main("two-sided transparent mesh pipeline", true, true),
        ];

        let shadow_depth = wgpu::DepthStencilState {
            format: inputs.depth_format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState { constant: 0, slope_scale: 1.5, clamp: 0.0 },
        };
        let unlit = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("two-sided shadow pipeline layout"),
            bind_group_layouts: &[inputs.globals_layout],
            push_constant_ranges: &[],
        });
        let shadow_two_sided = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("two-sided shadow pipeline"),
            layout: Some(&unlit),
            vertex: wgpu::VertexState {
                module: inputs.sky_shader,
                entry_point: Some("vs_shadow"),
                buffers: &[inputs.vertex.clone(), inputs.instance_model.clone()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(shadow_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let cutout_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cutout shadow pipeline layout"),
            bind_group_layouts: &[inputs.globals_layout, &layout],
            push_constant_ranges: &[],
        });
        let shadow_cutout = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cutout shadow pipeline"),
            layout: Some(&cutout_layout),
            vertex: wgpu::VertexState {
                module: inputs.mesh_shader,
                entry_point: Some("vs_shadow_cutout"),
                buffers: &[inputs.vertex.clone(), inputs.instance_cutout.clone()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: inputs.mesh_shader,
                entry_point: Some("fs_shadow_cutout"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            // Листва и решётки обычно двусторонние: тень от обеих сторон.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(shadow_depth),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            layout,
            pipelines,
            shadow_two_sided,
            shadow_cutout,
            binds: Vec::new(),
            bind_ids: HashMap::new(),
            materials: HashMap::new(),
            model_materials: HashMap::new(),
            linear: HashMap::new(),
            failed: HashSet::new(),
        }
    }

    /// Забыть всё, что прочитано с диска (ассеты изменились).
    pub fn clear(&mut self) {
        self.binds.clear();
        self.bind_ids.clear();
        self.materials.clear();
        self.model_materials.clear();
        self.linear.clear();
        self.failed.clear();
    }

    /// Сколько групп привязок материалов сейчас на GPU.
    pub fn bind_count(&self) -> usize {
        self.binds.len()
    }
}

impl Renderer {
    /// Забыть один материал (файл `.material` изменился): в следующем кадре он будет
    /// прочитан заново. Текстуры остаются в кэше. Редактор вызывает это после сохранения.
    pub fn reload_material(&mut self, guid: Guid) {
        self.mesh_pass.materials.remove(&MaterialSource::Asset(guid));
        self.mesh_pass.failed.remove(&MaterialSource::Asset(guid));
    }

    /// Сколько разных групп привязок материалов создано (каждая — отдельная пачка).
    pub fn material_bind_groups(&self) -> usize {
        self.mesh_pass.bind_count()
    }

    /// Группа привязок встроенного материала `MeshRenderer` с текстурой `texture`.
    pub(super) fn legacy_bind(&mut self, texture: Option<Guid>, assets: Option<&AssetDatabase>) -> u32 {
        self.bind_for(
            BindKey { textures: [texture, None, None, None, None], uniform: MaterialUniform::LEGACY.bits() },
            MaterialUniform::LEGACY,
            assets,
        )
    }

    /// Материал источника: из кэша или загрузить. `None` — загрузить не удалось.
    pub(super) fn resolve_material(
        &mut self,
        source: MaterialSource,
        assets: Option<&AssetDatabase>,
        runtime: Option<&Materials>,
    ) -> Option<Resolved> {
        let runtime_entry = match source {
            MaterialSource::Asset(guid) => runtime.and_then(|r| r.entry(guid)),
            MaterialSource::Model(..) => None,
        };
        if let Some(cached) = self.mesh_pass.materials.get(&source) {
            if cached.revision == runtime_entry.map(|(_, revision)| revision) {
                return Some(cached.resolved);
            }
        }
        if self.mesh_pass.failed.contains(&source) && runtime_entry.is_none() {
            return None;
        }
        let loaded: Result<Material, String> = match (source, runtime_entry) {
            (_, Some((material, _))) => Ok(material.clone()),
            (MaterialSource::Asset(guid), None) => assets
                .ok_or_else(|| "no AssetDatabase in world.resources".to_string())
                .and_then(|db| db.load_material(guid).map_err(|e| e.to_string())),
            (MaterialSource::Model(mesh, index), None) => {
                self.mesh_pass
                    .model_materials
                    .entry(mesh)
                    .or_insert_with(|| assets.map(|db| db.model_materials(mesh).unwrap_or_default()).unwrap_or_default())
                    .get(index as usize)
                    .cloned()
                    .ok_or_else(|| format!("the model has no material #{index}"))
            }
        };
        let material = match loaded {
            Ok(material) => material,
            Err(reason) => {
                if self.mesh_pass.failed.insert(source) {
                    let name = match source {
                        MaterialSource::Asset(guid) => assets
                            .and_then(|db| db.relative_path_of(guid))
                            .map_or_else(|| format!("material {guid}"), |p| format!("`{p}`")),
                        MaterialSource::Model(mesh, index) => format!("material #{index} of model {mesh}"),
                    };
                    log::error!("{name}: {reason}; drawing it magenta");
                }
                return None;
            }
        };
        let uniform = MaterialUniform::of(&material);
        let bind = self.bind_for(
            BindKey {
                textures: [
                    texture_slot(material.base_color_texture),
                    texture_slot(material.metallic_roughness_texture),
                    texture_slot(material.normal_texture),
                    texture_slot(material.emissive_texture),
                    texture_slot(material.occlusion_texture),
                ],
                uniform: uniform.bits(),
            },
            uniform,
            assets,
        );
        let blend = material.alpha_mode == AlphaMode::Blend;
        let resolved = Resolved {
            bind,
            pass: if blend { PASS_BLEND } else { PASS_OPAQUE } + u8::from(material.double_sided),
            shadow: match (material.alpha_mode, material.double_sided) {
                (AlphaMode::Opaque, false) => SHADOW_OPAQUE,
                (AlphaMode::Opaque, true) => SHADOW_TWO_SIDED,
                _ => SHADOW_CUTOUT,
            },
            base_color: material.base_color.to_array(),
            emissive: [
                material.emissive.r * material.emissive_intensity,
                material.emissive.g * material.emissive_intensity,
                material.emissive.b * material.emissive_intensity,
                1.0,
            ],
            params: [material.metallic, material.roughness, material.world_tiling.max(0.0), flag(material.grid)],
            casts_shadows: material.cast_shadows,
        };
        self.mesh_pass.materials.insert(source, Cached { resolved, revision: runtime_entry.map(|(_, r)| r) });
        Some(resolved)
    }

    /// Группа привязок по ключу: найти готовую или собрать (загрузив текстуры).
    fn bind_for(&mut self, key: BindKey, uniform: MaterialUniform, assets: Option<&AssetDatabase>) -> u32 {
        if let Some(id) = self.mesh_pass.bind_ids.get(&key) {
            return *id;
        }
        let [base, surface, normal, emissive, occlusion] = key.textures;
        for guid in [base, emissive].into_iter().flatten() {
            self.ensure_texture(guid, assets);
        }
        for guid in [surface, normal, occlusion].into_iter().flatten() {
            self.ensure_linear_texture(guid, assets);
        }
        let srgb = |guid: Option<Guid>| -> &GpuTexture {
            match guid {
                None => &self.white,
                Some(guid) => self.textures.get(&guid).unwrap_or(&self.missing),
            }
        };
        let linear = |guid: Option<Guid>| -> &GpuTexture {
            match guid {
                None => &self.white,
                Some(guid) => self.mesh_pass.linear.get(&guid).unwrap_or(&self.white),
            }
        };
        // Пиксель-арт остаётся резким, как и у встроенного материала.
        let sampler = if srgb(base).pixel {
            &self.sampler_tiled_pixel
        } else if srgb(base).nearest {
            &self.sampler_tiled_nearest
        } else if srgb(base).atlas {
            &self.sampler_atlas
        } else {
            &self.sampler_tiled
        };
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material params"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        fn view(texture: &GpuTexture) -> wgpu::BindingResource<'_> {
            wgpu::BindingResource::TextureView(&texture.view)
        }
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.mesh_pass.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: view(srgb(base)) },
                wgpu::BindGroupEntry { binding: 3, resource: view(linear(surface)) },
                wgpu::BindGroupEntry { binding: 4, resource: view(linear(normal)) },
                wgpu::BindGroupEntry { binding: 5, resource: view(srgb(emissive)) },
                wgpu::BindGroupEntry { binding: 6, resource: view(linear(occlusion)) },
            ],
        });
        let id = self.mesh_pass.binds.len() as u32;
        self.mesh_pass.binds.push(bind_group);
        self.mesh_pass.bind_ids.insert(key, id);
        id
    }

    /// Загрузить текстуру с линейными данными (без перевода из sRGB), если её ещё нет.
    fn ensure_linear_texture(&mut self, guid: Guid, assets: Option<&AssetDatabase>) {
        if self.mesh_pass.linear.contains_key(&guid) {
            return;
        }
        let decoded = assets
            .ok_or_else(|| "no AssetDatabase in world.resources".to_string())
            .and_then(|db| {
                let bytes = db.read(guid).map_err(|e| e.to_string())?;
                image::load_from_memory(&bytes).map_err(|e| format!("cannot decode `{}`: {e}", db.relative_path_of(guid).unwrap_or("?")))
            });
        match decoded {
            Ok(image) => {
                let texture = super::upload_texture_as(
                    &self.device,
                    &self.queue,
                    &self.texture_layout,
                    &self.sampler_linear,
                    &image.to_rgba8(),
                    true,
                    wgpu::TextureFormat::Rgba8Unorm,
                );
                self.mesh_pass.linear.insert(guid, texture);
            }
            Err(reason) => {
                // Без карты поверхность остаётся плоской / без AO — не пурпурной.
                log::error!("texture {guid}: {reason}");
                let white = super::upload_texture_as(
                    &self.device,
                    &self.queue,
                    &self.texture_layout,
                    &self.sampler_nearest,
                    &image::RgbaImage::from_raw(1, 1, vec![255; 4]).expect("size matches"),
                    false,
                    wgpu::TextureFormat::Rgba8Unorm,
                );
                self.mesh_pass.linear.insert(guid, white);
            }
        }
    }
}

/// Объект с `MeshRenderer` на материалах-ассетах — то, что знает о нём извлечение кадра.
pub(super) struct MaterialObject<'a> {
    pub mesh: &'a MeshRenderer,
    pub key: MeshKey,
    pub mesh_id: u32,
    pub matrix: Mat4,
    pub visible: bool,
    pub world_bounds: (Vec3, Vec3),
}

/// Кэши кадра: без них каждый объект искал бы материал в хеш-таблицах.
#[derive(Default)]
pub(super) struct FrameMaterials {
    pub resolved: RecentCache<MaterialSource, Option<Resolved>>,
    pub legacy: RecentCache<Option<Guid>, u32>,
}

impl Renderer {
    /// Встроенный материал объекта (через кэш кадра).
    pub(super) fn cached_legacy_bind(&mut self, cache: &mut FrameMaterials, texture: Option<Guid>, assets: Option<&AssetDatabase>) -> u32 {
        if let Some(bind) = cache.legacy.get(&texture) {
            return bind;
        }
        let bind = self.legacy_bind(texture, assets);
        cache.legacy.put(texture, bind);
        bind
    }

    /// Материал части `part` объекта: слот `materials` (последний — для частей без
    /// своего слота), иначе материал файла модели, иначе встроенный (`None`).
    fn part_material(
        &mut self,
        cache: &mut FrameMaterials,
        object: &MaterialObject,
        part: usize,
        assets: Option<&AssetDatabase>,
        runtime: Option<&Materials>,
    ) -> Option<Resolved> {
        let mesh = object.mesh;
        let slot = mesh.materials.get(part).or(mesh.materials.last()).filter(|h| !h.is_none());
        let model_index = match object.key {
            MeshKey::Asset(guid) if mesh.uses_material_assets() => self
                .meshes
                .get(&object.key)
                .and_then(|gpu| gpu.parts.get(part))
                .and_then(|(_, material)| *material)
                .map(|index| (guid, index)),
            _ => None,
        };
        let source = match (slot, model_index) {
            (Some(handle), _) => MaterialSource::Asset(handle.guid()),
            (None, Some((guid, index))) => MaterialSource::Model(guid, index),
            (None, None) => return None,
        };
        let resolved = match cache.resolved.get(&source) {
            Some(resolved) => resolved,
            None => {
                let resolved = self.resolve_material(source, assets, runtime);
                cache.resolved.put(source, resolved);
                resolved
            }
        };
        Some(resolved.unwrap_or_else(|| missing_material(self.cached_legacy_bind(cache, None, assets))))
    }

    /// Встроенный материал как [`Resolved`].
    pub(super) fn inline_material(&mut self, cache: &mut FrameMaterials, mesh: &MeshRenderer, assets: Option<&AssetDatabase>) -> Resolved {
        // Без своей текстуры модель Piska Modeler (`.pmodel`) рисуется атласом из файла.
        let texture = (!mesh.texture.is_none()).then(|| mesh.texture.guid()).or_else(|| {
            let model = mesh.mesh.guid();
            (!mesh.mesh.is_none() && assets.is_some_and(|a| a.has_embedded_texture(model))).then_some(model)
        });
        Resolved {
            bind: self.cached_legacy_bind(cache, texture, assets),
            pass: PASS_OPAQUE,
            shadow: SHADOW_OPAQUE,
            base_color: mesh.color.to_array(),
            emissive: mesh.emissive.to_array(),
            params: [mesh.metallic, mesh.roughness, mesh.tiling.max(0.0), flag(mesh.grid)],
            casts_shadows: true,
        }
    }

    /// Добавить в кадр части объекта, рисуемого материалами-ассетами. Если все части
    /// получились с одним материалом, объект рисуется целиком одним вызовом.
    pub(super) fn push_material_parts(
        &mut self,
        items: &mut Vec<MeshItem>,
        cache: &mut FrameMaterials,
        object: MaterialObject,
        assets: Option<&AssetDatabase>,
        runtime: Option<&Materials>,
    ) {
        let parts = self.meshes.get(&object.key).map_or(0, |gpu| gpu.parts.len()).max(1);
        // Правило: свечение материала + `MeshRenderer::emissive` объекта (как `tint` для цвета:
        // своя добавка к общему материалу, пачки не рвутся). Чёрный — без добавки.
        let extra = object.mesh.emissive;
        let glow = |mut r: Resolved| {
            r.emissive = [r.emissive[0] + extra.r, r.emissive[1] + extra.g, r.emissive[2] + extra.b, r.emissive[3]];
            r
        };
        let mut first: Option<Resolved> = None;
        let mut uniform = true;
        for part in 0..parts {
            let resolved = match self.part_material(cache, &object, part, assets, runtime) {
                Some(resolved) => glow(resolved),
                None => self.inline_material(cache, object.mesh, assets),
            };
            match first {
                None => first = Some(resolved),
                Some(f) if f != resolved => uniform = false,
                Some(_) => {}
            }
        }
        let tint = object.mesh.tint.to_array();
        let mut push = |part: u32, resolved: Resolved| {
            let c = resolved.base_color;
            items.push(MeshItem {
                key: object.key,
                mesh_id: object.mesh_id,
                part,
                bind: resolved.bind,
                pass: resolved.pass,
                shadow: resolved.shadow,
                matrix: object.matrix,
                base_color: [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2], c[3] * tint[3]],
                emissive: resolved.emissive,
                params: resolved.params,
                casts_shadows: object.mesh.cast_shadows && resolved.casts_shadows,
                visible: object.visible,
                world_bounds: object.world_bounds,
            });
        };
        if uniform {
            if let Some(resolved) = first {
                push(WHOLE_MESH, resolved);
            }
            return;
        }
        for part in 0..parts {
            let resolved = match self.part_material(cache, &object, part, assets, runtime) {
                Some(resolved) => glow(resolved),
                None => self.inline_material(cache, object.mesh, assets),
            };
            push(part as u32, resolved);
        }
    }

    /// Нарисовать пачки мешей основного прохода. Возвращает (draw call'ы, треугольники).
    pub(super) fn draw_mesh_batches(&self, pass: &mut wgpu::RenderPass<'_>, items: &[MeshItem], batches: &[MeshBatch]) -> (u32, u32) {
        if batches.is_empty() {
            return (0, 0);
        }
        pass.set_bind_group(0, &self.lit_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let (mut draws, mut triangles) = (0, 0);
        let mut current: Option<u8> = None;
        for batch in batches {
            let item = &items[batch.first as usize];
            let (Some(mesh), Some(bind)) = (self.meshes.get(&item.key), self.mesh_pass.binds.get(item.bind as usize)) else {
                continue;
            };
            if current != Some(item.pass) {
                current = Some(item.pass);
                pass.set_pipeline(&self.mesh_pass.pipelines[item.pass as usize]);
            }
            let range = part_range(mesh, item.part);
            pass.set_bind_group(1, bind, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_vertex_buffer(2, mesh.tangents.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            triangles += range.len() as u32 / 3 * batch.instances.len() as u32;
            pass.draw_indexed(range, 0, batch.instances.clone());
            draws += 1;
        }
        (draws, triangles)
    }

    /// Нарисовать карту теней. Возвращает число draw call'ов.
    pub(super) fn draw_shadow_batches(&self, pass: &mut wgpu::RenderPass<'_>, items: &[MeshItem], batches: &[MeshBatch]) -> u32 {
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut draws = 0;
        let mut current: Option<u8> = None;
        for batch in batches {
            let item = &items[batch.first as usize];
            let Some(mesh) = self.meshes.get(&item.key) else {
                continue;
            };
            if current != Some(item.shadow) {
                current = Some(item.shadow);
                pass.set_pipeline(match item.shadow {
                    SHADOW_OPAQUE => &self.shadow_pipeline,
                    SHADOW_TWO_SIDED => &self.mesh_pass.shadow_two_sided,
                    _ => &self.mesh_pass.shadow_cutout,
                });
            }
            if item.shadow == SHADOW_CUTOUT {
                let Some(bind) = self.mesh_pass.binds.get(item.bind as usize) else {
                    continue;
                };
                pass.set_bind_group(1, bind, &[]);
            }
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(part_range(mesh, item.part), 0, batch.instances.clone());
            draws += 1;
        }
        draws
    }
}

/// Диапазон индексов части меша.
fn part_range(mesh: &GpuMesh, part: u32) -> std::ops::Range<u32> {
    if part == WHOLE_MESH {
        return 0..mesh.index_count;
    }
    mesh.parts.get(part as usize).map_or(0..mesh.index_count, |(range, _)| range.clone())
}

fn texture_slot(handle: engine_assets::AssetHandle<engine_assets::Texture>) -> Option<Guid> {
    (!handle.is_none()).then(|| handle.guid())
}

/// Материал, которым рисуется то, что не удалось загрузить: заметный пурпурный.
pub(super) fn missing_material(bind: u32) -> Resolved {
    Resolved {
        bind,
        pass: PASS_OPAQUE,
        shadow: SHADOW_OPAQUE,
        base_color: [1.0, 0.0, 1.0, 1.0],
        emissive: [0.25, 0.0, 0.25, 1.0],
        params: [0.0, 0.6, 0.0, 0.0],
        casts_shadows: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_reflects_material_settings() {
        let material = Material::new().cutout(0.3).double_sided(true).unlit(true).tiling(2.0, 3.0);
        let uniform = MaterialUniform::of(&material);
        assert_eq!(uniform.uv, [2.0, 3.0, 0.0, 0.0]);
        assert_eq!(uniform.factors[2], 0.3);
        assert_eq!(uniform.flags, [1.0, 1.0, 1.0, 1.0]);
        // Без карты нормалей сила — ноль (шейдер берёт нормаль вершины как есть).
        assert_eq!(uniform.factors[0], 0.0);
        // Два материала, отличающиеся только цветом, дают одну группу привязок.
        let red = MaterialUniform::of(&Material::new().color(Color::rgb(1.0, 0.0, 0.0)).metallic(1.0));
        let blue = MaterialUniform::of(&Material::new().color(Color::rgb(0.0, 0.0, 1.0)).roughness(0.1));
        assert_eq!(red.bits(), blue.bits());
    }
}
