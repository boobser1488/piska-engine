//! Сцены, которые правят руками: генератор не перезаписывает их, а **сливает** свежий
//! результат с файлом на диске.
//!
//! * У каждого объекта, который ставит генератор, — постоянный номер (`id` в файле): хеш
//!   его места в иерархии, имени, префаба и позиции ([`stable_ids`]). Тот же объект при
//!   следующем запуске получает тот же номер.
//! * Рядом с каталогом ассетов (`bake_state/<сцена>.baked.ron`) лежит запись того, что
//!   генератор записал в прошлый раз ([`Record`]): по объекту — родитель и собственные
//!   данные (без детей).
//! * [`merge`]: объект в файле совпадает с записью — его не трогали, он обновляется до
//!   свежего; отличается — его правили в редакторе, он остаётся как есть; есть в записи, но
//!   нет в файле — его удалили, он не возвращается; нет в записи — его добавили руками, он
//!   остаётся. Объект, который генератор больше не ставит, уходит, только если его не
//!   трогали.
//!
//! Модуль не зависит от мира движка — его подключают и генератор, и тест
//! `tests/bake_merge.rs`.

use std::collections::{HashMap, HashSet};

use engine::prelude::{NodeData, SceneData};
use serde::{Deserialize, Serialize};

/// Что генератор записал в сцену в прошлый раз.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// По объекту в порядке обхода: (номер, номер родителя или 0, собственные данные объекта
    /// в RON без детей).
    pub objects: Vec<(u64, u64, String)>,
}

impl Record {
    /// Номер → (родитель, данные).
    pub fn index(&self) -> HashMap<u64, (u64, &str)> {
        self.objects.iter().map(|(id, parent, text)| (*id, (*parent, text.as_str()))).collect()
    }
}

/// Что сделало слияние (для печати и для тестов).
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Обновлены до свежего (не правились руками и изменились в генераторе).
    pub updated: usize,
    /// Новые объекты генератора.
    pub added: Vec<String>,
    /// Правленные руками: оставлены как есть.
    pub kept_edits: Vec<String>,
    /// Удалённые руками: не возвращены.
    pub kept_deleted: Vec<String>,
    /// Добавленные руками: оставлены.
    pub kept_added: Vec<String>,
    /// Больше не ставятся генератором и не правились: убраны.
    pub removed: Vec<String>,
    /// Больше не ставятся генератором, но правились руками: оставлены.
    pub orphans_kept: Vec<String>,
    /// Новые объекты генератора на месте правленного или удалённого руками: не поставлены.
    pub kept_place: Vec<String>,
}

impl Report {
    pub fn print(&self, scene: &str) {
        println!(
            "{scene}: updated {}, added {}, kept edited {}, kept deleted {}, kept added {}, removed {}, kept orphans {}, not placed over your edits {}",
            self.updated,
            self.added.len(),
            self.kept_edits.len(),
            self.kept_deleted.len(),
            self.kept_added.len(),
            self.removed.len(),
            self.orphans_kept.len(),
            self.kept_place.len()
        );
        for (what, list) in [
            ("kept edited", &self.kept_edits),
            ("kept deleted", &self.kept_deleted),
            ("kept added", &self.kept_added),
            ("kept orphan", &self.orphans_kept),
            ("not placed over your edit", &self.kept_place),
        ] {
            for name in list {
                println!("  {what}: {name}");
            }
        }
    }
}

/// Собственные данные узла (без детей) — то, что сравнивается с записью.
pub fn own_text(node: &NodeData) -> String {
    let mut own = node.clone();
    own.children.clear();
    ron::ser::to_string(&own).expect("node is serializable")
}

/// Плоский список: (узел без детей, номер родителя), в порядке обхода.
fn flatten(nodes: &[NodeData], parent: u64, out: &mut Vec<(NodeData, u64)>) {
    for node in nodes {
        let mut own = node.clone();
        own.children.clear();
        out.push((own, parent));
        flatten(&node.children, node.id, out);
    }
}

/// Дерево из плоского списка (порядок детей — порядок списка).
fn build_tree(flat: Vec<(NodeData, u64)>) -> Vec<NodeData> {
    let ids: HashSet<u64> = flat.iter().map(|(n, _)| n.id).collect();
    let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut nodes: HashMap<u64, NodeData> = HashMap::new();
    let mut roots: Vec<u64> = Vec::new();
    for (node, parent) in flat {
        if parent != 0 && ids.contains(&parent) {
            children.entry(parent).or_default().push(node.id);
        } else {
            roots.push(node.id);
        }
        nodes.insert(node.id, node);
    }
    fn take(id: u64, nodes: &mut HashMap<u64, NodeData>, children: &mut HashMap<u64, Vec<u64>>) -> NodeData {
        let mut node = nodes.remove(&id).expect("node");
        for child in children.remove(&id).unwrap_or_default() {
            node.children.push(take(child, nodes, children));
        }
        node
    }
    roots.into_iter().map(|id| take(id, &mut nodes, &mut children)).collect()
}

/// Запись о сцене, которую генератор только что построил.
pub fn record(fresh: &SceneData) -> Record {
    let mut flat = Vec::new();
    flatten(&fresh.objects, 0, &mut flat);
    Record { objects: flat.iter().map(|(n, p)| (n.id, *p, own_text(n))).collect() }
}

/// Сцена, какой её записал генератор в прошлый раз (из записи; для теста `bake_merge`).
#[allow(dead_code)]
pub fn scene_from_record(name: &str, record: &Record) -> SceneData {
    let mut flat = Vec::new();
    for (_, parent, text) in &record.objects {
        let node: NodeData = ron::from_str(text).expect("record node");
        flat.push((node, *parent));
    }
    SceneData { name: name.to_string(), objects: build_tree(flat) }
}

/// Слить свежую сцену генератора с файлом пользователя (см. описание модуля).
pub fn merge(user: &SceneData, record: &Record, fresh: &SceneData) -> (SceneData, Report) {
    let last = record.index();
    let mut report = Report::default();
    let (mut u, mut f) = (Vec::new(), Vec::new());
    flatten(&user.objects, 0, &mut u);
    flatten(&fresh.objects, 0, &mut f);
    let fresh_by: HashMap<u64, usize> = f.iter().enumerate().map(|(i, (n, _))| (n.id, i)).collect();
    let user_ids: HashSet<u64> = u.iter().map(|(n, _)| n.id).collect();
    let label = |n: &NodeData| format!("{} (id {})", n.name, n.id);

    let mut out: Vec<(NodeData, u64)> = Vec::new();
    let mut dropped: HashMap<u64, u64> = HashMap::new(); // убранный узел → его родитель
    for (node, parent) in &u {
        let untouched = last.get(&node.id).is_some_and(|(_, text)| *text == own_text(node));
        match fresh_by.get(&node.id) {
            Some(&i) => {
                let fresh_node = &f[i].0;
                if untouched {
                    if own_text(fresh_node) != own_text(node) {
                        report.updated += 1;
                    }
                    out.push((fresh_node.clone(), *parent));
                } else {
                    if last.contains_key(&node.id) {
                        report.kept_edits.push(label(node));
                    }
                    out.push((node.clone(), *parent));
                }
            }
            None if last.contains_key(&node.id) => {
                if untouched {
                    report.removed.push(label(node));
                    dropped.insert(node.id, *parent);
                } else {
                    report.orphans_kept.push(label(node));
                    out.push((node.clone(), *parent));
                }
            }
            None => {
                report.kept_added.push(label(node));
                out.push((node.clone(), *parent));
            }
        }
    }
    // Дети убранного узла, которые остались, переходят к ближайшему оставшемуся предку.
    for (_, parent) in out.iter_mut() {
        while let Some(up) = dropped.get(parent) {
            *parent = *up;
        }
    }
    // Места, которые занимает правка пользователя: объект прошлого запуска, которого нет в
    // свежем (генератор поставил на его место другой вариант — у окна другой префаб), а
    // пользователь его правил или удалил. Новый объект на том же месте (тот же родитель,
    // имя и позиция до сантиметра) не ставится — правка пользователя важнее.
    let place = |parent: u64, n: &NodeData| {
        let p = n.transform.position;
        (parent, n.name.clone(), (p[0] * 100.0).round() as i64, (p[1] * 100.0).round() as i64, (p[2] * 100.0).round() as i64)
    };
    let user_text: HashMap<u64, String> = u.iter().map(|(n, _)| (n.id, own_text(n))).collect();
    let mut claimed = HashSet::new();
    for (id, parent, text) in &record.objects {
        if fresh_by.contains_key(id) || user_text.get(id).is_some_and(|t| t == text) {
            continue;
        }
        if let Ok(old) = ron::from_str::<NodeData>(text) {
            claimed.insert(place(*parent, &old));
        }
    }
    // Новые объекты генератора — под своим родителем, если он есть.
    let mut present: HashSet<u64> = out.iter().map(|(n, _)| n.id).collect();
    for (node, parent) in &f {
        if user_ids.contains(&node.id) {
            continue;
        }
        if last.contains_key(&node.id) {
            report.kept_deleted.push(label(node));
            continue;
        }
        if claimed.contains(&place(*parent, node)) {
            report.kept_place.push(label(node));
            continue;
        }
        let parent = if *parent == 0 || present.contains(parent) { *parent } else { 0 };
        // После последнего уже стоящего ребёнка того же родителя — порядок как в генераторе.
        let at = out.iter().rposition(|(_, p)| *p == parent && parent != 0).map(|i| i + 1).unwrap_or(out.len());
        out.insert(at, (node.clone(), parent));
        present.insert(node.id);
        report.added.push(label(node));
    }
    (SceneData { name: user.name.clone(), objects: build_tree(out) }, report)
}

/// Что потеряется, если перезаписать файл свежей сценой (`--overwrite-scene`).
pub fn losses(user: &SceneData, record: &Record) -> Vec<String> {
    let last = record.index();
    let mut u = Vec::new();
    flatten(&user.objects, 0, &mut u);
    let mut out = Vec::new();
    for (node, _) in &u {
        match last.get(&node.id) {
            Some((_, text)) if *text == own_text(node) => {}
            Some(_) => out.push(format!("edited: {} (id {})", node.name, node.id)),
            None => out.push(format!("added: {} (id {})", node.name, node.id)),
        }
    }
    let ids: HashSet<u64> = u.iter().map(|(n, _)| n.id).collect();
    for (id, _, text) in &record.objects {
        if !ids.contains(id) {
            let name = ron::from_str::<NodeData>(text).map(|n| n.name).unwrap_or_default();
            out.push(format!("deleted: {name} (id {id})"));
        }
    }
    out
}

/// Постоянные номера объектов свежей сцены: FNV-1a от пути «родитель / имя | префаб |
/// позиция в сантиметрах # порядковый номер среди полных тёзок». Ссылки на объекты
/// (`Object(…)` в полях компонентов и в переопределениях) переводятся на новые номера.
/// `prefab_path` — путь префаба по его GUID (чтобы номер не зависел от GUID).
pub fn stable_ids(scene: &SceneData, prefab_path: &dyn Fn(&str) -> String) -> SceneData {
    fn fnv(text: &str) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // splitmix64 — как в движке; номера — 48 бит.
        let mut z = hash.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let uid = z & ((1 << 48) - 1);
        if uid == 0 { 1 } else { uid }
    }
    fn walk(nodes: &mut [NodeData], parent_key: &str, map: &mut HashMap<u64, u64>, used: &mut HashSet<u64>, prefab_path: &dyn Fn(&str) -> String) {
        let mut seen: HashMap<String, u32> = HashMap::new();
        for node in nodes.iter_mut() {
            let p = node.transform.position;
            let cm = |v: f32| (v * 100.0).round() as i64;
            let prefab = node.prefab.as_deref().map(prefab_path).unwrap_or_default();
            let base = format!("{parent_key}/{}|{prefab}|{},{},{}", node.name, cm(p[0]), cm(p[1]), cm(p[2]));
            let n = seen.entry(base.clone()).or_insert(0);
            let key = format!("{base}#{n}");
            *n += 1;
            let mut uid = fnv(&key);
            let mut salt = 0;
            while used.contains(&uid) {
                salt += 1;
                uid = fnv(&format!("{key}~{salt}"));
            }
            used.insert(uid);
            if node.id != 0 {
                map.insert(node.id, uid);
            }
            node.id = uid;
            walk(&mut node.children, &key, map, used, prefab_path);
        }
    }
    let mut out = scene.clone();
    let mut map = HashMap::new();
    walk(&mut out.objects, "", &mut map, &mut HashSet::new(), prefab_path);
    // Ссылки: `Object(<номер>)` в тексте сцены.
    let text = remap_refs(&out.to_ron(), &map);
    SceneData::from_ron(&text, "stable ids").expect("renumbered scene parses")
}

/// Заменить номера в `Object(<номер>)` по таблице.
pub fn remap_refs(text: &str, map: &HashMap<u64, u64>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("Object(") {
        let (head, tail) = rest.split_at(at + "Object(".len());
        out.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && tail[digits..].starts_with(')') {
            let old: u64 = tail[..digits].parse().unwrap_or(0);
            out.push_str(&map.get(&old).copied().unwrap_or(old).to_string());
            rest = &tail[digits..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}
