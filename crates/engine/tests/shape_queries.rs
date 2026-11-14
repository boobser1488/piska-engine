//! Запросы формой у 3D-физики (`cast_box`, `cast_shape`, `overlap_box`,
//! `compute_penetration_box`, `collider_pose`) и захват курсора у `Input`.
//! `cargo test -p engine --no-default-features --features "physics3d audio audio-wav ui gamepad" --test shape_queries`.
#![cfg(feature = "physics3d")]

use engine::prelude::*;

fn world(build: impl FnOnce(&mut App)) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    build(&mut app);
    app.run_headless(2, 1.0 / 60.0); // тела попадают в запросы после шага физики
    app
}

fn physics(app: &App) -> &Physics3D {
    app.world.resources.get::<Physics3D>().expect("physics")
}

#[test]
fn cast_box_stops_exactly_at_a_box_even_on_a_huge_floor() {
    let app = world(|app| {
        app.spawn("Floor").at3(0.0, -0.5, 0.0).with(Collider::cuboid(400.0, 1.0, 400.0));
        app.spawn("Wall").at3(0.0, 1.0, -10.0).with(Collider::cuboid(4.0, 2.0, 1.0));
    });
    let p = physics(&app);
    let floor = app.world.scene.find_by_name("Floor").unwrap();
    let wall = app.world.scene.find_by_name("Wall").unwrap();
    let half = Vec3::new(0.3, 0.7, 0.3);
    // Вниз на пол: точная дистанция (у GJK на таком полу погрешность — сантиметры).
    for (x, z) in [(0.0, 0.0), (37.3, -91.7), (-150.2, 120.9)] {
        let hit = p.cast_box(Vec3::new(x, 2.0, z), half, Vec3::new(0.0, -3.0, 0.0), QueryFilter3D::solid()).expect("hits the floor");
        assert_eq!(hit.object, floor);
        assert!((hit.distance - 1.3).abs() < 1e-4, "distance {}", hit.distance);
        assert!((hit.normal - Vec3::Y).length() < 1e-5, "normal {}", hit.normal);
        assert!(!hit.started_inside);
    }
    // Вперёд в стену: передняя грань стены z = −9.5, ящик 0.3 → центр останавливается на −9.2.
    let hit = p.cast_box(Vec3::new(0.0, 1.0, 0.0), half, Vec3::new(0.0, 0.0, -20.0), QueryFilter3D::solid()).unwrap();
    assert_eq!(hit.object, wall);
    assert!((hit.distance - 9.2).abs() < 1e-4 && (hit.fraction - 9.2 / 20.0).abs() < 1e-5);
    assert!((hit.normal - Vec3::Z).length() < 1e-5);
    // Скольжение вдоль пола с зазором в миллиметр — не касание.
    assert!(p.cast_box(Vec3::new(0.0, 0.701, 0.0), half, Vec3::new(30.0, 0.0, 5.0), QueryFilter3D::solid()).is_none());
    // Стоя на полу и уходя от него — тоже.
    assert!(p.cast_box(Vec3::new(5.0, 0.7, 5.0), half, Vec3::new(1.0, 0.5, 0.0), QueryFilter3D::solid()).is_none());
    // Внутри стены и движемся глубже — касание сразу, «с начала внутри».
    let hit = p.cast_box(Vec3::new(0.0, 1.0, -9.4), half, Vec3::new(0.0, 0.0, -1.0), QueryFilter3D::solid()).unwrap();
    assert!(hit.started_inside && hit.fraction == 0.0 && (hit.normal - Vec3::Z).length() < 1e-5);
    // Себя можно исключить.
    assert!(p.cast_box(Vec3::new(0.0, 2.0, 0.0), half, Vec3::new(0.0, -3.0, 0.0), QueryFilter3D::ignoring(floor)).is_none());
}

#[test]
fn cast_box_against_rotated_boxes_and_other_shapes() {
    let a = 30f32.to_radians();
    let app = world(|app| {
        app.spawn("Ramp").at3(0.0, 0.0, 0.0).rotation(Quat::from_rotation_x(a)).with(Collider::cuboid(4.0, 1.0, 10.0));
        app.spawn("Ball").at3(20.0, 0.0, 0.0).with(Collider::sphere(1.0));
    });
    let p = physics(&app);
    let half = Vec3::splat(0.25);
    let hit = p.cast_box(Vec3::new(0.0, 5.0, 0.0), half, Vec3::new(0.0, -10.0, 0.0), QueryFilter3D::solid()).unwrap();
    let n = Quat::from_rotation_x(a) * Vec3::Y;
    assert!((hit.normal - n).length() < 1e-4, "ramp normal {} vs {n}", hit.normal);
    // Верх рампы над центром — 0.5/cos 30°, нижний «гребень» ящика касается его ребром.
    let expected = 5.0 - 0.5 / a.cos() - 0.25 - 0.25 * a.tan();
    assert!((hit.distance - expected).abs() < 1e-3, "{} vs {expected}", hit.distance);
    // Сфера — через GJK: тоже находится, нормаль от центра сферы.
    let hit = p.cast_box(Vec3::new(20.0, 5.0, 0.0), half, Vec3::new(0.0, -10.0, 0.0), QueryFilter3D::solid()).unwrap();
    assert!((hit.distance - 3.75).abs() < 0.01 && hit.normal.y > 0.99);
    // cast_shape сферой и капсулой.
    let ball = p.cast_shape(CastShape3D::Sphere { radius: 0.5 }, Vec3::new(20.0, 5.0, 0.0), Quat::IDENTITY, Vec3::new(0.0, -10.0, 0.0), QueryFilter3D::solid()).unwrap();
    assert!((ball.distance - 3.5).abs() < 0.01);
    let capsule = CastShape3D::Capsule { radius: 0.3, half_height: 0.6 };
    let hit = p.cast_shape(capsule, Vec3::new(20.0, 5.0, 0.0), Quat::IDENTITY, Vec3::new(0.0, -10.0, 0.0), QueryFilter3D::solid()).unwrap();
    assert!((hit.distance - 3.1).abs() < 0.01, "capsule {}", hit.distance);
    assert!(p.cast_box(Vec3::ZERO, half, Vec3::ZERO, QueryFilter3D::solid()).is_none(), "zero motion is no cast");
}

#[test]
fn triggers_and_layers_follow_the_filter() {
    let app = world(|app| {
        app.spawn("Trigger").at3(0.0, 0.0, -5.0).with(Collider::cuboid(2.0, 2.0, 1.0).trigger());
        app.spawn("Glass").at3(0.0, 0.0, -8.0).layer(9).with(Collider::cuboid(2.0, 2.0, 1.0));
        app.spawn("Wall").at3(0.0, 0.0, -12.0).with(Collider::cuboid(2.0, 2.0, 1.0));
    });
    let p = physics(&app);
    let name = |hit: Option<ShapeHit3D>| hit.and_then(|h| app.world.scene.get(h.object).map(|o| o.name.clone()));
    let go = |filter| name(p.cast_box(Vec3::ZERO, Vec3::splat(0.2), Vec3::new(0.0, 0.0, -20.0), filter));
    assert_eq!(go(QueryFilter3D::solid()).as_deref(), Some("Glass"), "triggers are skipped by default");
    assert_eq!(go(QueryFilter3D::solid().with_triggers()).as_deref(), Some("Trigger"));
    assert_eq!(go(QueryFilter3D::solid().with_layers(!(1 << 9))).as_deref(), Some("Wall"), "layer 9 masked out");
    // overlap_box: касание — не пересечение; триггеры — по фильтру.
    let names = |ids: Vec<GameObjectId>| {
        let mut n: Vec<String> = ids.iter().filter_map(|id| app.world.scene.get(*id).map(|o| o.name.clone())).collect();
        n.sort();
        n
    };
    assert_eq!(names(p.overlap_box(Vec3::new(0.0, 0.0, -6.5), Vec3::new(0.5, 0.5, 1.2), QueryFilter3D::solid())), vec!["Glass"]);
    assert_eq!(names(p.overlap_box(Vec3::new(0.0, 0.0, -6.5), Vec3::new(0.5, 0.5, 1.2), QueryFilter3D::solid().with_triggers())), vec!["Glass", "Trigger"]);
    assert!(p.overlap_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.5, 0.5, 1.0), QueryFilter3D::solid()).is_empty(), "the gap between boxes is empty");
}

#[test]
fn penetration_pushes_a_box_out_and_poses_follow_kinematic_bodies() {
    let mut app = world(|app| {
        app.spawn("Floor").at3(0.0, -0.5, 0.0).with(Collider::cuboid(20.0, 1.0, 20.0));
        app.spawn("Wall").at3(2.0, 1.0, 0.0).with(Collider::cuboid(1.0, 2.0, 4.0));
        app.spawn("Lift").at3(-5.0, 1.0, 0.0).with(Rigidbody::kinematic()).with(Collider::cuboid(2.0, 0.2, 2.0));
    });
    let p = physics(&app);
    let half = Vec3::new(0.3, 0.7, 0.3);
    // В полу на 5 см и в стене на 10 см: выталкивает вверх и влево.
    let push = p.compute_penetration_box(Vec3::new(1.3, 0.65, 0.0), half, QueryFilter3D::solid()).expect("overlaps");
    assert!((push.y - 0.05).abs() < 2e-3 && (push.x + 0.1).abs() < 2e-3 && push.z.abs() < 1e-3, "push {push}");
    let out = Vec3::new(1.3, 0.65, 0.0) + push;
    assert!(p.compute_penetration_box(out, half * 0.999, QueryFilter3D::solid()).is_none(), "free after the push");
    assert!(p.compute_penetration_box(Vec3::new(0.0, 0.71, 0.0), half, QueryFilter3D::solid()).is_none());
    // Поза тела для запросов — после шага физики.
    let lift = app.world.scene.find_by_name("Lift").unwrap();
    let (before, _) = physics(&app).collider_pose(lift).unwrap();
    app.world.scene.get_mut(lift).unwrap().transform.position.y += 1.0;
    app.run_headless(1, 1.0 / 60.0);
    let (after, rotation) = physics(&app).collider_pose(lift).unwrap();
    assert!((after.y - before.y - 1.0).abs() < 1e-4 && rotation.is_near_identity());
}

#[test]
fn cursor_capture_is_a_request_the_host_grants() {
    let mut input = Input::default();
    assert!(!input.cursor_capture_requested() && !input.is_cursor_captured());
    input.set_cursor_captured(true);
    assert!(input.cursor_capture_requested() && !input.is_cursor_captured(), "only a request until the host applies it");
    input.set_cursor_capture_state(true);
    assert!(input.is_cursor_captured());
    // Пока курсор захвачен, движение курсора не даёт смещения — только сырые события.
    input.set_mouse_position(Vec2::new(500.0, 300.0));
    assert_eq!(input.mouse_delta(), Vec2::ZERO);
    input.add_mouse_delta(Vec2::new(3.0, -2.0));
    assert_eq!(input.mouse_delta(), Vec2::new(3.0, -2.0));
    // Потеря фокуса / пауза: просьба снимается, хозяин отпускает курсор.
    input.release_all();
    assert!(!input.cursor_capture_requested());
    assert!(input.is_cursor_captured(), "the host still has to release it");
    input.set_cursor_capture_state(false);
    input.end_frame();
    input.set_mouse_position(Vec2::new(510.0, 300.0));
    assert_eq!(input.mouse_delta(), Vec2::new(10.0, 0.0), "a free cursor moves as usual");
}
