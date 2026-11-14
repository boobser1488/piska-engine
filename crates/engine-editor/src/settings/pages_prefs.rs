//! Страницы Preferences: Appearance и Game View (поля [`Preferences`]).

use crate::i18n::{tr_s, trf};
use egui::{vec2, Sense, Stroke};

use super::SettingsUi;
use crate::prefs::{Preferences, FPS_CAPS, UI_SCALES};
use crate::state::Editor;
use crate::theme;

/// Appearance: масштаб интерфейса и акцентный цвет.
pub(super) fn appearance(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    s.group(tr_s("Interface"));
    let scales: Vec<(u32, String)> = UI_SCALES.iter().map(|p| (*p, format!("{p}%"))).collect();
    let mut percent = (prefs.ui_scale * 100.0).round() as u32;
    let current = percent;
    let options: Vec<(u32, &str)> = scales.iter().map(|(p, name)| (*p, name.as_str())).collect();
    let options = if options.iter().any(|(p, _)| *p == percent) {
        options
    } else {
        // Масштаб, выставленный Ctrl + / Ctrl −, может не совпасть со списком.
        let mut options = options;
        options.push((percent, tr_s("Custom")));
        options
    };
    s.combo(
        tr_s("UI scale"),
        tr_s("Size of all text and controls. Ctrl + and Ctrl − change it too."),
        &mut percent,
        (defaults.ui_scale * 100.0).round() as u32,
        &options,
    );
    if percent != current {
        prefs.ui_scale = percent as f32 / 100.0;
    }

    let row = s.row(
        tr_s("Accent colour"),
        tr_s("Colour of the selection, the focused field and the main button."),
        prefs.accent != defaults.accent,
        |ui| {
            let mut picked = None;
            for (index, (name, color)) in theme::ACCENTS.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::click());
                let swatch = rect.shrink(3.0);
                ui.painter().rect_filled(swatch, theme::R_S, *color);
                if prefs.accent == index {
                    ui.painter().rect_stroke(rect.shrink(0.75), theme::R_M, Stroke::new(1.5_f32, theme::TEXT_STRONG));
                } else if response.hovered() {
                    ui.painter().rect_stroke(rect.shrink(0.5), theme::R_M, Stroke::new(1.0_f32, theme::LINE_HOVER));
                }
                if response.on_hover_text(tr_s(name)).clicked() {
                    picked = Some(index);
                }
            }
            if let Some((name, _)) = theme::ACCENTS.get(prefs.accent) {
                ui.add_space(theme::SP_S);
                ui.label(egui::RichText::new(tr_s(name)).color(theme::TEXT_MUTED));
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

    s.group(tr_s("Play mode"));
    s.percent(
        tr_s("Play mode tint"),
        tr_s("How strongly the toolbar is tinted while the game runs. 0% leaves only the accent line."),
        &mut prefs.play_tint,
        defaults.play_tint,
    );
}

/// General: история Undo, автосохранение, внешний редактор кода.
pub(super) fn general(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    // Язык интерфейса — общий для редактора, хаба и редактора моделей (editor.ron).
    s.group(&crate::i18n::tr("Interface"));
    let auto = format!("{} ({})", crate::i18n::tr("Auto"), crate::i18n::Language::system().native_name());
    let languages: [(Option<crate::i18n::Language>, &str); 3] = [
        (None, auto.as_str()),
        (Some(crate::i18n::Language::English), crate::i18n::Language::English.native_name()),
        (Some(crate::i18n::Language::Russian), crate::i18n::Language::Russian.native_name()),
    ];
    if s.combo(
        &crate::i18n::tr("Language"),
        &crate::i18n::tr("Language of the editor, the project hub and the model editor. Auto follows the system language."),
        &mut prefs.language,
        defaults.language,
        &languages,
    ) {
        prefs.apply_language();
    }
    s.group(tr_s("Scene"));
    s.number(
        tr_s("Undo steps"),
        tr_s("How many scene edits Undo remembers. Older steps are forgotten."),
        &mut prefs.undo_limit,
        defaults.undo_limit,
        10..=1000,
        1.0,
        "",
    );
    s.toggle(
        tr_s("Save before Play"),
        tr_s("Save the open scene when entering Play mode, if it already has a file."),
        &mut prefs.autosave_on_play,
        defaults.autosave_on_play,
    );
    s.number(
        tr_s("Auto-save after"),
        tr_s("Minutes after the first unsaved change; 0 turns auto-save off. Only in Edit mode, only scenes with a file."),
        &mut prefs.autosave_minutes,
        defaults.autosave_minutes,
        0.0..=120.0,
        0.1,
        tr_s(" min"),
    );

    s.group(tr_s("Code"));
    let row = s.row(
        tr_s("External code editor"),
        tr_s("Command that opens scripts; {file} and {line} are filled in. Empty: the built-in Code tab."),
        prefs.external_editor != defaults.external_editor,
        |ui| {
            // Кнопка справа, поле — на остаток строки (длинная команда прокручивается в поле).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let test = theme::button(ui, tr_s("Test"));
                let clicked = ui
                    .interact(test.rect, Editor::external_editor_test_id(), Sense::click())
                    .on_hover_text(tr_s("Open the file of the Code tab (or the assets folder) with this command"))
                    .clicked();
                crate::controls::text_field_font(ui, None, &mut prefs.external_editor, "code -g {file}:{line}", ui.available_width().max(80.0), theme::mono());
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
    s.group(tr_s("Grid"));
    s.color(tr_s("Grid colour"), tr_s("Colour of the grid lines in the Scene view."), &mut prefs.grid_color, defaults.grid_color);
    s.percent(
        tr_s("Grid opacity"),
        tr_s("100% is the usual strength; lower it to make the grid quieter."),
        &mut prefs.grid_opacity,
        defaults.grid_opacity,
    );
    s.color(
        tr_s("Background"),
        tr_s("Colour behind the scene in the 2D and 3D Scene view (the Game view uses the camera's own)."),
        &mut prefs.scene_background,
        defaults.scene_background,
    );
    s.group(tr_s("Snapping"));
    s.number(tr_s("Move step"), tr_s("Grid step of the Move tool with snapping on (or Ctrl held)."), &mut prefs.snap_move, defaults.snap_move, 0.01..=100.0, 0.01, " m");
    s.number(tr_s("Rotate step"), tr_s("Angle step of the Rotate tool with snapping on."), &mut prefs.snap_rotate, defaults.snap_rotate, 1.0..=90.0, 0.5, "°");
    s.number(tr_s("Scale step"), tr_s("Factor step of the Scale tool with snapping on."), &mut prefs.snap_scale, defaults.snap_scale, 0.01..=2.0, 0.01, "");
    s.group(tr_s("Camera"));
    s.number(tr_s("Look speed"), tr_s("How fast the 3D camera turns: right mouse button look and Alt + left mouse button orbit."), &mut prefs.orbit_speed, defaults.orbit_speed, 0.1..=5.0, 0.01, "×");
    s.number(tr_s("Fly speed"), tr_s("Speed of flying with the right mouse button held and W / A / S / D, Q / E. Shift flies three times faster; the wheel changes it while flying."), &mut prefs.fly_speed, defaults.fly_speed, 0.1..=200.0, 0.1, " m/s");
    s.toggle(tr_s("Fog in the Scene view"), tr_s("Draw the scene's fog in the Scene view too. Off by default, so distant objects stay visible while editing; the Game view always shows the fog."), &mut prefs.scene_fog, defaults.scene_fog);
    s.toggle(
        tr_s("Invert vertical orbit"),
        tr_s("Dragging up tilts the 3D camera down instead of up."),
        &mut prefs.invert_orbit_y,
        defaults.invert_orbit_y,
    );
    s.number(tr_s("Zoom speed"), tr_s("How far one wheel step zooms, in 2D and 3D."), &mut prefs.zoom_speed, defaults.zoom_speed, 0.1..=5.0, 0.01, "×");
}

/// Console & Diagnostics: что Console показывает при запуске, Profiler.
pub(super) fn console(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = Preferences::default();
    let prefs = &mut editor.prefs;
    let mut changed = false;
    s.group(tr_s("Console"));
    changed |= s.toggle(tr_s("Show messages"), tr_s("Info messages are listed when the editor starts."), &mut prefs.console_info, defaults.console_info);
    changed |= s.toggle(tr_s("Show warnings"), tr_s("Warnings are listed when the editor starts."), &mut prefs.console_warn, defaults.console_warn);
    changed |= s.toggle(tr_s("Show errors"), tr_s("Errors are listed when the editor starts."), &mut prefs.console_error, defaults.console_error);
    changed |= s.toggle(tr_s("Collapse"), tr_s("Identical messages are shown once, with a count."), &mut prefs.console_collapse, defaults.console_collapse);
    changed |= s.toggle(
        tr_s("Error Pause"),
        tr_s("A new error in Play mode pauses the game."),
        &mut prefs.console_error_pause,
        defaults.console_error_pause,
    );
    changed |= s.toggle(
        tr_s("Clear on Play"),
        tr_s("Entering Play mode empties the Console."),
        &mut prefs.console_clear_on_play,
        defaults.console_clear_on_play,
    );
    changed |= s.number(
        tr_s("Max messages"),
        tr_s("The Console keeps this many newest messages."),
        &mut prefs.console_max,
        defaults.console_max,
        50..=engine_core::logging::CAPACITY,
        10.0,
        "",
    );
    s.group(tr_s("Profiler"));
    changed |= s.number(
        tr_s("History length"),
        tr_s("How many recent frames the Profiler graph and averages cover."),
        &mut prefs.profiler_history,
        defaults.profiler_history,
        30..=3000,
        5.0,
        tr_s(" frames"),
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
    s.group(tr_s("Frame rate"));
    s.toggle(
        tr_s("Vertical sync"),
        tr_s("Match the monitor refresh rate. Off: Play mode runs as fast as it can, or at the limit below."),
        &mut prefs.vsync,
        defaults.vsync,
    );
    let names: Vec<(u32, String)> =
        FPS_CAPS.iter().map(|cap| (*cap, if *cap == 0 { tr_s("Unlimited").to_string() } else { trf!("{cap} FPS", cap = cap) })).collect();
    let options: Vec<(u32, &str)> = names.iter().map(|(cap, name)| (*cap, name.as_str())).collect();
    s.combo(
        tr_s("Play mode limit"),
        tr_s("Frames per second in Play mode when vertical sync is off."),
        &mut prefs.play_fps_cap,
        defaults.play_fps_cap,
        &options,
    );
    s.group(tr_s("Input"));
    s.toggle(
        tr_s("Focus Game view on Play"),
        tr_s("Play gives the Game view the keyboard focus. The game receives keys, mouse and gamepads only while the Game view has focus: click another window or press Shift+Esc to give them back to the editor, click the Game view to return them."),
        &mut prefs.focus_game_on_play,
        defaults.focus_game_on_play,
    );
    s.group(tr_s("Edit mode"));
    s.toggle(
        tr_s("Redraw on demand"),
        &trf!(
            "Redraw only when something changes; an idle editor redraws {} times a second and leaves the GPU alone.",
            (1000 / crate::ui::IDLE_REPAINT.as_millis().max(1)).max(1)
        ),
        &mut prefs.redraw_on_demand,
        defaults.redraw_on_demand,
    );
    if s.drawing() {
        s.note(&trf!("Now: {fps} FPS.", fps = format!("{:.0}", fps)));
    }
    // Звук игры в редакторе (editor_audio.rs).
    crate::editor_audio::preferences_rows(s, &mut editor.prefs);
}
