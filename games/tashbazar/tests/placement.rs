//! Расстановка на картах проверяется кодом, а не только глазами: ничего не висит в
//! воздухе и не утоплено, у моделей нет оторванных частей, начало координат реквизита — у
//! основания, нет мерцающих совпадающих граней, реквизит не входит в стены, настенное
//! касается стены (подробно — `tests/common/check.rs`). Печатает таблицу нарушителей.
//! `cargo test -p tashbazar --test placement -- --nocapture`;
//! `TASHBAZAR_CHECK_ASSETS=<каталог>` — проверить другую копию ассетов.

mod common;

use common::check::{check, table};
use engine::prelude::*;

fn run(scene: &str) -> String {
    std::env::set_var("ENGINE_AUDIO", "off");
    let assets = std::env::var("TASHBAZAR_CHECK_ASSETS").unwrap_or_else(|_| common::assets().to_string());
    let mut app = tashbazar::build_in(SaveData::in_memory(), assets, scene).expect("the game builds");
    app.run_headless(2, 1.0 / 64.0); // коллайдеры в запросах физики
    let offenders = check(&app.world);
    let text = table(&offenders);
    println!("== {scene}\n{text}");
    if offenders.is_empty() {
        String::new()
    } else {
        format!("{scene}: {}", text.lines().next().unwrap_or_default())
    }
}

#[test]
fn nothing_floats_sinks_breaks_apart_or_flickers() {
    let failures: Vec<String> = [tashbazar::MAP_SCENE, tashbazar::GYM_SCENE].iter().map(|s| run(s)).filter(|s| !s.is_empty()).collect();
    assert!(failures.is_empty(), "placement offenders (see the table above):\n{}", failures.join("\n"));
}
