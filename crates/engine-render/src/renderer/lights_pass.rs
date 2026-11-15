//! Текстуры ламп на GPU (lights.rs, lights.wgsl): параметры ламп, ячейки кластеров,
//! грани теней и атлас теней. Создаются при первом кадре со сценой «с лампами», атлас —
//! при первой лампе с тенью.

use crate::lights::{LightFrame, ATLAS_SIZE, CLUSTER_X, CLUSTER_Y, CLUSTER_Z, FACE_TEXELS, MAX_LIGHTS, PARAM_TEXELS, TILES};

pub(super) struct LightsGpu {
    params: wgpu::Texture,
    params_view: wgpu::TextureView,
    cells: wgpu::Texture,
    cells_view: wgpu::TextureView,
    faces: wgpu::Texture,
    faces_view: wgpu::TextureView,
    /// Атлас теней ламп (глубина): `None`, пока ни одной лампы с тенью не было.
    atlas: Option<wgpu::TextureView>,
    /// Заглушка 1×1 вместо атласа.
    no_atlas: wgpu::TextureView,
}

fn data_texture(device: &wgpu::Device, label: &'static str, width: u32, height: u32, format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn depth_texture(device: &wgpu::Device, label: &'static str, side: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl LightsGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let (params, params_view) = data_texture(device, "light params", PARAM_TEXELS, MAX_LIGHTS as u32, wgpu::TextureFormat::Rgba32Float);
        let (cells, cells_view) = data_texture(device, "light clusters", CLUSTER_X, CLUSTER_Y * CLUSTER_Z, wgpu::TextureFormat::Rgba32Uint);
        let (faces, faces_view) = data_texture(device, "light shadow faces", FACE_TEXELS, TILES as u32, wgpu::TextureFormat::Rgba32Float);
        Self {
            params,
            params_view,
            cells,
            cells_view,
            faces,
            faces_view,
            atlas: None,
            no_atlas: depth_texture(device, "no light shadows", 1),
        }
    }

    /// Создать атлас теней, если его нет. `true` — создан сейчас (группы привязок — заново).
    pub fn ensure_atlas(&mut self, device: &wgpu::Device) -> bool {
        if self.atlas.is_some() {
            return false;
        }
        self.atlas = Some(depth_texture(device, "light shadow atlas", ATLAS_SIZE));
        true
    }

    pub fn atlas(&self) -> Option<&wgpu::TextureView> {
        self.atlas.as_ref()
    }

    /// Виды для группы привязок: параметры, ячейки, атлас, грани.
    pub fn views(&self) -> [&wgpu::TextureView; 4] {
        [&self.params_view, &self.cells_view, self.atlas.as_ref().unwrap_or(&self.no_atlas), &self.faces_view]
    }

    /// Загрузить данные кадра. Возвращает число байт.
    pub fn upload(&self, queue: &wgpu::Queue, frame: &LightFrame) -> u64 {
        let write = |texture: &wgpu::Texture, bytes: &[u8], width_texels: u32, rows: u32| {
            if rows == 0 {
                return;
            }
            queue.write_texture(
                wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytes,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(width_texels * 16), rows_per_image: Some(rows) },
                wgpu::Extent3d { width: width_texels, height: rows, depth_or_array_layers: 1 },
            );
        };
        let params: &[u8] = bytemuck::cast_slice(&frame.params);
        write(&self.params, params, PARAM_TEXELS, frame.light_count as u32);
        let cells: &[u8] = bytemuck::cast_slice(&frame.cells);
        write(&self.cells, cells, CLUSTER_X, CLUSTER_Y * CLUSTER_Z);
        let faces: &[u8] = bytemuck::cast_slice(&frame.faces);
        write(&self.faces, faces, FACE_TEXELS, frame.shadow_faces.len() as u32);
        (params.len() + cells.len() + faces.len()) as u64
    }
}
