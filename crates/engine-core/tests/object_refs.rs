//! Поля-ссылки на объекты (`ObjectRef`, `ComponentRef<T>`), стабильные номера объектов
//! в файлах сцен и переопределения детей в экземплярах префабов.

use engine_core::prelude::*;
use engine_core::serialize::{self, MemoryPrefabs};

component! {
    pub struct Health {
        pub hp: i32 = 10,
    }
}
impl Component for Health {}

component! {
    pub struct Marker {
        pub note: String = String::new(),
    }
}
impl Component for Marker {}

component! {
    /// Держит ссылки на другие объекты.
    pub struct Door {
        pub lever: ObjectRef = ObjectRef::NONE,
        pub guard: ComponentRef<Health> = ComponentRef::NONE,
        pub lamps: Vec<ObjectRef> = Vec::new(),
    }
}
impl Component for Door {}

fn new_app() -> App {
    let mut app = App::new();
    app.register::<Health>().register::<Marker>().register::<Door>();
    app
}

fn name_of(world: &World, reference: ObjectRef) -> Option<String> {
    let id = world.scene.resolve(reference)?;
    Some(world.scene.get(id)?.name.clone())
}

fn door(world: &World, name: &str) -> Door {
    let id = world.scene.find_by_name(name).unwrap();
    world.scene.get_component::<Door>(id).unwrap().clone()
}

#[test]
fn references_survive_save_load_rename_and_reorder() {
    let mut app = new_app();
    let lever = app.spawn("Lever").id();
    let guard = app.spawn("Guard").with(Health::default()).id();
    let lamp = app.spawn("Lamp").id();
    let scene = &app.world.scene;
    let door_component = Door {
        lever: ObjectRef::new(scene, lever),
        guard: ComponentRef::new(scene, guard),
        lamps: vec![ObjectRef::new(scene, lamp), ObjectRef::NONE],
    };
    app.spawn("Door").with(door_component);

    let text = app.world.scene_to_string();
    assert!(text.contains("Object("), "references are written as Object(id):\n{text}");
    assert!(text.contains("id: "), "objects carry stable ids:\n{text}");

    // Загрузка, затем переименование и перестановка объектов — ссылки держатся.
    let mut other = new_app();
    other.world.load_scene_str(&text, "test.scene").unwrap();
    let lever = other.world.scene.find_by_name("Lever").unwrap();
    other.world.scene.get_mut(lever).unwrap().name = "Big Red Lever".into();
    let door_id = other.world.scene.find_by_name("Door").unwrap();
    other.world.scene.set_sibling_index(door_id, 0);
    let resaved = other.world.scene_to_string();

    let mut third = new_app();
    third.world.load_scene_str(&resaved, "test.scene").unwrap();
    let d = door(&third.world, "Door");
    assert_eq!(name_of(&third.world, d.lever).as_deref(), Some("Big Red Lever"));
    assert_eq!(d.guard.component(&third.world.scene).map(|h| h.hp), Some(10));
    assert_eq!(name_of(&third.world, d.lamps[0]).as_deref(), Some("Lamp"));
    assert!(d.lamps[1].is_none());
}

#[test]
fn old_scenes_without_ids_load_and_get_ids() {
    let mut app = new_app();
    let text = r#"(name: "Old", objects: [
        (name: "A", components: [(kind: "Health")]),
        (name: "B", children: [(name: "C")]),
    ])"#;
    app.world.load_scene_str(text, "old.scene").unwrap();
    let ids: Vec<u64> = app.world.scene.ids().iter().map(|id| app.world.scene.get(*id).unwrap().uid()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|uid| *uid != 0));
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 3, "every object gets its own id");
    for (id, uid) in app.world.scene.ids().into_iter().zip(&ids) {
        assert_eq!(app.world.scene.find_by_uid(*uid), Some(id));
    }
}

#[test]
fn unknown_references_load_as_none() {
    let mut app = new_app();
    let text = r#"(name: "S", objects: [
        (name: "Lever", id: 77),
        (name: "Door", id: 78, components: [(kind: "Door", fields: {
            "lever": Object(77),
            "lamps": List([Object(99999), Object(77)]),
        })]),
    ])"#;
    app.world.load_scene_str(text, "s.scene").unwrap();
    let d = door(&app.world, "Door");
    assert_eq!(name_of(&app.world, d.lever).as_deref(), Some("Lever"));
    assert!(d.lamps[0].is_none(), "a reference to a missing object becomes None");
    assert_eq!(name_of(&app.world, d.lamps[1]).as_deref(), Some("Lever"));
    // Значение «Nil» тоже читается как «не назначено».
    let mut component = Door::default();
    component.set_field("lever", &Value::Nil).unwrap();
    assert!(component.lever.is_none());
    assert!(component.set_field("lever", &Value::Int(3)).is_err());
}

#[test]
fn destroyed_targets_and_other_scenes_resolve_to_none() {
    let mut app = new_app();
    let target = app.spawn("Target").id();
    let reference = ObjectRef::new(&app.world.scene, target);
    assert_eq!(app.world.scene.resolve(reference), Some(target));
    app.world.scene.destroy(target);
    app.world.flush_destroyed();
    assert_eq!(app.world.scene.resolve(reference), None);

    let other = app.spawn("Other").id();
    let reference = ObjectRef::new(&app.world.scene, other);
    app.world.load_scene_str(r#"(name: "Next", objects: [(name: "X")])"#, "next.scene").unwrap();
    assert_eq!(app.world.scene.resolve(reference), None, "no dangling ids after a scene switch");
}

#[test]
fn component_ref_requires_the_component() {
    let mut app = new_app();
    let plain = app.spawn("Plain").id();
    let healthy = app.spawn("Healthy").with(Health { hp: 4 }).id();
    let scene = &app.world.scene;
    assert!(ComponentRef::<Health>::new(scene, plain).is_none());
    let r = ComponentRef::<Health>::new(scene, healthy);
    assert_eq!(r.get(scene), Some(healthy));
    // Компонент убрали — ссылка больше не разрешается.
    app.world.scene.get_mut(healthy).unwrap().remove::<Health>();
    assert_eq!(r.get(&app.world.scene), None);
    assert_eq!(Door::default().field_meta("guard").asset_kind, "Health");
    assert_eq!(Door::default().field_meta("lever").asset_kind, "");
}

#[test]
fn duplicating_a_group_remaps_internal_references_only() {
    let mut app = new_app();
    let outside = app.spawn("Outside").id();
    let lever = app.spawn("Lever").id();
    let scene = &app.world.scene;
    let door_component = Door {
        lever: ObjectRef::new(scene, lever),
        lamps: vec![ObjectRef::new(scene, outside)],
        ..Door::default()
    };
    let door_id = app.spawn("Door").with(door_component).id();

    let copies = app.world.duplicate_many(&[door_id, lever]).unwrap();
    let copy_door = app.world.scene.get_component::<Door>(copies[0]).unwrap().clone();
    assert_eq!(app.world.scene.resolve(copy_door.lever), Some(copies[1]), "inside the group: the copy");
    assert_eq!(app.world.scene.resolve(copy_door.lamps[0]), Some(outside), "outside the group: the original");
    let original = app.world.scene.get_component::<Door>(door_id).unwrap().clone();
    assert_eq!(app.world.scene.resolve(original.lever), Some(lever));

    // Одиночная копия объекта с ребёнком: ссылка на ребёнка — на ребёнка копии.
    let parent = app.spawn("Parent").id();
    let child = app.spawn("Child").child_of(parent).id();
    let r = ObjectRef::new(&app.world.scene, child);
    app.world.scene.get_mut(parent).unwrap().add(Door { lever: r, ..Door::default() });
    let copy = app.world.duplicate(parent).unwrap();
    let copy_child = app.world.scene.get(copy).unwrap().children()[0];
    let d = app.world.scene.get_component::<Door>(copy).unwrap();
    assert_eq!(app.world.scene.resolve(d.lever), Some(copy_child));
    // Номера у копий свои.
    assert_ne!(app.world.scene.get(copy).unwrap().uid(), app.world.scene.get(parent).unwrap().uid());

    // Вставка снятых узлов дважды — каждый раз свои копии.
    let nodes = vec![serialize::exact_node(&app.world.scene, parent)];
    let first = app.world.spawn_copies(&nodes, None).unwrap()[0];
    let second = app.world.spawn_copies(&nodes, None).unwrap()[0];
    for pasted in [first, second] {
        let child = app.world.scene.get(pasted).unwrap().children()[0];
        let d = app.world.scene.get_component::<Door>(pasted).unwrap();
        assert_eq!(app.world.scene.resolve(d.lever), Some(child));
    }
}

#[test]
fn undo_snapshots_keep_ids() {
    let mut app = new_app();
    let lever = app.spawn("Lever").id();
    let r = ObjectRef::new(&app.world.scene, lever);
    app.spawn("Door").with(Door { lever: r, ..Door::default() });
    let snapshot = app.world.snapshot();
    app.world.restore(&snapshot).unwrap();
    let d = door(&app.world, "Door");
    assert_eq!(name_of(&app.world, d.lever).as_deref(), Some("Lever"));
}

fn turret_prefab() -> Prefab {
    // Корень ссылается на своего ребёнка «Barrel»; у ребёнка «Light» нет id (старый файл).
    Prefab::from_ron(
        r#"(
            name: "Turret",
            id: 1,
            components: [(kind: "Door", fields: { "lever": Object(2) })],
            children: [
                (name: "Barrel", id: 2, components: [(kind: "Health", fields: { "hp": Int(3) })]),
                (name: "Light", components: [(kind: "Marker")]),
            ],
        )"#,
        "turret.prefab",
    )
    .unwrap()
}

#[test]
fn prefab_instances_point_at_their_own_children() {
    let mut app = new_app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("turret.prefab", turret_prefab()));
    let a = app.world.instantiate("turret.prefab").unwrap();
    let b = app.world.instantiate("turret.prefab").unwrap();
    assert_eq!(app.world.scene.get(a).unwrap().name, "Turret", "the instance takes the prefab root's name");
    for instance in [a, b] {
        let barrel = app.world.scene.find_child(instance, "Barrel").unwrap();
        let d = app.world.scene.get_component::<Door>(instance).unwrap();
        assert_eq!(app.world.scene.resolve(d.lever), Some(barrel));
    }

    // Объект сцены ссылается на ребёнка экземпляра; после сохранения и загрузки — тот же ребёнок.
    app.world.scene.get_mut(b).unwrap().name = "Turret B".into();
    let barrel_b = app.world.scene.find_child(b, "Barrel").unwrap();
    let r = ObjectRef::new(&app.world.scene, barrel_b);
    app.spawn("Watcher").with(Door { lever: r, ..Door::default() });
    let text = app.world.scene_to_string();
    assert!(!text.contains("\"Barrel\""), "instance children are not written out:\n{text}");

    let mut loaded = new_app();
    loaded.world.prefabs = Box::new(MemoryPrefabs::default().with("turret.prefab", turret_prefab()));
    loaded.world.load_scene_str(&text, "s.scene").unwrap();
    let b = loaded.world.scene.find_by_name("Turret B").unwrap();
    let barrel_b = loaded.world.scene.find_child(b, "Barrel").unwrap();
    let watcher = door(&loaded.world, "Watcher");
    assert_eq!(loaded.world.scene.resolve(watcher.lever), Some(barrel_b));
    // Сохранение без правок не пишет лишних переопределений номеров.
    let again = loaded.world.scene_to_string();
    assert!(!again.contains("\"id\""), "derived ids need no overrides:\n{again}");
}

#[test]
fn prefabs_made_from_objects_drop_outside_references() {
    let mut app = new_app();
    let outside = app.spawn("Outside").id();
    let root = app.spawn("Root").id();
    let child = app.spawn("Child").child_of(root).id();
    let scene = &app.world.scene;
    let component = Door {
        lever: ObjectRef::new(scene, child),
        lamps: vec![ObjectRef::new(scene, outside)],
        ..Door::default()
    };
    app.world.scene.get_mut(root).unwrap().add(component);
    let (prefab, cleared) = Prefab::from_object_report(&app.world.scene, root).unwrap();
    assert_eq!(cleared.len(), 1, "{cleared:?}");
    assert!(cleared[0].contains("lamps"), "{cleared:?}");

    // Объект становится экземпляром нового префаба: номера детей не меняются.
    let child_uid = app.world.scene.get(child).unwrap().uid();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("root.prefab", prefab.clone()));
    app.world.scene.get_mut(root).unwrap().set_prefab(Some("root.prefab".into()));
    let text = app.world.scene_to_string();
    let mut loaded = new_app();
    loaded.world.prefabs = Box::new(MemoryPrefabs::default().with("root.prefab", prefab));
    loaded.world.load_scene_str(&text, "s.scene").unwrap();
    let root = loaded.world.scene.find_by_name("Root").unwrap();
    let child = loaded.world.scene.find_child(root, "Child").unwrap();
    assert_eq!(loaded.world.scene.get(child).unwrap().uid(), child_uid);
    let d = loaded.world.scene.get_component::<Door>(root).unwrap();
    assert_eq!(loaded.world.scene.resolve(d.lever), Some(child));
    // Ссылка экземпляра наружу — переопределение экземпляра, она сохраняется.
    assert_eq!(name_of(&loaded.world, d.lamps[0]).as_deref(), Some("Outside"));
}

#[test]
fn loading_a_scene_additively_twice_keeps_each_copy_consistent() {
    let mut app = new_app();
    let text = r#"(name: "Room", objects: [
        (name: "Lever", id: 5),
        (name: "Door", id: 6, components: [(kind: "Door", fields: { "lever": Object(5) })]),
    ])"#;
    let first = app.world.load_scene_additive_str(text, "room.scene").unwrap();
    let second = app.world.load_scene_additive_str(text, "room.scene").unwrap();
    for roots in [first, second] {
        let d = app.world.scene.get_component::<Door>(roots[1]).unwrap();
        assert_eq!(app.world.scene.resolve(d.lever), Some(roots[0]));
    }
}

// ----------------------------------------------------------------------
// Переопределения детей в экземплярах префабов
// ----------------------------------------------------------------------

fn slat_prefab(light_hp: i32) -> Prefab {
    Prefab::from_ron(
        &format!(
            r#"(
            name: "Slat",
            children: [
                (name: "Light", components: [(kind: "Health", fields: {{ "hp": Int({light_hp}) }}), (kind: "Marker")]),
                (name: "Body", children: [(name: "Tip")]),
            ],
        )"#
        ),
        "slat.prefab",
    )
    .unwrap()
}

fn with_slat(light_hp: i32) -> App {
    let mut app = new_app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("slat.prefab", slat_prefab(light_hp)));
    app
}

#[test]
fn child_overrides_survive_save_and_load() {
    let mut app = with_slat(3);
    let slat = app.world.instantiate("slat.prefab").unwrap();
    let scene = &mut app.world.scene;
    let light = scene.find_child(slat, "Light").unwrap();
    let body = scene.find_child(slat, "Body").unwrap();
    let tip = scene.find_path(slat, "Body/Tip").unwrap();
    scene.get_mut(body).unwrap().active = false; // активность ребёнка
    scene.get_mut(tip).unwrap().add(Health { hp: 42 }); // добавленный компонент у внука
    scene.get_mut(light).unwrap().remove::<Marker>(); // удалённый компонент у ребёнка
    scene.get_component_mut::<Health>(light).unwrap().hp = 9; // поле компонента ребёнка
    scene.get_mut(slat).unwrap().name = "Slat Reverse".into(); // имя экземпляра
    app.world.flush_destroyed();

    let text = app.world.scene_to_string();
    assert!(text.contains("\"GameObject\""), "{text}");
    assert!(text.contains("child_components"), "{text}");

    let mut loaded = with_slat(3);
    loaded.world.load_scene_str(&text, "s.scene").unwrap();
    let scene = &loaded.world.scene;
    let slat = scene.find_by_name("Slat Reverse").expect("the instance name is kept");
    let light = scene.find_child(slat, "Light").unwrap();
    let body = scene.find_child(slat, "Body").unwrap();
    let tip = scene.find_path(slat, "Body/Tip").unwrap();
    assert!(!scene.get(body).unwrap().active);
    assert_eq!(scene.get_component::<Health>(tip).map(|h| h.hp), Some(42));
    assert!(scene.get_component::<Marker>(light).is_none());
    assert_eq!(scene.get_component::<Health>(light).map(|h| h.hp), Some(9));
    assert_eq!(scene.get(slat).unwrap().prefab(), Some("slat.prefab"));

    // Сохранение без правок даёт тот же файл.
    assert_eq!(loaded.world.scene_to_string(), text);
}

#[test]
fn prefab_edits_reach_only_fields_that_are_not_overridden() {
    let text = {
        let mut app = with_slat(3);
        let a = app.world.instantiate("slat.prefab").unwrap();
        let b = app.world.instantiate("slat.prefab").unwrap();
        app.world.scene.get_mut(a).unwrap().name = "A".into();
        app.world.scene.get_mut(b).unwrap().name = "B".into();
        let light_b = app.world.scene.find_child(b, "Light").unwrap();
        app.world.scene.get_component_mut::<Health>(light_b).unwrap().hp = 50;
        let body_a = app.world.scene.find_child(a, "Body").unwrap();
        app.world.scene.get_mut(body_a).unwrap().active = false;
        app.world.scene_to_string()
    };
    // Префаб изменили: у Light теперь 7 HP.
    let mut loaded = with_slat(7);
    loaded.world.load_scene_str(&text, "s.scene").unwrap();
    let scene = &loaded.world.scene;
    let a = scene.find_by_name("A").unwrap();
    let b = scene.find_by_name("B").unwrap();
    let hp = |id| scene.get_component::<Health>(scene.find_child(id, "Light").unwrap()).unwrap().hp;
    assert_eq!(hp(a), 7, "not overridden: follows the prefab");
    assert_eq!(hp(b), 50, "overridden: keeps the instance value");
    assert!(!scene.get(scene.find_child(a, "Body").unwrap()).unwrap().active);
    assert!(scene.get(scene.find_child(b, "Body").unwrap()).unwrap().active);
}

#[test]
fn instance_overrides_lists_what_differs() {
    let mut app = with_slat(3);
    let slat = app.world.instantiate("slat.prefab").unwrap();
    let world = &app.world;
    assert!(serialize::instance_overrides(&world.scene, slat, world.prefabs.as_ref(), &world.registry).is_empty());
    let light = app.world.scene.find_child(slat, "Light").unwrap();
    app.world.scene.get_mut(light).unwrap().active = false;
    let world = &app.world;
    let overrides = serialize::instance_overrides(&world.scene, slat, world.prefabs.as_ref(), &world.registry);
    assert_eq!(overrides.len(), 1);
    assert_eq!((overrides[0].path.as_str(), overrides[0].component.as_str(), overrides[0].field.as_str()), ("Light", "GameObject", "active"));
}

#[test]
fn bad_child_overrides_explain_themselves() {
    let mut app = with_slat(3);
    let text = r#"(name: "S", objects: [(name: "X", prefab: Some("slat.prefab"), child_components: [(path: "Lihgt", removed: ["Marker"])])])"#;
    let err = app.world.load_scene_str(text, "s.scene").unwrap_err().to_string();
    assert!(err.contains("Did you mean `Light`?"), "{err}");
}

#[test]
fn renamed_prefab_children_stay_the_same_object() {
    let mut app = with_slat(3);
    let slat = app.world.instantiate("slat.prefab").unwrap();
    let light = app.world.scene.find_child(slat, "Light").unwrap();
    let tip = app.world.scene.find_path(slat, "Body/Tip").unwrap();
    let body = app.world.scene.find_child(slat, "Body").unwrap();
    app.world.scene.get_mut(light).unwrap().name = "Lamp".into();
    app.world.scene.get_component_mut::<Health>(light).unwrap().hp = 8;
    app.world.scene.get_mut(body).unwrap().name = "Frame".into();
    app.world.scene.get_mut(tip).unwrap().name = "Point".into();
    let r = ObjectRef::new(&app.world.scene, light);
    app.spawn("Watcher").with(Door { lever: r, ..Door::default() });
    let text = app.world.scene_to_string();

    let mut loaded = with_slat(3);
    loaded.world.load_scene_str(&text, "s.scene").unwrap();
    let scene = &loaded.world.scene;
    let slat = scene.find_by_name("Slat").unwrap();
    assert_eq!(scene.get(slat).unwrap().children().len(), 2, "a renamed child is not duplicated:\n{text}");
    let lamp = scene.find_child(slat, "Lamp").expect("the rename is kept");
    assert!(scene.find_child(slat, "Light").is_none());
    assert_eq!(scene.get_component::<Health>(lamp).map(|h| h.hp), Some(8));
    assert!(scene.find_path(slat, "Frame/Point").is_some(), "nested renames are kept:\n{text}");
    let watcher = door(&loaded.world, "Watcher");
    assert_eq!(scene.resolve(watcher.lever), Some(lamp));
    assert_eq!(loaded.world.scene_to_string(), text, "a second save writes the same file");
}

#[test]
fn prefab_source_node_gives_the_unmodified_prefab_values() {
    let mut app = with_slat(3);
    let slat = app.world.instantiate("slat.prefab").unwrap();
    let light = app.world.scene.find_child(slat, "Light").unwrap();
    app.world.scene.get_component_mut::<Health>(light).unwrap().hp = 99;
    app.world.scene.get_mut(light).unwrap().name = "Lamp".into();
    let world = &app.world;
    let base = serialize::prefab_source_node(&world.scene, light, world.prefabs.as_ref(), &world.registry).unwrap();
    assert_eq!(base.name, "Light");
    let health = base.components.iter().find(|c| c.kind == "Health").unwrap();
    assert_eq!(health.fields.get("hp"), Some(&Value::Int(3)));
    let root = serialize::prefab_source_node(&world.scene, slat, world.prefabs.as_ref(), &world.registry).unwrap();
    assert_eq!(root.children.len(), 2);
    let plain = app.spawn("Plain").id();
    let world = &app.world;
    assert!(serialize::prefab_source_node(&world.scene, plain, world.prefabs.as_ref(), &world.registry).is_none());
}
