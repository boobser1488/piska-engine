//! Снимки заставки запуска хаба, экрана ошибки и строк прогресса (`kit::boot`) без окна:
//! английский при 100 % и русский при 150 %. Куда писать — `PISKA_SHOTS_DIR` (без неё только
//! проверка, что кадр не пустой). Один тест на файл: видеокарта одна.

use std::sync::mpsc;
use std::time::Duration;

use engine_editor::kit::boot::{progress_row, BootState};
use engine_editor::kit::i18n::{self, tr_s, Language};
use engine_editor::kit::{controls, theme};
use engine_editor::UiPainter;
use piska_hub::startup;
use piska_hub::{Hub, HubOptions};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: (u32, u32) = (720, 440);

/// Рисует `frames` кадров `ui(ctx, время)` при масштабе `scale` и возвращает последний.
fn render(scale: f32, frames: usize, mut ui: impl FnMut(&egui::Context, f64)) -> image::RgbaImage {
    let size = (SIZE.0 as f32 * scale).round() as u32;
    let height = (SIZE.1 as f32 * scale).round() as u32;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let gpu = pollster::block_on(engine_render::Gpu::request(&instance, None)).expect("a GPU");
    let mut painter = UiPainter::new(&gpu.device, FORMAT);
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("loading shot"),
        size: wgpu::Extent3d { width: size, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let ctx = egui::Context::default();
    ctx.set_zoom_factor(scale);
    for i in 0..frames {
        let time = i as f64 * 0.05;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.0 as f32, SIZE.1 as f32))),
            time: Some(time),
            predicted_dt: 0.05,
            ..egui::RawInput::default()
        };
        let full = ctx.run(raw, |ctx| ui(ctx, time));
        painter.paint(&gpu.device, &gpu.queue, &ctx, full, &view, [size, height]);
    }
    engine_render::read_texture(&gpu.device, &gpu.queue, &target, size, height).expect("read back")
}

fn save(image: &image::RgbaImage, name: &str) {
    let first = image.get_pixel(0, 0);
    assert!(image.pixels().any(|p| p != first), "{name}: the picture is empty");
    if let Ok(dir) = std::env::var("PISKA_SHOTS_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image.save(dir.join(name)).unwrap();
    }
}

fn gallery(ctx: &egui::Context, density: theme::Density) {
    if !engine_editor::kit::boot::theme_ready(ctx) {
        return;
    }
    theme::set_density(ctx, density);
    egui::CentralPanel::default().frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(egui::Margin::same(theme::SP_8))).show(ctx, |ui| {
        controls::card(ui, |ui| {
            progress_row(ui, "Building the game: compiling engine-render (12 of 48)", Some(0.25), true);
            ui.add_space(theme::SP_6);
            progress_row(ui, tr_s("Looking at the files…"), None, false);
            ui.add_space(theme::SP_6);
            progress_row(ui, "Packing assets/models/characters/hero_idle_forward_and_backwards_walk_cycle.glb", Some(0.62), true);
        });
    });
}

#[test]
fn the_start_up_screens_look_right_in_english_and_russian() {
    let dir = std::env::temp_dir().join(format!("piska-hub-loading-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    for (language, scale, tag) in [(Language::English, 1.0, "en100"), (Language::Russian, 1.5, "ru150")] {
        i18n::set(language);
        // Заставка: хаб не собран, пока тест не отпустит (настоящий шаг и настоящие строки).
        let (release, gate) = mpsc::channel::<()>();
        let options = HubOptions { config_dir: Some(dir.clone()), dry_run: true, tools_dir: Some(dir.join("tools")), ..HubOptions::default() };
        let mut boot = startup::start_with(move |progress| {
            progress.set_step(tr_s("Reading settings and the project list…"));
            gate.recv().ok();
            Ok(Hub::new(options))
        });
        let image = render(scale, 6, |ctx, _| startup::frame(&mut boot, ctx));
        assert_eq!(boot.state(), BootState::Loading);
        save(&image, &format!("hub_splash_{tag}.png"));
        release.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(50));

        let mut boot = startup::start_with(|_| Err("cannot read `hub.ron`: the file is locked by another program (os error 32)".to_string()));
        std::thread::sleep(Duration::from_millis(100));
        let image = render(scale, 6, |ctx, _| startup::frame(&mut boot, ctx));
        assert_eq!(boot.state(), BootState::Failed);
        save(&image, &format!("hub_error_{tag}.png"));

        // Строки прогресса: плотность хаба.
        let image = render(scale, 6, |ctx, _| gallery(ctx, theme::Density::Comfortable));
        save(&image, &format!("progress_rows_hub_{tag}.png"));
    }
    i18n::set(Language::English);
    let _ = std::fs::remove_dir_all(&dir);
}
