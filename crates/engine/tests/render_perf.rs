//! Замер времени кадра (GPU, с ожиданием) на долине витрины и на `terrain.scene`,
//! 1280 × 720, камерой сцены. Для сравнения до / после правок шейдеров:
//! `cargo test -p engine --test render_perf -- --ignored --nocapture`.

#[path = "../examples/showcase/main.rs"]
#[allow(dead_code)]
mod showcase;

use std::path::PathBuf;

use engine::prelude::*;
use engine_render::{wgpu, Renderer};

fn frame_times(renderer: &mut Renderer, app: &App, frames: usize) -> (f64, f64) {
    let (w, h) = (1280u32, 720u32);
    let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("perf target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = target.create_view(&Default::default());
    // Прогрев: загрузка рельефа и ассетов.
    for _ in 0..5 {
        renderer.render(&app.world, &target, w, h);
        renderer.device().poll(wgpu::Maintain::Wait);
    }
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        let clock = std::time::Instant::now();
        renderer.render(&app.world, &target, w, h);
        renderer.device().poll(wgpu::Maintain::Wait);
        times.push(clock.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    (times.iter().sum::<f64>() / times.len() as f64, times[times.len() / 2])
}

#[test]
#[ignore]
fn perf_valley_and_terrain_scene() {
    std::env::set_var("ENGINE_AUDIO", "off");
    std::env::set_var("SHOWCASE_FRAMES", "10");
    let assets = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let mut renderer = Renderer::headless().expect("GPU");
    let mut valley = showcase::load_showcase(&assets, "valley", "hero").expect("valley loads");
    valley.run_headless(10, 1.0 / 60.0);
    let (mean, median) = frame_times(&mut renderer, &valley, 120);
    println!("valley hero 1280x720: mean {mean:.2} ms, median {median:.2} ms");
    let mut terrain = engine::app();
    terrain.world.load_scene_str(include_str!("../../../assets/scenes/terrain.scene"), "scenes/terrain.scene").unwrap();
    let (mean, median) = frame_times(&mut renderer, &terrain, 120);
    println!("terrain.scene camera 1280x720: mean {mean:.2} ms, median {median:.2} ms");
}
