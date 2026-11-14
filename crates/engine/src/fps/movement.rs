//! Движение игрока в классической схеме «шутеров на движке с BSP»: категоризация
//! (земля / воздух), трение, разгон на земле, разгон в воздухе с ограничением желаемой
//! скорости (из-за него работают стрейф в воздухе и распрыжка), гравитация двумя
//! полушагами вокруг перемещения, прыжок мгновенной вертикальной скоростью, срезание
//! скорости о плоскости касания до четырёх раз за шаг, подъём на ступеньку и спуск с неё,
//! приседание с низким корпусом и прыжок с приседом.
//!
//! Корпус — ящик, выровненный по осям мира, его «трассы» — протаскивание ящика через
//! запросы [`Physics3D::cast_box`]. Код не знает о компонентах: [`player_move`] берёт
//! настройки, состояние, команду и мир трасс — его можно гонять в тестах и для ботов.
//! Все величины — в метрах и секундах; классические единицы переводит [`UNIT`].

use engine_core::prelude::*;
use engine_physics::{Physics3D, QueryFilter3D, ShapeHit3D};

/// Одна классическая единица длины в метрах (0.75 дюйма): 250 ед/с = 4.7625 м/с.
pub const UNIT: f32 = 0.01905;

/// Зазор, который корпус держит до поверхностей. Пока ящик висит над полом на зазор,
/// горизонтальное движение не цепляет рёбра соседних плит пола («призрачные» касания).
pub const SKIN: f32 = 0.002;
/// Мелкие составляющие скорости после срезания обнуляются (0.1 ед/с).
const STOP_EPSILON: f32 = 0.1 * UNIT;
/// Быстрее этого вверх — точно не на земле (прыжок, толчок): 140 ед/с.
const NON_JUMP_VELOCITY: f32 = 140.0 * UNIT;
/// Как глубоко под ногами ищется земля: 2 ед.
const GROUND_PROBE: f32 = 2.0 * UNIT;
const MAX_CLIP_PLANES: usize = 5;
const MAX_BUMPS: usize = 4;

/// Команда игрока на один шаг (как «usercmd»): куда идти, куда смотреть, что нажато.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UserCmd {
    /// Вперёд (+1) / назад (−1).
    pub forward: f32,
    /// Вправо (+1) / влево (−1).
    pub side: f32,
    /// Вверх / вниз — только в режиме noclip.
    pub up: f32,
    /// Поворот вокруг вертикали, градусы (0 — взгляд вдоль −Z, плюс — влево).
    pub yaw_deg: f32,
    /// Наклон взгляда, градусы (плюс — вверх).
    pub pitch_deg: f32,
    /// Прыжок удерживается.
    pub jump: bool,
    /// Присед удерживается.
    pub duck: bool,
    /// Шаг (тихая ходьба) удерживается.
    pub walk: bool,
    /// Бег удерживается (работает, только если `sprint` включён в настройках).
    pub sprint: bool,
}

/// Настройки движения (метры, секунды). По умолчанию — классические значения.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveSettings {
    pub gravity: f32,
    pub max_speed: f32,
    pub accelerate: f32,
    pub air_accelerate: f32,
    pub air_speed_cap: f32,
    pub friction: f32,
    pub stop_speed: f32,
    pub jump_height: f32,
    pub step_size: f32,
    pub walkable_normal: f32,
    pub max_velocity: f32,
    pub hull_width: f32,
    pub stand_height: f32,
    pub stand_eye: f32,
    pub duck_height: f32,
    pub duck_eye: f32,
    pub duck_time: f32,
    pub unduck_time: f32,
    pub duck_speed_scale: f32,
    pub walk_speed_scale: f32,
    pub sprint: bool,
    pub sprint_speed_scale: f32,
    pub auto_hop: bool,
    pub noclip: bool,
    pub noclip_speed: f32,
}

impl Default for MoveSettings {
    fn default() -> Self {
        Self {
            gravity: 800.0 * UNIT,
            max_speed: 250.0 * UNIT,
            accelerate: 5.5,
            air_accelerate: 12.0,
            air_speed_cap: 30.0 * UNIT,
            friction: 5.2,
            stop_speed: 80.0 * UNIT,
            jump_height: 57.0 * UNIT,
            step_size: 18.0 * UNIT,
            walkable_normal: 0.7,
            max_velocity: 3500.0 * UNIT,
            hull_width: 32.0 * UNIT,
            stand_height: 72.0 * UNIT,
            stand_eye: 64.0 * UNIT,
            duck_height: 54.0 * UNIT,
            duck_eye: 46.0 * UNIT,
            duck_time: 0.2,
            unduck_time: 0.2,
            duck_speed_scale: 0.34,
            walk_speed_scale: 0.52,
            sprint: false,
            sprint_speed_scale: 1.3,
            auto_hop: false,
            noclip: false,
            noclip_speed: 800.0 * UNIT,
        }
    }
}

impl MoveSettings {
    /// Скорость отрыва, дающая высоту прыжка `jump_height`: √(2·g·h).
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity.max(0.0) * self.jump_height.max(0.0)).sqrt()
    }

    /// Высота корпуса в текущем положении.
    pub fn hull_height(&self, ducked: bool) -> f32 {
        if ducked {
            self.duck_height.min(self.stand_height)
        } else {
            self.stand_height
        }
    }

    /// Половины размеров корпуса.
    pub fn hull_half(&self, ducked: bool) -> Vec3 {
        let w = self.hull_width.max(0.02) * 0.5;
        Vec3::new(w, self.hull_height(ducked).max(0.02) * 0.5, w)
    }
}

/// Состояние движения между шагами.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveState {
    /// Центр подошвы корпуса (позиция объекта), мир.
    pub origin: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub ground_normal: Vec3,
    pub ground_object: Option<GameObjectId>,
    /// Корпус низкий (присед завершён).
    pub ducked: bool,
    /// 0..1: насколько опущен взгляд (на земле присед идёт плавно).
    pub duck_amount: f32,
    /// Высота глаз над `origin`.
    pub eye_height: f32,
    /// Прыжок был нажат на прошлом шаге (новый прыжок — только по новому нажатию).
    pub old_jump: bool,
    /// На этом шаге: оторвался прыжком.
    pub jumped: bool,
    /// На этом шаге: приземлился; `land_speed` — вертикальная скорость удара, м/с.
    pub landed: bool,
    pub land_speed: f32,
}

impl MoveState {
    /// Стоит в точке `origin`.
    pub fn at(origin: Vec3, settings: &MoveSettings) -> Self {
        Self {
            origin,
            velocity: Vec3::ZERO,
            on_ground: false,
            ground_normal: Vec3::Y,
            ground_object: None,
            ducked: false,
            duck_amount: 0.0,
            eye_height: settings.stand_eye,
            old_jump: false,
            jumped: false,
            landed: false,
            land_speed: 0.0,
        }
    }

    /// Мировая позиция глаз.
    pub fn eye(&self) -> Vec3 {
        self.origin + Vec3::Y * self.eye_height
    }

    /// Горизонтальная скорость в классических единицах в секунду (для спидометра).
    pub fn units_per_second(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length() / UNIT
    }
}

/// Мир, в котором ходит корпус: протаскивание ящика и выталкивание из геометрии.
pub trait HullWorld {
    /// Протащить ящик (центр, половины) на `motion`; первое касание.
    fn cast(&self, center: Vec3, half: Vec3, motion: Vec3) -> Option<ShapeHit3D>;
    /// Насколько сдвинуть ящик, чтобы он ни с чем не пересекался (`None` — свободен).
    fn penetration(&self, center: Vec3, half: Vec3) -> Option<Vec3>;
}

/// [`HullWorld`] поверх 3D-физики движка: твёрдые коллайдеры, без триггеров и без себя.
pub struct PhysicsHullWorld<'a> {
    pub physics: &'a Physics3D,
    pub filter: QueryFilter3D,
}

impl HullWorld for PhysicsHullWorld<'_> {
    fn cast(&self, center: Vec3, half: Vec3, motion: Vec3) -> Option<ShapeHit3D> {
        self.physics.cast_box(center, half, motion, self.filter)
    }

    fn penetration(&self, center: Vec3, half: Vec3) -> Option<Vec3> {
        self.physics.compute_penetration_box(center, half, self.filter)
    }
}

/// Направления «вперёд» и «вправо» по горизонтали для поворота `yaw_deg`.
pub fn yaw_vectors(yaw_deg: f32) -> (Vec3, Vec3) {
    let (sin, cos) = yaw_deg.to_radians().sin_cos();
    (Vec3::new(-sin, 0.0, -cos), Vec3::new(cos, 0.0, -sin))
}

/// Направление взгляда для поворота и наклона.
pub fn view_direction(yaw_deg: f32, pitch_deg: f32) -> Vec3 {
    let (forward, _) = yaw_vectors(yaw_deg);
    let (sin, cos) = pitch_deg.to_radians().sin_cos();
    forward * cos + Vec3::Y * sin
}

/// Срезать скорость о плоскость с нормалью `normal` (overbounce 1 — просто скольжение).
pub fn clip_velocity(velocity: Vec3, normal: Vec3, overbounce: f32) -> Vec3 {
    let backoff = velocity.dot(normal) * overbounce;
    let mut out = velocity - normal * backoff;
    for axis in 0..3 {
        if out[axis].abs() < STOP_EPSILON {
            out[axis] = 0.0;
        }
    }
    let adjust = out.dot(normal);
    if adjust < 0.0 {
        out -= normal * adjust;
    }
    out
}

/// Результат трассы корпуса.
#[derive(Clone, Copy, Debug)]
struct Trace {
    /// Доля пути до остановки (0..1); 1 — путь свободен.
    fraction: f32,
    /// Где корпус остановился (с зазором до поверхности).
    end: Vec3,
    normal: Vec3,
    object: Option<GameObjectId>,
    start_solid: bool,
}

/// Один шаг движения: команда `cmd` за время `dt` в мире `world`.
pub fn player_move(settings: &MoveSettings, state: &mut MoveState, cmd: &UserCmd, dt: f32, world: &impl HullWorld) {
    if dt.is_nan() || dt <= 0.0 {
        return;
    }
    let mut mover = Mover { s: settings, st: state, world, dt };
    mover.full_move(cmd);
}

struct Mover<'a, W: HullWorld> {
    s: &'a MoveSettings,
    st: &'a mut MoveState,
    world: &'a W,
    dt: f32,
}

impl<W: HullWorld> Mover<'_, W> {
    fn full_move(&mut self, cmd: &UserCmd) {
        let s = self.s;
        let dt = self.dt;
        self.st.jumped = false;
        self.st.landed = false;
        if !self.st.velocity.is_finite() {
            self.st.velocity = Vec3::ZERO;
        }

        // Застрял в геометрии (появился внутри ящика, его надвинула платформа) — вытолкнуть.
        self.unstick();

        self.duck(cmd);
        self.update_eye();

        if s.noclip {
            self.fly(cmd);
            return;
        }

        let g = s.gravity;
        // Первая половина гравитации — до перемещения.
        if !self.st.on_ground {
            self.st.velocity.y -= g * dt * 0.5;
        }

        // Прыжок: только с земли и только по новому нажатию (или с автопрыжком).
        if cmd.jump && self.st.on_ground && (!self.st.old_jump || s.auto_hop) {
            self.st.velocity.y = s.jump_speed() - g * dt * 0.5;
            self.st.on_ground = false;
            self.st.ground_object = None;
            self.st.jumped = true;
        }
        self.st.old_jump = cmd.jump;

        if self.st.on_ground {
            self.st.velocity.y = 0.0;
            self.friction();
        }
        self.clamp_velocity();

        let (wish_dir, wish_speed) = self.wish(cmd);
        if self.st.on_ground {
            self.walk_move(wish_dir, wish_speed);
        } else {
            self.air_move(wish_dir, wish_speed);
        }

        let fall_speed = -self.st.velocity.y;
        self.categorize();
        if self.st.landed {
            self.st.land_speed = fall_speed.max(0.0);
        }
        // Вторая половина гравитации — после перемещения.
        if !self.st.on_ground {
            self.st.velocity.y -= g * dt * 0.5;
        } else {
            self.st.velocity.y = 0.0;
        }
        self.clamp_velocity();
    }

    fn half(&self) -> Vec3 {
        self.s.hull_half(self.st.ducked)
    }

    fn trace(&self, from: Vec3, to: Vec3) -> Trace {
        self.trace_hull(from, to, self.half())
    }

    fn trace_hull(&self, from: Vec3, to: Vec3, half: Vec3) -> Trace {
        let motion = to - from;
        let length = motion.length();
        let clear = Trace { fraction: 1.0, end: to, normal: Vec3::ZERO, object: None, start_solid: false };
        if length.is_nan() || length <= 1e-7 {
            return Trace { end: from, ..clear };
        }
        let Some(hit) = self.world.cast(from + Vec3::Y * half.y, half, motion) else {
            return clear;
        };
        let dir = motion / length;
        let normal = if hit.normal.is_finite() && hit.normal.length_squared() > 0.25 {
            hit.normal.normalize()
        } else {
            -dir
        };
        let along = -dir.dot(normal);
        // Останавливаемся с зазором SKIN до поверхности: при лобовом касании — отступив
        // назад по пути, при скользящем — отодвинувшись по нормали (по пути назад
        // пришлось бы отступать очень далеко).
        let end = if along >= 0.25 {
            from + dir * (hit.distance - SKIN / along).max(0.0)
        } else {
            from + dir * hit.distance + normal * SKIN
        };
        Trace {
            fraction: ((end - from).dot(dir) / length).clamp(0.0, 1.0),
            end,
            normal,
            object: Some(hit.object),
            start_solid: hit.started_inside,
        }
    }

    fn blocked(&self, origin: Vec3, ducked: bool) -> bool {
        let half = self.s.hull_half(ducked);
        self.world.penetration(origin + Vec3::Y * half.y, half).is_some()
    }

    fn unstick(&mut self) {
        let half = self.half();
        if let Some(push) = self.world.penetration(self.st.origin + Vec3::Y * half.y, half) {
            // Глубже полуметра — это не «застрял», а телепорт в стену: не дёргаем.
            if push.length() < 0.5 {
                self.st.origin += push + push.normalize_or_zero() * SKIN;
            }
        }
    }

    // ------------------------------------------------------------------
    // Присед
    // ------------------------------------------------------------------

    fn duck(&mut self, cmd: &UserCmd) {
        let s = self.s;
        let delta = (s.stand_height - s.hull_height(true)).max(0.0);
        if cmd.duck && !s.noclip {
            if self.st.ducked {
                self.st.duck_amount = 1.0;
                return;
            }
            if !self.st.on_ground {
                // В воздухе — сразу: ноги подтягиваются вверх, глаза остаются на месте
                // (так «прыжок с приседом» достаёт выше обычного).
                self.st.origin.y += delta;
                self.st.ducked = true;
                self.st.duck_amount = 1.0;
            } else {
                self.st.duck_amount = (self.st.duck_amount + self.dt / s.duck_time.max(1e-3)).min(1.0);
                if self.st.duck_amount >= 1.0 {
                    self.st.ducked = true;
                }
            }
        } else if self.st.ducked {
            if self.st.on_ground || s.noclip {
                if !self.blocked(self.st.origin, false) {
                    self.st.ducked = false; // корпус выше сразу, взгляд поднимается плавно
                }
            } else {
                // В воздухе ноги опускаются обратно — если под ними есть место.
                let lowered = self.st.origin - Vec3::Y * delta;
                if !self.blocked(lowered, false) {
                    self.st.origin = lowered;
                    self.st.ducked = false;
                    self.st.duck_amount = 0.0;
                }
            }
        } else {
            self.st.duck_amount = (self.st.duck_amount - self.dt / s.unduck_time.max(1e-3)).max(0.0);
        }
    }

    fn update_eye(&mut self) {
        let t = self.st.duck_amount.clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.st.eye_height = self.s.stand_eye + (self.s.duck_eye - self.s.stand_eye) * eased;
    }

    // ------------------------------------------------------------------
    // Разгон и трение
    // ------------------------------------------------------------------

    fn wish(&self, cmd: &UserCmd) -> (Vec3, f32) {
        let (forward, right) = yaw_vectors(cmd.yaw_deg);
        let f = cmd.forward.clamp(-1.0, 1.0);
        let r = cmd.side.clamp(-1.0, 1.0);
        let wish = forward * f + right * r;
        let amount = Vec2::new(f, r).length().min(1.0);
        let Some(dir) = wish.try_normalize() else {
            return (Vec3::ZERO, 0.0);
        };
        let s = self.s;
        let mut speed = s.max_speed * amount;
        if self.st.on_ground {
            if self.st.ducked {
                speed *= s.duck_speed_scale;
            } else if cmd.walk {
                speed *= s.walk_speed_scale;
            } else if cmd.sprint && s.sprint {
                speed *= s.sprint_speed_scale;
            }
        }
        (dir, speed)
    }

    fn friction(&mut self) {
        let speed = self.st.velocity.length();
        if speed < 0.1 * UNIT {
            return;
        }
        let control = speed.max(self.s.stop_speed);
        let drop = control * self.s.friction * self.dt;
        let new_speed = (speed - drop).max(0.0);
        self.st.velocity *= new_speed / speed;
    }

    fn accelerate(&mut self, wish_dir: Vec3, wish_speed: f32, accel: f32) {
        let current = self.st.velocity.dot(wish_dir);
        let add = wish_speed - current;
        if add <= 0.0 {
            return;
        }
        let amount = (accel * self.dt * wish_speed).min(add);
        self.st.velocity += wish_dir * amount;
    }

    fn air_accelerate(&mut self, wish_dir: Vec3, wish_speed: f32, accel: f32) {
        // Желаемая скорость в воздухе срезана до маленькой: разгоняться можно только
        // поперёк текущей скорости — отсюда стрейф в воздухе и распрыжка.
        let capped = wish_speed.min(self.s.air_speed_cap);
        let current = self.st.velocity.dot(wish_dir);
        let add = capped - current;
        if add <= 0.0 {
            return;
        }
        let amount = (accel * wish_speed * self.dt).min(add);
        self.st.velocity += wish_dir * amount;
    }

    fn clamp_velocity(&mut self) {
        let max = self.s.max_velocity.max(0.0);
        self.st.velocity = self.st.velocity.clamp(Vec3::splat(-max), Vec3::splat(max));
    }

    // ------------------------------------------------------------------
    // Перемещение
    // ------------------------------------------------------------------

    fn walk_move(&mut self, wish_dir: Vec3, wish_speed: f32) {
        self.st.velocity.y = 0.0;
        self.accelerate(wish_dir, wish_speed, self.s.accelerate);
        self.st.velocity.y = 0.0;
        if self.st.velocity.length() < 1.0 * UNIT {
            self.st.velocity = Vec3::ZERO;
            return;
        }
        let dest = self.st.origin + self.st.velocity * self.dt;
        let trace = self.trace(self.st.origin, dest);
        if trace.fraction >= 1.0 {
            self.st.origin = trace.end;
        } else {
            self.step_move();
        }
        self.stay_on_ground();
    }

    fn air_move(&mut self, wish_dir: Vec3, wish_speed: f32) {
        self.air_accelerate(wish_dir, wish_speed, self.s.air_accelerate);
        self.try_player_move();
    }

    /// Скольжение с до четырёх касаний за шаг: после каждого скорость срезается о
    /// плоскость касания (и о все плоскости, накопленные с последнего продвижения).
    fn try_player_move(&mut self) -> u32 {
        let mut blocked = 0;
        let mut planes = [Vec3::ZERO; MAX_CLIP_PLANES];
        let mut num_planes = 0;
        let primal = self.st.velocity;
        let mut original = self.st.velocity;
        let mut time_left = self.dt;
        let mut all_fraction = 0.0;
        for _ in 0..MAX_BUMPS {
            if self.st.velocity.length_squared() < 1e-12 {
                break;
            }
            let end = self.st.origin + self.st.velocity * time_left;
            let trace = self.trace(self.st.origin, end);
            all_fraction += trace.fraction;
            if trace.fraction > 0.0 {
                self.st.origin = trace.end;
                original = self.st.velocity;
                num_planes = 0;
            } else if trace.object.is_some() {
                // Касание без продвижения: всё равно держим зазор.
                self.st.origin = trace.end;
            }
            if trace.object.is_none() {
                break;
            }
            if trace.normal.y > self.s.walkable_normal {
                blocked |= 1;
            }
            if trace.normal.y.abs() < 1e-3 {
                blocked |= 2;
            }
            time_left -= time_left * trace.fraction;
            if num_planes >= MAX_CLIP_PLANES {
                self.st.velocity = Vec3::ZERO;
                break;
            }
            planes[num_planes] = trace.normal;
            num_planes += 1;

            if num_planes == 1 && !self.st.on_ground {
                let clipped = clip_velocity(original, planes[0], 1.0);
                self.st.velocity = clipped;
                original = clipped;
            } else {
                let mut found = None;
                for i in 0..num_planes {
                    let v = clip_velocity(original, planes[i], 1.0);
                    if (0..num_planes).all(|j| j == i || v.dot(planes[j]) >= 0.0) {
                        found = Some(v);
                        break;
                    }
                }
                match found {
                    Some(v) => self.st.velocity = v,
                    None => {
                        if num_planes != 2 {
                            self.st.velocity = Vec3::ZERO;
                            break;
                        }
                        // Вдоль ребра, где сходятся две плоскости.
                        let crease = planes[0].cross(planes[1]).normalize_or_zero();
                        self.st.velocity = crease * crease.dot(self.st.velocity);
                    }
                }
                // Скорость развернулась против исходной — стоп (не дрожать в углах).
                if self.st.velocity.dot(primal) <= 0.0 {
                    self.st.velocity = Vec3::ZERO;
                    break;
                }
            }
        }
        if all_fraction == 0.0 {
            self.st.velocity = Vec3::ZERO;
        }
        blocked
    }

    /// Шаг с подъёмом на ступеньку: пробуем просто скользить и «подняться на `step_size`,
    /// скользить, опуститься», берём то, что увело дальше по горизонтали.
    fn step_move(&mut self) {
        let start = self.st.origin;
        let start_velocity = self.st.velocity;

        self.try_player_move();
        let down_origin = self.st.origin;
        let down_velocity = self.st.velocity;

        self.st.origin = start;
        self.st.velocity = start_velocity;
        let up = self.trace(start, start + Vec3::Y * self.s.step_size);
        self.st.origin = up.end;
        self.try_player_move();
        let step_down = self.trace(self.st.origin, self.st.origin - Vec3::Y * self.s.step_size);
        self.st.origin = step_down.end;

        // Не встали на ровное — ступеньки не было: берём обычное скольжение.
        if step_down.object.is_none() || step_down.normal.y < self.s.walkable_normal {
            self.st.origin = down_origin;
            self.st.velocity = down_velocity;
            return;
        }
        let horizontal = |p: Vec3| Vec2::new(p.x - start.x, p.z - start.z).length_squared();
        if horizontal(down_origin) > horizontal(self.st.origin) {
            self.st.origin = down_origin;
            self.st.velocity = down_velocity;
        } else {
            self.st.velocity.y = down_velocity.y;
        }
    }

    /// Прижаться к земле, идя вниз по ступенькам и склонам (не отрываясь на каждой).
    fn stay_on_ground(&mut self) {
        let origin = self.st.origin;
        let up = self.trace(origin, origin + Vec3::Y * GROUND_PROBE);
        let start = up.end;
        let trace = self.trace(start, origin - Vec3::Y * self.s.step_size);
        if trace.object.is_some()
            && trace.fraction > 0.0
            && trace.fraction < 1.0
            && !trace.start_solid
            && trace.normal.y >= self.s.walkable_normal
            && (origin.y - trace.end.y).abs() > 0.0003
        {
            self.st.origin = trace.end;
        }
    }

    /// Земля под ногами: короткая трасса вниз; «земля» — если нормаль достаточно пологая.
    fn categorize(&mut self) {
        let was_on_ground = self.st.on_ground;
        if self.st.velocity.y > NON_JUMP_VELOCITY {
            self.st.on_ground = false;
            self.st.ground_object = None;
            return;
        }
        let origin = self.st.origin;
        let trace = self.trace(origin, origin - Vec3::Y * GROUND_PROBE);
        match trace.object {
            Some(object) if trace.normal.y >= self.s.walkable_normal => {
                // Встаём ровно на зазор над землёй: иначе приземление «на 2 ед выше» дало
                // бы следующий прыжок выше обычного.
                self.st.origin = trace.end;
                self.st.on_ground = true;
                self.st.ground_normal = trace.normal;
                self.st.ground_object = Some(object);
                if !was_on_ground {
                    self.st.landed = true;
                }
            }
            _ => {
                self.st.on_ground = false;
                self.st.ground_object = None;
            }
        }
    }

    /// Свободный полёт сквозь стены (отладка).
    fn fly(&mut self, cmd: &UserCmd) {
        let (_, right) = yaw_vectors(cmd.yaw_deg);
        let view = view_direction(cmd.yaw_deg, cmd.pitch_deg);
        let wish = view * cmd.forward.clamp(-1.0, 1.0) + right * cmd.side.clamp(-1.0, 1.0) + Vec3::Y * cmd.up.clamp(-1.0, 1.0);
        let amount = wish.length().min(1.0);
        let mut speed = self.s.noclip_speed * amount;
        if cmd.walk {
            speed *= self.s.walk_speed_scale;
        }
        self.st.velocity = wish.normalize_or_zero() * speed;
        self.st.origin += self.st.velocity * self.dt;
        self.st.on_ground = false;
        self.st.ground_object = None;
    }
}
