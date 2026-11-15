//! Замер экрана ошибки запуска и строк прогресса (`kit::boot`) тем же щупом, что весь интерфейс
//! (`kit::metrics`): английский и русский × масштаб 100 / 125 / 150 % × плотность редактора и
//! хаба, окна от самого узкого до широкого, кнопкам — наведение, нажатие, фокус. Нарушений нет:
//! высота контролов — токен, ничто не выходит из контейнера, текст не налезает на соседей.

use std::time::Duration;

use engine_editor::kit::boot::{progress_row, Boot, BootState, Task};
use engine_editor::kit::i18n::{self, Language};
use engine_editor::kit::metrics::{self, Report, Rules};
use engine_editor::kit::{controls, theme};

fn input(width: f32, time: f64, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 520.0))),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        events,
        ..egui::RawInput::default()
    }
}

fn mark(painter: &egui::Painter, rect: egui::Rect, alpha: f32) {
    engine_editor::kit::window::paint_engine_mark(painter, rect, alpha);
}

fn rows(ctx: &egui::Context, density: theme::Density) {
    if !engine_editor::kit::boot::theme_ready(ctx) {
        return;
    }
    theme::set_density(ctx, density);
    egui::CentralPanel::default().frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(egui::Margin::same(theme::SP_8))).show(ctx, |ui| {
        controls::card(ui, |ui| {
            progress_row(ui, "Building the game: compiling engine-render (12 of 48)", Some(0.25), true);
            ui.add_space(theme::SP_6);
            progress_row(ui, "Counting files…", None, false);
            ui.add_space(theme::SP_6);
            progress_row(ui, "Packing assets/models/characters/hero_idle_forward_and_backwards_walk_cycle.glb", Some(0.62), true);
        });
    });
}

#[test]
fn progress_rows_and_the_error_screen_measure_up() {
    let mut total = Report::default();
    let mut states = 0;
    for language in [Language::English, Language::Russian] {
        i18n::set(language);
        for scale in [1.0_f32, 1.25, 1.5] {
            for width in [640.0_f32, 960.0, 1400.0] {
                // Строки прогресса в обеих плотностях.
                for (density, rules, tag) in [(theme::Density::Compact, Rules::compact(), "editor"), (theme::Density::Comfortable, Rules::comfortable(), "hub")] {
                    let ctx = egui::Context::default();
                    ctx.set_zoom_factor(scale);
                    let mut time = 0.0;
                    let mut frame = |ctx: &egui::Context, events: Vec<egui::Event>| {
                        time += 1.0 / 60.0;
                        ctx.run(input(width, time, events), |ctx| rows(ctx, density)).shapes
                    };
                    for _ in 0..3 {
                        frame(&ctx, Vec::new());
                    }
                    let screen = format!("{language:?} {scale}x {width}px progress rows ({tag})");
                    let shapes = frame(&ctx, Vec::new());
                    total.merge(metrics::analyze_shapes(&screen, &metrics::widgets(&ctx), &shapes, rules));
                    let (report, n) = metrics::analyze_states(&screen, &ctx.clone(), rules, |events| frame(&ctx, events));
                    total.merge(report);
                    states += n;
                }
                // Экран ошибки запуска.
                let task = Task::<u32>::spawn("metrics-fail", "x", |_| Err("cannot read `assets/scenes/main.scene`: the file is locked by another program (os error 32)".to_string()));
                let mut boot = Boot::start("Piska Engine", "1.0", mark, task);
                let ctx = egui::Context::default();
                ctx.set_zoom_factor(scale);
                let mut time = 0.0;
                let mut frame = |ctx: &egui::Context, boot: &mut Boot<u32>, events: Vec<egui::Event>| {
                    time += 1.0 / 60.0;
                    ctx.run(input(width, time, events), |ctx| boot.frame(ctx, |_, _| None)).shapes
                };
                let started = std::time::Instant::now();
                while boot.state() == BootState::Loading && started.elapsed() < Duration::from_secs(5) {
                    frame(&ctx, &mut boot, Vec::new());
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert_eq!(boot.state(), BootState::Failed);
                for _ in 0..3 {
                    frame(&ctx, &mut boot, Vec::new());
                }
                let screen = format!("{language:?} {scale}x {width}px error screen");
                let shapes = frame(&ctx, &mut boot, Vec::new());
                total.merge(metrics::analyze_shapes(&screen, &metrics::widgets(&ctx), &shapes, Rules::compact()));
                let (report, n) = metrics::analyze_states(&screen, &ctx.clone(), Rules::compact(), |events| frame(&ctx, &mut boot, events));
                total.merge(report);
                states += n;
            }
        }
    }
    i18n::set(Language::English);
    assert!(states > 40, "control states driven: {states}");
    assert!(total.offenders.is_empty(), "{} offenders, first ones:\n{}", total.offenders.len(), total.offenders.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
}
