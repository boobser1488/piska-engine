//! Запуск редактора с заставкой (`run_with`) без окна и GPU: кадры ведёт тот же автомат
//! (`boot_editor`), что и окно. Заставка рисуется ДО того, как редактор собран (сборка идёт
//! только после первого полного кадра в цветах темы), сбой сборки — экран ошибки с текстом,
//! а обычный `run` остался как был (та же сигнатура, редактор приходит уже готовым).
#![cfg(feature = "editor")]

use std::cell::Cell;
use std::rc::Rc;

use engine::editor::egui;
use engine::editor::kit::boot::{painted_fills, painted_texts, BootState};
use engine::editor::kit::theme;
use engine::editor::{boot_editor, Editor, EditorConfig, EditorError};

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);

fn build() -> Result<Editor, EditorError> {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).map_err(|e| EditorError::Other(e.to_string()))?;
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene"))
}

fn run(ctx: &egui::Context, boot: &mut engine::editor::Boot<Editor>, time: f64) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        ..egui::RawInput::default()
    };
    ctx.run(input, |ctx| {
        boot.frame(ctx, |ctx, editor| {
            editor.ui(ctx);
            None
        })
    })
}

#[test]
fn run_with_shows_the_splash_before_the_editor_is_built() {
    let built = Rc::new(Cell::new(false));
    let flag = built.clone();
    let mut boot = boot_editor(move |progress| {
        flag.set(true);
        progress.set_step("Scanning assets…");
        build()
    });
    assert_eq!(boot.state(), BootState::Loading);
    assert!(!built.get(), "nothing is built before the window draws its first frame");
    let ctx = egui::Context::default();
    // Кадр 0 ставит тему и шрифты: окно ещё прятать, сборку не начинать.
    run(&ctx, &mut boot, 0.0);
    assert!(!boot.themed() && !boot.should_build() && !built.get());
    // Кадр 1 — настоящая заставка в цветах темы, с шагом.
    let full = run(&ctx, &mut boot, 0.016);
    assert!(boot.themed(), "the window may be shown: a themed frame was drawn");
    assert!(!built.get(), "the editor is still not built while the first splash frame is drawn");
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t == "Piska Engine"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Loading the project")), "{texts:?}");
    let (rect, fill) = painted_fills(&full)[0];
    assert!(rect.width() >= SCREEN.x - 1.0 && fill == theme::BG_APP, "the themed background, never white or black");
    // Теперь окно занимает поток сборкой.
    assert!(boot.should_build());
    boot.run_pending();
    assert!(built.get(), "the editor is built after the splash was shown");
    assert_eq!(boot.state(), BootState::Ready);
    // Дальше рисуется сам редактор; заставка растворяется под ним, а не обрывается.
    let full = run(&ctx, &mut boot, 0.032);
    assert!(painted_texts(&full).iter().any(|t| t == "File"), "the editor's own menu is drawn: {:?}", painted_texts(&full));
    assert!(boot.splash_visible());
    let mut time = 0.032;
    while boot.splash_visible() && time < 5.0 {
        time += 0.05;
        run(&ctx, &mut boot, time);
    }
    assert!(!boot.splash_visible(), "the start-up splash goes away");
    let editor = boot.take_value().expect("the editor");
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"), "the scene named by the configuration is open");
}

#[test]
fn a_failed_build_is_an_error_screen_not_a_hang() {
    let mut boot = boot_editor(|_| Err(EditorError::Other("scene `scenes/missing.scene` not found".to_string())));
    let ctx = egui::Context::default();
    run(&ctx, &mut boot, 0.0);
    run(&ctx, &mut boot, 0.016);
    boot.run_pending();
    assert_eq!(boot.state(), BootState::Failed);
    let texts = painted_texts(&run(&ctx, &mut boot, 0.032));
    assert!(texts.iter().any(|t| t.contains("could not start")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("scenes/missing.scene")), "the message is on the screen: {texts:?}");
    assert!(texts.iter().any(|t| t == "Close"), "{texts:?}");
}

#[test]
fn plain_run_keeps_its_signature_and_takes_a_ready_editor() {
    // `run` и каждый `edit` игры принимают уже собранный редактор, как раньше.
    let _: fn(Editor) -> Result<Editor, EditorError> = engine::editor::run;
    // Редактор без заставки запуска (то, что даёт `run`): интерфейс рисуется с первого полного кадра.
    let mut editor = build().unwrap();
    let ctx = egui::Context::default();
    for i in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(f64::from(i) * 0.016),
            predicted_dt: 1.0 / 60.0,
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, |ctx| editor.ui(ctx));
    }
    assert!(editor.themed(), "the editor draws without any start-up boot");
    assert!(!editor.splash_visible(), "its own splash is only enabled by the window runner");
}
