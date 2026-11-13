//! Логика окна Project без egui: история переходов, разбор пути и поискового запроса,
//! выделение, набор имени «на лету», навигация стрелками и настройки окна, которые
//! переживают перезапуск. Рисует всё это `project*.rs`; здесь — то, что проверяют тесты.

use std::collections::BTreeSet;

use egui::Rect;

use crate::sorting::TypeFilter;

/// «Место» окна Code в истории переходов (каталоги — их путь, корень — пустая строка).
pub(crate) const CODE_LOC: &str = "\u{1}code";

// ----------------------------------------------------------------------
// История
// ----------------------------------------------------------------------

/// История переходов: Назад / Вперёд как в проводнике.
#[derive(Default, Debug, Clone)]
pub(crate) struct History {
    entries: Vec<String>,
    position: usize,
}

impl History {
    const LIMIT: usize = 100;

    /// Записать переход. Тот же каталог подряд не дублируется; переход после «Назад»
    /// отбрасывает «Вперёд».
    pub fn visit(&mut self, location: &str) {
        if self.entries.is_empty() {
            self.entries.push(location.to_string());
            self.position = 0;
            return;
        }
        if self.entries[self.position] == location {
            return;
        }
        self.entries.truncate(self.position + 1);
        self.entries.push(location.to_string());
        if self.entries.len() > Self::LIMIT {
            self.entries.remove(0);
        }
        self.position = self.entries.len() - 1;
    }

    pub fn can_back(&self) -> bool {
        self.position > 0
    }

    pub fn can_forward(&self) -> bool {
        self.position + 1 < self.entries.len()
    }

    /// Шаг назад к ближайшему месту, которое ещё существует.
    pub fn back(&mut self, exists: &dyn Fn(&str) -> bool) -> Option<String> {
        while self.position > 0 {
            self.position -= 1;
            if exists(&self.entries[self.position]) {
                return Some(self.entries[self.position].clone());
            }
        }
        None
    }

    pub fn forward(&mut self, exists: &dyn Fn(&str) -> bool) -> Option<String> {
        while self.position + 1 < self.entries.len() {
            self.position += 1;
            if exists(&self.entries[self.position]) {
                return Some(self.entries[self.position].clone());
            }
        }
        None
    }
}

// ----------------------------------------------------------------------
// Путь, набранный в строке пути
// ----------------------------------------------------------------------

/// Куда ведёт набранный путь.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PathTarget {
    Folder(String),
    /// Файл: открыть его каталог и выделить его.
    Asset(String),
}

/// Разобрать путь из строки пути: `Assets/models`, `models`, `/models/`, `models\crystal.obj`.
/// `folders` и `files` — всё, что есть в проекте.
pub(crate) fn resolve_path(text: &str, folders: &[String], files: &[String]) -> Result<PathTarget, String> {
    let cleaned = text.trim().trim_matches('"').replace('\\', "/");
    let mut path = cleaned.trim_matches('/').to_string();
    if path.eq_ignore_ascii_case("assets") {
        path.clear();
    } else if path.len() > 7 && path[..7].eq_ignore_ascii_case("assets/") {
        path = path[7..].to_string();
    }
    if path.is_empty() {
        return Ok(PathTarget::Folder(String::new()));
    }
    if let Some(found) = folders.iter().find(|f| f.eq_ignore_ascii_case(&path)) {
        return Ok(PathTarget::Folder(found.clone()));
    }
    if let Some(found) = files.iter().find(|f| f.eq_ignore_ascii_case(&path)) {
        return Ok(PathTarget::Asset(found.clone()));
    }
    Err(format!("`{}` is not a folder in this project", text.trim()))
}

// ----------------------------------------------------------------------
// Поиск
// ----------------------------------------------------------------------

/// Область поиска.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Scope {
    /// Текущий каталог и всё, что в нём.
    #[default]
    ThisFolder,
    AllAssets,
}

impl Scope {
    pub fn id(self) -> &'static str {
        match self {
            Scope::ThisFolder => "folder",
            Scope::AllAssets => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::ThisFolder => "This Folder",
            Scope::AllAssets => "All Assets",
        }
    }

    pub fn parse(text: &str) -> Option<Scope> {
        [Scope::ThisFolder, Scope::AllAssets].into_iter().find(|s| s.id() == text.trim())
    }
}

/// Разобранный запрос: слова (все должны встретиться в пути) и фильтры `t:тип`.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    pub words: Vec<String>,
    pub types: BTreeSet<TypeFilter>,
    /// Токены `t:…` с типом, которого нет: ничего не находят, интерфейс подсказывает.
    pub unknown_types: Vec<String>,
}

/// Тип по слову из токена: `t:script`, `t:scenes`, `t:model` …
pub(crate) fn type_token(word: &str) -> Option<TypeFilter> {
    match word.to_ascii_lowercase().trim_end_matches('s') {
        "script" | "code" | "scenario" | "rust" | "cpp" => Some(TypeFilter::Scripts),
        "scene" | "level" => Some(TypeFilter::Scenes),
        "prefab" => Some(TypeFilter::Prefabs),
        "model" | "mesh" => Some(TypeFilter::Models),
        "material" => Some(TypeFilter::Materials),
        "texture" | "image" | "sprite" => Some(TypeFilter::Textures),
        "audio" | "sound" => Some(TypeFilter::Audio),
        "other" | "font" | "text" | "file" => Some(TypeFilter::Other),
        _ => None,
    }
}

impl Query {
    pub fn parse(text: &str) -> Query {
        let mut query = Query::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(kind) = lower.strip_prefix("t:") {
                match type_token(kind) {
                    Some(filter) => {
                        query.types.insert(filter);
                    }
                    None if kind.is_empty() => {}
                    None => query.unknown_types.push(kind.to_string()),
                }
            } else {
                query.words.push(lower);
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.types.is_empty() && self.unknown_types.is_empty()
    }

    /// Подходит ли ассет: путь содержит все слова, тип — один из `t:`.
    pub fn matches(&self, path: &str, importer: &str) -> bool {
        if !self.unknown_types.is_empty() {
            return false;
        }
        if !self.types.is_empty() && !self.types.contains(&TypeFilter::of_importer(importer)) {
            return false;
        }
        let lower = path.to_lowercase();
        self.words.iter().all(|w| lower.contains(w.as_str()))
    }
}

// ----------------------------------------------------------------------
// Выделение
// ----------------------------------------------------------------------

/// Щелчок по элементу `index` списка `keys` (в порядке показа).
pub(crate) fn click_select(
    selection: &mut BTreeSet<String>,
    anchor: &mut Option<String>,
    keys: &[String],
    index: usize,
    ctrl: bool,
    shift: bool,
) {
    let key = &keys[index];
    if shift {
        let from = anchor.as_ref().and_then(|a| keys.iter().position(|k| k == a)).unwrap_or(index);
        let (low, high) = (from.min(index), from.max(index));
        if !ctrl {
            selection.clear();
        }
        selection.extend(keys[low..=high].iter().cloned());
        // Якорь при Shift остаётся: диапазон можно менять.
        if anchor.is_none() {
            *anchor = Some(key.clone());
        }
    } else if ctrl {
        if !selection.remove(key) {
            selection.insert(key.clone());
        }
        *anchor = Some(key.clone());
    } else {
        selection.clear();
        selection.insert(key.clone());
        *anchor = Some(key.clone());
    }
}

// ----------------------------------------------------------------------
// Набор имени «на лету»
// ----------------------------------------------------------------------

/// Буква за буквой набранное имя: пауза больше секунды начинает слово заново.
#[derive(Default, Debug)]
pub(crate) struct TypeAhead {
    buffer: String,
    last: f64,
}

impl TypeAhead {
    const PAUSE: f64 = 1.0;

    pub fn feed(&mut self, text: &str, now: f64) {
        if now - self.last > Self::PAUSE {
            self.buffer.clear();
        }
        self.last = now;
        self.buffer.push_str(&text.to_lowercase());
    }

    /// Какой из `names` выбрать. Одна буква (или её повтор) перебирает подряд идущие
    /// совпадения, начиная с элемента после `current`; несколько разных — первое совпадение.
    pub fn find(&self, names: &[String], current: Option<usize>) -> Option<usize> {
        if self.buffer.is_empty() || names.is_empty() {
            return None;
        }
        let first = self.buffer.chars().next()?;
        let repeated = self.buffer.chars().all(|c| c == first);
        let needle: &str = if repeated { &self.buffer[..first.len_utf8()] } else { &self.buffer };
        let starts = |i: usize| names[i].to_lowercase().starts_with(needle);
        if repeated {
            let from = current.map_or(0, |c| c + 1);
            (from..names.len()).chain(0..from.min(names.len())).find(|&i| starts(i))
        } else {
            (0..names.len()).find(|&i| starts(i))
        }
    }
}

// ----------------------------------------------------------------------
// Навигация стрелками
// ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Соседний элемент в направлении `dir` по прямоугольникам: работает и для плиток, и для
/// списка, и для плиток в нескольких группах.
pub(crate) fn neighbour(rects: &[Rect], current: usize, dir: Dir) -> Option<usize> {
    let from = rects.get(current)?;
    let center = from.center();
    let half = from.height() * 0.5;
    let mut best: Option<(usize, f32)> = None;
    let mut consider = |i: usize, score: f32| {
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((i, score));
        }
    };
    for (i, rect) in rects.iter().enumerate() {
        if i == current {
            continue;
        }
        let c = rect.center();
        match dir {
            Dir::Left if (c.y - center.y).abs() < half && c.x < center.x => consider(i, center.x - c.x),
            Dir::Right if (c.y - center.y).abs() < half && c.x > center.x => consider(i, c.x - center.x),
            Dir::Up if c.y < center.y - half => consider(i, (center.y - c.y) * 4.0 + (c.x - center.x).abs()),
            Dir::Down if c.y > center.y + half => consider(i, (c.y - center.y) * 4.0 + (c.x - center.x).abs()),
            _ => {}
        }
    }
    match (best, dir) {
        (Some((i, _)), _) => Some(i),
        // Конец строки: Влево/Вправо переходят на соседний по порядку элемент.
        (None, Dir::Left) if current > 0 => Some(current - 1),
        (None, Dir::Right) if current + 1 < rects.len() => Some(current + 1),
        _ => None,
    }
}

// ----------------------------------------------------------------------
// Настройки окна Project
// ----------------------------------------------------------------------

/// Что окно Project помнит между запусками (хранится в `Preferences`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectPrefs {
    /// Закреплённые каталоги в «Favorites».
    pub favorites: Vec<String>,
    /// Сохранённые поиски: (имя, запрос).
    pub searches: Vec<(String, String)>,
    /// Ширины столбцов списка: Type, Modified, Size.
    pub columns: [f32; 3],
    /// Ширина дерева каталогов слева; 0 — по умолчанию.
    pub split: f32,
    /// Панель сведений открыта.
    pub details: bool,
    /// Плитки (`true`) или список.
    pub grid: bool,
    /// Размер плитки.
    pub tile: f32,
    /// Область поиска.
    pub scope: String,
}

impl Default for ProjectPrefs {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            searches: Vec::new(),
            columns: [104.0, 96.0, 66.0],
            split: 0.0,
            details: true,
            grid: true,
            tile: 88.0,
            scope: Scope::ThisFolder.id().to_string(),
        }
    }
}

/// Символы, которые ломают построчный формат настроек, записываются как `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '%' | ',' | ':' | '|' | '~' | '+' | '\n' | '\r' | '(' | ')') {
            out.push_str(&format!("%{:02X}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

fn decode(text: &str) -> String {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let value = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(value) = value {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl ProjectPrefs {
    /// Применить одну настройку `pj_*`. Незнакомое и испорченное пропускается.
    pub(crate) fn set(&mut self, key: &str, value: &str) {
        match key {
            "pj_favorites" => {
                self.favorites = value.split('|').filter(|p| !p.is_empty()).map(decode).collect();
            }
            "pj_searches" => {
                self.searches = value
                    .split('|')
                    .filter_map(|entry| entry.split_once('~'))
                    .map(|(name, query)| (decode(name), decode(query)))
                    .filter(|(name, _)| !name.is_empty())
                    .collect();
            }
            "pj_columns" => {
                let widths: Vec<f32> = value.split('/').filter_map(|w| w.trim().parse().ok()).collect();
                if let [a, b, c] = widths[..] {
                    self.columns = [a, b, c].map(|w: f32| w.clamp(40.0, 400.0));
                }
            }
            "pj_split" => {
                if let Ok(v) = value.parse::<f32>() {
                    self.split = if v >= 60.0 { v.min(900.0) } else { 0.0 };
                }
            }
            "pj_details" => self.details = value != "false",
            "pj_grid" => self.grid = value != "false",
            "pj_tile" => {
                if let Ok(v) = value.parse::<f32>() {
                    self.tile = v.clamp(40.0, 200.0);
                }
            }
            "pj_scope" if Scope::parse(value).is_some() => self.scope = value.trim().to_string(),
            _ => {}
        }
    }

    pub(crate) fn to_text(&self) -> String {
        let favorites: Vec<String> = self.favorites.iter().map(|f| encode(f)).collect();
        let searches: Vec<String> = self.searches.iter().map(|(n, q)| format!("{}~{}", encode(n), encode(q))).collect();
        format!(
            "    pj_favorites: {},\n    pj_searches: {},\n    pj_columns: {:.0}/{:.0}/{:.0},\n    pj_split: {:.0},\n    pj_details: {},\n    pj_grid: {},\n    pj_tile: {:.0},\n    pj_scope: {},\n",
            favorites.join("|"),
            searches.join("|"),
            self.columns[0],
            self.columns[1],
            self.columns[2],
            self.split,
            self.details,
            self.grid,
            self.tile,
            self.scope,
        )
    }

    pub(crate) fn scope(&self) -> Scope {
        Scope::parse(&self.scope).unwrap_or_default()
    }
}

/// Сохранённые поиски по умолчанию: «All Scripts», «All Scenes» …
pub(crate) const DEFAULT_SEARCHES: [(&str, &str); 5] = [
    ("All Scripts", "t:script"),
    ("All Scenes", "t:scene"),
    ("All Models", "t:model"),
    ("All Materials", "t:material"),
    ("All Prefabs", "t:prefab"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn history_goes_back_and_forward_and_forgets_the_future() {
        let mut history = History::default();
        let all = |_: &str| true;
        history.visit("");
        history.visit("a");
        history.visit("a/b");
        history.visit("a/b");
        assert_eq!(history.back(&all).as_deref(), Some("a"));
        assert_eq!(history.back(&all).as_deref(), Some(""));
        assert_eq!(history.back(&all), None);
        assert!(history.can_forward());
        assert_eq!(history.forward(&all).as_deref(), Some("a"));
        history.visit("z");
        assert!(!history.can_forward(), "a new visit drops the forward stack");
        assert_eq!(history.back(&all).as_deref(), Some("a"));
    }

    #[test]
    fn history_skips_folders_that_no_longer_exist() {
        let mut history = History::default();
        for place in ["", "gone", "gone/inner", "kept"] {
            history.visit(place);
        }
        let exists = |p: &str| !p.starts_with("gone");
        assert_eq!(history.back(&exists).as_deref(), Some(""));
    }

    #[test]
    fn typed_paths_resolve() {
        let folders = vec!["models".to_string(), "models/props".to_string()];
        let files = vec!["models/crystal.obj".to_string()];
        let ok = |t: &str| resolve_path(t, &folders, &files);
        assert_eq!(ok("Assets/models"), Ok(PathTarget::Folder("models".into())));
        assert_eq!(ok("  /MODELS/props/ "), Ok(PathTarget::Folder("models/props".into())));
        assert_eq!(ok("Assets"), Ok(PathTarget::Folder(String::new())));
        assert_eq!(ok("models\\crystal.obj"), Ok(PathTarget::Asset("models/crystal.obj".into())));
        assert!(ok("nowhere").unwrap_err().contains("not a folder"));
    }

    #[test]
    fn queries_split_words_and_type_tokens() {
        let q = Query::parse("Coin t:scene t:materials");
        assert_eq!(q.words, vec!["coin"]);
        assert!(q.types.contains(&TypeFilter::Scenes) && q.types.contains(&TypeFilter::Materials));
        assert!(q.matches("levels/COIN.scene", "Scene"));
        assert!(!q.matches("levels/coin.png", "Texture"));
        assert!(!q.matches("levels/other.scene", "Scene"));
        let bad = Query::parse("t:banana");
        assert_eq!(bad.unknown_types, vec!["banana"]);
        assert!(!bad.matches("a", "Scene"));
        assert!(Query::parse("   ").is_empty());
    }

    #[test]
    fn click_selection_follows_ctrl_and_shift() {
        let keys: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        let (mut sel, mut anchor) = (BTreeSet::new(), None);
        click_select(&mut sel, &mut anchor, &keys, 1, false, false);
        click_select(&mut sel, &mut anchor, &keys, 3, false, true);
        assert_eq!(sel.iter().cloned().collect::<Vec<_>>(), ["b", "c", "d"]);
        click_select(&mut sel, &mut anchor, &keys, 0, false, true);
        assert_eq!(sel.len(), 2, "the range follows the anchor `b`: a..b");
        click_select(&mut sel, &mut anchor, &keys, 4, true, false);
        assert!(sel.contains("e") && sel.contains("a"));
        click_select(&mut sel, &mut anchor, &keys, 4, true, false);
        assert!(!sel.contains("e"), "ctrl toggles");
        click_select(&mut sel, &mut anchor, &keys, 2, false, false);
        assert_eq!(sel.len(), 1);
    }

    #[test]
    fn type_ahead_builds_words_and_cycles_one_letter() {
        let names: Vec<String> = ["apple", "banana", "blue", "bluebell", "cherry"].iter().map(|s| s.to_string()).collect();
        let mut ahead = TypeAhead::default();
        ahead.feed("b", 0.0);
        assert_eq!(ahead.find(&names, None), Some(1));
        assert_eq!(ahead.find(&names, Some(1)), Some(2), "the same letter again moves on");
        ahead.feed("l", 0.3);
        assert_eq!(ahead.find(&names, Some(1)), Some(2), "`bl` is a word now");
        ahead.feed("u", 0.5);
        ahead.feed("e", 0.6);
        ahead.feed("b", 0.7);
        assert_eq!(ahead.find(&names, None), Some(3), "`blueb`");
        ahead.feed("c", 5.0);
        assert_eq!(ahead.find(&names, None), Some(4), "a pause starts a new word");
    }

    #[test]
    fn arrows_walk_a_grid() {
        // Сетка 3 × 2 + хвост из одного элемента.
        let rects: Vec<Rect> = (0..7)
            .map(|i| Rect::from_min_size(pos2((i % 3) as f32 * 100.0, (i / 3) as f32 * 100.0), vec2(90.0, 90.0)))
            .collect();
        assert_eq!(neighbour(&rects, 0, Dir::Right), Some(1));
        assert_eq!(neighbour(&rects, 1, Dir::Down), Some(4));
        assert_eq!(neighbour(&rects, 5, Dir::Down), Some(6), "the nearest item of the row below");
        assert_eq!(neighbour(&rects, 6, Dir::Down), None, "no row below");
        assert_eq!(neighbour(&rects, 2, Dir::Right), Some(3), "the end of a row wraps to the next item");
        assert_eq!(neighbour(&rects, 3, Dir::Left), Some(2));
        assert_eq!(neighbour(&rects, 3, Dir::Up), Some(0));
        assert_eq!(neighbour(&rects, 0, Dir::Up), None);
    }

    #[test]
    fn project_prefs_survive_a_text_round_trip() {
        let prefs = ProjectPrefs {
            favorites: vec!["models".into(), "weird,name: (1)+x|y".into()],
            searches: vec![("Big 100% ~ ones".into(), "t:texture size,big".into())],
            columns: [120.0, 80.0, 70.0],
            split: 210.0,
            details: false,
            grid: false,
            tile: 120.0,
            scope: "all".into(),
        };
        let mut back = ProjectPrefs::default();
        for line in prefs.to_text().lines() {
            let (key, value) = line.split_once(':').unwrap();
            back.set(key.trim(), value.trim().trim_end_matches(','));
        }
        assert_eq!(back, prefs);
    }
}
