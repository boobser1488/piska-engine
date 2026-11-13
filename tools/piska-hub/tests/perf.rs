//! Время кадра интерфейса с 50 проектами в списке (без GPU: построение интерфейса egui
//! и раскладка в треугольники — то, что делает поток окна). Печатает среднее и худшее
//! время кадра; `cargo test -p piska-hub --test perf -- --nocapture`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use piska_hub::config::{HubConfig, ViewMode};
use piska_hub::{paths, Hub, HubAction, HubOptions};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn fake_projects(root: &std::path::Path, count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|i| {
            let dir = root.join(format!("perf_game_{i:02}"));
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"perf_game_{i:02}\"\n[dependencies]\nengine = {{ path = \"x\" }}\n\
                     [[bin]]\nname = \"perf_game_{i:02}-editor\"\nrequired-features = [\"editor\"]\n"
                ),
            )
            .unwrap();
            std::fs::write(dir.join("src").join("lib.rs"), "// SpriteRenderer\n").unwrap();
            dir
        })
        .collect()
}

/// Средний и худший кадр за `frames` кадров (мышь ездит по списку — наведение меняется).
fn measure(hub: &mut Hub, ctx: &egui::Context, clock: &mut f64, frames: usize) -> (Duration, Duration) {
    let mut total = Duration::ZERO;
    let mut worst = Duration::ZERO;
    for frame in 0..frames {
        let y = 160.0 + (frame % 40) as f32 * 18.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(*clock),
            predicted_dt: 1.0 / 60.0,
            events: vec![egui::Event::PointerMoved(egui::pos2(700.0, y))],
            ..egui::RawInput::default()
        };
        *clock += 1.0 / 60.0;
        let started = Instant::now();
        hub.update();
        let output = ctx.run(input, |ctx| hub.ui(ctx));
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        let spent = started.elapsed();
        total += spent;
        worst = worst.max(spent);
    }
    (total / frames as u32, worst)
}

#[test]
fn fifty_projects_render_well_within_a_frame() {
    let root = std::env::temp_dir().join(format!("piska-hub-perf-{}", std::process::id()));
    let config_dir = root.join("config");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    let projects = fake_projects(&root, 50);
    let mut config = HubConfig::default();
    for (i, dir) in projects.iter().enumerate() {
        config.touch(dir, 1_763_000_000 - i as u64 * 3000);
    }
    config.save(&config_dir.join(paths::CONFIG_FILE)).unwrap();
    let mut hub = Hub::new(HubOptions {
        config_dir: Some(config_dir),
        dry_run: true,
        ..HubOptions::default()
    });
    assert!(hub.projects().len() >= 50);

    let ctx = egui::Context::default();
    let mut clock = 0.0;
    measure(&mut hub, &ctx, &mut clock, 10); // шрифты, первая раскладка
    let (list_avg, list_worst) = measure(&mut hub, &ctx, &mut clock, 120);
    hub.apply(HubAction::SetView(ViewMode::Grid));
    measure(&mut hub, &ctx, &mut clock, 5);
    let (grid_avg, grid_worst) = measure(&mut hub, &ctx, &mut clock, 120);
    eprintln!(
        "{} projects: list {:.2} ms/frame (worst {:.2}), grid {:.2} ms/frame (worst {:.2})",
        hub.projects().len(),
        list_avg.as_secs_f64() * 1000.0,
        list_worst.as_secs_f64() * 1000.0,
        grid_avg.as_secs_f64() * 1000.0,
        grid_worst.as_secs_f64() * 1000.0
    );
    // Щедрая граница: машина под нагрузкой других сборок. Обычно — единицы миллисекунд.
    assert!(list_avg < Duration::from_millis(25), "list frame {list_avg:?}");
    assert!(grid_avg < Duration::from_millis(25), "grid frame {grid_avg:?}");
    drop(hub);
    let _ = std::fs::remove_dir_all(&root);
}
