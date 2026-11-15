//! Свои шейдеры на GPU (`.shader`, docs/api/shaders.md): загрузка и горячая перезагрузка
//! файлов, модули и конвейеры (лениво, по первому использованию), группы привязок
//! параметров (группа 2 у мешей и спрайтов, 1 у эффектов), эффекты на весь экран.
//!
//! Всё живёт в `Renderer::custom`, который создаётся при первом своём шейдере: сцена без
//! них не создаёт ни одного объекта GPU отсюда и не платит ни за что, кроме проверки
//! счётчиков компонентов.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use engine_assets::{AlphaMode, AssetDatabase, Guid, Material, ShaderKind, ShaderParams, TextureDefault};
use engine_core::prelude::*;
use wgpu::util::DeviceExt;

use super::frame::{MeshItem, INSTANCE_ATTRIBUTES};
use super::material::{Resolved, SHADOW_CUTOUT, TANGENT_ATTRIBUTES};
use super::{GpuMesh, GpuTexture, Renderer};
use crate::shader_components::{MaterialParams, PostEffect, SpriteShader};
use crate::shaders::{self, Compiled};

/// Старший бит номера группы материала: часть меша рисуется своим шейдером (номер — в
/// списке `CustomShaders::mesh_binds`).
pub(super) const CUSTOM_BIT: u32 = 1 << 31;

/// Как часто смотреть, не изменились ли файлы шейдеров на диске (горячая перезагрузка).
const POLL_SECONDS: f32 = 0.25;

/// Время кадра для шейдеров (`EngineFrame` в shaders/mod.rs).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    time: f32,
    delta_time: f32,
    unused: [f32; 2],
    screen: [f32; 4],
}

/// Файл `.shader` и его последняя рабочая версия.
struct ShaderEntry {
    /// Путь для сообщений (относительно каталога ассетов).
    name: String,
    /// Файл на диске (нет — ассеты из пака: перезагрузки нет).
    path: Option<PathBuf>,
    /// Время изменения и длина файла при последней проверке.
    stamp: Option<(std::time::SystemTime, u64)>,
    /// Текст последней прочитанной версии (рабочей или нет).
    text: String,
    /// Растёт при каждой успешной перезагрузке: ключ конвейеров и групп.
    revision: u32,
    /// Последняя версия без ошибок (`None` — рабочей версии ещё не было).
    compiled: Option<Arc<Compiled>>,
    /// Модули GPU по варианту (у не-мешевых — один).
    modules: Vec<Option<Arc<wgpu::ShaderModule>>>,
}

/// Группа параметров части меша со своим шейдером.
struct MeshBind {
    shader: Guid,
    revision: u32,
    /// Группа стандартного материала (текстуры, порог выреза, флаги) — группа 1.
    material_bind: u32,
    group: wgpu::BindGroup,
    two_sided: bool,
    /// Значения материала (под значения объекта из `MaterialParams`).
    material_params: ShaderParams,
    /// Проход теней — свой (сдвиг вершин, `discard`, прозрачность).
    own_shadow: bool,
}

/// Спрайты одного шейдера с одинаковыми параметрами.
struct SpriteBind {
    shader: Guid,
    revision: u32,
    group: wgpu::BindGroup,
}

/// Ключ группы: шейдер, версия, байты параметров, текстуры, группа материала.
#[derive(Clone, PartialEq, Eq, Hash)]
struct BindKey {
    shader: Guid,
    revision: u32,
    bytes: Vec<u8>,
    textures: Vec<Option<Guid>>,
    material_bind: u32,
}

/// Эффект кадра: шейдер, группа параметров, где он идёт.
struct PostPass {
    shader: Guid,
    revision: u32,
    group: wgpu::BindGroup,
    hdr: bool,
}

/// Промежуточные кадры цепочки эффектов (под размер цели).
struct PostTargets {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    /// Два кадра в формате цели (после тонмаппинга) и один HDR (до него).
    ldr: [(wgpu::TextureView, wgpu::BindGroup); 2],
    hdr: (wgpu::TextureView, wgpu::BindGroup),
    /// Группа, читающая HDR-кадр сцены (поколение `PostChain`).
    scene: Option<wgpu::BindGroup>,
}

pub(super) struct CustomShaders {
    shaders: HashMap<Guid, ShaderEntry>,
    /// Ошибки, уже выведенные в лог (шейдер, хеш текста).
    reported: HashSet<(Guid, u64)>,
    frame: wgpu::Buffer,
    /// Раскладки групп параметров по числу текстур.
    layouts: Vec<wgpu::BindGroupLayout>,
    /// Текстуры 1×1 по умолчанию (`TextureDefault`, линейные данные).
    defaults: Vec<wgpu::TextureView>,
    sampler: wgpu::Sampler,
    /// Текстуры-параметры как данные (без sRGB): объявленные `= gray` / `= normal`.
    linear: HashMap<Guid, GpuTexture>,
    mesh_binds: Vec<MeshBind>,
    mesh_bind_ids: HashMap<BindKey, u32>,
    mesh_pipelines: HashMap<(Guid, u32, usize, u8), wgpu::RenderPipeline>,
    shadow_pipelines: HashMap<(Guid, u32, bool), wgpu::RenderPipeline>,
    sprite_binds: Vec<SpriteBind>,
    sprite_bind_ids: HashMap<BindKey, u32>,
    sprite_pipelines: HashMap<(Guid, u32), wgpu::RenderPipeline>,
    post_pipelines: HashMap<(Guid, u32, wgpu::TextureFormat), wgpu::RenderPipeline>,
    post_source_layout: wgpu::BindGroupLayout,
    post_sampler: wgpu::Sampler,
    /// Перенос кадра без изменений (цепочка до тонмаппинга кончается в HDR-кадре).
    copy: Option<(Arc<Compiled>, wgpu::ShaderModule)>,
    copy_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    post_targets: Option<PostTargets>,
    last_poll: Option<web_time::Instant>,
    /// Секунды с создания (время шейдеров, если в мире время не идёт).
    started: web_time::Instant,
}

/// Раскладка группы параметров: время кадра, параметры, `textures` пар текстура + сэмплер.
fn params_layout(device: &wgpu::Device, textures: usize) -> wgpu::BindGroupLayout {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let mut entries = vec![uniform(0), uniform(1)];
    for k in 0..textures as u32 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 + 2 * k,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3 + 2 * k,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("custom shader params layout"), entries: &entries })
}

fn hash_text(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn file_stamp(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl CustomShaders {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("custom shader frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layouts = (0..=shaders::MAX_TEXTURE_PARAMS).map(|n| params_layout(device, n)).collect();
        let defaults = TextureDefault::ALL
            .iter()
            .map(|d| {
                device
                    .create_texture_with_data(
                        queue,
                        &wgpu::TextureDescriptor {
                            label: Some("custom shader default texture"),
                            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::util::TextureDataOrder::LayerMajor,
                        &d.rgba(),
                    )
                    .create_view(&Default::default())
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("custom shader texture sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let texture_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let post_source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post effect source layout"),
            entries: &[
                texture_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post effect sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            shaders: HashMap::new(),
            reported: HashSet::new(),
            frame,
            layouts,
            defaults,
            sampler,
            linear: HashMap::new(),
            mesh_binds: Vec::new(),
            mesh_bind_ids: HashMap::new(),
            mesh_pipelines: HashMap::new(),
            shadow_pipelines: HashMap::new(),
            sprite_binds: Vec::new(),
            sprite_bind_ids: HashMap::new(),
            sprite_pipelines: HashMap::new(),
            post_pipelines: HashMap::new(),
            post_source_layout,
            post_sampler,
            copy: None,
            copy_pipelines: HashMap::new(),
            post_targets: None,
            last_poll: None,
            started: web_time::Instant::now(),
        }
    }

    /// Забыть группы параметров (шейдер или материалы изменились).
    fn clear_binds(&mut self) {
        self.mesh_binds.clear();
        self.mesh_bind_ids.clear();
        self.sprite_binds.clear();
        self.sprite_bind_ids.clear();
    }

    /// Собранная рабочая версия шейдера (загрузить при первом обращении).
    fn compiled(&mut self, guid: Guid, assets: Option<&AssetDatabase>) -> Option<(Arc<Compiled>, u32)> {
        if !self.shaders.contains_key(&guid) {
            let db = assets?;
            let name = db.relative_path_of(guid).unwrap_or("?").to_string();
            let path = (!db.is_packed()).then(|| db.path_of(guid)).flatten();
            let stamp = path.as_deref().and_then(file_stamp);
            let mut entry = ShaderEntry { name, path, stamp, text: String::new(), revision: 0, compiled: None, modules: Vec::new() };
            match db.read_text(guid) {
                Ok(text) => {
                    entry.text = text;
                    self.recompile(guid, &mut entry);
                }
                Err(error) => log::error!("shader {}: {error}", entry.name),
            }
            self.shaders.insert(guid, entry);
        }
        let entry = self.shaders.get(&guid)?;
        entry.compiled.clone().map(|c| (c, entry.revision))
    }

    /// Проверить и собрать текст записи; при ошибке оставить прошлую рабочую версию.
    /// `true` — версия сменилась.
    fn recompile(&mut self, guid: Guid, entry: &mut ShaderEntry) -> bool {
        let started = web_time::Instant::now();
        match shaders::compile(&entry.text) {
            Ok(compiled) => {
                entry.compiled = Some(Arc::new(compiled));
                entry.revision += 1;
                entry.modules.clear();
                log::debug!("shader {} compiled in {:.1} ms", entry.name, started.elapsed().as_secs_f32() * 1000.0);
                true
            }
            Err(d) => {
                if self.reported.insert((guid, hash_text(&entry.text))) {
                    let kept = if entry.compiled.is_some() { "the previous version stays in use" } else { "drawing with the standard material" };
                    log::error!("{}:{}:{}: {}{}; {kept}", entry.name, d.line, d.column, d.message, d.notes.iter().map(|n| format!("; {n}")).collect::<String>());
                }
                false
            }
        }
    }

    /// Горячая перезагрузка: перечитать изменившиеся на диске файлы. `true` — какой-то
    /// шейдер сменил версию (группы и кэш материалов надо собрать заново).
    fn poll(&mut self) -> bool {
        let now = web_time::Instant::now();
        if self.last_poll.is_some_and(|t| now.duration_since(t).as_secs_f32() < POLL_SECONDS) {
            return false;
        }
        self.last_poll = Some(now);
        let mut changed = false;
        let guids: Vec<Guid> = self.shaders.keys().copied().collect();
        for guid in guids {
            let Some(mut entry) = self.shaders.remove(&guid) else { continue };
            if let Some(path) = entry.path.clone() {
                let stamp = file_stamp(&path);
                if stamp.is_some() && stamp != entry.stamp {
                    entry.stamp = stamp;
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if text != entry.text {
                            entry.text = text;
                            if self.recompile(guid, &mut entry) {
                                log::info!("shader {} reloaded", entry.name);
                                self.mesh_pipelines.retain(|k, _| k.0 != guid);
                                self.shadow_pipelines.retain(|k, _| k.0 != guid);
                                self.sprite_pipelines.retain(|k, _| k.0 != guid);
                                self.post_pipelines.retain(|k, _| k.0 != guid);
                                changed = true;
                            }
                        }
                    }
                }
            }
            self.shaders.insert(guid, entry);
        }
        if changed {
            self.clear_binds();
        }
        changed
    }

    /// Модуль GPU шейдера для варианта (собирается при первом обращении).
    fn module(&mut self, device: &wgpu::Device, guid: Guid, variant: usize) -> Option<Arc<wgpu::ShaderModule>> {
        let entry = self.shaders.get_mut(&guid)?;
        let compiled = entry.compiled.clone()?;
        let index = variant.min(compiled.sources.len() - 1);
        if entry.modules.len() < compiled.sources.len() {
            entry.modules.resize_with(compiled.sources.len(), || None);
        }
        if entry.modules[index].is_none() {
            let started = web_time::Instant::now();
            let source = compiled.sources[index].clone();
            let module = guarded(device, "shader module", || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("custom shader"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
            })?;
            entry.modules[index] = Some(Arc::new(module));
            log::debug!("shader module {} (variant {index}) created in {:.1} ms", entry.name, started.elapsed().as_secs_f32() * 1000.0);
        }
        entry.modules[index].clone()
    }
}

/// Значения параметров и текстуры для группы.
struct BindInputs<'a> {
    compiled: &'a Compiled,
    layers: Vec<&'a ShaderParams>,
}

/// Байты uniform-блока и текстуры параметров — ключ группы.
fn param_values(inputs: &BindInputs) -> (Vec<u8>, Vec<Option<Guid>>) {
    let compiled = inputs.compiled;
    let bytes = compiled.layout.bytes(&compiled.info, &inputs.layers);
    let textures = compiled
        .layout
        .textures
        .iter()
        .map(|&i| compiled.info.value(&compiled.info.params[i], &inputs.layers).texture().map(|h| h.guid()))
        .collect();
    (bytes, textures)
}

/// Откуда взять текстуру-параметр.
#[derive(Clone, Copy)]
enum ViewSource {
    Default(usize),
    Linear(Guid),
    Srgb(Guid),
}

/// Диапазон индексов части меша (как в material.rs).
fn part_range(mesh: &GpuMesh, part: u32) -> std::ops::Range<u32> {
    if part == super::frame::WHOLE_MESH {
        return 0..mesh.index_count;
    }
    mesh.parts.get(part as usize).map_or(0..mesh.index_count, |(range, _)| range.clone())
}

impl Renderer {
    fn custom_shaders(&mut self) -> &mut CustomShaders {
        if self.custom.is_none() {
            self.custom = Some(Box::new(CustomShaders::new(&self.device, &self.queue)));
        }
        self.custom.as_mut().expect("just created")
    }

    /// Начало кадра: горячая перезагрузка своих шейдеров (если они есть).
    pub(super) fn poll_custom_shaders(&mut self) {
        let Some(custom) = self.custom.as_mut() else { return };
        if custom.poll() {
            // Материалы со старыми номерами групп — собрать заново.
            self.mesh_pass.clear();
        }
    }

    /// Ошибки и версии своих шейдеров (для тестов и редактора): путь → текст последней
    /// ошибки сборки не хранится; есть ли рабочая версия и её номер.
    pub fn shader_revision(&self, guid: Guid) -> Option<u32> {
        let entry = self.custom.as_ref()?.shaders.get(&guid)?;
        entry.compiled.as_ref().map(|_| entry.revision)
    }

    /// Текстура-параметр: цвет (sRGB) или данные (`= gray`, `= normal`: без перевода).
    fn param_texture_source(&mut self, guid: Option<Guid>, default: TextureDefault, assets: Option<&AssetDatabase>) -> ViewSource {
        let data = matches!(default, TextureDefault::Gray | TextureDefault::Normal);
        let fallback = ViewSource::Default(TextureDefault::ALL.iter().position(|d| *d == default).unwrap_or(0));
        let Some(guid) = guid else { return fallback };
        if !data {
            self.ensure_texture(guid, assets);
            return ViewSource::Srgb(guid);
        }
        if self.custom_shaders().linear.contains_key(&guid) {
            return ViewSource::Linear(guid);
        }
        let decoded = assets.ok_or_else(|| "no AssetDatabase in world.resources".to_string()).and_then(|db| {
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
                self.custom_shaders().linear.insert(guid, texture);
                ViewSource::Linear(guid)
            }
            Err(reason) => {
                log::error!("texture {guid}: {reason}");
                fallback
            }
        }
    }

    /// Группа параметров (время, uniform-блок, текстуры).
    fn params_group(&mut self, inputs: &BindInputs, assets: Option<&AssetDatabase>) -> wgpu::BindGroup {
        let compiled = inputs.compiled;
        let (bytes, textures) = param_values(inputs);
        self.custom_shaders();
        let sources: Vec<ViewSource> = compiled
            .layout
            .textures
            .iter()
            .zip(&textures)
            .map(|(&i, guid)| self.param_texture_source(*guid, compiled.info.params[i].texture_default, assets))
            .collect();
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("custom shader params"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let custom = self.custom.as_ref().expect("created above");
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: custom.frame.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: buffer.as_entire_binding() },
        ];
        for (k, source) in sources.iter().enumerate() {
            let view = match *source {
                ViewSource::Default(i) => &custom.defaults[i],
                ViewSource::Linear(guid) => custom.linear.get(&guid).map_or(&custom.defaults[0], |t| &t.view),
                ViewSource::Srgb(guid) => &self.textures.get(&guid).unwrap_or(&self.missing).view,
            };
            entries.push(wgpu::BindGroupEntry { binding: 2 + 2 * k as u32, resource: wgpu::BindingResource::TextureView(view) });
            entries.push(wgpu::BindGroupEntry { binding: 3 + 2 * k as u32, resource: wgpu::BindingResource::Sampler(&custom.sampler) });
        }
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom shader params"),
            layout: &custom.layouts[compiled.layout.textures.len()],
            entries: &entries,
        })
    }

    /// Ассеты перечитаны (`reload_assets`): группы параметров ссылаются на старые группы
    /// материалов и текстуры — собрать заново.
    pub(super) fn forget_custom_binds(&mut self) {
        if let Some(custom) = self.custom.as_mut() {
            custom.clear_binds();
            custom.linear.clear();
        }
    }

    /// Материал со своим шейдером: та же группа стандартного материала, но свой конвейер и
    /// группа параметров. Шейдер не собрался и рабочей версии нет — стандартный материал.
    pub(super) fn custom_material(&mut self, material: &Material, standard: Resolved, object: Option<&ShaderParams>, assets: Option<&AssetDatabase>) -> Resolved {
        let guid = material.shader.guid();
        let Some((compiled, revision)) = self.custom_shaders().compiled(guid, assets) else {
            return standard;
        };
        if !compiled.info.kind.is_material() {
            let custom = self.custom_shaders();
            if custom.reported.insert((guid, u64::MAX)) {
                let name = custom.shaders.get(&guid).map_or("?", |e| e.name.as_str()).to_string();
                log::error!("{name}: a {} shader cannot draw a material; use `//! kind: surface` or `unlit`", compiled.info.kind.keyword());
            }
            return standard;
        }
        let own_shadow = compiled.has_vertex || compiled.has_discard || material.alpha_mode != AlphaMode::Opaque;
        let mut layers = vec![&material.shader_params];
        if let Some(object) = object {
            layers.push(object);
        }
        let inputs = BindInputs { compiled: &compiled, layers };
        let (bytes, textures) = param_values(&inputs);
        let key = BindKey { shader: guid, revision, bytes, textures, material_bind: standard.bind };
        let known = self.custom_shaders().mesh_bind_ids.get(&key).copied();
        let id = match known {
            Some(id) => id,
            None => {
                let group = self.params_group(&inputs, assets);
                let custom = self.custom_shaders();
                let id = custom.mesh_binds.len() as u32;
                custom.mesh_binds.push(MeshBind {
                    shader: guid,
                    revision,
                    material_bind: standard.bind,
                    group,
                    two_sided: material.double_sided,
                    material_params: material.shader_params.clone(),
                    own_shadow,
                });
                custom.mesh_bind_ids.insert(key, id);
                id
            }
        };
        Resolved { bind: CUSTOM_BIT | id, shadow: if own_shadow { SHADOW_CUTOUT } else { standard.shadow }, ..standard }
    }

    /// Значения объекта (`MaterialParams`) поверх материала части со своим шейдером: своя
    /// группа параметров (объект рисуется своей пачкой), остальное — как у материала.
    pub(super) fn custom_override(&mut self, resolved: Resolved, object: &ShaderParams, assets: Option<&AssetDatabase>) -> Resolved {
        if object.is_empty() || resolved.bind & CUSTOM_BIT == 0 {
            return resolved;
        }
        let Some(custom) = self.custom.as_ref() else { return resolved };
        let Some(bind) = custom.mesh_binds.get((resolved.bind & !CUSTOM_BIT) as usize) else { return resolved };
        let (guid, revision, material_bind, two_sided, own_shadow) = (bind.shader, bind.revision, bind.material_bind, bind.two_sided, bind.own_shadow);
        let base = bind.material_params.clone();
        let Some(compiled) = custom.shaders.get(&guid).and_then(|e| e.compiled.clone()) else { return resolved };
        let inputs = BindInputs { compiled: &compiled, layers: vec![&base, object] };
        let (bytes, textures) = param_values(&inputs);
        let key = BindKey { shader: guid, revision, bytes, textures, material_bind };
        if let Some(id) = self.custom_shaders().mesh_bind_ids.get(&key) {
            return Resolved { bind: CUSTOM_BIT | id, ..resolved };
        }
        let group = self.params_group(&inputs, assets);
        let custom = self.custom_shaders();
        let id = custom.mesh_binds.len() as u32;
        custom.mesh_binds.push(MeshBind { shader: guid, revision, material_bind, group, two_sided, material_params: base.clone(), own_shadow });
        custom.mesh_bind_ids.insert(key, id);
        Resolved { bind: CUSTOM_BIT | id, ..resolved }
    }

    /// Параметры объекта поверх материала (`MaterialParams`).
    pub(super) fn object_params(object: &GameObject) -> Option<&ShaderParams> {
        object.get::<MaterialParams>().map(|p| &p.params)
    }

    /// Перед проходами: время кадра и конвейеры частей мешей со своими шейдерами для
    /// варианта кадра.
    pub(super) fn prepare_custom(&mut self, world: &World, items: &[MeshItem], width: u32, height: u32) {
        if self.custom.is_none() {
            return;
        }
        self.write_custom_frame(world, width, height);
        let variant = self.frame_variant;
        let binds = self.custom.as_ref().map_or(0, |c| c.mesh_binds.len());
        let mut seen: Vec<bool> = vec![false; binds];
        for item in items {
            if item.bind & CUSTOM_BIT == 0 {
                continue;
            }
            let id = (item.bind & !CUSTOM_BIT) as usize;
            if seen.get(id).copied().unwrap_or(true) {
                continue;
            }
            seen[id] = true;
            // Конвейер части — по её материалу (`pass` у пачки с одной группой один).
            self.ensure_mesh_pipeline(id, variant, item.pass);
            if item.shadow == SHADOW_CUTOUT {
                self.ensure_shadow_pipeline(id);
            }
        }
    }

    /// Время кадра и размер кадра сцены для своих шейдеров.
    fn write_custom_frame(&mut self, world: &World, width: u32, height: u32) {
        let Some(custom) = self.custom.as_ref() else { return };
        // Время мира; в редакторе вне игры оно стоит — тогда часы рендерера (материалы живые,
        // как «Animated Materials» в Unity).
        let time = if world.time.elapsed > 0.0 { world.time.elapsed as f32 } else { custom.started.elapsed().as_secs_f32() };
        let frame = FrameUniform {
            time,
            delta_time: world.time.delta,
            unused: [0.0; 2],
            screen: [width as f32, height as f32, 1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
        };
        self.queue.write_buffer(&custom.frame, 0, bytemuck::bytes_of(&frame));
    }

    fn ensure_mesh_pipeline(&mut self, id: usize, variant: usize, pass: u8) {
        let Some(custom) = self.custom.as_mut() else { return };
        let Some(bind) = custom.mesh_binds.get(id) else { return };
        let (guid, revision) = (bind.shader, bind.revision);
        let key = (guid, revision, variant, pass);
        if custom.mesh_pipelines.contains_key(&key) {
            return;
        }
        let Some(textures) = custom.shaders.get(&guid).and_then(|e| e.compiled.as_ref()).map(|c| c.layout.textures.len()) else { return };
        let Some(module) = custom.module(&self.device, guid, variant) else { return };
        let custom = self.custom.as_ref().expect("checked");
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom mesh pipeline layout"),
            bind_group_layouts: &[&self.lit_layouts[variant], &self.mesh_pass.layout, &custom.layouts[textures]],
            push_constant_ranges: &[],
        });
        let blend = pass >= super::material::PASS_BLEND;
        let two_sided = pass % 2 == 1;
        let pipeline = guarded(&self.device, "render pipeline", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("custom mesh pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_surface"),
                buffers: &mesh_buffers(),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_surface"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::post::HDR_FORMAT,
                    blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: (!two_sided).then_some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::DEPTH_FORMAT,
                depth_write_enabled: !blend,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: super::SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
        }));
        let Some(pipeline) = pipeline else { return };
        self.custom.as_mut().expect("checked").mesh_pipelines.insert(key, pipeline);
    }

    fn ensure_shadow_pipeline(&mut self, id: usize) {
        let Some(custom) = self.custom.as_mut() else { return };
        let Some(bind) = custom.mesh_binds.get(id) else { return };
        let (guid, revision, two_sided) = (bind.shader, bind.revision, bind.two_sided);
        let key = (guid, revision, two_sided);
        if custom.shadow_pipelines.contains_key(&key) {
            return;
        }
        let Some(textures) = custom.shaders.get(&guid).and_then(|e| e.compiled.as_ref()).map(|c| c.layout.textures.len()) else { return };
        let Some(module) = custom.module(&self.device, guid, shaders::SHADOW_PASS) else { return };
        let custom = self.custom.as_ref().expect("checked");
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom shadow pipeline layout"),
            bind_group_layouts: &[&self.globals_layout, &self.mesh_pass.layout, &custom.layouts[textures]],
            push_constant_ranges: &[],
        });
        let pipeline = guarded(&self.device, "render pipeline", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("custom shadow pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_surface_shadow"),
                buffers: &mesh_buffers(),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_surface_shadow"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: (!two_sided).then_some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: 0, slope_scale: 1.5, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        }));
        let Some(pipeline) = pipeline else { return };
        self.custom.as_mut().expect("checked").shadow_pipelines.insert(key, pipeline);
    }

    /// Нарисовать пачку части меша со своим шейдером. `shadow` — проход теней. `false` —
    /// нечем рисовать (конвейер не собран); пачка пропускается.
    pub(super) fn draw_custom_mesh(&self, pass: &mut wgpu::RenderPass<'_>, item: &MeshItem, mesh: &GpuMesh, instances: std::ops::Range<u32>, shadow: bool) -> bool {
        let Some(custom) = self.custom.as_ref() else { return false };
        let Some(bind) = custom.mesh_binds.get((item.bind & !CUSTOM_BIT) as usize) else { return false };
        let pipeline = if shadow {
            custom.shadow_pipelines.get(&(bind.shader, bind.revision, bind.two_sided))
        } else {
            custom.mesh_pipelines.get(&(bind.shader, bind.revision, self.frame_variant, item.pass))
        };
        let (Some(pipeline), Some(material)) = (pipeline, self.mesh_pass.binds.get(bind.material_bind as usize)) else {
            return false;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, material, &[]);
        pass.set_bind_group(2, &bind.group, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(2, mesh.tangents.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        self.count_state(6);
        pass.draw_indexed(part_range(mesh, item.part), 0, instances);
        true
    }

    // ---------------------------------------------------------------- спрайты

    /// Номер пачки спрайтов со своим шейдером (`Blend::Custom`); `None` — шейдер не
    /// собран: спрайт рисуется обычным.
    pub(super) fn sprite_shader_bind(&mut self, shader: &SpriteShader, assets: Option<&AssetDatabase>) -> Option<u32> {
        let guid = shader.shader.guid();
        if shader.shader.is_none() {
            return None;
        }
        let (compiled, revision) = self.custom_shaders().compiled(guid, assets)?;
        if compiled.info.kind != ShaderKind::Sprite {
            let custom = self.custom_shaders();
            if custom.reported.insert((guid, u64::MAX)) {
                let name = custom.shaders.get(&guid).map_or("?", |e| e.name.as_str()).to_string();
                log::error!("{name}: SpriteShader needs `//! kind: sprite`, this one is `{}`", compiled.info.kind.keyword());
            }
            return None;
        }
        let inputs = BindInputs { compiled: &compiled, layers: vec![&shader.params] };
        let (bytes, textures) = param_values(&inputs);
        let key = BindKey { shader: guid, revision, bytes, textures, material_bind: 0 };
        if let Some(id) = self.custom_shaders().sprite_bind_ids.get(&key) {
            return Some(*id);
        }
        let group = self.params_group(&inputs, assets);
        self.ensure_sprite_pipeline(guid, revision, compiled.layout.textures.len())?;
        let custom = self.custom_shaders();
        let id = custom.sprite_binds.len() as u32;
        custom.sprite_binds.push(SpriteBind { shader: guid, revision, group });
        custom.sprite_bind_ids.insert(key, id);
        Some(id)
    }

    fn ensure_sprite_pipeline(&mut self, guid: Guid, revision: u32, textures: usize) -> Option<()> {
        if self.custom.as_ref()?.sprite_pipelines.contains_key(&(guid, revision)) {
            return Some(());
        }
        let module = self.custom.as_mut()?.module(&self.device, guid, 0)?;
        let custom = self.custom.as_ref()?;
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom sprite pipeline layout"),
            bind_group_layouts: &[&self.globals_layout, &self.texture_layout, &custom.layouts[textures]],
            push_constant_ranges: &[],
        });
        let pipeline = guarded(&self.device, "render pipeline", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("custom sprite pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_sprite"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::draw2d::SpriteVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_sprite"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::post::HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: super::SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
        }));
        let pipeline = pipeline?;
        self.custom.as_mut()?.sprite_pipelines.insert((guid, revision), pipeline);
        Some(())
    }

    /// Поставить конвейер и группу параметров пачки спрайтов со своим шейдером.
    pub(super) fn bind_custom_sprites(&self, pass: &mut wgpu::RenderPass<'_>, id: u32) -> bool {
        let Some(custom) = self.custom.as_ref() else { return false };
        let Some(bind) = custom.sprite_binds.get(id as usize) else { return false };
        let Some(pipeline) = custom.sprite_pipelines.get(&(bind.shader, bind.revision)) else { return false };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, &bind.group, &[]);
        self.count_state(2);
        true
    }

    // ---------------------------------------------------------------- эффекты на весь экран

    /// Эффекты камеры вида (по `order`), готовые к запуску. Пусто — эффектов нет.
    fn collect_post_effects(&mut self, world: &World, assets: Option<&AssetDatabase>) -> Vec<PostPass> {
        let scene = &world.scene;
        // Камера — как у `scene_view`: активная с наибольшим приоритетом (при равенстве — последняя).
        let mut best: Option<(GameObjectId, i32)> = None;
        scene.visit_active_with::<crate::Camera>(|id, camera| {
            if best.is_none_or(|(_, p)| camera.priority >= p) {
                best = Some((id, camera.priority));
            }
        });
        let Some((camera, _)) = best else { return Vec::new() };
        let Some(object) = scene.get(camera) else { return Vec::new() };
        let mut effects: Vec<(i32, usize, PostEffect)> = object
            .components()
            .filter_map(|c| c.as_any().downcast_ref::<PostEffect>())
            .filter(|e| e.enabled && !e.shader.is_none())
            .cloned()
            .enumerate()
            .map(|(i, e)| (e.order, i, e))
            .collect();
        effects.sort_by_key(|(order, i, _)| (*order, *i));
        let mut out = Vec::new();
        for (_, _, effect) in effects {
            let guid = effect.shader.guid();
            let Some((compiled, revision)) = self.custom_shaders().compiled(guid, assets) else { continue };
            if compiled.info.kind != ShaderKind::Post {
                let custom = self.custom_shaders();
                if custom.reported.insert((guid, u64::MAX)) {
                    let name = custom.shaders.get(&guid).map_or("?", |e| e.name.as_str()).to_string();
                    log::error!("{name}: PostEffect needs `//! kind: post`, this one is `{}`", compiled.info.kind.keyword());
                }
                continue;
            }
            let inputs = BindInputs { compiled: &compiled, layers: vec![&effect.params] };
            let group = self.params_group(&inputs, assets);
            out.push(PostPass { shader: guid, revision, group, hdr: effect.before_tonemap });
        }
        out
    }

    fn post_pipeline(&mut self, guid: Guid, revision: u32, format: wgpu::TextureFormat) -> Option<()> {
        if self.custom.as_ref()?.post_pipelines.contains_key(&(guid, revision, format)) {
            return Some(());
        }
        let textures = self.custom.as_ref()?.shaders.get(&guid)?.compiled.as_ref()?.layout.textures.len();
        let module = self.custom.as_mut()?.module(&self.device, guid, 0)?;
        let pipeline = self.make_post_pipeline(&module, textures, format)?;
        self.custom.as_mut()?.post_pipelines.insert((guid, revision, format), pipeline);
        Some(())
    }

    fn make_post_pipeline(&self, module: &wgpu::ShaderModule, textures: usize, format: wgpu::TextureFormat) -> Option<wgpu::RenderPipeline> {
        let custom = self.custom.as_ref()?;
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post effect pipeline layout"),
            bind_group_layouts: &[&custom.post_source_layout, &custom.layouts[textures]],
            push_constant_ranges: &[],
        });
        guarded(&self.device, "post effect pipeline", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post effect pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module, entry_point: Some("vs_post"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_post"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        }))
    }

    /// Конвейер переноса кадра без изменений (встроенный эффект `post` → `in.color`).
    fn copy_pipeline(&mut self, format: wgpu::TextureFormat) -> Option<()> {
        if self.custom.as_ref()?.copy.is_none() {
            let compiled = shaders::compile("//! kind: post\nfn post(in: PostIn) -> vec4<f32> {\n    return in.color;\n}\n").ok()?;
            let source = compiled.sources[0].clone();
            let module = guarded(&self.device, "shader module", || {
                self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("post effect copy"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
            })?;
            self.custom.as_mut()?.copy = Some((Arc::new(compiled), module));
        }
        if !self.custom.as_ref()?.copy_pipelines.contains_key(&format) {
            let module = &self.custom.as_ref()?.copy.as_ref()?.1;
            let pipeline = self.make_post_pipeline(module, 0, format)?;
            self.custom.as_mut()?.copy_pipelines.insert(format, pipeline);
        }
        Some(())
    }

    /// Кадры цепочки эффектов под размер цели.
    fn ensure_post_targets(&mut self, width: u32, height: u32) {
        let size = [width.max(1), height.max(1)];
        let format = self.format;
        let device = &self.device;
        let Some(custom) = self.custom.as_mut() else { return };
        if custom.post_targets.as_ref().is_some_and(|t| t.size == size && t.format == format) {
            return;
        }
        let make = |format: wgpu::TextureFormat| {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("post effect target"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post effect source"),
                layout: &custom.post_source_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&custom.post_sampler) },
                ],
            });
            (view, bind)
        };
        custom.post_targets = Some(PostTargets { size, format, ldr: [make(format), make(format)], hdr: make(crate::post::HDR_FORMAT), scene: None });
    }

    /// Сведение кадра с эффектами: `true` — эффекты были и кадр уже в `target`; `false` —
    /// эффектов нет, сводить как обычно.
    pub(super) fn run_post_effects(
        &mut self,
        world: &World,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene_size: [u32; 2],
        settings: crate::post::PostSettings,
    ) -> bool {
        if world.scene.count_with::<PostEffect>() == 0 {
            return false;
        }
        let assets = world.resources.get::<AssetDatabase>();
        let effects = self.collect_post_effects(world, assets);
        if effects.is_empty() {
            return false;
        }
        let format = self.format;
        for effect in &effects {
            let format = if effect.hdr { crate::post::HDR_FORMAT } else { format };
            if self.post_pipeline(effect.shader, effect.revision, format).is_none() {
                return false;
            }
        }
        self.write_custom_frame(world, scene_size[0], scene_size[1]);
        let hdr_count = effects.iter().filter(|e| e.hdr).count();
        if hdr_count % 2 == 1 && self.copy_pipeline(crate::post::HDR_FORMAT).is_none() {
            return false;
        }
        // Кадры цепочки — на размер цели (кадр сцены может быть другого размера: эффекты
        // читают по UV).
        self.ensure_post_targets(size[0], size[1]);
        let Some(scene_view) = self.post.scene_target() else { return false };
        let custom = self.custom.as_mut().expect("effects exist");
        let targets = custom.post_targets.as_mut().expect("ensured");
        let scene_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post effect scene source"),
            layout: &custom.post_source_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scene_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&custom.post_sampler) },
            ],
        });
        targets.scene = Some(scene_bind);
        let custom = self.custom.as_ref().expect("effects exist");
        let targets = custom.post_targets.as_ref().expect("ensured");
        let run = |encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, source: &wgpu::BindGroup, params: &wgpu::BindGroup, view: &wgpu::TextureView| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post effect"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, source, &[]);
            pass.set_bind_group(1, params, &[]);
            pass.draw(0..3, 0..1);
        };
        // 1. До тонмаппинга: кадр сцены ↔ HDR-кадр цепочки; кончается в кадре сцены.
        let hdr: Vec<&PostPass> = effects.iter().filter(|e| e.hdr).collect();
        let scene_source = targets.scene.as_ref().expect("set above");
        for (k, effect) in hdr.iter().enumerate() {
            let pipeline = &custom.post_pipelines[&(effect.shader, effect.revision, crate::post::HDR_FORMAT)];
            let (source, view) = if k % 2 == 0 { (scene_source, &targets.hdr.0) } else { (&targets.hdr.1, scene_view) };
            run(encoder, pipeline, source, &effect.group, view);
        }
        if hdr.len() % 2 == 1 {
            let copy = &custom.copy_pipelines[&crate::post::HDR_FORMAT];
            let params = &hdr[0].group;
            run(encoder, copy, &targets.hdr.1, params, scene_view);
        }
        // 2. Сведение (тонмаппинг, блум) — в первый LDR-кадр или сразу в цель.
        let ldr: Vec<&PostPass> = effects.iter().filter(|e| !e.hdr).collect();
        if ldr.is_empty() {
            self.post.run(&self.queue, encoder, target, settings);
            return true;
        }
        self.post.run(&self.queue, encoder, &targets.ldr[0].0, settings);
        // 3. После тонмаппинга: LDR-кадры по очереди, последний — в цель.
        for (k, effect) in ldr.iter().enumerate() {
            let pipeline = &custom.post_pipelines[&(effect.shader, effect.revision, format)];
            let source = &targets.ldr[k % 2].1;
            let view = if k + 1 == ldr.len() { target } else { &targets.ldr[(k + 1) % 2].0 };
            run(encoder, pipeline, source, &effect.group, view);
        }
        true
    }
}

/// Создать объект GPU, перехватив ошибку проверки wgpu: драйвер или WebGPU не принял
/// шейдер — сообщение в лог и `None` (иначе ошибка ушла бы в обработчик по умолчанию,
/// который роняет программу).
fn guarded<T>(device: &wgpu::Device, what: &str, make: impl FnOnce() -> T) -> Option<T> {
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = make();
    let scope = device.pop_error_scope();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(error) = pollster::block_on(scope) {
        log::error!("custom shader: the GPU rejected the {what}: {error}");
        return None;
    }
    #[cfg(target_arch = "wasm32")]
    drop(scope);
    Some(value)
}

/// Вершинные буферы мешей: вершины, экземпляры, касательные (как `main_pipelines`).
fn mesh_buffers() -> [wgpu::VertexBufferLayout<'static>; 3] {
    const VERTEX: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<super::MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<super::frame::MeshInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        },
        wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &TANGENT_ATTRIBUTES },
    ]
}
