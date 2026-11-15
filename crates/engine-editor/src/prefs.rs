//! Личные настройки редактора (окно Preferences): General, Appearance, Scene View,
//! Game View, Console & Diagnostics, Build & Tools, Shortcuts — и то, как они действуют
//! на редактор (`impl Editor` ниже).
//!
//! Хранятся в `%APPDATA%/PiskaEngine/editor.ron` (Linux, macOS —
//! `~/.config/PiskaEngine/editor.ron`), по строке `ключ: значение` на настройку.

use crate::i18n::{tr_s, trf};
use std::path::PathBuf;

/// Настройки редактора. Меняются в окне Preferences и переживают перезапуск.
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    /// Масштаб интерфейса: 1.0 — 100 %, 1.5 — 150 %.
    pub ui_scale: f32,
    /// Номер акцентного цвета в палитре (Blue, Orange, Green, Purple, Teal).
    pub accent: usize,
    /// Вертикальная синхронизация. Выключена — в режиме Play кадры идут без ограничения
    /// (или с `play_fps_cap`).
    pub vsync: bool,
    /// Ограничение частоты кадров в режиме Play; 0 — без ограничения.
    pub play_fps_cap: u32,
    /// Заглушить звук игры в редакторе (кнопка-динамик окна Game, Preferences → Game View).
    pub editor_audio_muted: bool,
    /// Громкость игры в редакторе, 0..1 (поверх шин самой игры).
    pub editor_volume: f32,
    /// В режиме Edit перерисовывать окно только когда что-то меняется (ввод, сборка,
    /// новые сообщения в консоли), а в покое — несколько раз в секунду.
    /// Выключено — редактор рисует непрерывно, как в Play.
    pub redraw_on_demand: bool,
    /// Play отдаёт фокус (и ввод игры) окну Game. Игра получает клавиатуру, мышь и
    /// геймпады, только пока фокус у окна Game (Preferences → Game View → Input).
    pub focus_game_on_play: bool,
    /// Раскладка окон (док, плавающие окна) — текст [`crate::Editor::layout_text`].
    /// `None` — не задана: редактор оставляет свою (Default).
    pub layout: Option<String>,
    /// Именованные раскладки из Window → Layouts → Save Layout…: (имя, текст).
    pub saved_layouts: Vec<(String, String)>,
    /// Где пользователь оставил окна Build, Preferences, Project Settings и Undo History:
    /// (имя окна, смещение от центра экрана). Остальные окна помнят место до закрытия
    /// редактора.
    pub window_offsets: Vec<(String, [f32; 2])>,
    /// Сортировка в окне Project: `name`, `type`, `modified` или `size`.
    pub project_sort: String,
    /// Сортировка по убыванию.
    pub project_sort_desc: bool,
    /// Включённые фильтры по типу, через `+` (`scripts+scenes`); пусто — показывать всё.
    pub project_types: String,
    /// Недавно добавленные компоненты (меню Add Component), новые первыми.
    pub recent_components: Vec<String>,
    /// Недавние сцены по проектам (File → Open Recent Scene): (каталог ассетов, сцены —
    /// новые первыми, не больше 10).
    pub recent_scenes: Vec<(String, Vec<String>)>,
    /// Окно Project: избранное, сохранённые поиски, ширины столбцов, вид (см. `project_nav`).
    pub project_ui: crate::project_nav::ProjectPrefs,
    /// Переназначенные горячие клавиши (Preferences → Shortcuts), только отличия.
    pub shortcuts: crate::shortcuts::Keymap,
    /// Вкладка Code (Preferences → Code Editor).
    pub code: crate::code_prefs::CodePrefs,

    // --- General ---
    /// Язык интерфейса (Preferences → General → Language; общий для редактора, хаба и
    /// редактора моделей). `None` — «Auto»: язык системы (русский, если система русская).
    pub language: Option<engine_i18n::Language>,
    /// Сколько шагов помнит Undo сцены.
    pub undo_limit: usize,
    /// Сохранять сцену (если у неё уже есть файл) перед входом в Play.
    pub autosave_on_play: bool,
    /// Автосохранение сцены через столько минут после первой несохранённой правки;
    /// 0 — выключено. Только в режиме Edit и только сцены, у которой уже есть файл.
    pub autosave_minutes: f32,
    /// Внешний редактор кода: команда, `{file}` и `{line}` подставляются (без `{file}`
    /// путь дописывается в конец). Пусто — файлы открываются во вкладке Code.
    pub external_editor: String,

    // --- Appearance ---
    /// Насколько заметен оттенок панели инструментов в режиме Play: 0 — нет, 1 — обычный.
    pub play_tint: f32,

    // --- Scene View ---
    /// Цвет линий сетки вида Scene.
    pub grid_color: egui::Color32,
    /// Непрозрачность сетки относительно обычной: 1 — как есть, 0 — не видно.
    pub grid_opacity: f32,
    /// Фон вида Scene (2D и 3D).
    pub scene_background: egui::Color32,
    /// Шаг привязки перемещения, м.
    pub snap_move: f32,
    /// Шаг привязки поворота, градусы.
    pub snap_rotate: f32,
    /// Шаг привязки масштаба (множитель).
    pub snap_scale: f32,
    /// Скорость вращения 3D-камеры правой кнопкой (множитель).
    pub orbit_speed: f32,
    /// Скорость приближения колесом (множитель), 2D и 3D.
    pub zoom_speed: f32,
    /// Перевернуть вращение 3D-камеры по вертикали.
    pub invert_orbit_y: bool,
    /// Скорость полёта в виде Scene (ПКМ + W/A/S/D), м/с; колесо при полёте меняет её.
    pub fly_speed: f32,
    /// Туман сцены в виде Scene (по умолчанию выключен, как переключатель Fog в Unity).
    pub scene_fog: bool,

    // --- Console & Diagnostics ---
    /// Console при запуске показывает сообщения / предупреждения / ошибки.
    pub console_info: bool,
    pub console_warn: bool,
    pub console_error: bool,
    /// Console при запуске схлопывает повторы.
    pub console_collapse: bool,
    /// Error Pause при запуске включён.
    pub console_error_pause: bool,
    /// Очищать Console при входе в Play.
    pub console_clear_on_play: bool,
    /// Сколько последних записей держит Console (не больше `engine_core::logging::CAPACITY`).
    pub console_max: usize,
    /// Сколько последних кадров показывает Profiler.
    pub profiler_history: usize,

    // --- Build & Tools ---
    /// Пути к инструментам; пусто — искать как обычно (PATH, переменные окружения).
    pub tool_cargo: String,
    pub tool_android_sdk: String,
    pub tool_android_ndk: String,
    pub tool_java: String,
    pub tool_zig: String,
    pub tool_cmake: String,
    pub tool_cxx: String,
    /// Каталог экспорта по умолчанию (относительно рабочего пространства или абсолютный):
    /// окно Build кладёт сборку в `<каталог>/<игра>/<платформа>`.
    pub export_folder: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            accent: 0,
            // В такт монитору: без этого движение в режиме Play идёт рывками при любом FPS.
            vsync: true,
            play_fps_cap: 0,
            editor_audio_muted: false,
            editor_volume: 0.5,
            redraw_on_demand: true,
            focus_game_on_play: true,
            layout: None,
            saved_layouts: Vec::new(),
            window_offsets: Vec::new(),
            project_sort: "name".to_string(),
            project_sort_desc: false,
            project_types: String::new(),
            recent_components: Vec::new(),
            recent_scenes: Vec::new(),
            project_ui: Default::default(),
            shortcuts: Default::default(),
            code: Default::default(),
            language: None,
            undo_limit: 100,
            autosave_on_play: false,
            autosave_minutes: 0.0,
            external_editor: String::new(),
            play_tint: 1.0,
            grid_color: egui::Color32::from_rgb(231, 234, 238),
            grid_opacity: 1.0,
            scene_background: egui::Color32::from_rgb(34, 34, 34),
            snap_move: 0.5,
            snap_rotate: 15.0,
            snap_scale: 0.1,
            orbit_speed: 1.0,
            zoom_speed: 1.0,
            invert_orbit_y: false,
            fly_speed: 5.0,
            scene_fog: false,
            console_info: true,
            console_warn: true,
            console_error: true,
            console_collapse: false,
            console_error_pause: false,
            console_clear_on_play: false,
            console_max: engine_core::logging::CAPACITY,
            profiler_history: 300,
            tool_cargo: String::new(),
            tool_android_sdk: String::new(),
            tool_android_ndk: String::new(),
            tool_java: String::new(),
            tool_zig: String::new(),
            tool_cmake: String::new(),
            tool_cxx: String::new(),
            export_folder: "dist".to_string(),
        }
    }
}

/// Строка в файле настроек: без `,` (разделитель записей) и переводов строк.
fn escape(value: &str) -> String {
    value.replace('%', "%25").replace(',', "%2C").replace('\n', "%0A")
}

fn unescape(value: &str) -> String {
    value.replace("%0A", "\n").replace("%2C", ",").replace("%25", "%")
}

/// Цвет как `#rrggbbaa`.
fn color_text(color: egui::Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn parse_color(value: &str) -> Option<egui::Color32> {
    let hex = value.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(egui::Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(egui::Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// Варианты ограничения кадров в Preferences.
pub(crate) const FPS_CAPS: [u32; 7] = [0, 30, 60, 120, 144, 240, 360];

/// Варианты масштаба интерфейса.
pub(crate) const UI_SCALES: [u32; 9] = [80, 90, 100, 110, 125, 150, 175, 200, 250];

impl Preferences {
    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
        })?;
        Some(base.join("PiskaEngine").join("editor.ron"))
    }

    /// Разобрать текст файла настроек. Незнакомые и испорченные строки пропускаются.
    pub fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        let body = text.trim().trim_start_matches('(').trim_end_matches(')');
        // Раскладки окон — по строке на раскладку: `layout: …` и `layout.<имя>: …`.
        let mut rest = String::new();
        for line in body.lines() {
            let Some((key, value)) = line.split_once(':') else {
                rest.push_str(line);
                rest.push('\n');
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_end_matches(',').trim();
            if key == "layout" {
                prefs.layout = (!value.is_empty()).then(|| value.to_string());
            } else if let Some(name) = key.strip_prefix("layout.") {
                if !name.trim().is_empty() && !value.is_empty() {
                    prefs.saved_layouts.push((name.trim().to_string(), value.to_string()));
                }
            } else if let Some(name) = key.strip_prefix("window.") {
                // `window.<имя>: x y` — место окна (смещение от центра экрана).
                let numbers: Vec<f32> = value.split_whitespace().filter_map(|n| n.parse().ok()).collect();
                if let [x, y] = numbers[..] {
                    if x.is_finite() && y.is_finite() && !name.trim().is_empty() {
                        prefs.window_offsets.push((name.trim().to_string(), [x, y]));
                    }
                }
            } else {
                rest.push_str(line);
                rest.push('\n');
            }
        }
        for entry in rest.split([',', '\n']) {
            let Some((key, value)) = entry.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "ui_scale" => {
                    if let Ok(v) = value.parse::<f32>() {
                        prefs.ui_scale = v.clamp(0.5, 3.0);
                    }
                }
                "accent" => {
                    if let Ok(v) = value.parse::<usize>() {
                        prefs.accent = v;
                    }
                }
                // Ключ `vsync` из старых файлов не читается: там всегда стояло прежнее
                // значение по умолчанию (выключено), которое давало рывки камеры.
                "play_vsync" => prefs.vsync = value != "false",
                "play_fps_cap" => {
                    if let Ok(v) = value.parse() {
                        prefs.play_fps_cap = v;
                    }
                }
                "redraw_on_demand" => prefs.redraw_on_demand = value != "false",
                "focus_game_on_play" => prefs.focus_game_on_play = value != "false",
                "project_sort" => {
                    if crate::sorting::SortKey::parse(value).is_some() {
                        prefs.project_sort = value.to_string();
                    }
                }
                "project_sort_desc" => prefs.project_sort_desc = value == "true",
                "project_types" => {
                    prefs.project_types = crate::sorting::filters_text(&crate::sorting::parse_filters(value));
                }
                "recent_components" => {
                    prefs.recent_components =
                        value.split('+').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect();
                }
                // `recent_scenes: <каталог ассетов>|сцена+сцена` — строка на проект.
                "recent_scenes" => {
                    if let Some((root, scenes)) = value.split_once('|') {
                        let scenes: Vec<String> = scenes
                            .split('+')
                            .map(|s| unescape(s.trim()).replace("%2B", "+"))
                            .filter(|s| !s.is_empty())
                            .take(crate::scene_picker::RECENT_MAX)
                            .collect();
                        if !root.trim().is_empty() && !scenes.is_empty() {
                            prefs.recent_scenes.push((unescape(root.trim()), scenes));
                        }
                    }
                }
                other if other.starts_with("pj_") => prefs.project_ui.set(other, value),
                other if other.starts_with("code_") => prefs.code.set(other, value),
                other if other.starts_with("shortcut.") => prefs.shortcuts.parse_entry(&other["shortcut.".len()..], value),
                "language" => prefs.language = engine_i18n::Language::from_code(value),
                "undo_limit" => set_num(&mut prefs.undo_limit, value, 1, 10_000),
                "autosave_on_play" => prefs.autosave_on_play = value == "true",
                "autosave_minutes" => set_num(&mut prefs.autosave_minutes, value, 0.0, 600.0),
                "external_editor" => prefs.external_editor = unescape(value),
                "play_tint" => set_num(&mut prefs.play_tint, value, 0.0, 1.0),
                "editor_audio_muted" => prefs.editor_audio_muted = value == "true",
                "editor_volume" => set_num(&mut prefs.editor_volume, value, 0.0, 1.0),
                "grid_color" => prefs.grid_color = parse_color(value).unwrap_or(prefs.grid_color),
                "grid_opacity" => set_num(&mut prefs.grid_opacity, value, 0.0, 1.0),
                "scene_background" => prefs.scene_background = parse_color(value).unwrap_or(prefs.scene_background),
                "snap_move" => set_num(&mut prefs.snap_move, value, 0.001, 1000.0),
                "snap_rotate" => set_num(&mut prefs.snap_rotate, value, 0.1, 180.0),
                "snap_scale" => set_num(&mut prefs.snap_scale, value, 0.001, 10.0),
                "orbit_speed" => set_num(&mut prefs.orbit_speed, value, 0.05, 10.0),
                "zoom_speed" => set_num(&mut prefs.zoom_speed, value, 0.05, 10.0),
                "invert_orbit_y" => prefs.invert_orbit_y = value == "true",
                "fly_speed" => set_num(&mut prefs.fly_speed, value, 0.1, 200.0),
                "scene_fog" => prefs.scene_fog = value == "true",
                "console_info" => prefs.console_info = value != "false",
                "console_warn" => prefs.console_warn = value != "false",
                "console_error" => prefs.console_error = value != "false",
                "console_collapse" => prefs.console_collapse = value == "true",
                "console_error_pause" => prefs.console_error_pause = value == "true",
                "console_clear_on_play" => prefs.console_clear_on_play = value == "true",
                "console_max" => set_num(&mut prefs.console_max, value, 10, engine_core::logging::CAPACITY),
                "profiler_history" => set_num(&mut prefs.profiler_history, value, 30, 3000),
                "tool_cargo" => prefs.tool_cargo = unescape(value),
                "tool_android_sdk" => prefs.tool_android_sdk = unescape(value),
                "tool_android_ndk" => prefs.tool_android_ndk = unescape(value),
                "tool_java" => prefs.tool_java = unescape(value),
                "tool_zig" => prefs.tool_zig = unescape(value),
                "tool_cmake" => prefs.tool_cmake = unescape(value),
                "tool_cxx" => prefs.tool_cxx = unescape(value),
                "export_folder" => prefs.export_folder = unescape(value),
                _ => {}
            }
        }
        prefs
    }

    /// Текст файла настроек (то, что пишет [`Preferences::save`]; читает [`Preferences::parse`]).
    pub fn to_text(&self) -> String {
        let mut text = format!(
            "(\n    ui_scale: {:.2},\n    accent: {},\n    play_vsync: {},\n    play_fps_cap: {},\n    redraw_on_demand: {},\n",
            self.ui_scale, self.accent, self.vsync, self.play_fps_cap, self.redraw_on_demand
        );
        text.push_str(&format!(
            "    project_sort: {},
    project_sort_desc: {},
    project_types: {},
    recent_components: {},
",
            self.project_sort,
            self.project_sort_desc,
            self.project_types,
            self.recent_components.join("+")
        ));
        text.push_str(&self.project_ui.to_text());
        text.push_str(&self.shortcuts.to_text());
        text.push_str(&self.code.to_text());
        let mut line = |key: &str, value: String| text.push_str(&format!("    {key}: {value},\n"));
        line("language", self.language.map_or("auto", |l| l.code()).to_string());
        line("undo_limit", self.undo_limit.to_string());
        line("autosave_on_play", self.autosave_on_play.to_string());
        line("autosave_minutes", self.autosave_minutes.to_string());
        line("external_editor", escape(&self.external_editor));
        line("play_tint", self.play_tint.to_string());
        line("focus_game_on_play", self.focus_game_on_play.to_string());
        for (root, scenes) in &self.recent_scenes {
            let scenes: Vec<String> = scenes.iter().map(|s| escape(s).replace('+', "%2B")).collect();
            line("recent_scenes", format!("{}|{}", escape(root), scenes.join("+")));
        }
        line("editor_audio_muted", self.editor_audio_muted.to_string());
        line("editor_volume", self.editor_volume.to_string());
        line("grid_color", color_text(self.grid_color));
        line("grid_opacity", self.grid_opacity.to_string());
        line("scene_background", color_text(self.scene_background));
        line("snap_move", self.snap_move.to_string());
        line("snap_rotate", self.snap_rotate.to_string());
        line("snap_scale", self.snap_scale.to_string());
        line("orbit_speed", self.orbit_speed.to_string());
        line("zoom_speed", self.zoom_speed.to_string());
        line("invert_orbit_y", self.invert_orbit_y.to_string());
        line("fly_speed", self.fly_speed.to_string());
        line("scene_fog", self.scene_fog.to_string());
        line("console_info", self.console_info.to_string());
        line("console_warn", self.console_warn.to_string());
        line("console_error", self.console_error.to_string());
        line("console_collapse", self.console_collapse.to_string());
        line("console_error_pause", self.console_error_pause.to_string());
        line("console_clear_on_play", self.console_clear_on_play.to_string());
        line("console_max", self.console_max.to_string());
        line("profiler_history", self.profiler_history.to_string());
        line("tool_cargo", escape(&self.tool_cargo));
        line("tool_android_sdk", escape(&self.tool_android_sdk));
        line("tool_android_ndk", escape(&self.tool_android_ndk));
        line("tool_java", escape(&self.tool_java));
        line("tool_zig", escape(&self.tool_zig));
        line("tool_cmake", escape(&self.tool_cmake));
        line("tool_cxx", escape(&self.tool_cxx));
        line("export_folder", escape(&self.export_folder));
        // Текст раскладки — одна строка без `,` и `:` (так его пишет док).
        let one_line = |value: &str| value.replace(['\n', '\r'], " ");
        if let Some(layout) = &self.layout {
            text.push_str(&format!("    layout: {},\n", one_line(layout)));
        }
        for (name, layout) in &self.saved_layouts {
            text.push_str(&format!("    layout.{}: {},\n", one_line(name), one_line(layout)));
        }
        for (name, [x, y]) in &self.window_offsets {
            text.push_str(&format!("    window.{}: {x} {y},\n", one_line(name)));
        }
        text.push_str(")\n");
        text
    }

    /// Язык интерфейса с учётом «Auto»: выбранный явно либо язык системы.
    pub fn resolved_language(&self) -> engine_i18n::Language {
        engine_i18n::resolve(self.language)
    }

    /// Применить язык настроек ко всему процессу (интерфейс перерисуется на нём
    /// в следующем кадре). Вызывается при загрузке и при смене в Preferences.
    pub fn apply_language(&self) {
        engine_i18n::set(self.resolved_language());
    }

    /// Прочитать с диска; нет файла — значения по умолчанию. Заодно ставит язык
    /// интерфейса процесса ([`Preferences::apply_language`]).
    pub fn load() -> Self {
        let prefs = Self::file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default();
        prefs.apply_language();
        prefs
    }

    /// Записать на диск. Ошибка записи не мешает работе — она попадает в лог.
    pub fn save(&self) {
        let Some(file) = Self::file() else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = std::fs::write(&file, self.to_text()) {
            log::warn!("cannot save editor preferences to `{}`: {err}", file.display());
        }
    }
}

// ---------------------------------------------------------------------------
// Как настройки действуют на редактор
// ---------------------------------------------------------------------------

impl crate::state::Editor {
    /// Сетка вида Scene, какой её получит рендер в этом кадре (`None` — сетка выключена):
    /// цвет и непрозрачность — из Preferences → Scene View.
    pub fn scene_grid(&self) -> Option<engine_render::SceneGrid> {
        if !self.show_grid {
            return None;
        }
        let mut grid = if self.scene_3d {
            engine_render::SceneGrid::ground().with_fade_distance(self.orbit.distance * 40.0 + 60.0)
        } else {
            engine_render::SceneGrid::wall()
        };
        let color = egui::Rgba::from(self.prefs.grid_color);
        grid.color = [color.r(), color.g(), color.b()];
        grid.opacity *= self.prefs.grid_opacity.clamp(0.0, 1.0);
        Some(grid)
    }

    /// Цвет фона вида Scene (линейный RGB) — Preferences → Scene View → Background.
    pub fn scene_background(&self) -> [f32; 3] {
        let color = egui::Rgba::from(self.prefs.scene_background);
        [color.r(), color.g(), color.b()]
    }

    /// Сколько кадров в истории Profiler сейчас (не больше Preferences → History length).
    pub fn profiler_frames(&self) -> usize {
        self.profiler.recorded()
    }

    /// Сколько сообщений держит Console сейчас (не больше Preferences → Max messages).
    pub fn console_message_count(&self) -> usize {
        self.console.shown()
    }

    /// Флажки Console сейчас: [сообщения, предупреждения, ошибки, Collapse, Error Pause].
    pub fn console_toggles(&self) -> [bool; 5] {
        let c = &self.console;
        [c.show_info, c.show_warn, c.show_error, c.collapse, c.error_pause]
    }

    /// Шаги привязки гизмо: (метры, радианы, множитель масштаба).
    pub(crate) fn snap_steps(&self) -> (f32, f32, f32) {
        (
            self.prefs.snap_move.max(1e-3),
            self.prefs.snap_rotate.max(0.1).to_radians(),
            self.prefs.snap_scale.max(1e-3),
        )
    }

    /// Применить к окнам то, что берётся из настроек при запуске: фильтры и флажки
    /// Console, длину истории Profiler.
    pub(crate) fn apply_panel_prefs(&mut self) {
        let prefs = &self.prefs;
        let console = &mut self.console;
        console.show_info = prefs.console_info;
        console.show_warn = prefs.console_warn;
        console.show_error = prefs.console_error;
        console.collapse = prefs.console_collapse;
        console.error_pause = prefs.console_error_pause;
        console.set_max(prefs.console_max);
        self.profiler.set_history(prefs.profiler_history);
        self.apply_code_prefs();
    }

    /// Перед входом в Play: автосохранение сцены и очистка Console (если включены).
    pub(crate) fn before_play(&mut self) {
        if self.prefs.console_clear_on_play {
            self.console.clear();
        }
        if self.prefs.autosave_on_play && self.dirty && self.scene_path.is_some() {
            if let Err(error) = self.try_apply(crate::state::Action::Save) {
                log::error!("auto-save before Play failed: {error}");
            }
        }
    }

    /// Автосохранение по времени: вызывается каждый кадр со временем egui (секунды).
    pub(crate) fn autosave_tick(&mut self, now: f64) {
        let minutes = self.prefs.autosave_minutes;
        let waiting = minutes > 0.0
            && self.dirty
            && self.mode == crate::state::Mode::Edit
            && self.scene_path.is_some()
            && self.dialog.is_none();
        if !waiting {
            self.settings.autosave_since = None;
            return;
        }
        let since = *self.settings.autosave_since.get_or_insert(now);
        if now - since >= f64::from(minutes) * 60.0 {
            self.settings.autosave_since = None;
            match self.try_apply(crate::state::Action::Save) {
                Ok(()) => log::info!("auto-saved `{}`", self.scene_path.as_deref().unwrap_or_default()),
                Err(error) => log::error!("auto-save failed: {error}"),
            }
        }
    }

    /// Открыть файл во внешнем редакторе кода (Preferences → General → External code
    /// editor). Ошибка — команда не задана или не запустилась.
    pub fn open_in_external_editor(&self, file: &std::path::Path, line: usize) -> Result<(), crate::EditorError> {
        let (program, args) = external_command(&self.prefs.external_editor, file, line).ok_or_else(|| {
            crate::EditorError::Other(
                tr_s("no external code editor is set: enter its command in Preferences → General").to_string(),
            )
        })?;
        engine_build::host::command(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                crate::EditorError::Other(trf!(
                    "cannot start the external editor `{program}`: {e}. Check the command in Preferences → General"
                , program = program, e = e))
            })
    }
}

impl crate::state::Editor {
    /// Кнопка Test строки External code editor (Preferences → General).
    pub fn external_editor_test_id() -> egui::Id {
        egui::Id::new("prefs_external_editor_test")
    }

    /// Кнопка Open Externally во вкладке Code.
    pub fn open_externally_id() -> egui::Id {
        egui::Id::new("code_open_externally")
    }

    /// Открыть файл вкладки Code во внешнем редакторе; ошибка — в Console.
    pub(crate) fn open_active_externally(&mut self) {
        let Some(path) = self.code.active().map(|(path, _, _)| path.to_path_buf()) else { return };
        match self.open_in_external_editor(&path, 1) {
            Ok(()) => log::info!("opened `{}` in the external editor", path.display()),
            Err(error) => log::error!("{error}"),
        }
    }

    /// Кнопка Test: открыть файл вкладки Code (или каталог ассетов) командой из настроек;
    /// итог — в строке состояния окна Preferences.
    pub(crate) fn test_external_editor(&mut self) {
        let target = self
            .code
            .active()
            .map(|(path, _, _)| path.to_path_buf())
            .or_else(|| self.assets_root())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let status = match self.open_in_external_editor(&target, 1) {
            Ok(()) => (trf!("Started the external editor with {}", target.display()), false),
            Err(error) => (error.to_string(), true),
        };
        self.set_preferences_status(status.0, status.1);
    }

    /// Команда сборки с путями инструментов из Preferences → Build & Tools: `cargo` и
    /// `cmake` заменяются заданными программами, пути SDK / NDK / Java / zig / C++
    /// уходят в переменные окружения (`ANDROID_HOME`, `ANDROID_NDK_HOME`, `JAVA_HOME`,
    /// `PISKA_ZIG`, `CXX`). Пустые поля ничего не меняют.
    pub(crate) fn with_tools(&self, mut command: crate::code::BuildCommand) -> crate::code::BuildCommand {
        let prefs = &self.prefs;
        let given = |value: &str| Some(value.trim().to_string()).filter(|v| !v.is_empty());
        let program = match command.program.as_str() {
            "cargo" => given(&prefs.tool_cargo),
            "cmake" => given(&prefs.tool_cmake),
            _ => None,
        };
        if let Some(program) = program {
            command.program = program;
        }
        let env: [(&[&str], &String); 5] = [
            (&["ANDROID_HOME", "ANDROID_SDK_ROOT"], &prefs.tool_android_sdk),
            (&["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"], &prefs.tool_android_ndk),
            (&["JAVA_HOME"], &prefs.tool_java),
            (&["PISKA_ZIG"], &prefs.tool_zig),
            (&["CXX"], &prefs.tool_cxx),
        ];
        for (keys, value) in env {
            if let Some(value) = given(value) {
                for key in keys {
                    command.env.push((key.to_string(), value.clone()));
                }
            }
        }
        command
    }
}

/// Команда внешнего редактора: программа и аргументы. Слова делятся пробелами, кавычки
/// `"…"` держат пробелы внутри; `{file}` и `{line}` подставляются, а без `{file}` путь
/// добавляется последним аргументом. `None` — команда пуста.
pub(crate) fn external_command(command: &str, file: &std::path::Path, line: usize) -> Option<(String, Vec<String>)> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in command.trim().chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    let path = file.display().to_string();
    let mut has_file = false;
    let mut words: Vec<String> = words
        .into_iter()
        .map(|w| {
            has_file |= w.contains("{file}");
            w.replace("{file}", &path).replace("{line}", &line.max(1).to_string())
        })
        .collect();
    if words.is_empty() {
        return None;
    }
    if !has_file {
        words.push(path);
    }
    let program = words.remove(0);
    Some((program, words))
}

/// Число из текста в пределах `min..=max`; испорченное значение оставляет прежнее.
fn set_num<T: std::str::FromStr + PartialOrd + Copy>(field: &mut T, value: &str, min: T, max: T) {
    if let Ok(v) = value.trim().parse::<T>() {
        *field = if v < min {
            min
        } else if v > max {
            max
        } else {
            v
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_through_text() {
        let prefs = Preferences {
            ui_scale: 1.5,
            accent: 2,
            vsync: true,
            play_fps_cap: 144,
            redraw_on_demand: false,
            layout: Some("piska-layout 1; main (H 0.5000 (L 0 scene) (L 1 game code)); maxplay 0".to_string()),
            saved_layouts: vec![
                ("Coding".to_string(), "piska-layout 1; main (L 0 code); maxplay 1".to_string()),
                ("Two views".to_string(), "piska-layout 1; main -; maxplay 0".to_string()),
            ],
            project_sort: "size".to_string(),
            project_sort_desc: true,
            project_types: "scripts+audio".to_string(),
            recent_components: vec!["Mover".to_string(), "Collider".to_string()],
            project_ui: crate::project_nav::ProjectPrefs {
                favorites: vec!["models".to_string()],
                grid: false,
                ..Default::default()
            },
            shortcuts: {
                let mut keys = crate::shortcuts::Keymap::default();
                keys.set("play", Some(crate::shortcuts::KeyBinding::key(egui::Key::F5)));
                keys.set("preferences", Some(crate::shortcuts::KeyBinding::ctrl_shift(egui::Key::Comma)));
                keys.set("quit", None);
                keys
            },
            ..Preferences::default()
        };
        assert_eq!(Preferences::parse(&prefs.to_text()), prefs);
    }

    #[test]
    fn every_preferences_page_field_round_trips() {
        let prefs = Preferences {
            undo_limit: 25,
            autosave_on_play: true,
            autosave_minutes: 2.5,
            external_editor: "code -g {file}:{line}, --wait".to_string(),
            play_tint: 0.25,
            editor_audio_muted: true,
            editor_volume: 0.3,
            grid_color: egui::Color32::from_rgba_unmultiplied(10, 200, 30, 255),
            grid_opacity: 0.5,
            scene_background: egui::Color32::from_rgb(40, 50, 60),
            snap_move: 0.25,
            snap_rotate: 45.0,
            snap_scale: 0.5,
            orbit_speed: 2.0,
            zoom_speed: 0.5,
            invert_orbit_y: true,
            console_info: false,
            console_warn: false,
            console_error: false,
            console_collapse: true,
            console_error_pause: true,
            console_clear_on_play: true,
            console_max: 500,
            profiler_history: 120,
            tool_cargo: "C:\\Rust\\cargo.exe".to_string(),
            tool_android_sdk: "D:\\Android, SDK".to_string(),
            tool_android_ndk: "/opt/ndk/27".to_string(),
            tool_java: "C:/jdk-17".to_string(),
            tool_zig: "zig 100%".to_string(),
            tool_cmake: "cmake".to_string(),
            tool_cxx: "g++".to_string(),
            export_folder: "builds".to_string(),
            ..Preferences::default()
        };
        assert_eq!(Preferences::parse(&prefs.to_text()), prefs);
        // Значения вне пределов обрезаются, испорченные — остаются по умолчанию.
        let clamped = Preferences::parse("(undo_limit: 0, snap_move: -3, grid_color: #zz, profiler_history: 1e9)");
        assert_eq!(clamped.undo_limit, 1);
        assert_eq!(clamped.snap_move, 0.001);
        assert_eq!(clamped.grid_color, Preferences::default().grid_color);
        assert_eq!(clamped.profiler_history, Preferences::default().profiler_history);
    }

    #[test]
    fn external_editor_commands_split_and_fill_placeholders() {
        let file = std::path::Path::new("src/my game/player.rs");
        let (program, args) = external_command("code -g {file}:{line}", file, 12).unwrap();
        assert_eq!(program, "code");
        assert_eq!(args, ["-g", "src/my game/player.rs:12"]);
        let (program, args) = external_command("\"C:/Program Files/Notepad++/notepad++.exe\" -n{line}", file, 3).unwrap();
        assert_eq!(program, "C:/Program Files/Notepad++/notepad++.exe");
        assert_eq!(args, ["-n3", "src/my game/player.rs"], "without {{file}} the path goes last");
        assert!(external_command("   ", file, 1).is_none());
    }

    #[test]
    fn the_old_single_value_file_still_loads() {
        let prefs = Preferences::parse("(ui_scale: 1.25)\n");
        assert_eq!(prefs.ui_scale, 1.25);
        assert_eq!(prefs.accent, 0);
        assert!(prefs.redraw_on_demand);
        assert_eq!(Preferences::parse("garbage"), Preferences::default());
    }
}
