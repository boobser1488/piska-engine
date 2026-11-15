//! Страницы Project Settings: Player, Time, Graphics, Audio.

use crate::i18n::{tr_s};
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

    s.group(tr_s("Identity"));
    s.text(tr_s("Product name"), tr_s("Title of the game window. Empty: the title the game sets in code."), &mut p.title, &d.title, tr_s("My Game"));
    s.text(tr_s("Company name"), tr_s("Who makes the game. Kept with the project; the game window does not show it."), &mut p.company, &d.company, tr_s("My Studio"));
    s.text(tr_s("Version"), tr_s("Version of the game, for example 1.2.0. Kept with the project."), &mut p.version, &d.version, "0.1.0");
    s.choice(
        tr_s("Icon"),
        tr_s("Window icon: a PNG from the assets. None: icon.png if there is one, otherwise the engine icon."),
        &mut p.icon,
        &d.icon,
        &icons,
        tr_s("None (icon.png)"),
    );

    s.group(tr_s("Window"));
    s.number(tr_s("Width"), tr_s("Window width at start, in logical pixels (desktop). At 1280 the size set in code is kept."), &mut p.width, d.width, 160..=7680, 1.0, " px");
    s.number(tr_s("Height"), tr_s("Window height at start, in logical pixels (desktop). At 720 the size set in code is kept."), &mut p.height, d.height, 120..=4320, 1.0, " px");
    s.toggle(tr_s("Resizable window"), tr_s("The player can resize the game window (desktop)."), &mut p.resizable, d.resizable);
    let modes: Vec<(FullscreenMode, &str)> = FullscreenMode::ALL.iter().map(|m| (*m, tr_s(m.label()))).collect();
    // i18n-keys: "Windowed" "Fullscreen (borderless)" "Low" "Medium" "High"
    s.combo(tr_s("Fullscreen mode"), tr_s("How the game window opens at start (desktop)."), &mut p.fullscreen, d.fullscreen, &modes);
    s.toggle(tr_s("VSync"), tr_s("Match the monitor refresh rate in the game."), &mut p.vsync, d.vsync);
    s.number(
        tr_s("Frame rate limit"),
        tr_s("Most frames per second the game draws (desktop and Android). 0: no limit."),
        &mut p.fps_cap,
        d.fps_cap,
        0..=1000,
        1.0,
        tr_s(" FPS"),
    );

    s.group(tr_s("Startup"));
    s.choice(
        tr_s("Default scene"),
        tr_s("Scene loaded at start when the game's build() creates no objects."),
        &mut p.default_scene,
        &d.default_scene,
        &scenes,
        tr_s("None"),
    );
}

/// Time: шаг физики, самый длинный кадр, скорость времени.
pub(super) fn time(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = TimeSettings::default();
    let t = &mut editor.project_draft_mut().time;
    s.group(tr_s("Time"));
    s.number(
        tr_s("Fixed timestep"),
        tr_s("Seconds between FixedUpdate calls and physics steps (1/60 s = 0.0167)."),
        &mut t.fixed_timestep,
        d.fixed_timestep,
        0.001..=0.5,
        0.0005,
        tr_s(" s"),
    );
    s.number(
        tr_s("Maximum allowed timestep"),
        tr_s("Longest frame the game sees: after a hitch, time jumps at most this far."),
        &mut t.max_timestep,
        d.max_timestep,
        0.01..=1.0,
        0.005,
        tr_s(" s"),
    );
    s.number(
        tr_s("Time scale"),
        tr_s("Speed of game time at start: 1 is normal, 0.5 is slow motion, 0 is paused."),
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
    s.group(tr_s("Quality"));
    let current = g.quality;
    let row = s.row(
        tr_s("Quality level"),
        tr_s("Fills the fields below with the level's values; games switch levels at run time with ctx.set_quality."),
        current != d.quality,
        |ui| {
            let mut level = current;
            let options: Vec<(QualityLevel, &str, &str)> = QualityLevel::ALL
                .iter()
                .map(|l| {
                    (
                        *l,
                        tr_s(l.label()),
                        match l {
                            QualityLevel::Low => tr_s("No shadows, no bloom"),
                            QualityLevel::Medium => tr_s("Shadows up to 30 m, soft bloom"),
                            QualityLevel::High => tr_s("All effects"),
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

    s.group(tr_s("Shadows"));
    s.toggle(tr_s("Shadows"), tr_s("Sun shadows on 3D meshes."), &mut g.shadows, d.shadows);
    s.number(
        tr_s("Shadow distance"),
        tr_s("Farthest shadows, in metres; the smaller of this and the scene Environment's distance is used."),
        &mut g.shadow_distance,
        d.shadow_distance,
        1.0..=5000.0,
        0.5,
        tr_s(" m"),
    );
    s.number(
        tr_s("Shadow cascades"),
        tr_s("Most cascades a scene's SoftShadows component may use: near shadows stay sharp, far ones cover the whole distance."),
        &mut g.shadow_cascades,
        d.shadow_cascades,
        1..=4,
        0.05,
        "",
    );
    s.number(
        tr_s("Soft shadow filter"),
        tr_s("Best shadow edge filter for SoftShadows: 0 is a hard 3x3 filter, 1 is a soft edge, 2 adds contact hardening."),
        &mut g.shadow_filter,
        d.shadow_filter,
        0..=2,
        0.05,
        "",
    );
    s.group(tr_s("Lights"));
    s.number(
        tr_s("Max lights"),
        tr_s("Most point and spot lights a frame may use when the scene has a ManyLights component; the nearest to the camera come first."),
        &mut g.max_lights,
        d.max_lights,
        4..=255,
        0.2,
        "",
    );
    s.number(
        tr_s("Shadowed lights"),
        tr_s("How many point and spot lights with a LightShadows component cast shadows; a point light costs six shadow passes, a spot light one."),
        &mut g.light_shadows,
        d.light_shadows,
        0..=4,
        0.05,
        "",
    );
    s.number(
        tr_s("Water detail"),
        tr_s("Water with a WaterEffects component: 0 is waves and sun glint, 1 adds refraction, depth colour and shore foam, 2 adds scene reflections."),
        &mut g.water_detail,
        d.water_detail,
        0..=2,
        0.05,
        "",
    );
    s.group(tr_s("Post-processing"));
    s.toggle(tr_s("Bloom"), tr_s("Glow around bright pixels."), &mut g.bloom, d.bloom);
    s.number(tr_s("Bloom intensity"), tr_s("Multiplies the bloom strength set in the scene Environment."), &mut g.bloom_intensity, d.bloom_intensity, 0.0..=4.0, 0.01, "");
    s.number(tr_s("Bloom threshold"), tr_s("Brightness above which pixels glow."), &mut g.bloom_threshold, d.bloom_threshold, 0.0..=10.0, 0.01, "");
    s.number(tr_s("Fog density scale"), tr_s("Multiplies the fog density of the scene Environment; 0 turns fog off."), &mut g.fog_scale, d.fog_scale, 0.0..=4.0, 0.01, "");
    s.toggle(tr_s("Ambient occlusion"), tr_s("Allows the AmbientOcclusion component: darker creases, corners and ground under objects."), &mut g.ambient_occlusion, d.ambient_occlusion);
    s.toggle(tr_s("High quality bloom"), tr_s("Allows the 13-sample bloom of the PostQuality component: smoother glow, no flickering sparks."), &mut g.high_quality_bloom, d.high_quality_bloom);
    s.toggle(tr_s("FXAA"), tr_s("Allows FXAA of the PostQuality component: smooths edges that MSAA misses."), &mut g.fxaa, d.fxaa);
    s.group(tr_s("Performance"));
    s.number(
        tr_s("Render scale"),
        tr_s("3D scene resolution relative to the window; 0.75 is much faster on weak GPUs, the interface stays sharp."),
        &mut g.render_scale,
        d.render_scale,
        0.25..=2.0,
        0.01,
        "",
    );
    s.toggle(tr_s("Terrain detail"), tr_s("Anti-tiling second texture sample and triplanar rock on terrain; off is much cheaper."), &mut g.terrain_detail, d.terrain_detail);
}

/// Audio: громкость шин микшера при старте.
pub(super) fn audio(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = AudioSettings::default();
    let a = &mut editor.project_draft_mut().audio;
    s.group(tr_s("Volume"));
    s.percent(tr_s("Master volume"), tr_s("Volume of everything the game plays."), &mut a.master_volume, d.master_volume);
    s.percent(tr_s("Music volume"), tr_s("Volume of the Music bus."), &mut a.music_volume, d.music_volume);
    s.percent(tr_s("Effects volume"), tr_s("Volume of the SFX bus."), &mut a.sfx_volume, d.sfx_volume);
    s.percent(tr_s("UI volume"), tr_s("Volume of the UI bus."), &mut a.ui_volume, d.ui_volume);
    s.group(tr_s("Focus"));
    s.toggle(tr_s("Mute when unfocused"), tr_s("Silence the game while its window is not in focus."), &mut a.mute_when_unfocused, d.mute_when_unfocused);
}
