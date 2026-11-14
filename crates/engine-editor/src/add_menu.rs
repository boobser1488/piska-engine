//! Меню Add Component: компоненты по категориям (из реестра типов), поиск с ранжированием
//! и список недавно добавленных. Чистая логика — рисует её `inspector.rs`.

use engine_core::prelude::TypeRegistry;

use crate::icons::Icon;
use crate::sorting::natural_cmp;
use crate::theme;

// i18n-keys: "Scripts (C++)" "Scripts (Scenario)" "Physics" "Camera"  — категории из реестра типов, переводятся при показе
/// Порядок категорий в меню; неизвестные (из чужих крейтов) идут за ними по алфавиту.
pub(crate) const CATEGORY_ORDER: [&str; 10] = [
    "Rendering",
    "Physics 2D",
    "Physics 3D",
    "Audio",
    "UI",
    "Effects",
    "Animation",
    "Scripts",
    "Scripts (C++)",
    "Scripts (Scenario)",
];

/// Сколько недавних компонентов помнит редактор.
pub(crate) const RECENT_LIMIT: usize = 8;

/// Сколько недавних показывает меню без запроса.
pub(crate) const RECENT_SHOWN: usize = 5;

/// Тип компонента в меню.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AddEntry {
    pub kind: &'static str,
    /// Читаемое имя: `SpriteRenderer` → «Sprite Renderer».
    pub label: String,
    pub category: &'static str,
}

/// Все типы реестра с их категориями. Тип без категории — скрипт игры («Scripts»).
pub(crate) fn entries(registry: &TypeRegistry) -> Vec<AddEntry> {
    registry
        .names()
        .map(|kind| AddEntry {
            kind,
            label: theme::pretty_type(kind),
            category: registry.category_of(kind).unwrap_or("Scripts"),
        })
        .collect()
}

pub(crate) fn category_icon(category: &str) -> Icon {
    match category {
        "Rendering" => Icon::Cube,
        "Physics 2D" | "Physics 3D" => Icon::Physics,
        "Audio" => Icon::Audio,
        "UI" => Icon::Ui,
        "Effects" => Icon::Particles,
        "Animation" => Icon::Animation,
        _ => Icon::Script,
    }
}

fn category_rank(category: &str) -> usize {
    CATEGORY_ORDER.iter().position(|c| *c == category).unwrap_or(CATEGORY_ORDER.len())
}

/// Категории по порядку, внутри — по алфавиту.
pub(crate) fn grouped(entries: &[AddEntry]) -> Vec<(&'static str, Vec<&AddEntry>)> {
    let mut categories: Vec<&'static str> = entries.iter().map(|e| e.category).collect();
    categories.sort_by(|a, b| category_rank(a).cmp(&category_rank(b)).then_with(|| a.cmp(b)));
    categories.dedup();
    categories
        .into_iter()
        .map(|category| {
            let mut items: Vec<&AddEntry> = entries.iter().filter(|e| e.category == category).collect();
            items.sort_by(|a, b| natural_cmp(&a.label, &b.label));
            (category, items)
        })
        .collect()
}

/// Насколько запрос подходит типу: 0 — совпадает целиком, 1 — начало имени, 2 — начало
/// слова (`ren` → «Sprite Renderer»), 3 — подстрока; `None` — не подходит.
pub(crate) fn rank(query: &str, entry: &AddEntry) -> Option<u8> {
    let query: String = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(3);
    }
    let label = entry.label.to_lowercase();
    let kind = entry.kind.to_lowercase();
    let squashed = label.replace(' ', "");
    if label == query || kind == query || squashed == query {
        return Some(0);
    }
    if label.starts_with(&query) || kind.starts_with(&query) || squashed.starts_with(&query) {
        return Some(1);
    }
    if label.split(' ').any(|word| word.starts_with(&query)) {
        return Some(2);
    }
    if label.contains(&query) || kind.contains(&query) || squashed.contains(&query) {
        return Some(3);
    }
    None
}

/// Результаты поиска: лучшие совпадения первыми, при равенстве — по алфавиту.
pub(crate) fn search<'a>(entries: &'a [AddEntry], query: &str) -> Vec<&'a AddEntry> {
    let mut found: Vec<(u8, &AddEntry)> = entries.iter().filter_map(|e| rank(query, e).map(|r| (r, e))).collect();
    found.sort_by(|(ra, a), (rb, b)| ra.cmp(rb).then_with(|| natural_cmp(&a.label, &b.label)));
    found.into_iter().map(|(_, e)| e).collect()
}

/// Недавние типы, которые ещё есть в реестре, — новые первыми.
pub(crate) fn recent_entries<'a>(entries: &'a [AddEntry], recent: &[String]) -> Vec<&'a AddEntry> {
    recent
        .iter()
        .filter_map(|kind| entries.iter().find(|e| e.kind == kind))
        .take(RECENT_SHOWN)
        .collect()
}

/// Запомнить, что добавили `kind`: он становится первым, повторов нет.
pub(crate) fn note_used(recent: &mut Vec<String>, kind: &str) {
    recent.retain(|k| k != kind);
    recent.insert(0, kind.to_string());
    recent.truncate(RECENT_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &'static str, category: &'static str) -> AddEntry {
        AddEntry { kind, label: theme::pretty_type(kind), category }
    }

    fn sample() -> Vec<AddEntry> {
        vec![
            entry("SpriteRenderer", "Rendering"),
            entry("Rigidbody2D", "Physics 2D"),
            entry("Rigidbody", "Physics 3D"),
            entry("Renderer", "Rendering"),
            entry("PlayerController", "Scripts"),
            entry("Spawner", "Scripts"),
            entry("EnemyAi", "Scripts (C++)"),
            entry("AudioSource", "Audio"),
            entry("Weird", "Zeta pack"),
        ]
    }

    #[test]
    fn categories_come_in_menu_order_and_sort_alphabetically_inside() {
        let entries = sample();
        let groups = grouped(&entries);
        let names: Vec<&str> = groups.iter().map(|(c, _)| *c).collect();
        assert_eq!(names, ["Rendering", "Physics 2D", "Physics 3D", "Audio", "Scripts", "Scripts (C++)", "Zeta pack"]);
        let rendering: Vec<&str> = groups[0].1.iter().map(|e| e.kind).collect();
        assert_eq!(rendering, ["Renderer", "SpriteRenderer"]);
        let scripts: Vec<&str> = groups[4].1.iter().map(|e| e.kind).collect();
        assert_eq!(scripts, ["PlayerController", "Spawner"]);
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_word_start_then_substring() {
        let entries = sample();
        let kinds = |query: &str| search(&entries, query).into_iter().map(|e| e.kind).collect::<Vec<_>>();
        // «render»: «Renderer» — начало имени, «SpriteRenderer» — начало второго слова.
        assert_eq!(kinds("render"), ["Renderer", "SpriteRenderer"]);
        // Точное совпадение — раньше префиксов.
        assert_eq!(kinds("rigidbody"), ["Rigidbody", "Rigidbody2D"]);
        // Подстрока — в конце.
        assert_eq!(kinds("er"), ["PlayerController", "Renderer", "Spawner", "SpriteRenderer"]);
        assert_eq!(kinds("zzz"), Vec::<&str>::new());
        assert_eq!(search(&entries, "").len(), entries.len(), "an empty query lists everything");
        assert_eq!(rank("sprite renderer", &entries[0]), Some(0));
        assert_eq!(rank("sprite", &entries[0]), Some(1));
        assert_eq!(rank("rend", &entries[0]), Some(2));
        assert_eq!(rank("ite", &entries[0]), Some(3));
    }

    #[test]
    fn prefix_beats_word_start_beats_substring() {
        let entries = vec![entry("MyPlayer", "Scripts"), entry("Player", "Scripts"), entry("Superplayerish", "Scripts"), entry("PlayerHud", "Scripts")];
        let kinds: Vec<&str> = search(&entries, "play").into_iter().map(|e| e.kind).collect();
        // Player, PlayerHud — префикс; MyPlayer — начало слова; Superplayerish — подстрока.
        assert_eq!(kinds, ["Player", "PlayerHud", "MyPlayer", "Superplayerish"]);
    }

    #[test]
    fn recent_components_are_deduplicated_and_bounded() {
        let mut recent = Vec::new();
        for kind in ["A", "B", "C", "B", "D", "E", "F", "G", "H", "I", "J"] {
            note_used(&mut recent, kind);
        }
        assert_eq!(recent.first().map(String::as_str), Some("J"));
        assert_eq!(recent.len(), RECENT_LIMIT);
        assert_eq!(recent.iter().filter(|k| *k == "B").count(), 1, "B was added twice but is listed once");
        assert!(!recent.contains(&"A".to_string()) && !recent.contains(&"C".to_string()), "the oldest fell off the end");
        note_used(&mut recent, "H");
        assert_eq!(recent[0], "H");
        assert_eq!(recent.iter().filter(|k| *k == "H").count(), 1);

        let entries = vec![entry("H", "Scripts"), entry("J", "Scripts")];
        let shown: Vec<&str> = recent_entries(&entries, &recent).into_iter().map(|e| e.kind).collect();
        assert_eq!(shown, ["H", "J"], "unknown types are skipped, order is most recent first");
    }
}
