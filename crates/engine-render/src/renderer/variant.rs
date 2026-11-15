//! Варианты шейдеров мешей и рельефа.
//!
//! Код каскадов теней (`SoftShadows`) тяжёлый: 16 выборок по диску Пуассона, оценка
//! заслонителя, смешивание каскадов. Ветвление по параметрам кадра его не выключает —
//! компилятор GPU всё равно держит под него регистры, а выборки идут из массива слоёв.
//! Поэтому шейдер собирается в вариантах, и кадр берёт тот, которому хватает:
//!
//! * `0` — одна карта теней (`shadow_single.wgsl`, 2D-карта, PCF 3×3) — как до каскадов;
//! * [`CASCADES`] — каскады (`shadow.wgsl`, массив слоёв);
//! * [`LIGHTS`] — лампы сверх четырёх, прожекторы, тени ламп, заполняющий свет
//!   (`lights.wgsl`, lights.rs); без бита — `lights_off.wgsl`, где этот код отключён
//!   константой.
//!
//! Биты складываются: каскады с лампами — вариант 3.
//!
//! Вариант 0 собирается при создании рендерера, остальные — при первом кадре, которому
//! они нужны (сцена без `SoftShadows` их никогда не компилирует).

/// Бит варианта: каскадные тени.
pub(super) const CASCADES: usize = 1;
/// Бит варианта: лампы (lights.rs).
pub(super) const LIGHTS: usize = 2;
/// Сколько вариантов.
pub(super) const COUNT: usize = 4;

const COMMON: &str = include_str!("../common.wgsl");
const SHADOW_SINGLE: &str = include_str!("../shadow_single.wgsl");
const SHADOW_CASCADES: &str = include_str!("../shadow.wgsl");
const LIGHTS_ON: &str = include_str!("../lights.wgsl");
const LIGHTS_OFF: &str = include_str!("../lights_off.wgsl");

fn lights_part(variant: usize) -> &'static str {
    if variant & LIGHTS != 0 {
        LIGHTS_ON
    } else {
        LIGHTS_OFF
    }
}

fn shadow_part(variant: usize) -> &'static str {
    if variant & CASCADES != 0 {
        SHADOW_CASCADES
    } else {
        SHADOW_SINGLE
    }
}

/// Исходник шейдера мешей варианта.
pub(super) fn mesh_source(variant: usize) -> String {
    [COMMON, shadow_part(variant), lights_part(variant), include_str!("../mesh.wgsl")].concat()
}

/// Исходник шейдера рельефа варианта.
pub(super) fn terrain_source(variant: usize) -> String {
    [COMMON, shadow_part(variant), lights_part(variant), include_str!("../terrain.wgsl")].concat()
}

/// Раскладка группы 0 освещённых шейдеров варианта: параметры кадра, карта теней
/// (2D или массив слоёв) и сравнивающий сэмплер.
pub(super) fn lit_layout(device: &wgpu::Device, variant: usize) -> wgpu::BindGroupLayout {
    let data = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    // Лампы: параметры, ячейки кластеров, атлас теней, грани теней (lights.wgsl).
    let lights = [
        data(3, wgpu::TextureSampleType::Float { filterable: false }),
        data(4, wgpu::TextureSampleType::Uint),
        data(5, wgpu::TextureSampleType::Depth),
        data(6, wgpu::TextureSampleType::Float { filterable: false }),
    ];
    let base = [
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
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: if variant & CASCADES != 0 {
                    wgpu::TextureViewDimension::D2Array
                } else {
                    wgpu::TextureViewDimension::D2
                },
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
            count: None,
        },
    ];
    let mut entries = base.to_vec();
    if variant & LIGHTS != 0 {
        entries.extend_from_slice(&lights);
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals + shadow map layout"),
        entries: &entries,
    })
}

/// Карты теней для групп привязок: все слои (массив) и слой 0 (одна карта); текстуры
/// ламп (для вариантов с [`LIGHTS`]; `None` — они ещё не созданы).
pub(super) struct ShadowViews<'a> {
    pub all: &'a wgpu::TextureView,
    pub first: &'a wgpu::TextureView,
    pub sampler: &'a wgpu::Sampler,
    /// Параметры ламп, ячейки, атлас теней, грани.
    pub lights: Option<[&'a wgpu::TextureView; 4]>,
}

/// Группа 0 освещённых шейдеров варианта.
pub(super) fn lit_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    variant: usize,
    globals: &wgpu::Buffer,
    shadows: &ShadowViews,
) -> Option<wgpu::BindGroup> {
    let map = if variant & CASCADES != 0 { shadows.all } else { shadows.first };
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(map) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(shadows.sampler) },
    ];
    if variant & LIGHTS != 0 {
        let views = shadows.lights?;
        for (i, view) in views.into_iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: 3 + i as u32, resource: wgpu::BindingResource::TextureView(view) });
        }
    }
    Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("globals + shadow map"), layout, entries: &entries }))
}
