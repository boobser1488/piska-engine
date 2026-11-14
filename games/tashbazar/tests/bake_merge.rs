//! Генератор ассетов не теряет правки, сделанные в редакторе (tools/bake/merge.rs).
//!
//! * `the_next_bake_keeps_every_manual_change` — сцены, которые лежат в `assets/` сейчас:
//!   слияние с тем, что генератор поставил в прошлый раз, возвращает файл без единой потери.
//! * Остальные — правила слияния на маленькой сцене: правленный объект остаётся правленным,
//!   удалённый не возвращается, добавленный остаётся, нетронутый обновляется.

#[path = "../tools/bake/merge.rs"]
#[allow(dead_code)]
mod merge;

use std::collections::BTreeMap;
use std::path::PathBuf;

use engine::prelude::{NodeData, SceneData};

fn assets() -> PathBuf {
    std::env::var_os("TASHBAZAR_CHECK_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))
}

/// Номер → (родитель, собственные данные): сцена без учёта порядка.
fn flat(scene: &SceneData) -> BTreeMap<u64, (u64, String)> {
    fn walk(nodes: &[NodeData], parent: u64, out: &mut BTreeMap<u64, (u64, String)>) {
        for node in nodes {
            out.insert(node.id, (parent, merge::own_text(node)));
            walk(&node.children, node.id, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(&scene.objects, 0, &mut out);
    out
}

#[test]
fn the_next_bake_keeps_every_manual_change() {
    let root = assets();
    let state = root.parent().unwrap().join("bake_state");
    for scene in ["tashbazar", "gym"] {
        let text = std::fs::read_to_string(root.join(format!("scenes/{scene}.scene"))).unwrap();
        let user = SceneData::from_ron(&text, scene).unwrap();
        let record_text = std::fs::read_to_string(state.join(format!("{scene}.baked.ron")))
            .unwrap_or_else(|_| panic!("{scene}: нет записи генератора bake_state/{scene}.baked.ron"));
        let record: merge::Record = ron::from_str(&record_text).unwrap();
        let fresh = merge::scene_from_record(&user.name, &record);
        let (merged, report) = merge::merge(&user, &record, &fresh);
        report.print(scene);
        let (before, after) = (flat(&user), flat(&merged));
        let lost: Vec<_> = before.keys().filter(|id| !after.contains_key(id)).collect();
        let changed: Vec<_> = before.iter().filter(|(id, v)| after.get(id).is_some_and(|w| w != *v)).map(|(id, _)| id).collect();
        let revived: Vec<_> = after.keys().filter(|id| !before.contains_key(id)).collect();
        assert!(
            lost.is_empty() && changed.is_empty() && revived.is_empty(),
            "{scene}: a bake would lose {} object(s), overwrite {} edited, bring back {} deleted",
            lost.len(),
            changed.len(),
            revived.len()
        );
    }
}

fn node(id: u64, name: &str, x: f32, children: Vec<NodeData>) -> NodeData {
    let mut n = NodeData { id, name: name.to_string(), children, ..NodeData::default() };
    n.transform.position = [x, 0.0, 0.0];
    n
}

fn scene(objects: Vec<NodeData>) -> SceneData {
    SceneData { name: "test".to_string(), objects }
}

#[test]
fn edits_deletions_and_additions_survive_while_untouched_objects_update() {
    // Прошлый запуск генератора: группа с четырьмя объектами.
    let baked = scene(vec![node(1, "Group", 0.0, vec![node(2, "edited", 1.0, vec![]), node(3, "deleted", 2.0, vec![]), node(4, "untouched", 3.0, vec![]), node(5, "dropped", 4.0, vec![])])]);
    let record = merge::record(&baked);
    // Пользователь: подвинул 2, удалил 3, добавил 9 под группу.
    let user = scene(vec![node(1, "Group", 0.0, vec![node(2, "edited", 1.5, vec![]), node(4, "untouched", 3.0, vec![]), node(5, "dropped", 4.0, vec![]), node(9, "mine", 7.0, vec![])])]);
    // Новый запуск: все объекты сдвинуты генератором, 5 больше не ставится, 6 — новый.
    let fresh = scene(vec![node(1, "Group", 0.0, vec![node(2, "edited", 10.0, vec![]), node(3, "deleted", 20.0, vec![]), node(4, "untouched", 30.0, vec![]), node(6, "new", 60.0, vec![])])]);
    let (merged, report) = merge::merge(&user, &record, &fresh);
    let out = flat(&merged);
    let x = |id: u64| ron::from_str::<NodeData>(&out[&id].1).unwrap().transform.position[0];
    assert_eq!(x(2), 1.5, "the manual move stays");
    assert!(!out.contains_key(&3), "the deleted object stays deleted");
    assert_eq!(x(4), 30.0, "the untouched object follows the generator");
    assert!(!out.contains_key(&5), "an untouched object the generator dropped goes away");
    assert_eq!(x(6), 60.0, "a new generator object appears");
    assert_eq!(out[&6].0, 1, "under its group");
    assert_eq!(out[&9].0, 1, "the user's own object stays where they put it");
    assert_eq!(report.kept_edits.len(), 1);
    assert_eq!(report.kept_deleted.len(), 1);
    assert_eq!(report.kept_added.len(), 1);
    // Второй запуск с тем же результатом ничего не меняет.
    let record = merge::record(&fresh);
    let (again, _) = merge::merge(&merged, &record, &fresh);
    assert_eq!(flat(&again), flat(&merged));
}

#[test]
fn a_new_variant_does_not_land_on_an_edited_or_deleted_object() {
    // Генератор поменял префаб окна (другой номер): старое окно правили, другое удалили.
    let baked = scene(vec![node(1, "Group", 0.0, vec![node(2, "window", 1.0, vec![]), node(3, "window", 5.0, vec![]), node(4, "window", 9.0, vec![])])]);
    let record = merge::record(&baked);
    let mut edited = node(2, "window", 1.0, vec![]);
    edited.active = false;
    let user = scene(vec![node(1, "Group", 0.0, vec![edited, node(4, "window", 9.0, vec![])])]);
    let fresh = scene(vec![node(1, "Group", 0.0, vec![node(12, "window", 1.0, vec![]), node(13, "window", 5.0, vec![]), node(14, "window", 9.0, vec![])])]);
    let (merged, report) = merge::merge(&user, &record, &fresh);
    let out = flat(&merged);
    assert!(out.contains_key(&2) && !out.contains_key(&12), "the edited window stays alone at its place");
    assert!(!out.contains_key(&13), "the deleted window does not come back as a new variant");
    assert!(!out.contains_key(&4) && out.contains_key(&14), "an untouched window becomes the new variant");
    assert_eq!(report.kept_place.len(), 2);
}

#[test]
fn an_edited_object_the_generator_dropped_is_kept_and_its_user_children_survive() {
    let baked = scene(vec![node(1, "Group", 0.0, vec![node(2, "a", 1.0, vec![])])]);
    let record = merge::record(&baked);
    // Пользователь положил свой объект внутрь 2 и не трогал 2.
    let user = scene(vec![node(1, "Group", 0.0, vec![node(2, "a", 1.0, vec![node(8, "mine", 0.0, vec![])])])]);
    let fresh = scene(vec![node(1, "Group", 0.0, vec![])]);
    let (merged, report) = merge::merge(&user, &record, &fresh);
    let out = flat(&merged);
    assert!(!out.contains_key(&2));
    assert_eq!(out[&8].0, 1, "the user's child moves up to the nearest surviving parent");
    assert_eq!(report.removed.len(), 1);
}

#[test]
fn stable_ids_do_not_depend_on_load_order_numbers_and_follow_references() {
    let mut a = scene(vec![node(111, "Group", 0.0, vec![node(222, "x", 1.0, vec![]), node(333, "x", 1.0, vec![])])]);
    let mut b = a.clone();
    b.objects[0].id = 999;
    b.objects[0].children[0].id = 777;
    b.objects[0].children[1].id = 555;
    let path = |g: &str| g.to_string();
    let (a2, b2) = (merge::stable_ids(&a, &path), merge::stable_ids(&b, &path));
    assert_eq!(flat(&a2), flat(&b2));
    let ids: Vec<u64> = a2.objects[0].children.iter().map(|n| n.id).collect();
    assert_ne!(ids[0], ids[1], "identical twins get different numbers");
    // Ссылки переводятся вместе с номерами.
    a.objects[0].overrides.clear();
    let text = merge::remap_refs("value: Object(222), other: Object(5)", &[(222, 42)].into_iter().collect());
    assert_eq!(text, "value: Object(42), other: Object(5)");
}
