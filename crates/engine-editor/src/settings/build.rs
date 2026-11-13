//! Страница Build: настройки экспорта, которые понимает `piska-export`, — таблица
//! `[package.metadata.piska]` в `Cargo.toml` крейта игры (см. docs/api/export.md):
//!
//! | ключ | что |
//! |---|---|
//! | `title` | заголовок веб-страницы, имя приложения Android |
//! | `icon` | PNG из ассетов — значок на всех платформах |
//! | `android.package` | id приложения Android |
//! | `android.version_code` / `android.version_name` | версия для магазина |
//! | `android.orientation` | `landscape` / `portrait` / `any` |
//! | `android.min_sdk` | наименьший API Android (не ниже 26) |
//!
//! Файл правится через `toml_edit`: меняются только ключи, значение которых изменилось,
//! остальной текст (комментарии, порядок, другие таблицы) остаётся байт в байт.
//! Неверное значение не записывается: строка показывает ошибку, файл не меняется.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{vec2, Sense};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use super::rows::WIDE_W;
use super::{SettingsScope, SettingsUi};
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;
use crate::EditorError;

/// Наименьший API Android, который принимает экспорт (звук идёт через AAudio).
pub const ANDROID_MIN_SDK: u32 = 26;
/// Наибольший `android:versionCode`, который принимает Google Play.
pub const ANDROID_MAX_VERSION_CODE: u32 = 2_100_000_000;

/// Ориентация экрана Android (`android.orientation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AndroidOrientation {
    /// `landscape` — альбомная, с поворотом на 180° (по умолчанию).
    Landscape,
    /// `portrait` — книжная.
    Portrait,
    /// `any` — как держат телефон.
    Any,
}

impl AndroidOrientation {
    pub const ALL: [AndroidOrientation; 3] = [AndroidOrientation::Landscape, AndroidOrientation::Portrait, AndroidOrientation::Any];

    /// Значение в `Cargo.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            AndroidOrientation::Landscape => "landscape",
            AndroidOrientation::Portrait => "portrait",
            AndroidOrientation::Any => "any",
        }
    }

    /// Из `Cargo.toml` (`auto` — старое имя `any`, его понимает и экспорт).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "landscape" => Some(AndroidOrientation::Landscape),
            "portrait" => Some(AndroidOrientation::Portrait),
            "any" | "auto" => Some(AndroidOrientation::Any),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            AndroidOrientation::Landscape => "Landscape",
            AndroidOrientation::Portrait => "Portrait",
            AndroidOrientation::Any => "Any (follows the phone)",
        }
    }
}

/// Настройки экспорта из `[package.metadata.piska]`. `None` — ключа нет, экспорт берёт
/// своё значение по умолчанию.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildOptions {
    /// `title`: заголовок веб-страницы и имя приложения Android (по умолчанию — имя пакета).
    pub title: Option<String>,
    /// `icon`: путь PNG внутри `assets/` (по умолчанию `assets/icon.png`, иначе значок движка).
    pub icon: Option<String>,
    /// `android.package`: id приложения (по умолчанию `org.piska.<каталог игры>`).
    pub android_package: Option<String>,
    /// `android.version_code` (по умолчанию 1).
    pub android_version_code: Option<u32>,
    /// `android.version_name` (по умолчанию версия пакета).
    pub android_version_name: Option<String>,
    /// `android.orientation` (по умолчанию `landscape`).
    pub android_orientation: Option<AndroidOrientation>,
    /// `android.min_sdk` (по умолчанию 26).
    pub android_min_sdk: Option<u32>,
}

/// Проверить id приложения Android — то же правило, что у `piska-export`.
pub fn validate_android_package(id: &str) -> Result<(), String> {
    let parts: Vec<&str> = id.split('.').collect();
    let valid_part = |p: &&str| {
        let mut chars = p.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if parts.len() >= 2 && parts.iter().all(valid_part) {
        Ok(())
    } else {
        Err(format!(
            "`{id}` is not a valid Android package id: use at least two dot-separated parts, each starting \
             with a letter and containing only letters, digits and `_` (for example `com.example.my_game`)"
        ))
    }
}

impl BuildOptions {
    /// Проверить значения так же, как это сделает экспорт; ошибка объясняет, что исправить.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err("the title cannot be blank: clear the field to use the package name".to_string());
        }
        if let Some(icon) = &self.icon {
            if !icon.to_ascii_lowercase().ends_with(".png") {
                return Err(format!("icon `{icon}` must be a PNG file inside assets/"));
            }
        }
        if let Some(id) = &self.android_package {
            validate_android_package(id)?;
        }
        if let Some(code) = self.android_version_code {
            if code == 0 || code > ANDROID_MAX_VERSION_CODE {
                return Err(format!("the Android version code must be from 1 to {ANDROID_MAX_VERSION_CODE}, not {code}"));
            }
        }
        if self.android_version_name.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err("the Android version name cannot be blank: clear the field to use the package version".to_string());
        }
        if let Some(sdk) = self.android_min_sdk {
            if !(ANDROID_MIN_SDK..=99).contains(&sdk) {
                return Err(format!(
                    "the minimum Android SDK must be at least {ANDROID_MIN_SDK} (Android 8.0: the engine plays sound \
                     through AAudio) and at most 99, not {sdk}"
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Чтение и запись Cargo.toml
// ---------------------------------------------------------------------------

/// Таблица `[package.metadata.piska]` документа (если есть).
fn piska_table(doc: &DocumentMut) -> Option<&dyn TableLike> {
    doc.get("package")?.as_table_like()?.get("metadata")?.as_table_like()?.get("piska")?.as_table_like()
}

/// Прочитать настройки экспорта из текста `Cargo.toml`. Второе значение — ключи с
/// неверным значением (они не попадают в настройки; экспорт на них остановится).
pub fn read_build_options(manifest: &str) -> Result<(BuildOptions, Vec<String>), EditorError> {
    let doc: DocumentMut = manifest
        .parse()
        .map_err(|e| EditorError::Other(format!("Cargo.toml is not valid TOML: {e}. Fix the file and reopen this page")))?;
    if doc.get("package").and_then(Item::as_table_like).is_none() {
        return Err(EditorError::Other("Cargo.toml has no [package] table: it is not a game crate".to_string()));
    }
    let mut options = BuildOptions::default();
    let mut problems = Vec::new();
    let Some(piska) = piska_table(&doc) else {
        return Ok((options, problems));
    };
    let mut string = |table: &dyn TableLike, key: &str, path: &str| -> Option<String> {
        let item = table.get(key)?;
        match item.as_str() {
            Some(text) => Some(text.to_string()),
            None => {
                problems.push(format!("`{path}` must be a string, not `{}`", item.to_string().trim()));
                None
            }
        }
    };
    options.title = string(piska, "title", "title");
    options.icon = string(piska, "icon", "icon");
    let android = piska.get("android").and_then(Item::as_table_like);
    if let Some(android) = android {
        options.android_package = string(android, "package", "android.package");
        options.android_version_name = string(android, "version_name", "android.version_name");
        let orientation = string(android, "orientation", "android.orientation");
        let mut number = |key: &str| -> Option<u32> {
            let item = android.get(key)?;
            match item.as_integer() {
                Some(n) if n > 0 && n <= i64::from(u32::MAX >> 1) => Some(n as u32),
                _ => {
                    problems.push(format!("`android.{key}` must be a positive whole number, not `{}`", item.to_string().trim()));
                    None
                }
            }
        };
        options.android_version_code = number("version_code");
        options.android_min_sdk = number("min_sdk");
        if let Some(text) = orientation {
            match AndroidOrientation::parse(&text) {
                Some(o) => options.android_orientation = Some(o),
                None => problems.push(format!("`android.orientation = \"{text}\"` is not supported: use landscape, portrait or any")),
            }
        }
    }
    Ok((options, problems))
}

/// Дочерняя таблица `key` (создаётся при `create`): обычная `[a.b]` у обычной таблицы,
/// встроенная `{ … }` у встроенной.
fn child<'a>(parent: &'a mut dyn TableLike, key: &str, create: bool, implicit: bool) -> Result<Option<&'a mut dyn TableLike>, EditorError> {
    if parent.get(key).is_none() {
        if !create {
            return Ok(None);
        }
        let mut table = Table::new();
        table.set_implicit(implicit);
        parent.insert(key, Item::Table(table));
    }
    match parent.get_mut(key).and_then(Item::as_table_like_mut) {
        Some(table) => Ok(Some(table)),
        None => Err(EditorError::Other(format!("`{key}` in Cargo.toml is not a table: fix it by hand, then edit it here"))),
    }
}

/// Записать ключ: `None` — удалить. Комментарий рядом со старым значением сохраняется.
fn put(table: &mut dyn TableLike, key: &str, new: Option<Value>) {
    match new {
        None => {
            table.remove(key);
        }
        Some(mut value) => {
            if let Some(old) = table.get_mut(key).and_then(Item::as_value_mut) {
                *value.decor_mut() = old.decor().clone();
                *old = value;
            } else {
                table.insert(key, Item::Value(value));
            }
        }
    }
}

/// Вернуть текст `Cargo.toml` с настройками `options` в `[package.metadata.piska]`.
/// Меняются только ключи, чьё значение отличается от записанного; всё остальное — как было.
pub fn write_build_options(manifest: &str, options: &BuildOptions) -> Result<String, EditorError> {
    options.validate().map_err(EditorError::Other)?;
    let (current, _) = read_build_options(manifest)?;
    if current == *options {
        return Ok(manifest.to_string());
    }
    let mut doc: DocumentMut = manifest.parse().map_err(|e| EditorError::Other(format!("Cargo.toml is not valid TOML: {e}")))?;
    let str_value = |v: &Option<String>| v.as_ref().map(|s| Value::from(s.as_str()));
    let int_value = |v: Option<u32>| v.map(|n| Value::from(i64::from(n)));

    let top: Vec<(&str, Option<Value>, bool)> = vec![
        ("title", str_value(&options.title), current.title != options.title),
        ("icon", str_value(&options.icon), current.icon != options.icon),
    ];
    let android: Vec<(&str, Option<Value>, bool)> = vec![
        ("package", str_value(&options.android_package), current.android_package != options.android_package),
        ("version_code", int_value(options.android_version_code), current.android_version_code != options.android_version_code),
        ("version_name", str_value(&options.android_version_name), current.android_version_name != options.android_version_name),
        (
            "orientation",
            options.android_orientation.map(|o| Value::from(o.as_str())),
            current.android_orientation != options.android_orientation,
        ),
        ("min_sdk", int_value(options.android_min_sdk), current.android_min_sdk != options.android_min_sdk),
    ];
    let need_piska = top.iter().chain(&android).any(|(_, v, changed)| *changed && v.is_some());
    let need_android = android.iter().any(|(_, v, changed)| *changed && v.is_some());

    let package = doc
        .get_mut("package")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| EditorError::Other("Cargo.toml has no [package] table".to_string()))?;
    let Some(metadata) = child(package, "metadata", need_piska, true)? else {
        return Ok(manifest.to_string());
    };
    let Some(piska) = child(metadata, "piska", need_piska, false)? else {
        return Ok(manifest.to_string());
    };
    for (key, value, changed) in top {
        if changed {
            put(piska, key, value);
        }
    }
    if let Some(table) = child(piska, "android", need_android, false)? {
        for (key, value, changed) in android {
            if changed {
                put(table, key, value);
            }
        }
        // Пустую таблицу Android, оставшуюся после удаления ключей, убираем.
        if table.is_empty() {
            piska.remove("android");
        }
    }
    Ok(doc.to_string())
}

// ---------------------------------------------------------------------------
// Крейт игры
// ---------------------------------------------------------------------------

/// `Cargo.toml` крейта игры: пакет (не `engine`), который зависит от `engine`.
fn is_game_manifest(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    let name = doc.get("package").and_then(|p| p.get("name")).and_then(Item::as_str);
    let uses_engine = doc.get("dependencies").and_then(Item::as_table_like).is_some_and(|d| d.contains_key("engine"));
    name.is_some_and(|n| n != "engine") && uses_engine
}

/// Прочитанный `Cargo.toml`: настройки и ключи с неверным значением — или ошибка чтения.
type Loaded = Result<(BuildOptions, Vec<String>), String>;

/// Состояние страницы между кадрами.
#[derive(Default)]
pub(crate) struct BuildPageState {
    /// Прочитанный файл: путь, время изменения, содержимое.
    loaded: Option<(PathBuf, Option<SystemTime>, Loaded)>,
    /// Текстовые поля, которые сейчас неверны: ключ → (текст, ошибка).
    drafts: Vec<(&'static str, String, String)>,
    /// Изменённые настройки ждут записи (мышь отпущена, никто не печатает).
    pending: Option<BuildOptions>,
}

impl Editor {
    /// `Cargo.toml` крейта игры, который правит страница Build: каталог над каталогом
    /// ассетов, а если там не крейт игры — `games/<game_crate>` рабочего пространства.
    pub fn game_manifest_path(&self) -> Option<PathBuf> {
        let above_assets = self.assets_root().and_then(|root| root.parent().map(|p| p.join("Cargo.toml")));
        let named = self.config.game_crate.as_ref().map(|game| {
            self.config
                .workspace
                .clone()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_default()
                .join("games")
                .join(game)
                .join("Cargo.toml")
        });
        above_assets.into_iter().chain(named).find(|path| is_game_manifest(path))
    }

    /// Настройки экспорта из `Cargo.toml` игры.
    pub fn build_options(&self) -> Result<BuildOptions, EditorError> {
        let path = self.game_manifest_path().ok_or_else(no_game_crate)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| EditorError::Other(format!("cannot read `{}`: {e}", path.display())))?;
        read_build_options(&text).map(|(options, _)| options)
    }

    /// Проверить и записать настройки экспорта в `[package.metadata.piska]` `Cargo.toml`
    /// игры (остальной файл не меняется).
    pub fn set_build_options(&mut self, options: &BuildOptions) -> Result<(), EditorError> {
        let path = self.game_manifest_path().ok_or_else(no_game_crate)?;
        let result = write_manifest(&path, options);
        self.settings.build.loaded = None;
        let status = &mut self.settings.status[SettingsScope::Project.index()];
        match &result {
            Ok(()) => *status = Some((format!("Saved export options to {}", short_path(&path)), false)),
            Err(error) => *status = Some((error.to_string(), true)),
        }
        result
    }

    /// Поле страницы Build для ключа `key` (`title`, `android.package`, `android.min_sdk`…).
    pub fn build_field_id(key: &str) -> egui::Id {
        egui::Id::new(("settings_build_field", key.to_string()))
    }

    /// Ошибка поля `key` страницы Build: введённый текст неверен и в файл не записан.
    pub fn build_field_error(&self, key: &str) -> Option<String> {
        self.settings.build.drafts.iter().find(|(k, ..)| *k == key).map(|(_, _, error)| error.clone())
    }
}

fn no_game_crate() -> EditorError {
    EditorError::Other(
        "no game crate found: the Build page edits the Cargo.toml of the crate above the assets folder \
         (games/<name>/assets), or of `EditorConfig::game_crate(\"<name>\")`"
            .to_string(),
    )
}

fn write_manifest(path: &Path, options: &BuildOptions) -> Result<(), EditorError> {
    let text = std::fs::read_to_string(path).map_err(|e| EditorError::Other(format!("cannot read `{}`: {e}", path.display())))?;
    let new = write_build_options(&text, options)?;
    if new != text {
        std::fs::write(path, new).map_err(|e| EditorError::Other(format!("cannot write `{}`: {e}. Is the file read-only?", path.display())))?;
    }
    Ok(())
}

/// `games/lantern/Cargo.toml` — последние три части пути.
fn short_path(path: &Path) -> String {
    let parts: Vec<String> = path.components().rev().take(3).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

// ---------------------------------------------------------------------------
// Страница
// ---------------------------------------------------------------------------

/// Строка с текстом-необязательным значением: пусто — ключа нет. Неверный текст остаётся
/// в поле с ошибкой и не записывается.
#[allow(clippy::too_many_arguments)]
fn text_row(
    s: &mut SettingsUi<'_>,
    drafts: &mut Vec<(&'static str, String, String)>,
    key: &'static str,
    label: &str,
    description: &str,
    hint: &str,
    value: &mut Option<String>,
    check: impl Fn(&str) -> Result<(), String>,
) {
    let draft = drafts.iter().find(|(k, ..)| *k == key).map(|(_, text, error)| (text.clone(), error.clone()));
    let shown = draft.as_ref().map_or_else(|| value.clone().unwrap_or_default(), |(text, _)| text.clone());
    let row = s.row(label, description, value.is_some() || draft.is_some(), |ui| {
        let mut text = shown.clone();
        let edit = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id(Editor::build_field_id(key))
                .hint_text(egui::RichText::new(hint).color(theme::TEXT_DISABLED))
                .vertical_align(egui::Align::Center)
                .min_size(vec2(0.0, theme::FIELD_H))
                .desired_width(WIDE_W),
        );
        if let Some((_, error)) = &draft {
            let (rect, response) = ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
            icons::paint(ui.painter(), rect.shrink(1.0), Icon::Error, theme::ERROR);
            response.on_hover_text(error);
            ui.add(egui::Label::new(egui::RichText::new(error).font(theme::caption()).color(theme::ERROR)).truncate());
        }
        edit.changed().then_some(text)
    });
    drafts.retain(|(k, ..)| *k != key || !row.reset);
    if row.reset {
        *value = None;
        return;
    }
    if let Some(Some(text)) = row.inner {
        let trimmed = text.trim();
        drafts.retain(|(k, ..)| *k != key);
        if trimmed.is_empty() {
            *value = None;
        } else {
            match check(trimmed) {
                Ok(()) => *value = Some(trimmed.to_string()),
                Err(error) => drafts.push((key, text, error)),
            }
        }
    }
}

/// Строка с необязательным числом: пока ключа нет, показывается значение по умолчанию.
#[allow(clippy::too_many_arguments)]
fn number_row(
    s: &mut SettingsUi<'_>,
    key: &'static str,
    label: &str,
    description: &str,
    value: &mut Option<u32>,
    default: u32,
    range: std::ops::RangeInclusive<u32>,
) {
    let mut number = value.unwrap_or(default);
    let row = s.row(label, description, value.is_some(), |ui| {
        let response = ui.add_sized(
            vec2(super::rows::NUMBER_W, theme::FIELD_H),
            egui::DragValue::new(&mut number).range(range).speed(0.2),
        );
        let _ = ui.interact(response.rect, Editor::build_field_id(key), Sense::hover());
        if value.is_none() {
            ui.label(egui::RichText::new("default").font(theme::caption()).color(theme::TEXT_DISABLED));
        }
        response.changed()
    });
    if row.reset {
        *value = None;
    } else if row.inner == Some(true) {
        *value = Some(number);
    }
}

pub(super) fn build(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let Some(path) = editor.game_manifest_path() else {
        s.group("Game crate");
        s.note(
            "This project has no game crate, so there are no export options to edit. The Build page edits \
             [package.metadata.piska] in the Cargo.toml of the crate that holds the assets folder \
             (games/<name>/Cargo.toml, see docs/api/export.md). Create the game from games/template, or start \
             the editor with EditorConfig::game_crate(\"<name>\").",
        );
        return;
    };
    // Перечитать файл, если его поменяли на диске (или ещё не читали).
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let state = &mut editor.settings.build;
    let stale = match &state.loaded {
        Some((loaded, time, _)) => *loaded != path || *time != modified,
        None => true,
    };
    if stale && state.pending.is_none() {
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))
            .and_then(|text| read_build_options(&text).map_err(|e| e.to_string()));
        state.loaded = Some((path.clone(), modified, loaded));
        state.drafts.clear();
    }
    let loaded = state.loaded.as_ref().map(|(_, _, l)| l.clone()).expect("loaded above");
    let (saved, problems) = match loaded {
        Ok(found) => found,
        Err(error) => {
            s.group("Game crate");
            s.note(&format!("{error}. The export options cannot be edited until the file is fixed."));
            return;
        }
    };
    let mut options = state.pending.clone().unwrap_or_else(|| saved.clone());
    let icons = super::pages_project::assets_with(editor, "png");
    let package_name = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.parse::<DocumentMut>().ok())
        .and_then(|d| d.get("package")?.get("name")?.as_str().map(str::to_string))
        .unwrap_or_default();
    let state = &mut editor.settings.build;

    s.group("Game crate");
    s.row("Manifest", "Export options live in [package.metadata.piska] of this file; the rest of it is never touched.", false, |ui| {
        ui.label(egui::RichText::new(short_path(&path)).font(theme::mono()).color(theme::TEXT_MUTED))
            .on_hover_text(path.display().to_string());
    });
    for problem in &problems {
        s.note(&format!("Cargo.toml: {problem}. Export stops on it: set the value here or fix the file."));
    }

    s.group("All platforms");
    text_row(
        s,
        &mut state.drafts,
        "title",
        "Title",
        "Web page title and Android app name. Empty: the package name.",
        &package_name,
        &mut options.title,
        |_| Ok(()),
    );
    let mut icon = options.icon.clone().unwrap_or_default();
    if s.choice("Icon", "Icon of the exported game on every platform. None: assets/icon.png, otherwise the engine icon.", &mut icon, "", &icons, "None (icon.png)") {
        options.icon = (!icon.is_empty()).then_some(icon);
    }

    s.group("Android");
    text_row(
        s,
        &mut state.drafts,
        "android.package",
        "Package id",
        "Application id in the store, for example com.example.my_game. Empty: org.piska.<game folder>.",
        &format!("org.piska.{}", package_name.replace('-', "_")),
        &mut options.android_package,
        validate_android_package,
    );
    number_row(
        s,
        "android.version_code",
        "Version code",
        "Whole number that must grow with every release you upload to a store.",
        &mut options.android_version_code,
        1,
        1..=ANDROID_MAX_VERSION_CODE,
    );
    text_row(
        s,
        &mut state.drafts,
        "android.version_name",
        "Version name",
        "Version players see, for example 1.2.0. Empty: the package version.",
        "package version",
        &mut options.android_version_name,
        |_| Ok(()),
    );
    let current = options.android_orientation;
    let row = s.row("Orientation", "How the screen is held. Landscape and portrait still turn by 180°.", current.is_some(), |ui| {
        let mut value = current.unwrap_or(AndroidOrientation::Landscape);
        egui::ComboBox::from_id_salt(Editor::build_field_id("android.orientation"))
            .selected_text(if current.is_some() { value.label().to_string() } else { format!("{} (default)", value.label()) })
            .width(WIDE_W)
            .show_ui(ui, |ui| {
                for option in AndroidOrientation::ALL {
                    ui.selectable_value(&mut value, option, option.label());
                }
            });
        (Some(value) != current && (current.is_some() || value != AndroidOrientation::Landscape)).then_some(value)
    });
    if row.reset {
        options.android_orientation = None;
    } else if let Some(Some(value)) = row.inner {
        options.android_orientation = Some(value);
    }
    number_row(
        s,
        "android.min_sdk",
        "Minimum SDK",
        "Oldest Android API the game installs on. 26 (Android 8.0) is the lowest the engine supports.",
        &mut options.android_min_sdk,
        ANDROID_MIN_SDK,
        ANDROID_MIN_SDK..=99,
    );

    s.group("Windows and Linux");
    s.note(
        "Windows and Linux builds use the title and icon above. The glibc version of a Linux build is an option of \
         the export itself: piska-export linux --glibc 2.31.",
    );

    // Записать, когда правка закончилась (мышь отпущена, никто не печатает) — или сразу в
    // проходе Reset.
    let state = &mut editor.settings.build;
    // Поле с ошибкой оставляет в файле прежнее значение (а не то, что было до последней буквы).
    for (key, ..) in &state.drafts {
        match *key {
            "title" => options.title = saved.title.clone(),
            "android.package" => options.android_package = saved.android_package.clone(),
            "android.version_name" => options.android_version_name = saved.android_version_name.clone(),
            _ => {}
        }
    }
    if options != saved || state.pending.is_some() {
        state.pending = Some(options);
    }
    let quiet = s.quiet();
    if quiet {
        if let Some(options) = editor.settings.build.pending.take() {
            if options != saved {
                let _ = editor.set_build_options(&options);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"# Игра «Фонарь».
[package]
name = "lantern"   # имя пакета
version.workspace = true
edition.workspace = true

# Настройки экспорта.
[package.metadata.piska]
title = "Lantern"    # заголовок
icon = "sprites/icon.png"

[package.metadata.piska.android] # Android
package = "com.example.lantern" # id
orientation = "portrait"

[dependencies]
# Движок без редактора.
engine = { path = "../../crates/engine", default-features = false, features = ["ui"] }

[features]
default = []
"#;

    #[test]
    fn reads_every_key_the_export_tool_understands() {
        let (options, problems) = read_build_options(MANIFEST).unwrap();
        assert!(problems.is_empty());
        assert_eq!(options.title.as_deref(), Some("Lantern"));
        assert_eq!(options.icon.as_deref(), Some("sprites/icon.png"));
        assert_eq!(options.android_package.as_deref(), Some("com.example.lantern"));
        assert_eq!(options.android_orientation, Some(AndroidOrientation::Portrait));
        assert_eq!(options.android_version_code, None);
    }

    #[test]
    fn writing_changes_only_the_edited_keys_and_keeps_comments_and_other_tables() {
        let (mut options, _) = read_build_options(MANIFEST).unwrap();
        options.android_package = Some("org.studio.lantern".into());
        options.android_min_sdk = Some(30);
        options.icon = None;
        let text = write_build_options(MANIFEST, &options).unwrap();
        // Изменилось ровно три строки.
        assert!(text.contains(r#"package = "org.studio.lantern" # id"#), "{text}");
        assert!(text.contains("min_sdk = 30"), "{text}");
        assert!(!text.contains("icon ="), "{text}");
        for kept in [
            "# Игра «Фонарь».",
            r#"name = "lantern"   # имя пакета"#,
            "version.workspace = true",
            "# Настройки экспорта.",
            r#"title = "Lantern"    # заголовок"#,
            "[package.metadata.piska.android] # Android",
            "# Движок без редактора.",
            r#"engine = { path = "../../crates/engine", default-features = false, features = ["ui"] }"#,
            "[features]\ndefault = []",
        ] {
            assert!(text.contains(kept), "`{kept}` is kept:\n{text}");
        }
        assert_eq!(read_build_options(&text).unwrap().0, options, "round trip");
        // Ещё раз то же самое — текст не меняется.
        assert_eq!(write_build_options(&text, &options).unwrap(), text);
    }

    #[test]
    fn a_manifest_without_the_table_gets_it_after_package_and_loses_it_when_cleared() {
        let bare = "[package]\nname = \"g\"\nversion = \"0.1.0\"\n\n[dependencies]\nengine = { path = \"../../crates/engine\" }\n";
        let options = BuildOptions {
            title: Some("G".into()),
            android_orientation: Some(AndroidOrientation::Any),
            ..BuildOptions::default()
        };
        let text = write_build_options(bare, &options).unwrap();
        assert!(!text.contains("[package.metadata]\n"), "metadata stays implicit:\n{text}");
        let piska = text.find("[package.metadata.piska]").expect("table added");
        assert!(piska < text.find("[dependencies]").unwrap(), "next to [package]:\n{text}");
        assert!(text.contains("[package.metadata.piska.android]\norientation = \"any\""), "{text}");
        assert_eq!(read_build_options(&text).unwrap().0, options);
        let cleared = write_build_options(&text, &BuildOptions::default()).unwrap();
        assert!(!cleared.contains("android"), "an emptied android table goes away:\n{cleared}");
        assert!(cleared.contains("[dependencies]\nengine"), "{cleared}");
    }

    #[test]
    fn invalid_values_are_rejected_with_a_hint() {
        let mut options = BuildOptions { android_package: Some("1game".into()), ..BuildOptions::default() };
        let error = write_build_options(MANIFEST, &options).unwrap_err().to_string();
        assert!(error.contains("not a valid Android package id") && error.contains("com.example.my_game"), "{error}");
        options.android_package = None;
        options.android_min_sdk = Some(21);
        let error = options.validate().unwrap_err();
        assert!(error.contains("at least 26"), "{error}");
        options.android_min_sdk = None;
        options.icon = Some("icon.jpg".into());
        assert!(options.validate().unwrap_err().contains("PNG"));
        let (_, problems) = read_build_options("[package]\nname = \"g\"\n[package.metadata.piska.android]\norientation = \"sideways\"\nmin_sdk = \"x\"\n").unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
