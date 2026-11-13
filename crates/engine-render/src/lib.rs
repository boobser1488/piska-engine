//! `engine-render` — рендер на `wgpu` (Vulkan / Metal / DX12 / WebGPU).
//!
//! Что есть сейчас:
//! * 2D: спрайты с батчингом по текстуре, слои сортировки;
//! * 3D: меши (встроенные формы, `.obj`, `.gltf`) с PBR-материалом, солнце с тенями,
//!   точечные источники, небо по высоте солнца, туман, MSAA, HDR с блумом и
//!   тонмаппингом ACES, текстуры с мипмапами и тайлингом в мировых координатах;
//! * ортографическая и перспективная камера с маской слоёв, окно на `winit`,
//!   захват кадра в картинку без окна ([`Renderer::capture`]).
//!
//! Рендер только *читает* мир: `renderer.render(&world, …)`. Компоненты
//! [`SpriteRenderer`] и [`Camera`] — обычные данные с reflection.
#![forbid(unsafe_code)]

#[macro_use]
mod rtmacro;

mod animation;
mod camera2d;
mod components;
mod draw2d;
mod grid;
mod overlay;
mod particles;
mod tilemap;
mod trail;
mod post;
mod primitives;
mod renderer;
mod terrain;
mod terrain_data;
mod terrain_mesh;
mod terrain_noise;
mod terrain_textures;
mod appicon;
mod window;

pub use components::{
    Background, Camera, Environment, Light, LightKind, MeshRenderer, Projection, SpriteRenderer,
};
pub use animation::{
    AnimationClip, AnimationFinished, ClipError, ClipMode, SpriteAnimator, ANIMATION_FINISHED,
};
pub use camera2d::{CameraController2D, SHAKE_MESSAGE};
pub use particles::{
    EmitterShape, Particle, ParticleBlend, ParticleShape, ParticleSystem, SimulationSpace,
};
pub use tilemap::{TileCollision, TileGrid, TileRect, Tilemap, TILE_COLLIDER_NAME};
pub use trail::TrailRenderer;
pub use grid::{GridPlane, SceneGrid};
pub use overlay::{Overlay, OverlayBatch, OverlayImage, OverlayTexture, OverlayVertex};
/// Версия wgpu, на которой собран рендер: для своих целей отрисовки (`render_with`).
pub use wgpu;
pub use post::PostSettings;
pub use primitives::MeshShape;
pub use renderer::{read_texture, transform_bounds, CameraView, Gpu, RenderStats, Renderer};
pub use appicon::{decode_icon, with_window_icon, ENGINE_ICON_PNG, ENGINE_ICON_SMALL_PNG};
pub use window::{run, WindowConfig, WEB_CANVAS_ID};
pub use renderer::TerrainStats;
pub use terrain::{
    snap_to_terrain, terrain_at, terrain_height, terrain_normal, terrain_raycast, BrushTool, Terrain,
    TerrainBrush, TerrainEdits, TerrainHit, TerrainSource, Water, TERRAIN_LAYERS, TERRAIN_SHAPE_FIELDS,
};
pub use terrain_data::{TerrainLayout, TerrainSampler, LAYER_COUNT as TERRAIN_LAYER_COUNT};
#[cfg(target_os = "android")]
pub use window::{set_android_app, AndroidApp};

use engine_core::reflect::TypeRegistry;

/// Зарегистрировать компоненты рендера в реестре типов (для загрузки сцен).
pub fn register_components(registry: &mut TypeRegistry) {
    registry
        .register::<SpriteRenderer>()
        .register::<Camera>()
        .register::<MeshRenderer>()
        .register::<Light>()
        .register::<Environment>()
        .register::<SpriteAnimator>()
        .register::<ParticleSystem>()
        .register::<Tilemap>()
        .register::<TrailRenderer>()
        .register::<CameraController2D>()
        .register::<Terrain>()
        .register::<Water>();
    // Категории меню Add Component в редакторе.
    registry
        .categorize(
            &[
                "SpriteRenderer", "Camera", "MeshRenderer", "Light", "Environment", "Tilemap", "CameraController2D",
                "Terrain", "Water",
            ],
            "Rendering",
        )
        .categorize(&["SpriteAnimator", "ParticleSystem", "TrailRenderer"], "Effects");
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(
        "no compatible GPU adapter found. Update the graphics driver, or check that \
         Vulkan/DX12/Metal is available on this machine"
    )]
    NoAdapter,
    #[error("GPU device request failed: {0}")]
    Device(String),
    #[error("cannot create the window or its surface: {0}")]
    Window(String),
    #[error("frame capture failed: {0}")]
    Capture(String),
    /// Видеокарта или браузер не умеют того, без чего рендер не работает.
    #[error("the GPU is not capable enough: {0}")]
    Unsupported(String),
}
