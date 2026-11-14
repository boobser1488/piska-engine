//! Запуск хаба без окна и GPU: окно показывается только после полного кадра в цветах темы
//! (`themed`), заставка пишет настоящий шаг («Scanning projects…»), держится не меньше
//! 0.6 с и уходит, когда список готов; без заставки (тесты, снимки) её нет вовсе.

use std::path::Path;

use piska_hub::{Hub, HubOptions};

fn hub(config_dir: &Path) -> Hub {
    let tools = config_dir.join("tools");
    let _ = std::fs::create_dir_all(&tools);
    Hub::new(HubOptions { config_dir: Some(config_dir.to_path_buf()), dry_run: true, tools_dir: Some(tools), ..HubOptions::default() })
}

fn run(ctx: &egui::Context, hub: &mut Hub, time: f64) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        ..egui::RawInput::default()
    };
    hub.update();
    let _ = ctx.run(input, |ctx| hub.ui(ctx));
}

#[test]
fn the_window_waits_for_a_themed_frame_and_the_splash_follows_the_scan() {
    let dir = std::env::temp_dir().join(format!("piska-hub-splash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut hub = hub(&dir);
    hub.enable_splash();
    assert!(!hub.themed(), "nothing drawn yet: the window stays hidden");
    assert!(hub.splash_visible());
    let ctx = egui::Context::default();
    // Первый кадр только ставит тему и шрифты (кадр пропускается) — окно ещё скрыто.
    run(&ctx, &mut hub, 0.0);
    assert!(!hub.themed(), "the theme-setting frame is not shown");
    run(&ctx, &mut hub, 0.016);
    assert!(hub.themed(), "a full themed frame was drawn: the window may be shown");

    // Пока идёт обход проектов — шаг на заставке; потом заставка уходит (не раньше 0.6 с).
    let mut time = 0.032;
    let mut saw_step = hub.startup_step().is_some();
    while hub.splash_visible() && time < 30.0 {
        run(&ctx, &mut hub, time);
        saw_step |= hub.startup_step().is_some();
        time += 0.05;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!hub.splash_visible(), "the splash goes away once the list is ready");
    assert!(time >= 0.6, "the splash stays at least 0.6 s once visible (gone at {time:.2} s)");
    let _ = saw_step;
    assert!(hub.startup_step().is_none(), "nothing left to load");

    // Без заставки (тесты, снимки) её нет.
    let mut plain = self::hub(&dir);
    run(&ctx, &mut plain, 0.0);
    assert!(!plain.splash_visible());
    let _ = std::fs::remove_dir_all(&dir);
}
