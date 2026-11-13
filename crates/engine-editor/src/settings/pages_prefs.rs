//! Страницы Preferences: Appearance и Game View (поля [`Preferences`]).

use egui::{vec2, Sense, Stroke};

use super::SettingsUi;
use crate::prefs::{Preferences, FPS_CAPS, UI_SCALES};
use crate::state::Editor;
use crate::theme;

/// Appearance: масштаб интерфейса и акцентный цвет.
pub(super) fn appearance(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    s.group("Interface");
    let scales: Vec<(u32, String)> = UI_SCALES.iter().map(|p| (*p, format!("{p}%"))).collect();
    let mut percent = (prefs.ui_scale * 100.0).round() as u32;
    let current = percent;
    let options: Vec<(u32, &str)> = scales.iter().map(|(p, name)| (*p, name.as_str())).collect();
    let options = if options.iter().any(|(p, _)| *p == percent) {
        options
    } else {
        // Масштаб, выставленный Ctrl + / Ctrl −, может не совпасть со списком.
        let mut options = options;
        options.push((percent, "Custom"));
        options
    };
    s.combo(
        "UI scale",
        "Size of all text and controls. Ctrl + and Ctrl − change it too.",
        &mut percent,
        (defaults.ui_scale * 100.0).round() as u32,
        &options,
    );
    if percent != current {
        prefs.ui_scale = percent as f32 / 100.0;
    }

    let row = s.row(
        "Accent colour",
        "Colour of the selection, the focused field and the main button.",
        prefs.accent != defaults.accent,
        |ui| {
            let mut picked = None;
            for (index, (name, color)) in theme::ACCENTS.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(theme::FIELD_H + 2.0, theme::FIELD_H + 2.0), Sense::click());
                let swatch = rect.shrink(3.0);
                ui.painter().rect_filled(swatch, theme::R_S, *color);
                if prefs.accent == index {
                    ui.painter().rect_stroke(rect.shrink(0.5), theme::R_M, Stroke::new(1.5_f32, theme::TEXT_STRONG));
                } else if response.hovered() {
                    ui.painter().rect_stroke(rect.shrink(0.5), theme::R_M, Stroke::new(1.0_f32, theme::LINE_HOVER));
                }
                if response.on_hover_text(*name).clicked() {
                    picked = Some(index);
                }
            }
            if let Some((name, _)) = theme::ACCENTS.get(prefs.accent) {
                ui.add_space(theme::SP_S);
                ui.label(egui::RichText::new(*name).color(theme::TEXT_MUTED));
            }
            picked
        },
    );
    if let Some(Some(index)) = row.inner {
        prefs.accent = index;
    }
    if row.reset {
        prefs.accent = defaults.accent;
    }

    s.group("Play mode");
    s.percent(
        "Play mode tint",
        "How strongly the toolbar is tinted while the game runs. 0% leaves only the accent line.",
        &mut prefs.play_tint,
        defaults.play_tint,
    );
}

/// General: история Undo, автосохранение, внешний редактор кода.
pub(super) fn general(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    s.group("Scene");
    s.number(
        "Undo steps",
        "How many scene edits Undo remembers. Older steps are forgotten.",
        &mut prefs.undo_limit,
        defaults.undo_limit,
        10..=1000,
        1.0,
        "",
    );
    s.toggle(
        "Save before Play",
        "Save the open scene when entering Play mode, if it already has a file.",
        &mut prefs.autosave_on_play,
        defaults.autosave_on_play,
    );
    s.number(
        "Auto-save after",
        "Minutes after the first unsaved change; 0 turns auto-save off. Only in Edit mode, only scenes with a file.",
        &mut prefs.autosave_minutes,
        defaults.autosave_minutes,
        0.0..=120.0,
        0.1,
        " min",
    );

    s.group("Code");
    let row = s.row(
        "External code editor",
        "Command that opens scripts; {file} and {line} are filled in. Empty: the built-in Code tab.",
        prefs.external_editor != defaults.external_editor,
        |ui| {
            // Кнопка справа, поле — на остаток строки (длинная команда прокручивается в поле).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let test = theme::button(ui, "Test");
                let clicked = ui
                    .interact(test.rect, Editor::external_editor_test_id(), Sense::click())
                    .on_hover_text("Open the file of the Code tab (or the assets folder) with this command")
                    .clicked();
                ui.add_sized(
                    vec2(ui.available_width().max(80.0), theme::FIELD_H),
                    egui::TextEdit::singleline(&mut prefs.external_editor)
                        .font(theme::mono())
                        .hint_text(egui::RichText::new("code -g {file}:{line}").color(theme::TEXT_DISABLED))
                        .vertical_align(egui::Align::Center),
                );
                clicked
            })
            .inner
        },
    );
    if row.reset {
        prefs.external_editor = defaults.external_editor.clone();
    }
    if row.inner == Some(true) {
        editor.test_external_editor();
    }
}

/// Scene View: сетка, фон, шаги привязки, камера.
pub(super) fn scene_view(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    s.group("Grid");
    s.color("Grid colour", "Colour of the grid lines in the Scene view.", &mut prefs.grid_color, defaults.grid_color);
    s.percent(
        "Grid opacity",
        "100% is the usual strength; lower it to make the grid quieter.",
        &mut prefs.grid_opacity,
        defaults.grid_opacity,
    );
    s.color(
        "Background",
        "Colour behind the scene in the 2D and 3D Scene view (the Game view uses the camera's own).",
        &mut prefs.scene_background,
        defaults.scene_background,
    );
    s.group("Snapping");
    s.number("Move step", "Grid step of the Move tool with snapping on (or Ctrl held).", &mut prefs.snap_move, defaults.snap_move, 0.01..=100.0, 0.01, " m");
    s.number("Rotate step", "Angle step of the Rotate tool with snapping on.", &mut prefs.snap_rotate, defaults.snap_rotate, 1.0..=90.0, 0.5, "°");
    s.number("Scale step", "Factor step of the Scale tool with snapping on.", &mut prefs.snap_scale, defaults.snap_scale, 0.01..=2.0, 0.01, "");
    s.group("Camera");
    s.number("Orbit speed", "How fast the 3D camera turns with the right mouse button.", &mut prefs.orbit_speed, defaults.orbit_speed, 0.1..=5.0, 0.01, "×");
    s.toggle(
        "Invert vertical orbit",
        "Dragging up tilts the 3D camera down instead of up.",
        &mut prefs.invert_orbit_y,
        defaults.invert_orbit_y,
    );
    s.number("Zoom speed", "How far one wheel step zooms, in 2D and 3D.", &mut prefs.zoom_speed, defaults.zoom_speed, 0.1..=5.0, 0.01, "×");
}

/// Console & Diagnostics: что Console показывает при запуске, Profiler.
pub(super) fn console(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    let mut changed = false;
    s.group("Console");
    changed |= s.toggle("Show messages", "Info messages are listed when the editor starts.", &mut prefs.console_info, defaults.console_info);
    changed |= s.toggle("Show warnings", "Warnings are listed when the editor starts.", &mut prefs.console_warn, defaults.console_warn);
    changed |= s.toggle("Show errors", "Errors are listed when the editor starts.", &mut prefs.console_error, defaults.console_error);
    changed |= s.toggle("Collapse", "Identical messages are shown once, with a count.", &mut prefs.console_collapse, defaults.console_collapse);
    changed |= s.toggle(
        "Error Pause",
        "A new error in Play mode pauses the game.",
        &mut prefs.console_error_pause,
        defaults.console_error_pause,
    );
    changed |= s.toggle(
        "Clear on Play",
        "Entering Play mode empties the Console.",
        &mut prefs.console_clear_on_play,
        defaults.console_clear_on_play,
    );
    changed |= s.number(
        "Max messages",
        "The Console keeps this many newest messages.",
        &mut prefs.console_max,
        defaults.console_max,
        50..=engine_core::logging::CAPACITY,
        10.0,
        "",
    );
    s.group("Profiler");
    changed |= s.number(
        "History length",
        "How many recent frames the Profiler graph and averages cover.",
        &mut prefs.profiler_history,
        defaults.profiler_history,
        30..=3000,
        5.0,
        " frames",
    );
    if changed {
        editor.apply_panel_prefs();
    }
}

/// Game View: частота кадров окна редактора.
pub(super) fn game_view(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let fps = editor.profiler.fps();
    let prefs = &mut editor.prefs;
    s.group("Frame rate");
    s.toggle(
        "Vertical sync",
        "Match the monitor refresh rate. Off: Play mode runs as fast as it can, or at the limit below.",
        &mut prefs.vsync,
        defaults.vsync,
    );
    let names: Vec<(u32, String)> =
        FPS_CAPS.iter().map(|cap| (*cap, if *cap == 0 { "Unlimited".to_string() } else { format!("{cap} FPS") })).collect();
    let options: Vec<(u32, &str)> = names.iter().map(|(cap, name)| (*cap, name.as_str())).collect();
    s.combo(
        "Play mode limit",
        "Frames per second in Play mode when vertical sync is off.",
        &mut prefs.play_fps_cap,
        defaults.play_fps_cap,
        &options,
    );
    s.group("Edit mode");
    s.toggle(
        "Redraw on demand",
        &format!(
            "Redraw only when something changes; an idle editor redraws {} times a second and leaves the GPU alone.",
            (1000 / crate::ui::IDLE_REPAINT.as_millis().max(1)).max(1)
        ),
        &mut prefs.redraw_on_demand,
        defaults.redraw_on_demand,
    );
    if s.drawing() {
        s.note(&format!("Now: {fps:.0} FPS."));
    }
}
