//! Как найти редактор моделей и язык Scenario: двойной щелчок по `.pmodel` в окне Project
//! открывает Piska Modeler (а без программы — окно с объяснением, а не тишину), по
//! `.scenario` — вкладку Code; меню Tools и Help, Getting Started. Без окна и GPU.
#![cfg(feature = "editor")]

mod files_support;

use engine::editor::{egui, ViewTab};
use files_support::*;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

fn sandbox() -> Sandbox {
    // Программы Piska Modeler «нет»: тест не открывает окон.
    std::env::set_var("PISKA_MODELER", "no-such-program-for-tests");
    let sandbox = Sandbox::new("discover");
    let model = std::fs::read_to_string(format!("{ASSETS}/models/crate.pmodel")).unwrap();
    let scenario = std::fs::read_to_string(format!("{ASSETS}/scenarios/demo.scenario")).unwrap();
    sandbox.file("crate.pmodel", &model).file("intro.scenario", &scenario);
    sandbox
}

#[test]
fn double_click_on_a_model_without_the_modeler_explains_how_to_get_it() {
    let sandbox = sandbox();
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    assert!(!editor.is_modeler_missing_shown());
    let model = hands.asset(&editor, "crate.pmodel");
    hands.double_click(&mut editor, model);
    assert!(editor.is_modeler_missing_shown(), "a dialog says how to build piska-modeler");
    assert!(hands.find_text(&mut editor, "cargo build --manifest-path modeler/Cargo.toml -p piska-modeler").is_some());
}

#[test]
fn double_click_on_a_scenario_opens_it_in_the_code_tab() {
    let sandbox = sandbox();
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let scenario = hands.asset(&editor, "intro.scenario");
    hands.double_click(&mut editor, scenario);
    hands.frames(&mut editor, 2);
    assert_eq!(editor.view_tab(), ViewTab::Code);
    let (path, _, _) = editor.code_document().expect("a document is open");
    assert!(path.ends_with("intro.scenario"), "{}", path.display());
}

#[test]
fn tools_and_help_menus_lead_to_the_modeler_scenarios_and_getting_started() {
    let sandbox = sandbox();
    let mut editor = editor_over(&sandbox.root);
    let mut hands = Hands::new(&mut editor);
    let tools = hands.find_text(&mut editor, "Tools").expect("a Tools menu");
    hands.click(&mut editor, tools);
    assert!(hands.find_text(&mut editor, "New Scenario Script…").is_some());
    let modeler = hands.find_text(&mut editor, "Model Editor (Piska Modeler)").expect("Tools → Model Editor");
    hands.click(&mut editor, modeler);
    assert!(editor.is_modeler_missing_shown());
    hands.press(&mut editor, egui::Key::Escape);

    editor.show_getting_started(true);
    hands.frames(&mut editor, 2);
    assert!(hands.find_text(&mut editor, "Write a Scenario script").is_some());
    editor.run_getting_started("scenario");
    hands.frames(&mut editor, 2);
    assert!(!editor.is_getting_started_open(), "the card's button closes the window");
    assert!(hands.find_text(&mut editor, "New Scenario Script").is_some(), "and opens the new-scenario dialog");

    // F1 — шпаргалка по клавишам из реестра.
    hands.press(&mut editor, egui::Key::Escape);
    hands.press(&mut editor, egui::Key::F1);
    assert!(editor.is_shortcuts_help_open());
    assert!(hands.find_text(&mut editor, "Keyboard Shortcuts").is_some());
}
