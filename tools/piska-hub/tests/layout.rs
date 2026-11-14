//! Раскладка списка проектов без окна и GPU: ни одна подпись строки или карточки
//! («Opened 13 hours ago» / «Открыт 13 часов назад», «Built …») не заходит под кнопки
//! «Open» / «Play» / «⋯» — в списке и сетке, на английском и русском, в масштабе 100% и
//! 150%, от узкого окна до широкого.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine_editor::kit::i18n::{self, Language};
use piska_hub::config::ViewMode;
use piska_hub::{paths, Hub, HubAction, HubOptions};

const DT: f32 = 1.0 / 60.0;
const NOW: u64 = 1_763_040_000;
static LANGUAGE: Mutex<()> = Mutex::new(());

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn staged_hub(config_dir: &Path) -> (Hub, Vec<PathBuf>) {
    let games = paths::find_engine_root().unwrap().join("games");
    let mut config = piska_hub::config::HubConfig::default();
    let projects: Vec<PathBuf> = ["lantern", "swarm", "template", "marble", "tashbazar"].iter().map(|g| games.join(g)).collect();
    // «13 часов назад» — самая длинная русская подпись из тех, что видел пользователь.
    for (i, project) in projects.iter().enumerate() {
        config.touch(project, NOW - [13 * 3600, 26 * 3600, 7 * 60, 3 * 86_400, 13 * 3600][i]);
    }
    config.save(&config_dir.join(paths::CONFIG_FILE)).unwrap();
    let tools = config_dir.join("tools");
    let _ = std::fs::create_dir_all(&tools);
    let mut hub = Hub::new(HubOptions {
        config_dir: Some(config_dir.to_path_buf()),
        dry_run: true,
        tools_dir: Some(tools),
        ..HubOptions::default()
    });
    hub.set_now(NOW);
    (hub, projects)
}

fn run(ctx: &egui::Context, hub: &mut Hub, size: egui::Vec2, time: &mut f64) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(*time),
        predicted_dt: DT,
        ..egui::RawInput::default()
    };
    *time += f64::from(DT);
    hub.update();
    let _ = ctx.run(input, |ctx| hub.ui(ctx));
}

#[test]
fn project_captions_never_run_under_the_buttons() {
    let _guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    let config = temp("captions");
    let mut checked = 0;
    for language in [Language::English, Language::Russian] {
        i18n::set(language);
        for scale in [1.0_f32, 1.5] {
            for view in [ViewMode::List, ViewMode::Grid] {
                for width in [760.0_f32, 980.0, 1280.0, 1700.0, 2200.0] {
                    let (mut hub, projects) = staged_hub(&config);
                    hub.apply(HubAction::SetUiScale(scale));
                    hub.apply(HubAction::SetView(view));
                    let ctx = egui::Context::default();
                    let mut time = 0.0;
                    // Размер окна в физических пикселях: при 150% точек меньше.
                    let size = egui::vec2(width, 900.0) / scale;
                    for _ in 0..6 {
                        run(&ctx, &mut hub, size, &mut time);
                    }
                    for project in &projects {
                        let row = Hub::row_id(project);
                        let texts: Vec<(&str, egui::Rect)> = ["caption", "opened", "built"]
                            .into_iter()
                            .filter_map(|name| ctx.read_response(row.with(name)).map(|r| (name, r.rect)))
                            .collect();
                        let buttons: Vec<(&str, egui::Rect)> = ["open", "play", "more", "remove", "locate"]
                            .into_iter()
                            .filter_map(|name| ctx.read_response(row.with(name)).map(|r| (name, r.rect)))
                            .collect();
                        if buttons.is_empty() {
                            continue; // ряд за пределами экрана
                        }
                        for (text, text_rect) in &texts {
                            checked += 1;
                            for (button, button_rect) in &buttons {
                                assert!(
                                    !text_rect.intersects(*button_rect),
                                    "{language:?} {scale}x {view:?} {width}px, {}: `{text}` {text_rect:?} runs under `{button}` {button_rect:?}",
                                    project.display()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    i18n::set(Language::English);
    assert!(checked > 40, "the captions were found on screen ({checked} checked)");
    let _ = std::fs::remove_dir_all(&config);
}

/// Страница Tools: кнопки карточек никогда не выходят за поле содержимого карточки, карточки
/// ряда одной высоты и не налезают друг на друга — на английском и русском, в масштабе
/// 100 / 125 / 150 %, от наименьшего окна (960 точек) до широкого.
#[test]
fn tools_cards_keep_their_buttons_inside() {
    let _guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    let config = temp("tools");
    let mut checked = 0;
    for language in [Language::English, Language::Russian] {
        i18n::set(language);
        for scale in [1.0_f32, 1.25, 1.5] {
            for width in [960.0_f32, 1100.0, 1280.0, 1600.0, 2200.0] {
                let (mut hub, projects) = staged_hub(&config);
                hub.apply(HubAction::SetUiScale(scale));
                hub.apply(HubAction::Select(Some(projects[0].clone())));
                hub.apply(HubAction::Navigate(piska_hub::Page::Tools));
                let ctx = egui::Context::default();
                let mut time = 0.0;
                // Окно в физических пикселях: наименьшее окно хаба — 960×600 точек при 100 %.
                let size = egui::vec2(width * scale, 900.0 * scale) / scale;
                for _ in 0..6 {
                    run(&ctx, &mut hub, size, &mut time);
                }
                let what = format!("{language:?} {scale}x {width}pt");
                let mut cards = Vec::new();
                for i in 0..3usize {
                    let card_id = egui::Id::new("hub-tool").with(i);
                    let card = ctx.read_response(card_id.with("card")).unwrap_or_else(|| panic!("{what}: card {i} is drawn")).rect;
                    let content: egui::Rect = ctx.data(|d| d.get_temp(card_id.with("content"))).unwrap();
                    assert!(card.right() <= size.x + 0.5, "{what}: card {i} {card:?} leaves the window");
                    for name in ["open", "doc", "new", "export"] {
                        if let Some(button) = ctx.read_response(card_id.with(name)) {
                            checked += 1;
                            assert!(
                                content.expand(0.5).contains_rect(button.rect),
                                "{what}: `{name}` {:?} leaves card {i}'s content {content:?}",
                                button.rect
                            );
                        }
                    }
                    cards.push(card);
                }
                for a in 0..3 {
                    for b in a + 1..3 {
                        assert!(!cards[a].intersects(cards[b].shrink(0.5)), "{what}: cards {a} and {b} overlap");
                        if (cards[a].top() - cards[b].top()).abs() < 0.5 {
                            assert!((cards[a].height() - cards[b].height()).abs() < 0.5, "{what}: cards in a row share one height");
                        }
                    }
                }
            }
        }
    }
    i18n::set(Language::English);
    assert!(checked > 100, "buttons checked: {checked}");
    let _ = std::fs::remove_dir_all(&config);
}
