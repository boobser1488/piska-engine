//! Компонент [`FpsController`]: ввод → команда → [`player_move`](super::movement::player_move)
//! на фиксированном шаге; взгляд мышью и сглаживание позы — каждый кадр.

use std::collections::HashMap;

use engine_core::prelude::*;
use engine_physics::{Physics3D, QueryFilter3D};
use engine_render::Camera;

use super::movement::{player_move, MoveSettings, MoveState, PhysicsHullWorld, UserCmd, UNIT};

component! {
    /// Контроллер от первого лица с классической физикой движения: разгон и трение на
    /// земле, стрейф в воздухе, распрыжка, присед и прыжок с приседом, ступеньки, скольжение
    /// по крутым склонам. Позиция объекта — у ног; дочерний объект с `Camera` — глаза
    /// (поле `camera` или первый ребёнок с камерой). Все размеры — в метрах; классическая
    /// единица = 0.01905 м (250 ед/с = 4.76 м/с). Подробно — docs/api/fps.md.
    pub struct FpsController {
        /// Ускорение свободного падения, м/с² (800 ед/с²).
        pub gravity: f32 = 800.0 * UNIT,
        /// Скорость бега, м/с (250 ед/с).
        pub max_speed: f32 = 250.0 * UNIT,
        /// Разгон на земле (безразмерный, как `sv_accelerate`).
        pub accelerate: f32 = 5.5,
        /// Разгон в воздухе (как `sv_airaccelerate`): больше — резче стрейф.
        pub air_accelerate: f32 = 12.0,
        /// Предел желаемой скорости в воздухе, м/с (30 ед/с). На нём держатся стрейф и распрыжка.
        pub air_speed_cap: f32 = 30.0 * UNIT,
        /// Трение на земле (как `sv_friction`).
        pub friction: f32 = 5.2,
        /// Ниже этой скорости трение тормозит как на ней — быстрая полная остановка, м/с (80 ед/с).
        pub stop_speed: f32 = 80.0 * UNIT,
        /// Высота прыжка, м (57 ед). Скорость отрыва — √(2·g·h).
        pub jump_height: f32 = 57.0 * UNIT,
        /// Ступенька, на которую персонаж шагает сам, м (18 ед).
        pub step_size: f32 = 18.0 * UNIT,
        /// Пол — поверхность, у которой вертикальная часть нормали не меньше этого (0.7 ≈ 45.6°).
        pub walkable_normal: f32 = 0.7,
        /// Предел каждой составляющей скорости, м/с (3500 ед/с).
        pub max_velocity: f32 = 3500.0 * UNIT,
        /// Ширина корпуса (квадрат), м (32 ед).
        pub hull_width: f32 = 32.0 * UNIT,
        /// Высота корпуса стоя, м (72 ед).
        pub stand_height: f32 = 72.0 * UNIT,
        /// Высота глаз стоя, м (64 ед).
        pub stand_eye: f32 = 64.0 * UNIT,
        /// Высота корпуса в приседе, м (54 ед).
        pub duck_height: f32 = 54.0 * UNIT,
        /// Высота глаз в приседе, м (46 ед).
        pub duck_eye: f32 = 46.0 * UNIT,
        /// За сколько секунд приседаешь на земле.
        pub duck_time: f32 = 0.2,
        /// За сколько секунд взгляд поднимается после вставания.
        pub unduck_time: f32 = 0.2,
        /// Множитель скорости в приседе (на земле).
        pub duck_speed_scale: f32 = 0.34,
        /// Множитель скорости шагом (Shift, на земле).
        pub walk_speed_scale: f32 = 0.52,
        /// Разрешить бег (действие `Sprint`). По умолчанию выключено, как в классике.
        pub sprint: bool = false,
        /// Множитель скорости бега.
        pub sprint_speed_scale: f32 = 1.3,
        /// Прыгать, пока прыжок удерживается (без него распрыжка — точным нажатием).
        pub auto_hop: bool = false,
        /// Полёт сквозь стены (отладка).
        pub noclip: bool = false,
        /// Скорость полёта, м/с (800 ед/с).
        pub noclip_speed: f32 = 800.0 * UNIT,
        /// Читать клавиатуру / геймпад. Выключите на паузе и в меню.
        pub input_enabled: bool = true,
        /// Колесо мыши — тоже прыжок (удобно для распрыжки).
        pub jump_on_wheel: bool = true,
        /// Поворачивать взгляд мышью и правым стиком.
        pub look_enabled: bool = true,
        /// Взгляд мышью — только пока курсор захвачен (`Input::set_cursor_captured`).
        pub look_requires_capture: bool = true,
        /// Чувствительность мыши (как `sensitivity`): градусы = отсчёты × чувствительность × m_yaw.
        pub sensitivity: f32 = 2.0,
        /// Градусов поворота на отсчёт мыши по горизонтали при чувствительности 1 (как `m_yaw`).
        pub m_yaw: f32 = 0.022,
        /// То же по вертикали (как `m_pitch`).
        pub m_pitch: f32 = 0.022,
        /// Инвертировать вертикаль мыши.
        pub invert_y: bool = false,
        /// Скорость поворота правым стиком, градусов в секунду.
        pub gamepad_look_speed: f32 = 200.0,
        /// Предел наклона взгляда вверх и вниз, градусы.
        pub pitch_limit: f32 = 89.0,
        /// Горизонтальный угол обзора в градусах для экрана 4:3. На широком экране
        /// вертикальный угол тот же, горизонтальный шире (Hor+): 90 → 73.7° по вертикали.
        pub fov_deg: f32 = 90.0,
        /// Камера-глаза. Пусто — первый ребёнок с `Camera`.
        pub camera: ObjectRef = ObjectRef::NONE,
        /// Поворот взгляда вокруг вертикали, градусы (0 — вдоль −Z).
        pub yaw_deg: f32 = 0.0,
        /// Наклон взгляда, градусы (плюс — вверх).
        pub pitch_deg: f32 = 0.0,
        /// Скорость, м/с (во время игры; можно задать скриптом — толчок, остановка).
        pub velocity: Vec3 = Vec3::ZERO,
        /// Стоит на земле (во время игры).
        pub on_ground: bool = false,
        /// Сидит (корпус низкий; во время игры).
        pub ducked: bool = false,
    }
}

/// Вертикальный угол обзора (градусы) для горизонтального угла `horizontal_4x3` на 4:3.
pub fn vertical_fov_deg(horizontal_4x3: f32) -> f32 {
    let half = (horizontal_4x3.clamp(1.0, 170.0) * 0.5).to_radians();
    2.0 * (half.tan() * 0.75).atan().to_degrees()
}

/// Горизонтальный угол обзора (градусы) на экране с отношением `aspect` (ширина / высота)
/// при настройке `horizontal_4x3`.
pub fn horizontal_fov_deg(horizontal_4x3: f32, aspect: f32) -> f32 {
    let vertical_half = (vertical_fov_deg(horizontal_4x3) * 0.5).to_radians();
    2.0 * (vertical_half.tan() * aspect.max(0.1)).atan().to_degrees()
}

impl FpsController {
    /// Настройки движения из полей компонента.
    pub fn settings(&self) -> MoveSettings {
        MoveSettings {
            gravity: self.gravity,
            max_speed: self.max_speed,
            accelerate: self.accelerate,
            air_accelerate: self.air_accelerate,
            air_speed_cap: self.air_speed_cap,
            friction: self.friction,
            stop_speed: self.stop_speed,
            jump_height: self.jump_height,
            step_size: self.step_size,
            walkable_normal: self.walkable_normal,
            max_velocity: self.max_velocity,
            hull_width: self.hull_width,
            stand_height: self.stand_height,
            stand_eye: self.stand_eye,
            duck_height: self.duck_height,
            duck_eye: self.duck_eye,
            duck_time: self.duck_time,
            unduck_time: self.unduck_time,
            duck_speed_scale: self.duck_speed_scale,
            walk_speed_scale: self.walk_speed_scale,
            sprint: self.sprint,
            sprint_speed_scale: self.sprint_speed_scale,
            auto_hop: self.auto_hop,
            noclip: self.noclip,
            noclip_speed: self.noclip_speed,
        }
    }

    /// Горизонтальная скорость в классических единицах в секунду — для спидометра.
    pub fn units_per_second(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length() / UNIT
    }

    /// Горизонтальная скорость, м/с.
    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length()
    }

    /// Направление взгляда (единичный вектор).
    pub fn view_direction(&self) -> Vec3 {
        super::movement::view_direction(self.yaw_deg, self.pitch_deg)
    }

    /// Команда из ввода (клавиатура, мышь, геймпад).
    fn read_command(&self, input: &Input, wheel_jump: bool) -> UserCmd {
        let mut cmd = UserCmd { yaw_deg: self.yaw_deg, pitch_deg: self.pitch_deg, ..UserCmd::default() };
        if !self.input_enabled {
            return cmd;
        }
        let held = |action: &str, fallback: &[Key]| {
            if input.has_button(action) {
                input.button(action) || input.button_down(action)
            } else {
                fallback.iter().any(|k| input.key(*k) || input.key_down(*k))
            }
        };
        cmd.forward = if input.has_axis("Vertical") { input.axis("Vertical") } else { 0.0 };
        cmd.side = if input.has_axis("Horizontal") { input.axis("Horizontal") } else { 0.0 };
        cmd.jump = held("Jump", &[Key::Space]) || wheel_jump;
        cmd.duck = held("Duck", &[Key::LCtrl, Key::C]);
        cmd.walk = held("Walk", &[Key::LShift]);
        cmd.sprint = input.has_button("Sprint") && input.button("Sprint");
        cmd.up = f32::from(u8::from(cmd.jump)) - f32::from(u8::from(cmd.duck));
        cmd
    }

    fn camera_id(&self, scene: &Scene, me: GameObjectId) -> Option<GameObjectId> {
        if let Some(id) = scene.resolve(self.camera) {
            return Some(id);
        }
        scene.get(me)?.children().iter().copied().find(|c| scene.get(*c).is_some_and(|o| o.has::<Camera>()))
    }
}

/// Функция-водитель бота: (номер шага, состояние) → команда.
type Driver = Box<dyn FnMut(u64, &MoveState) -> UserCmd>;

/// Внутреннее состояние одного контроллера (то, что не нужно в инспекторе).
struct FpsBody {
    state: MoveState,
    /// Позы для сглаживания: прошлый и текущий шаг (ноги и глаза, мир).
    previous_origin: Vec3,
    previous_eye: Vec3,
    /// Что контроллер записал в позицию объекта последним (иначе объект подвинули снаружи).
    written: Vec3,
    /// Опора (её поза на прошлом шаге): подвижная платформа везёт игрока.
    ground_pose: Option<(GameObjectId, Vec3, Quat)>,
    /// Сглаживание взгляда на ступеньках: тело поднялось на ступень за один шаг, а
    /// камера догоняет его за ~0.1 с (сейчас и на прошлом шаге), м.
    step_offset: f32,
    previous_step_offset: f32,
    /// Колесо мыши крутнули с прошлого шага.
    wheel_jump: bool,
    ticks: u64,
    command: Option<UserCmd>,
    driver: Option<Driver>,
    started: bool,
}

impl FpsBody {
    fn new() -> Self {
        Self {
            state: MoveState::at(Vec3::ZERO, &MoveSettings::default()),
            previous_origin: Vec3::ZERO,
            previous_eye: Vec3::ZERO,
            written: Vec3::NAN,
            ground_pose: None,
            step_offset: 0.0,
            previous_step_offset: 0.0,
            wheel_jump: false,
            ticks: 0,
            command: None,
            driver: None,
            started: false,
        }
    }
}

/// Ресурс со скрытым состоянием всех [`FpsController`] сцены: сглаживание, опора, боты.
///
/// Боты и тесты ведут игрока командами вместо ввода:
/// `world.resources.get_or_insert_with(FpsRuntime::default).set_command(id, Some(cmd))`
/// (одна команда до отмены) или `set_driver(id, |tick, state| cmd)` (команда на каждый шаг).
#[derive(Default)]
pub struct FpsRuntime {
    bodies: HashMap<GameObjectId, FpsBody>,
}

impl FpsRuntime {
    fn body(&mut self, id: GameObjectId) -> &mut FpsBody {
        self.bodies.entry(id).or_insert_with(FpsBody::new)
    }

    /// Вести игрока `id` этой командой на каждом шаге (`None` — снова ввод игрока).
    /// Поворот берётся из команды.
    pub fn set_command(&mut self, id: GameObjectId, command: Option<UserCmd>) {
        self.body(id).command = command;
    }

    /// Команду на каждый шаг даёт функция `(номер шага, состояние) -> команда`.
    pub fn set_driver(&mut self, id: GameObjectId, driver: impl FnMut(u64, &MoveState) -> UserCmd + 'static) {
        self.body(id).driver = Some(Box::new(driver));
    }

    /// Убрать функцию-водителя.
    pub fn clear_driver(&mut self, id: GameObjectId) {
        if let Some(body) = self.bodies.get_mut(&id) {
            body.driver = None;
        }
    }

    /// Сколько фиксированных шагов прошёл контроллер.
    pub fn ticks(&self, id: GameObjectId) -> u64 {
        self.bodies.get(&id).map_or(0, |b| b.ticks)
    }

    /// Точное состояние движения после последнего шага (без сглаживания).
    pub fn state(&self, id: GameObjectId) -> Option<&MoveState> {
        self.bodies.get(&id).filter(|b| b.started).map(|b| &b.state)
    }
}

/// Записать мировую позицию объекту (с учётом родителя).
fn write_world_position(scene: &mut Scene, id: GameObjectId, world: Vec3) {
    let parent = scene.get(id).and_then(|o| o.parent()).map(|p| scene.world_matrix(p));
    if let Some(object) = scene.get_mut(id) {
        object.transform.position = match parent {
            Some(m) => m.inverse().transform_point3(world),
            None => world,
        };
    }
}

fn wrap_degrees(angle: f32) -> f32 {
    let a = (angle + 180.0).rem_euclid(360.0) - 180.0;
    if a.is_finite() {
        a
    } else {
        0.0
    }
}

impl Component for FpsController {
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::FIXED_UPDATE | Hooks::UPDATE | Hooks::DESTROY
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        let id = ctx.id;
        let origin = ctx.world.scene.world_position(id);
        let settings = self.settings();
        let runtime = ctx.world.resources.get_or_insert_with(FpsRuntime::default);
        let body = runtime.body(id);
        body.state = MoveState { velocity: self.velocity, ducked: self.ducked, ..MoveState::at(origin, &settings) };
        body.previous_origin = origin;
        body.previous_eye = body.state.eye();
        body.written = origin;
        body.started = true;
        let eye = body_eye(ctx, id);
        self.apply_view(ctx, origin, eye);
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let id = ctx.id;
        let settings = self.settings();
        let World { resources, scene, input, .. } = &mut *ctx.world;
        let mut runtime = resources.remove::<FpsRuntime>().unwrap_or_default();
        let body = runtime.body(id);
        if !body.started {
            let origin = scene.world_position(id);
            body.state = MoveState::at(origin, &settings);
            body.previous_origin = origin;
            body.written = origin;
            body.started = true;
        }

        // Объект подвинули снаружи (телепорт, редактор, респаун) — начинаем оттуда.
        let now = scene.world_position(id);
        if !(now - body.written).length_squared().le(&1e-10) {
            body.state.origin = now;
            body.previous_origin = now;
            body.previous_eye = now + Vec3::Y * body.state.eye_height;
            body.ground_pose = None;
            body.step_offset = 0.0;
            body.previous_step_offset = 0.0;
        }
        // Поля компонента — часть состояния: скрипт мог задать скорость или присед.
        body.state.velocity = self.velocity;
        body.state.on_ground = self.on_ground;
        if self.ducked != body.state.ducked {
            body.state.ducked = self.ducked;
            body.state.duck_amount = if self.ducked { 1.0 } else { 0.0 };
        }

        let mut cmd = match (&mut body.driver, body.command) {
            (Some(driver), _) => driver(body.ticks, &body.state),
            (None, Some(cmd)) => cmd,
            (None, None) => {
                let mut cmd = self.read_command(input, body.wheel_jump);
                cmd.yaw_deg = self.yaw_deg;
                cmd.pitch_deg = self.pitch_deg;
                cmd
            }
        };
        body.wheel_jump = false;
        if body.driver.is_some() || body.command.is_some() {
            self.yaw_deg = wrap_degrees(cmd.yaw_deg);
            self.pitch_deg = cmd.pitch_deg.clamp(-self.pitch_limit.abs(), self.pitch_limit.abs());
        }

        body.previous_origin = body.state.origin;
        body.previous_eye = body.state.eye();
        if let Some(physics) = resources.get::<Physics3D>() {
            // Опора сдвинулась или повернулась с прошлого шага — едем вместе с ней.
            if let (true, Some((ground, old_position, old_rotation))) = (body.state.on_ground, body.ground_pose) {
                let pose = physics.collider_pose(ground);
                // Неподвижная опора: не трогаем (поворот «на ноль» с погрешностью float
                // сдвигал бы игрока на микроны за шаг вокруг далёкого центра).
                let moved = pose.is_some_and(|(p, r)| p != old_position || r != old_rotation);
                if let Some((position, rotation)) = pose.filter(|_| moved) {
                    let turn = rotation * old_rotation.inverse();
                    let carried = position + turn * (body.state.origin - old_position);
                    if carried.is_finite() && (carried - body.state.origin).length() < 5.0 {
                        body.state.origin = carried;
                        let ahead = turn * Vec3::NEG_Z;
                        let yaw_turn = (-ahead.x).atan2(-ahead.z).to_degrees();
                        if yaw_turn.abs() > 1e-4 {
                            self.yaw_deg = wrap_degrees(self.yaw_deg + yaw_turn);
                            cmd.yaw_deg += yaw_turn;
                        }
                    }
                }
            }
            let world = PhysicsHullWorld { physics, filter: QueryFilter3D::ignoring(id) };
            let (was_on_ground, old_y) = (body.state.on_ground, body.state.origin.y);
            player_move(&settings, &mut body.state, &cmd, dt, &world);
            // Ступенька: тело переносится на неё за шаг — камере такой рывок не нужен.
            // Склоны (меньше 0.1 м за шаг) и прыжки не сглаживаются.
            body.previous_step_offset = body.step_offset;
            let dy = body.state.origin.y - old_y;
            if was_on_ground && body.state.on_ground && !body.state.jumped && dy.abs() > 0.1 && dy.abs() <= settings.step_size + 0.05 {
                body.step_offset -= dy;
            }
            body.step_offset = (body.step_offset * (-dt / 0.045).exp()).clamp(-settings.step_size, settings.step_size);
            body.ground_pose = body
                .state
                .ground_object
                .filter(|_| body.state.on_ground)
                .and_then(|g| physics.collider_pose(g).map(|(p, r)| (g, p, r)));
        }
        body.ticks += 1;

        self.velocity = body.state.velocity;
        self.on_ground = body.state.on_ground;
        self.ducked = body.state.ducked;
        // Скрипты фиксированного шага видят точную позу.
        let origin = body.state.origin;
        write_world_position(scene, id, origin);
        body.written = scene.world_position(id);
        resources.insert(runtime);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let id = ctx.id;
        let alpha = ctx.world.time.fixed_alpha.clamp(0.0, 1.0);
        let (driven, wheel) = {
            let input = &ctx.world.input;
            let runtime = ctx.world.resources.get::<FpsRuntime>();
            let driven = runtime.and_then(|r| r.bodies.get(&id)).is_some_and(|b| b.command.is_some() || b.driver.is_some());
            (driven, self.jump_on_wheel && self.input_enabled && input.scroll_delta().y != 0.0)
        };

        // Взгляд — каждый кадр, по смещению мыши за кадр: не зависит от частоты кадров.
        if self.look_enabled && self.input_enabled && !driven {
            let input = &ctx.world.input;
            if input.is_cursor_captured() || !self.look_requires_capture {
                let delta = input.mouse_delta();
                let invert = if self.invert_y { -1.0 } else { 1.0 };
                self.yaw_deg -= delta.x * self.sensitivity * self.m_yaw;
                self.pitch_deg -= delta.y * self.sensitivity * self.m_pitch * invert;
            }
            let mut stick = Vec2::ZERO;
            for pad in input.gamepads() {
                let s = input.gamepad_right_stick(pad, 0.15);
                if s.length_squared() > stick.length_squared() {
                    stick = s;
                }
            }
            let step = self.gamepad_look_speed * dt;
            self.yaw_deg -= stick.x * step;
            self.pitch_deg += stick.y * step;
        }
        self.yaw_deg = wrap_degrees(self.yaw_deg);
        let limit = self.pitch_limit.abs().min(89.9);
        self.pitch_deg = if self.pitch_deg.is_finite() { self.pitch_deg.clamp(-limit, limit) } else { 0.0 };

        // Сглаженная поза между двумя последними шагами.
        let Some(runtime) = ctx.world.resources.get_mut::<FpsRuntime>() else {
            return;
        };
        let Some(body) = runtime.bodies.get_mut(&id).filter(|b| b.started) else {
            return;
        };
        if wheel {
            body.wheel_jump = true;
        }
        let origin = body.previous_origin.lerp(body.state.origin, alpha);
        let step = body.previous_step_offset + (body.step_offset - body.previous_step_offset) * alpha;
        let eye = body.previous_eye.lerp(body.state.eye(), alpha) + Vec3::Y * step;
        let now = ctx.world.scene.world_position(id);
        let moved_outside = !(now - body.written).length_squared().le(&1e-10);
        if moved_outside {
            return; // подвинули снаружи — шаг заберёт новую позицию
        }
        write_world_position(&mut ctx.world.scene, id, origin);
        let written = ctx.world.scene.world_position(id);
        if let Some(body) = ctx.world.resources.get_mut::<FpsRuntime>().and_then(|r| r.bodies.get_mut(&id)) {
            body.written = written;
        }
        self.apply_view(ctx, origin, eye);
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if let Some(runtime) = ctx.world.resources.get_mut::<FpsRuntime>() {
            runtime.bodies.remove(&ctx.id);
        }
    }
}

/// Мировая позиция глаз по точному состоянию.
fn body_eye(ctx: &Ctx, id: GameObjectId) -> Vec3 {
    ctx.world
        .resources
        .get::<FpsRuntime>()
        .and_then(|r| r.bodies.get(&id))
        .map_or_else(|| ctx.world.scene.world_position(id), |b| b.state.eye())
}

impl FpsController {
    /// Поворот тела (yaw), камера на высоте глаз с наклоном (pitch) и углом обзора.
    fn apply_view(&self, ctx: &mut Ctx, origin: Vec3, eye: Vec3) {
        let id = ctx.id;
        let yaw = Quat::from_rotation_y(self.yaw_deg.to_radians());
        if let Some(object) = ctx.world.scene.get_mut(id) {
            object.transform.rotation = yaw;
        }
        let Some(camera) = self.camera_id(&ctx.world.scene, id) else {
            return;
        };
        let scale = ctx.world.scene.get(id).map_or(Vec3::ONE, |o| o.transform.scale);
        let height = (eye.y - origin.y) / scale.y.abs().max(1e-4);
        let fov = vertical_fov_deg(self.fov_deg);
        if let Some(object) = ctx.world.scene.get_mut(camera) {
            object.transform.position = Vec3::new(0.0, height, 0.0);
            object.transform.rotation = Quat::from_rotation_x(self.pitch_deg.to_radians());
            if let Some(cam) = object.get_mut::<Camera>() {
                if (cam.fov_deg - fov).abs() > 1e-4 {
                    cam.fov_deg = fov;
                }
            }
        }
    }
}
