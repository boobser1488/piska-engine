//! Страницы Project Settings: Player, Time, Graphics, Audio.

use engine_core::settings::{
    AudioSettings, FullscreenMode, GraphicsSettings, PlayerSettings, QualityLevel, TimeSettings,
};

use super::SettingsUi;
use crate::state::Editor;
use crate::theme;

/// Пути ассетов с расширением `ext` (без точки, без учёта регистра), по алфавиту.
pub(super) fn assets_with(editor: &Editor, ext: &str) -> Vec<String> {
    let mut paths: Vec<String> = editor
        .assets
        .iter()
        .map(|a| a.path.clone())
        .filter(|p| p.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext)))
        .collect();
    paths.sort_by(|a, b| crate::sorting::natural_cmp(a, b));
    paths
}

/// Player: окно игры, название, версия, значок, первая сцена.
pub(super) fn player(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = PlayerSettings::default();
    let icons = assets_with(editor, "png");
    let scenes = assets_with(editor, "scene");
    let p = &mut editor.project_draft_mut().player;

    s.group("Identity");
    s.text("Product name", "Title of the game window. Empty: the title the game sets in code.", &mut p.title, &d.title, "My Game");
    s.text("Company name", "Who makes the game. Kept with the project; the game window does not show it.", &mut p.company, &d.company, "My Studio");
    s.text("Version", "Version of the game, for example 1.2.0. Kept with the project.", &mut p.version, &d.version, "0.1.0");
    s.choice(
        "Icon",
        "Window icon: a PNG from the assets. None: icon.png if there is one, otherwise the engine icon.",
        &mut p.icon,
        &d.icon,
        &icons,
        "None (icon.png)",
    );

    s.group("Window");
    s.number("Width", "Window width at start, in logical pixels (desktop). At 1280 the size set in code is kept.", &mut p.width, d.width, 160..=7680, 1.0, " px");
    s.number("Height", "Window height at start, in logical pixels (desktop). At 720 the size set in code is kept.", &mut p.height, d.height, 120..=4320, 1.0, " px");
    s.toggle("Resizable window", "The player can resize the game window (desktop).", &mut p.resizable, d.resizable);
    let modes: Vec<(FullscreenMode, &str)> = FullscreenMode::ALL.iter().map(|m| (*m, m.label())).collect();
    s.combo("Fullscreen mode", "How the game window opens at start (desktop).", &mut p.fullscreen, d.fullscreen, &modes);
    s.toggle("VSync", "Match the monitor refresh rate in the game.", &mut p.vsync, d.vsync);
    s.number(
        "Frame rate limit",
        "Most frames per second the game draws (desktop and Android). 0: no limit.",
        &mut p.fps_cap,
        d.fps_cap,
        0..=1000,
        1.0,
        " FPS",
    );

    s.group("Startup");
    s.choice(
        "Default scene",
        "Scene loaded at start when the game's build() creates no objects.",
        &mut p.default_scene,
        &d.default_scene,
        &scenes,
        "None",
    );
}

/// Time: шаг физики, самый длинный кадр, скорость времени.
pub(super) fn time(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = TimeSettings::default();
    let t = &mut editor.project_draft_mut().time;
    s.group("Time");
    s.number(
        "Fixed timestep",
        "Seconds between FixedUpdate calls and physics steps (1/60 s = 0.0167).",
        &mut t.fixed_timestep,
        d.fixed_timestep,
        0.001..=0.5,
        0.0005,
        " s",
    );
    s.number(
        "Maximum allowed timestep",
        "Longest frame the game sees: after a hitch, time jumps at most this far.",
        &mut t.max_timestep,
        d.max_timestep,
        0.01..=1.0,
        0.005,
        " s",
    );
    s.number(
        "Time scale",
        "Speed of game time at start: 1 is normal, 0.5 is slow motion, 0 is paused.",
        &mut t.time_scale,
        d.time_scale,
        0.0..=100.0,
        0.01,
        "",
    );
}

/// Graphics: пресет качества и его поля.
pub(super) fn graphics(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = GraphicsSettings::default();
    let g = &mut editor.project_draft_mut().graphics;
    s.group("Quality");
    let current = g.quality;
    let row = s.row(
        "Quality level",
        "Fills the fields below with the level's values; games switch levels at run time with ctx.set_quality.",
        current != d.quality,
        |ui| {
            let mut level = current;
            let options: Vec<(QualityLevel, &str, &str)> = QualityLevel::ALL
                .iter()
                .map(|l| {
                    (
                        *l,
                        l.label(),
                        match l {
                            QualityLevel::Low => "No shadows, no bloom",
                            QualityLevel::Medium => "Shadows up to 30 m, soft bloom",
                            QualityLevel::High => "All effects",
                        },
                    )
                })
                .collect();
            theme::segmented(ui, &mut level, &options).then_some(level)
        },
    );
    if let Some(Some(level)) = row.inner {
        *g = GraphicsSettings::preset(level);
    }
    if row.reset {
        g.quality = d.quality;
    }

    s.group("Shadows");
    s.toggle("Shadows", "Sun shadows on 3D meshes.", &mut g.shadows, d.shadows);
    s.number(
        "Shadow distance",
        "Farthest shadows, in metres; the smaller of this and the scene Environment's distance is used.",
        &mut g.shadow_distance,
        d.shadow_distance,
        1.0..=5000.0,
        0.5,
        " m",
    );
    s.group("Post-processing");
    s.toggle("Bloom", "Glow around bright pixels.", &mut g.bloom, d.bloom);
    s.number("Bloom intensity", "Multiplies the bloom strength set in the scene Environment.", &mut g.bloom_intensity, d.bloom_intensity, 0.0..=4.0, 0.01, "");
    s.number("Bloom threshold", "Brightness above which pixels glow.", &mut g.bloom_threshold, d.bloom_threshold, 0.0..=10.0, 0.01, "");
    s.number("Fog density scale", "Multiplies the fog density of the scene Environment; 0 turns fog off.", &mut g.fog_scale, d.fog_scale, 0.0..=4.0, 0.01, "");
}

/// Audio: громкость шин микшера при старте.
pub(super) fn audio(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = AudioSettings::default();
    let a = &mut editor.project_draft_mut().audio;
    s.group("Volume");
    s.percent("Master volume", "Volume of everything the game plays.", &mut a.master_volume, d.master_volume);
    s.percent("Music volume", "Volume of the Music bus.", &mut a.music_volume, d.music_volume);
    s.percent("Effects volume", "Volume of the SFX bus.", &mut a.sfx_volume, d.sfx_volume);
    s.percent("UI volume", "Volume of the UI bus.", &mut a.ui_volume, d.ui_volume);
    s.group("Focus");
    s.toggle("Mute when unfocused", "Silence the game while its window is not in focus.", &mut a.mute_when_unfocused, d.mute_when_unfocused);
}
