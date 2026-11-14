//! Замер интерфейса установщика без окна и GPU (`kit::metrics`, плотность хаба —
//! [`Rules::comfortable`]): каждая страница мастера (поддельные скрипты, НИЧЕГО НЕ
//! УСТАНАВЛИВАЕТСЯ) × английский и русский × масштаб 100 / 125 / 150 %. Те же нарушения, что в
//! хабе и редакторе; на каждой странице контролам прогоняются наведение, нажатие и фокус.
//!
//! `UI_METRICS_OUT=<файл>` — записать сводку в Markdown.

mod common;

use std::time::Duration;

use common::*;
use engine_editor::kit::i18n::Language;
use engine_editor::kit::metrics::{self, Report, Rules};
use piska_installer::app::Page;
use piska_installer::{Installer, InstallerOptions};

const LONG: Duration = Duration::from_secs(60);

fn measure(hands: &mut Hands, installer: &mut Installer, screen: &str, total: &mut Report, states: &mut usize) {
    hands.frames(installer, 4);
    let rules = Rules::comfortable();
    total.merge(metrics::analyze_shapes(screen, &metrics::widgets(&hands.ctx), &hands.shapes, rules));
    let ctx = hands.ctx.clone();
    let (report, n) = metrics::analyze_states(screen, &ctx, rules, |events| {
        hands.frame(installer, events);
        hands.shapes.clone()
    });
    total.merge(report);
    *states += n;
}

#[test]
fn installer_pages_measure_up() {
    let _serial = serial();
    let mut total = Report::default();
    let (mut screens, mut states) = (0, 0);
    for language in [Language::English, Language::Russian] {
        for scale in [1.0_f32, 1.25, 1.5] {
            let what = |page: &str| format!("{language:?} {scale}x {page}");
            let options = InstallerOptions { language: Some(language), ..InstallerOptions::default() };
            let (mut installer, _log) = fake_installer(Scenario::Success, 500 << 30, options);
            installer.ui_scale = scale;
            let mut hands = Hands::new(&mut installer);
            hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
            measure(&mut hands, &mut installer, &what("welcome"), &mut total, &mut states);
            for (button, page) in [("next", "components"), ("next", "options"), ("next", "licences")] {
                hands.click(&mut installer, button);
                measure(&mut hands, &mut installer, &what(page), &mut total, &mut states);
                screens += 1;
            }
            for l in installer.licences_needed() {
                hands.click(&mut installer, &format!("licence-{}", Installer::licence_key(&l)));
            }
            hands.click(&mut installer, "next");
            assert_eq!(installer.page, Page::Review);
            hands.until(&mut installer, LONG, "the plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
            measure(&mut hands, &mut installer, &what("review"), &mut total, &mut states);
            hands.click(&mut installer, "install");
            if installer.dialog.is_some() {
                measure(&mut hands, &mut installer, &what("elevation-dialog"), &mut total, &mut states);
                hands.click(&mut installer, "dialog-ok");
            }
            hands.frames(&mut installer, 2);
            let s = what("installing");
            hands.frames(&mut installer, 4);
            total.merge(metrics::analyze_shapes(&s, &metrics::widgets(&hands.ctx), &hands.shapes, Rules::comfortable()));
            hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
            measure(&mut hands, &mut installer, &what("finish"), &mut total, &mut states);
            screens += 5;

            let options = InstallerOptions { language: Some(language), uninstall: true, ..InstallerOptions::default() };
            let (mut installer, _log) = fake_installer(Scenario::Success, 500 << 30, options);
            installer.ui_scale = scale;
            let mut hands = Hands::new(&mut installer);
            hands.until(&mut installer, LONG, "the uninstall plan", |i| i.uninstall_plan.as_ref().is_some_and(|p| !p.running()));
            measure(&mut hands, &mut installer, &what("uninstall"), &mut total, &mut states);
            screens += 1;
        }
    }
    let report = format!(
        "# Installer UI metrics\n\n{screens} screens, {states} control states.\n\n## Control heights (pt)\n\n| height | count |\n|---|---|\n{}\n\n## Gaps between row neighbours (pt)\n\n| gap | count |\n|---|---|\n{}\n\n## Offenders ({})\n\n{}\n",
        Report::table(&total.heights),
        Report::table(&total.gaps),
        total.offenders.len(),
        total.offenders.iter().map(|o| format!("* {o}")).collect::<Vec<_>>().join("\n")
    );
    if let Some(out) = std::env::var_os("UI_METRICS_OUT") {
        std::fs::write(out, &report).unwrap();
    }
    assert!(screens >= 50, "pages measured: {screens}");
    assert!(states > 200, "control states driven: {states}");
    assert!(total.offenders.is_empty(), "{} offenders, first ones:\n{}", total.offenders.len(), total.offenders.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
}
