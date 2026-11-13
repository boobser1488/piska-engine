//! Сортировка, фильтры по типу и группировка в окне Project: чистая логика без egui,
//! чтобы её можно было проверить тестами. Рисует всё это `project.rs`.
//!
//! Выбор пользователя (ключ, направление, включённые фильтры) хранится в
//! [`Preferences`](crate::prefs::Preferences) строками и переживает перезапуск.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// По чему сортируется содержимое каталога.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortKey {
    Name,
    Type,
    Modified,
    Size,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [SortKey::Name, SortKey::Type, SortKey::Modified, SortKey::Size];

    /// Строка в файле настроек.
    pub fn id(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Type => "type",
            SortKey::Modified => "modified",
            SortKey::Size => "size",
        }
    }

    pub fn parse(text: &str) -> Option<SortKey> {
        SortKey::ALL.into_iter().find(|key| key.id() == text.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Type => "Type",
            SortKey::Modified => "Date modified",
            SortKey::Size => "Size",
        }
    }

    /// Какое направление естественно для первого щелчка по заголовку: новые файлы и
    /// большие размеры интересуют раньше старых и маленьких.
    pub fn default_descending(self) -> bool {
        matches!(self, SortKey::Modified | SortKey::Size)
    }
}

/// Ключ и направление.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SortSpec {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self { key: SortKey::Name, descending: false }
    }
}

impl SortSpec {
    /// Щелчок по заголовку столбца: тот же ключ — сменить направление, другой — взять
    /// его и естественное для него направление.
    pub fn click(&mut self, key: SortKey) {
        if self.key == key {
            self.descending = !self.descending;
        } else {
            *self = SortSpec { key, descending: key.default_descending() };
        }
    }
}

/// Тип ассета для фильтра-«чипа».
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TypeFilter {
    Scripts,
    Scenes,
    Prefabs,
    Models,
    Materials,
    Textures,
    Audio,
    Other,
}

impl TypeFilter {
    pub const ALL: [TypeFilter; 8] = [
        TypeFilter::Scripts,
        TypeFilter::Scenes,
        TypeFilter::Prefabs,
        TypeFilter::Models,
        TypeFilter::Materials,
        TypeFilter::Textures,
        TypeFilter::Audio,
        TypeFilter::Other,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TypeFilter::Scripts => "scripts",
            TypeFilter::Scenes => "scenes",
            TypeFilter::Prefabs => "prefabs",
            TypeFilter::Models => "models",
            TypeFilter::Materials => "materials",
            TypeFilter::Textures => "textures",
            TypeFilter::Audio => "audio",
            TypeFilter::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TypeFilter::Scripts => "Scripts",
            TypeFilter::Scenes => "Scenes",
            TypeFilter::Prefabs => "Prefabs",
            TypeFilter::Models => "Models",
            TypeFilter::Materials => "Materials",
            TypeFilter::Textures => "Textures",
            TypeFilter::Audio => "Audio",
            TypeFilter::Other => "Other",
        }
    }

    pub fn parse(text: &str) -> Option<TypeFilter> {
        TypeFilter::ALL.into_iter().find(|filter| filter.id() == text.trim())
    }

    /// К какому типу относится ассет с импортёром `importer`. «Scripts» — исходники на
    /// Rust, C++ и сценарии Scenario.
    pub fn of_importer(importer: &str) -> TypeFilter {
        match importer {
            "Code" | "Scenario" => TypeFilter::Scripts,
            "Scene" => TypeFilter::Scenes,
            "Prefab" => TypeFilter::Prefabs,
            "Mesh" => TypeFilter::Models,
            "Material" => TypeFilter::Materials,
            "Texture" => TypeFilter::Textures,
            "Audio" => TypeFilter::Audio,
            _ => TypeFilter::Other,
        }
    }
}

/// Набор включённых фильтров из настроек: `scripts+scenes`. Пустой набор — «All».
pub(crate) fn parse_filters(text: &str) -> BTreeSet<TypeFilter> {
    text.split('+').filter_map(TypeFilter::parse).collect()
}

pub(crate) fn filters_text(filters: &BTreeSet<TypeFilter>) -> String {
    filters.iter().map(|f| f.id()).collect::<Vec<_>>().join("+")
}

/// Подпись типа в столбце Type (и ключ сортировки по типу).
pub(crate) fn type_label(importer: &str, path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    match importer {
        "Code" => match extension.as_str() {
            "rs" => "Rust script",
            "cpp" | "cc" | "cxx" => "C++ source",
            "h" | "hpp" => "C++ header",
            _ => "Code",
        },
        "Scenario" => "Scenario script",
        "Scene" => "Scene",
        "Prefab" => "Prefab",
        "Mesh" => "Model",
        "Material" => "Material",
        "Texture" => "Texture",
        "Audio" => "Audio",
        "Font" => "Font",
        "Text" => "Text",
        _ => "File",
    }
}

/// Язык исходника для группировки в «Code».
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Language {
    Rust,
    Cpp,
    Scenario,
    Other,
}

impl Language {
    pub fn of_path(path: &str) -> Language {
        match path.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase().as_str() {
            "rs" => Language::Rust,
            "cpp" | "cc" | "cxx" | "h" | "hpp" => Language::Cpp,
            "scenario" => Language::Scenario,
            _ => Language::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Cpp => "C++",
            Language::Scenario => "Scenario",
            Language::Other => "Other",
        }
    }
}

/// Всё, что нужно сравнению об одном элементе каталога.
#[derive(Clone, Debug)]
pub(crate) struct ItemInfo {
    pub name: String,
    pub kind: &'static str,
    pub folder: bool,
    pub modified: Option<SystemTime>,
    pub size: Option<u64>,
}

/// Естественное сравнение имён без учёта регистра: `item2` раньше `item10`.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                // Число длиннее — больше; при равной длине достаточно сравнить строки.
                let order = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Порядок двух элементов: каталоги всегда впереди (и в убывающем порядке тоже), внутри —
/// по ключу; при равенстве — по имени по возрастанию, чтобы порядок был предсказуем.
pub(crate) fn compare(spec: SortSpec, a: &ItemInfo, b: &ItemInfo) -> Ordering {
    if a.folder != b.folder {
        return if a.folder { Ordering::Less } else { Ordering::Greater };
    }
    let by_key = match spec.key {
        SortKey::Name => natural_cmp(&a.name, &b.name),
        SortKey::Type => natural_cmp(a.kind, b.kind),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Size => a.size.cmp(&b.size),
    };
    let by_key = if spec.descending { by_key.reverse() } else { by_key };
    by_key.then_with(|| natural_cmp(&a.name, &b.name))
}

/// Отсортировать индексы `items` (возвращает порядок индексов).
pub(crate) fn sorted_order(spec: SortSpec, items: &[ItemInfo]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| compare(spec, &items[a], &items[b]));
    order
}

/// Размер и время правки файлов, которые видны в окне. `stat` дорог на медленных
/// дисках, поэтому ответ живёт две секунды.
#[derive(Default)]
pub(crate) struct MetaCache {
    map: HashMap<String, (Instant, Option<u64>, Option<SystemTime>)>,
}

impl MetaCache {
    const TTL: Duration = Duration::from_secs(2);

    pub fn get(&mut self, file: &std::path::Path) -> (Option<u64>, Option<SystemTime>) {
        let key = file.to_string_lossy().into_owned();
        if let Some((at, size, modified)) = self.map.get(&key) {
            if at.elapsed() < Self::TTL {
                return (*size, *modified);
            }
        }
        let meta = std::fs::metadata(file).ok();
        let size = meta.as_ref().map(|m| m.len());
        let modified = meta.and_then(|m| m.modified().ok());
        self.map.insert(key, (Instant::now(), size, modified));
        (size, modified)
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// «5 min ago», «3 h ago», «2 d ago», дальше — дата `2026-11-12` (UTC).
pub(crate) fn modified_label(now: SystemTime, modified: Option<SystemTime>) -> String {
    let Some(modified) = modified else {
        return String::new();
    };
    let Ok(age) = now.duration_since(modified) else {
        return "just now".to_string();
    };
    let seconds = age.as_secs();
    match seconds {
        0..=44 => "just now".to_string(),
        45..=3599 => format!("{} min ago", (seconds / 60).max(1)),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        86_400..=604_799 => format!("{} d ago", seconds / 86_400),
        _ => date_label(modified),
    }
}

/// `2026-11-12` по григорианскому календарю (UTC).
pub(crate) fn date_label(time: SystemTime) -> String {
    let days = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Алгоритм Говарда Хиннанта: дни от 1970-01-01 → год, месяц, день.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `2048` → «2.0 KB».
pub(crate) fn size_label(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, kind: &'static str, folder: bool, size: u64, age_secs: u64) -> ItemInfo {
        ItemInfo {
            name: name.to_string(),
            kind,
            folder,
            modified: Some(UNIX_EPOCH + Duration::from_secs(2_000_000_000 - age_secs)),
            size: Some(size),
        }
    }

    fn names(spec: SortSpec, items: &[ItemInfo]) -> Vec<&str> {
        sorted_order(spec, items).into_iter().map(|i| items[i].name.as_str()).collect()
    }

    fn sample() -> Vec<ItemInfo> {
        vec![
            item("player.rs", "Rust script", false, 4000, 50),
            item("Zebra", "Folder", true, 0, 10),
            item("coin.png", "Texture", false, 900, 5000),
            item("Level10.scene", "Scene", false, 300, 100),
            item("Level2.scene", "Scene", false, 700, 9000),
            item("alpha", "Folder", true, 0, 99_999),
        ]
    }

    #[test]
    fn natural_order_ignores_case_and_compares_numbers_by_value() {
        assert_eq!(natural_cmp("Level2", "level10"), Ordering::Less);
        assert_eq!(natural_cmp("a", "B"), Ordering::Less);
        assert_eq!(natural_cmp("item02", "item2"), Ordering::Equal);
        assert_eq!(natural_cmp("item", "item1"), Ordering::Less);
        assert_eq!(natural_cmp("x9", "x10"), Ordering::Less);
    }

    #[test]
    fn every_key_sorts_and_folders_stay_first() {
        let items = sample();
        let by = |key, descending| names(SortSpec { key, descending }, &items);
        assert_eq!(
            by(SortKey::Name, false),
            ["alpha", "Zebra", "coin.png", "Level2.scene", "Level10.scene", "player.rs"]
        );
        assert_eq!(
            by(SortKey::Name, true),
            ["Zebra", "alpha", "player.rs", "Level10.scene", "Level2.scene", "coin.png"],
            "descending flips the files, folders stay on top"
        );
        // Тип: «Rust script» < «Scene» < «Texture»; внутри типа — по имени.
        assert_eq!(
            by(SortKey::Type, false),
            ["alpha", "Zebra", "player.rs", "Level2.scene", "Level10.scene", "coin.png"]
        );
        // Размер: 300, 700, 900, 4000.
        assert_eq!(
            by(SortKey::Size, false),
            ["alpha", "Zebra", "Level10.scene", "Level2.scene", "coin.png", "player.rs"]
        );
        assert_eq!(
            by(SortKey::Size, true),
            ["alpha", "Zebra", "player.rs", "coin.png", "Level2.scene", "Level10.scene"]
        );
        // Время: старые раньше (по возрастанию).
        assert_eq!(
            by(SortKey::Modified, false),
            ["alpha", "Zebra", "Level2.scene", "coin.png", "Level10.scene", "player.rs"]
        );
        assert_eq!(
            by(SortKey::Modified, true),
            ["Zebra", "alpha", "player.rs", "Level10.scene", "coin.png", "Level2.scene"]
        );
    }

    #[test]
    fn clicking_a_header_toggles_then_switches() {
        let mut spec = SortSpec::default();
        spec.click(SortKey::Name);
        assert_eq!(spec, SortSpec { key: SortKey::Name, descending: true });
        spec.click(SortKey::Size);
        assert_eq!(spec, SortSpec { key: SortKey::Size, descending: true }, "size starts with the biggest");
        spec.click(SortKey::Size);
        assert!(!spec.descending);
        spec.click(SortKey::Type);
        assert_eq!(spec, SortSpec { key: SortKey::Type, descending: false });
    }

    #[test]
    fn filters_and_keys_round_trip_through_text() {
        let filters: BTreeSet<TypeFilter> = [TypeFilter::Scripts, TypeFilter::Audio].into_iter().collect();
        assert_eq!(parse_filters(&filters_text(&filters)), filters);
        assert!(parse_filters("").is_empty());
        assert!(parse_filters("nonsense+").is_empty());
        for key in SortKey::ALL {
            assert_eq!(SortKey::parse(key.id()), Some(key));
        }
        assert_eq!(TypeFilter::of_importer("Scenario"), TypeFilter::Scripts);
        assert_eq!(TypeFilter::of_importer("Code"), TypeFilter::Scripts);
        assert_eq!(TypeFilter::of_importer("Raw"), TypeFilter::Other);
        assert_eq!(type_label("Code", "src/a.hpp"), "C++ header");
        assert_eq!(Language::of_path("x/y.cc"), Language::Cpp);
        assert_eq!(Language::of_path("a.scenario"), Language::Scenario);
    }

    #[test]
    fn dates_and_ages_read_well() {
        // 2026-11-12 00:00:00 UTC.
        let time = UNIX_EPOCH + Duration::from_secs(1_794_441_600);
        assert_eq!(date_label(time), "2026-11-12");
        assert_eq!(date_label(UNIX_EPOCH), "1970-01-01");
        assert_eq!(date_label(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29");
        let at = |secs: u64| Some(time - Duration::from_secs(secs));
        assert_eq!(modified_label(time, at(5)), "just now");
        assert_eq!(modified_label(time, at(300)), "5 min ago");
        assert_eq!(modified_label(time, at(7200)), "2 h ago");
        assert_eq!(modified_label(time, at(3 * 86_400)), "3 d ago");
        assert_eq!(modified_label(time, at(30 * 86_400)), "2026-10-13");
        assert_eq!(size_label(2048), "2.0 KB");
        assert_eq!(size_label(12), "12 B");
    }
}
