//! Короткие формы для скриптов: `ctx.pos()`, `ctx.key(Key::W)`, `ctx.me_mut::<T>()`,
//! `ctx.find("Game")`, `ctx.spawn("prefabs/coin.prefab")`, `ctx.delay(1.0, |ctx| …)` и т. д.
//!
//! Каждая — тонкая обёртка над тем, что уже есть в [`Ctx`]; длинные формы (`transform()`,
//! `get_mut`, `find_by_name`, `instantiate_at`…) остаются и работают как раньше, скрипт
//! может смешивать обе. Таблица «старая форма ↔ короткая» — `docs/api/gameplay.md`.
//!
//! Короткие формы не паникуют и не возвращают `Option` там, где новичку это мешает:
//! если вещь не нашлась (нет компонента, нет объекта с таким именем, неизвестное действие
//! ввода), в лог один раз на место вызова уходит **обучающая ошибка** — что искали, на
//! каком объекте, что есть рядом («did you mean …») и как починить, — а вызов ничего не
//! меняет. Объект, которого нет, — это [`GameObjectId::NONE`]: любые короткие формы с
//! ним безопасно ничего не делают.
//!
//! ```
//! use engine_core::prelude::*;
//!
//! component! {
//!     /// Едет по стрелкам / WASD.
//!     pub struct Walker { pub speed: f32 = 3.0 }
//! }
//! impl Component for Walker {
//!     fn update(&mut self, ctx: &mut Ctx) {
//!         let dir = ctx.axis2("Horizontal", "Vertical");
//!         ctx.move_by(dir * self.speed * ctx.dt());
//!     }
//! }
//!
//! let mut app = App::new();
//! app.spawn("Hero").with(Walker::default());
//! app.world.input.set_key(Key::D, true);
//! app.run_headless(10, 0.1);
//! let hero = app.world.scene.find_by_name("Hero").unwrap();
//! assert!((app.world.scene.world_position(hero).x - 3.0).abs() < 1e-4);
//! ```

use std::collections::HashSet;
use std::panic::Location;

use crate::component::{Component, Ctx, Message};
use crate::error::did_you_mean;
use crate::input::{Key, MouseButton};
use crate::math::{Quat, Vec2, Vec3};
use crate::object::GameObjectId;
use crate::objref::ObjectRef;
use crate::reflect::{FieldValue, TypeName, Value};
use crate::schedule::TimerId;
use crate::tween::{Ease, Tween, TweenId};
use crate::world::World;

// ----------------------------------------------------------------------
// Векторы: одна функция для 2D и 3D
// ----------------------------------------------------------------------

/// Что можно передать туда, где ждут точку или смещение: `Vec3`, `Vec2` (z остаётся
/// прежним — так 2D-скрипт не сдвигает спрайт по глубине), `(x, y)` или `(x, y, z)`.
pub trait AsVec3: Copy {
    /// Вектор в 3D; `z` — что подставить, если у значения нет своей третьей координаты.
    fn with_z(self, z: f32) -> Vec3;
}

impl AsVec3 for Vec3 {
    fn with_z(self, _z: f32) -> Vec3 {
        self
    }
}

impl AsVec3 for Vec2 {
    fn with_z(self, z: f32) -> Vec3 {
        self.extend(z)
    }
}

impl AsVec3 for (f32, f32) {
    fn with_z(self, z: f32) -> Vec3 {
        Vec3::new(self.0, self.1, z)
    }
}

impl AsVec3 for (f32, f32, f32) {
    fn with_z(self, _z: f32) -> Vec3 {
        Vec3::new(self.0, self.1, self.2)
    }
}

/// Поворот для [`Ctx::rotate_by`]: число — градусы вокруг Z (2D), `Vec3` — углы
/// Эйлера в градусах (X, Y, Z), `Quat` — как есть.
pub trait AsRotation: Copy {
    fn to_quat(self) -> Quat;
}

impl AsRotation for f32 {
    fn to_quat(self) -> Quat {
        Quat::from_rotation_z(self.to_radians())
    }
}

impl AsRotation for Vec3 {
    fn to_quat(self) -> Quat {
        Quat::from_euler(
            crate::math::EulerRot::YXZ,
            self.y.to_radians(),
            self.x.to_radians(),
            self.z.to_radians(),
        )
    }
}

impl AsRotation for Quat {
    fn to_quat(self) -> Quat {
        self
    }
}

// ----------------------------------------------------------------------
// Обучающие ошибки
// ----------------------------------------------------------------------

/// Обучающие ошибки коротких форм (ресурс мира). Каждая печатается в лог (окно Console)
/// один раз на место вызова — не каждый кадр. Тесты и редактор читают их отсюда.
#[derive(Debug, Default)]
pub struct ScriptErrors {
    seen: HashSet<String>,
    messages: Vec<String>,
}

impl ScriptErrors {
    /// Все сообщения с начала игры, по порядку.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Последнее сообщение.
    pub fn last(&self) -> Option<&str> {
        self.messages.last().map(String::as_str)
    }

    /// Забыть, что уже сообщалось (новый запуск Play).
    pub fn clear(&mut self) {
        self.seen.clear();
        self.messages.clear();
    }
}

/// Сообщить обучающую ошибку один раз для места `site` и ключа `what`.
pub fn teach(world: &mut World, site: &Location<'_>, what: &str, message: impl FnOnce() -> String) {
    let key = format!("{}:{}:{}|{what}", site.file(), site.line(), site.column());
    let errors = world.resources.get_or_insert_with(ScriptErrors::default);
    if !errors.seen.insert(key) {
        return;
    }
    let text = format!("{} ({}:{})", message(), site.file(), site.line());
    log::error!(target: "script", "{text}");
    errors.messages.push(text);
}

/// Значения-«заглушки» для `me_mut`, когда компонента нет: запись в них ничего не меняет.
#[derive(Default)]
struct Scratch {
    values: crate::world::Resources,
}

fn scratch<T: Default + 'static>(world: &mut World) -> &mut T {
    let store = world.resources.get_or_insert_with(Scratch::default);
    let value = store.values.get_or_insert_with(T::default);
    *value = T::default();
    value
}

/// Имена компонентов объекта (для подсказок).
fn component_names(world: &World, id: GameObjectId) -> Vec<&'static str> {
    world
        .scene
        .get(id)
        .map(|o| o.components().map(|c| c.type_name()).collect())
        .unwrap_or_default()
}

fn list(names: &[&str]) -> String {
    if names.is_empty() {
        "nothing".to_string()
    } else {
        names.join(", ")
    }
}

// ----------------------------------------------------------------------
// Короткие формы
// ----------------------------------------------------------------------

impl Ctx<'_> {
    /// Имя этого объекта (или `"<destroyed object>"`).
    pub fn name(&self) -> &str {
        self.name_of(self.id)
    }

    /// Имя объекта `id` (или `"<destroyed object>"` / `"<none>"`).
    pub fn name_of(&self, id: GameObjectId) -> &str {
        match self.world.scene.get(id) {
            Some(object) => &object.name,
            None if id == GameObjectId::NONE => "<none>",
            None => "<destroyed object>",
        }
    }

    // ----- время -----

    /// Длительность текущего шага в секундах — то же `dt`, что приходит в длинные хуки:
    /// в `fixed_update` это шаг физики, в остальных — кадр (с учётом `time_scale`).
    pub fn dt(&self) -> f32 {
        self.world.phase_dt()
    }

    /// Секунд игрового времени с запуска (Unity: `Time.time`).
    pub fn elapsed(&self) -> f32 {
        self.world.time.elapsed as f32
    }

    // ----- Transform -----

    /// Мировая позиция объекта (Unity: `transform.position`).
    pub fn pos(&self) -> Vec3 {
        self.world.scene.world_position(self.id)
    }

    /// Мировая позиция в 2D (x, y).
    pub fn pos2(&self) -> Vec2 {
        self.pos().truncate()
    }

    /// Поставить объект в мировую точку: `ctx.set_pos(Vec2::new(0.0, 3.0))` (z сохраняется)
    /// или `ctx.set_pos(Vec3::new(1.0, 0.0, 2.0))`.
    pub fn set_pos(&mut self, position: impl AsVec3) {
        let id = self.id;
        set_world_position(self.world, id, position);
    }

    /// Сдвинуть на `delta` (в координатах родителя): `ctx.move_by(Vec2::X * speed * ctx.dt())`.
    pub fn move_by(&mut self, delta: impl AsVec3) {
        if let Some(object) = self.world.scene.get_mut(self.id) {
            object.transform.position += delta.with_z(0.0);
        }
    }

    /// Повернуть: число — градусы вокруг Z (2D), `Vec3` — градусы Эйлера (3D).
    /// `ctx.rotate_by(90.0 * ctx.dt())` — четверть оборота в секунду.
    pub fn rotate_by(&mut self, rotation: impl AsRotation) {
        if let Some(object) = self.world.scene.get_mut(self.id) {
            let transform = &mut object.transform;
            transform.rotation = (transform.rotation * rotation.to_quat()).normalize();
        }
    }

    /// Угол поворота вокруг Z в градусах (2D).
    pub fn angle(&self) -> f32 {
        self.world
            .scene
            .get(self.id)
            .map_or(0.0, |o| o.transform.angle_z().to_degrees())
    }

    /// Задать угол вокруг Z в градусах (2D).
    pub fn set_angle(&mut self, degrees: f32) {
        if let Some(object) = self.world.scene.get_mut(self.id) {
            object.transform.set_angle_z(degrees.to_radians());
        }
    }

    /// Локальный масштаб.
    pub fn scale(&self) -> Vec3 {
        self.world.scene.get(self.id).map_or(Vec3::ONE, |o| o.transform.scale)
    }

    /// Задать локальный масштаб: `ctx.set_scale(Vec3::splat(2.0))`; `Vec2` оставляет z.
    pub fn set_scale(&mut self, scale: impl AsVec3) {
        if let Some(object) = self.world.scene.get_mut(self.id) {
            let z = object.transform.scale.z;
            object.transform.scale = scale.with_z(z);
        }
    }

    /// Развернуть «вперёд» (−Z) на мировую точку `target` (3D).
    pub fn look_at(&mut self, target: impl AsVec3) {
        let target = target.with_z(self.pos().z);
        let Some(parent) = self.world.scene.get(self.id).map(|o| o.parent()) else { return };
        let world_pos = self.pos();
        let dir = target - world_pos;
        if dir.length_squared() < 1e-12 {
            return;
        }
        let up = if dir.normalize().cross(Vec3::Y).length_squared() < 1e-6 { Vec3::Z } else { Vec3::Y };
        let world_rot = Quat::from_mat4(&crate::math::Mat4::look_to_rh(Vec3::ZERO, dir, up)).inverse();
        let parent_rot = parent.map_or(Quat::IDENTITY, |p| self.world.scene.world_rotation(p));
        if let Some(object) = self.world.scene.get_mut(self.id) {
            object.transform.rotation = (parent_rot.inverse() * world_rot).normalize();
        }
    }

    /// Мировое направление «вперёд» (−Z).
    pub fn forward(&self) -> Vec3 {
        self.world.scene.world_rotation(self.id) * Vec3::NEG_Z
    }

    /// Мировое направление «вправо» (+X).
    pub fn right(&self) -> Vec3 {
        self.world.scene.world_rotation(self.id) * Vec3::X
    }

    /// Мировое направление «вверх» (+Y).
    pub fn up(&self) -> Vec3 {
        self.world.scene.world_rotation(self.id) * Vec3::Y
    }

    /// Плавно переехать в локальную точку за `seconds` (твин с мягким стартом и концом).
    pub fn move_to(&mut self, position: impl AsVec3, seconds: f32) -> TweenId {
        let z = self.world.scene.get(self.id).map_or(0.0, |o| o.transform.position.z);
        let tween = Tween::position(self.id, position.with_z(z))
            .duration(seconds)
            .ease(Ease::InOutQuad);
        self.world.start_tween(tween)
    }

    /// Плавно изменить масштаб за `seconds`.
    pub fn scale_to(&mut self, scale: impl AsVec3, seconds: f32) -> TweenId {
        let z = self.scale().z;
        let tween = Tween::scale(self.id, scale.with_z(z)).duration(seconds).ease(Ease::InOutQuad);
        self.world.start_tween(tween)
    }

    // ----- ввод -----

    /// Клавиша зажата (Unity: `Input.GetKey`).
    pub fn key(&self, key: Key) -> bool {
        self.world.input.key(key)
    }

    /// Клавишу нажали в этом кадре (Unity: `GetKeyDown`).
    pub fn key_pressed(&self, key: Key) -> bool {
        self.world.input.key_down(key)
    }

    /// Клавишу отпустили в этом кадре (Unity: `GetKeyUp`).
    pub fn key_released(&self, key: Key) -> bool {
        self.world.input.key_up(key)
    }

    /// Позиция мыши в пикселях окна (0,0 — левый верхний угол).
    pub fn mouse(&self) -> Vec2 {
        self.world.input.mouse_position()
    }

    /// Кнопку мыши нажали в этом кадре: `ctx.mouse_pressed(MouseButton::Left)`.
    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.world.input.mouse_button_down(button)
    }

    /// Кнопка мыши зажата.
    pub fn mouse_held(&self, button: MouseButton) -> bool {
        self.world.input.mouse_button(button)
    }

    /// Ось действия −1..1 (`"Horizontal"`, `"Vertical"` или ваша из Project Settings →
    /// Input): клавиатура, крестовина и стик сразу. Неизвестное имя — обучающая ошибка и 0.
    #[track_caller]
    pub fn axis(&mut self, name: &str) -> f32 {
        if !self.world.input.has_axis(name) {
            self.teach_unknown_action(Location::caller(), name, true);
            return 0.0;
        }
        self.world.input.axis(name)
    }

    /// Две оси сразу, длина не больше 1: `ctx.axis2("Horizontal", "Vertical")`.
    #[track_caller]
    pub fn axis2(&mut self, x: &str, y: &str) -> Vec2 {
        let site = Location::caller();
        let mut read = |name: &str| {
            if self.world.input.has_axis(name) {
                self.world.input.axis(name)
            } else {
                self.teach_unknown_action(site, name, true);
                0.0
            }
        };
        let (vx, vy) = (read(x), read(y));
        let v = Vec2::new(vx, vy);
        if v.length_squared() > 1.0 {
            v.normalize()
        } else {
            v
        }
    }

    /// Действие-кнопку нажали в этом кадре: `if ctx.pressed("Jump") { … }`. Клавиатура,
    /// мышь и геймпад — через привязки действия (Project Settings → Input).
    #[track_caller]
    pub fn pressed(&mut self, action: &str) -> bool {
        if !self.world.input.has_button(action) {
            self.teach_unknown_action(Location::caller(), action, false);
            return false;
        }
        self.world.input.button_down(action)
    }

    /// Действие-кнопка удерживается.
    #[track_caller]
    pub fn held(&mut self, action: &str) -> bool {
        if !self.world.input.has_button(action) {
            self.teach_unknown_action(Location::caller(), action, false);
            return false;
        }
        self.world.input.button(action)
    }

    /// Действие-кнопку отпустили в этом кадре.
    #[track_caller]
    pub fn released(&mut self, action: &str) -> bool {
        if !self.world.input.has_button(action) {
            self.teach_unknown_action(Location::caller(), action, false);
            return false;
        }
        self.world.input.button_up(action)
    }

    fn teach_unknown_action(&mut self, site: &Location<'_>, name: &str, axis: bool) {
        let input = &self.world.input;
        let mut known: Vec<String> = input.axis_names().into_iter().map(str::to_string).collect();
        known.extend(input.button_names().into_iter().map(str::to_string));
        known.sort();
        let object = self.name().to_string();
        let (kind, other) = if axis { ("axis", "button") } else { ("button", "axis") };
        let is_other = if axis { input.has_button(name) } else { input.has_axis(name) };
        teach(self.world, site, &format!("action:{name}"), || {
            let mut text = format!("{object}: there is no input {kind} `{name}`.");
            if is_other {
                text.push_str(&format!(
                    " `{name}` is an {other}: read it with {}.",
                    if axis { "ctx.pressed / ctx.held" } else { "ctx.axis" }
                ));
            } else {
                text.push_str(&did_you_mean(name, known.iter().map(String::as_str)));
            }
            text.push_str(&format!(
                " Known actions: {}. Add new ones in Project Settings → Input.",
                list(&known.iter().map(String::as_str).collect::<Vec<_>>())
            ));
            text
        });
    }

    // ----- компоненты -----

    /// Есть ли на объекте компонент `T`.
    pub fn has<T: Component>(&self) -> bool {
        self.world.scene.get(self.id).is_some_and(|o| o.has::<T>())
    }

    /// Компонент `T` этого объекта для чтения: `ctx.me::<Health>().hp`. Нет такого —
    /// обучающая ошибка (один раз) и значения по умолчанию.
    #[track_caller]
    pub fn me<T: Component + Default + TypeName>(&mut self) -> &T {
        self.me_mut::<T>()
    }

    /// Компонент `T` этого объекта для записи: `ctx.me_mut::<Rigidbody2D>().velocity.x = 5.0`.
    /// Нет такого — обучающая ошибка (один раз) и «заглушка»: запись в неё ничего не
    /// меняет, игра идёт дальше. Длинная форма с `Option` — [`get_mut`](Ctx::get_mut).
    #[track_caller]
    pub fn me_mut<T: Component + Default + TypeName>(&mut self) -> &mut T {
        let id = self.id;
        if self.world.scene.get(id).is_some_and(|o| o.has::<T>()) {
            return self
                .world
                .scene
                .get_mut(id)
                .and_then(|o| o.get_mut::<T>())
                .expect("checked above");
        }
        let site = Location::caller();
        self.teach_missing::<T>(site, id, "ctx.me_mut");
        scratch::<T>(self.world)
    }

    /// Сделать что-то с компонентом `T`, если он есть: `ctx.with::<Light>(|l| l.intensity = 2.0)`.
    /// Нет компонента — обучающая ошибка (один раз) и `None`.
    #[track_caller]
    pub fn with<T: Component + TypeName, R>(&mut self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let id = self.id;
        if let Some(component) = self.world.scene.get_mut(id).and_then(|o| o.get_mut::<T>()) {
            return Some(f(component));
        }
        self.teach_missing::<T>(Location::caller(), id, "ctx.with");
        None
    }

    /// Добавить компонент на этот объект и вернуть его: `ctx.add(Light::default())`.
    pub fn add<T: Component>(&mut self, component: T) -> Option<&mut T> {
        self.world.scene.get_mut(self.id).map(|o| o.add(component))
    }

    fn teach_missing<T: Component + TypeName>(&mut self, site: &Location<'_>, id: GameObjectId, call: &str) {
        let wanted = T::TYPE_NAME;
        let object = self.name_of(id).to_string();
        if self.world.scene.get(id).is_none() {
            teach(self.world, site, &format!("dead:{wanted}"), || {
                format!(
                    "{call}::<{wanted}>(): the object is {object}, so it has no components. \
                     Check `ctx.exists(id)` before using an object that may be gone"
                )
            });
            return;
        }
        let here = component_names(self.world, id);
        // Свой собственный компонент на время хука «вынут» из объекта: подсказать `self`.
        let running_self = self.world.scene.get(id).is_some_and(|o| {
            o.components.iter().any(|slot| !slot.removed && slot.comp.is_none() && slot.name == wanted)
        });
        let mut candidates: Vec<&str> = here.clone();
        candidates.extend(self.world.registry.names());
        let hint = did_you_mean(wanted, candidates.iter().copied().filter(|c| *c != wanted));
        teach(self.world, site, &format!("missing:{wanted}"), || {
            let mut text = format!("`{object}` has no `{wanted}` component ({call}::<{wanted}>()).");
            if running_self {
                text.push_str(&format!(
                    " If `{wanted}` is this script itself, use `self` instead: a script is taken out of its object while its hook runs."
                ));
            }
            text.push_str(&format!(" It has: Transform, {}.", list(&here)));
            text.push_str(&hint);
            text.push_str(&format!(
                " Add it in the Inspector (Add Component → {wanted}) or in code (`ctx.add({wanted}::default())`); until then this call changes nothing"
            ));
            text
        });
    }

    // ----- другие объекты -----

    /// Есть ли (ещё) объект `id`.
    pub fn exists(&self, id: GameObjectId) -> bool {
        self.world.scene.get(id).is_some()
    }

    /// Объект по имени (Unity: `GameObject.Find`). Нет такого — обучающая ошибка
    /// (один раз, с похожими именами) и [`GameObjectId::NONE`]. Проверить без ошибки —
    /// [`find_by_name`](Ctx::find_by_name).
    #[track_caller]
    pub fn find(&mut self, name: &str) -> GameObjectId {
        if let Some(id) = self.world.scene.find_by_name(name) {
            return id;
        }
        let names: Vec<String> = self
            .world
            .scene
            .ids()
            .into_iter()
            .filter_map(|id| self.world.scene.get(id).map(|o| o.name.clone()))
            .collect();
        let me = self.name().to_string();
        teach(self.world, Location::caller(), &format!("find:{name}"), || {
            format!(
                "{me}: ctx.find(\"{name}\") found no object named `{name}`.{} Names are case-sensitive; \
                 the result is GameObjectId::NONE, and calls with it do nothing",
                did_you_mean(name, names.iter().map(String::as_str))
            )
        });
        GameObjectId::NONE
    }

    /// Первый объект с тегом (Unity: `FindWithTag`). Нет — обучающая ошибка и `NONE`.
    #[track_caller]
    pub fn find_tag(&mut self, tag: &str) -> GameObjectId {
        if let Some(id) = self.world.scene.find_by_tag(tag) {
            return id;
        }
        let mut tags: Vec<String> = self
            .world
            .scene
            .ids()
            .into_iter()
            .filter_map(|id| self.world.scene.get(id).map(|o| o.tag.clone()))
            .filter(|t| !t.is_empty())
            .collect();
        tags.sort();
        tags.dedup();
        let me = self.name().to_string();
        teach(self.world, Location::caller(), &format!("tag:{tag}"), || {
            format!(
                "{me}: ctx.find_tag(\"{tag}\") found no object with tag `{tag}`.{} Tags in the scene: {}",
                did_you_mean(tag, tags.iter().map(String::as_str)),
                list(&tags.iter().map(String::as_str).collect::<Vec<_>>())
            )
        });
        GameObjectId::NONE
    }

    /// Все объекты с тегом.
    pub fn find_all_tag(&self, tag: &str) -> Vec<GameObjectId> {
        self.world.scene.find_all_by_tag(tag)
    }

    /// Первый объект с компонентом `T` или `NONE` (без ошибки).
    pub fn first_with<T: Component>(&self) -> GameObjectId {
        self.world
            .scene
            .find_with::<T>()
            .into_iter()
            .next()
            .unwrap_or(GameObjectId::NONE)
    }

    /// Все объекты с компонентом `T` (то же, что [`find_with`](Ctx::find_with)).
    pub fn all_with<T: Component>(&self) -> Vec<GameObjectId> {
        self.world.scene.find_with::<T>()
    }

    /// Объект из поля-ссылки (`pub target: ObjectRef`, назначается в инспекторе).
    /// Поле пустое или объект удалён — обучающая ошибка (один раз) и `NONE`.
    #[track_caller]
    pub fn obj(&mut self, reference: impl Into<ObjectRef>) -> GameObjectId {
        let reference = reference.into();
        if let Some(id) = self.world.scene.resolve(reference) {
            return id;
        }
        let me = self.name().to_string();
        let empty = reference == ObjectRef::NONE;
        teach(self.world, Location::caller(), "objref", || {
            if empty {
                format!(
                    "{me}: an object field is not assigned. Drag an object onto the field in the Inspector; \
                     until then ctx.obj(..) gives GameObjectId::NONE"
                )
            } else {
                format!("{me}: the object in this field was destroyed; ctx.obj(..) gives GameObjectId::NONE")
            }
        });
        GameObjectId::NONE
    }

    /// Контекст «как будто хук выполняется на объекте `id`»: все короткие формы — для
    /// него. `ctx.on(enemy).me_mut::<Health>().hp -= 1;`, `ctx.on(door).set_active_self(false);`
    /// Для объекта, которого нет, короткие формы ничего не делают (длинные `transform()`
    /// и `object()` требуют живой объект).
    pub fn on(&mut self, id: GameObjectId) -> Ctx<'_> {
        Ctx { world: &mut *self.world, id }
    }

    /// Тег объекта `id` (пустая строка, если тега или объекта нет).
    pub fn tag_of(&self, id: GameObjectId) -> &str {
        self.world.scene.get(id).map_or("", |o| o.tag.as_str())
    }

    /// Есть ли у объекта `id` тег `tag`: `if ctx.has_tag(other, "Player") { … }`.
    pub fn has_tag(&self, id: GameObjectId, tag: &str) -> bool {
        self.tag_of(id) == tag
    }

    // ----- сообщения -----

    /// Послать объекту сообщение без значения: `ctx.send(game, "Caught")`. Получатель
    /// ловит его в `on_message` (`if message.is("Caught")`).
    pub fn send(&mut self, target: GameObjectId, name: &str) {
        if self.world.scene.get(target).is_some() {
            self.world.send_message(target, name, Value::Nil);
        }
    }

    /// Послать сообщение со значением: `ctx.send_value(player, "Damage", 2)`.
    pub fn send_value<T: FieldValue>(&mut self, target: GameObjectId, name: &str, value: T) {
        if self.world.scene.get(target).is_some() {
            self.world.send_message(target, name, value.to_value());
        }
    }

    // ----- жизнь объектов -----

    /// Создать префаб в позиции этого объекта (Unity: `Instantiate`): `ctx.spawn("prefabs/coin.prefab")`.
    /// Возвращает новый объект; ошибка (нет файла…) — обучающая ошибка и `NONE`.
    #[track_caller]
    pub fn spawn(&mut self, prefab: &str) -> GameObjectId {
        let at = self.pos();
        self.spawn_at_site(Location::caller(), prefab, at, None)
    }

    /// Создать префаб в мировой точке: `ctx.spawn_at("prefabs/bullet.prefab", ctx.pos() + ctx.up())`.
    #[track_caller]
    pub fn spawn_at(&mut self, prefab: &str, position: impl AsVec3) -> GameObjectId {
        let at = position.with_z(0.0);
        self.spawn_at_site(Location::caller(), prefab, at, None)
    }

    /// Создать префаб ребёнком `parent` (в его начале координат).
    #[track_caller]
    pub fn spawn_child(&mut self, prefab: &str, parent: GameObjectId) -> GameObjectId {
        self.spawn_at_site(Location::caller(), prefab, Vec3::ZERO, Some(parent))
    }

    fn spawn_at_site(
        &mut self,
        site: &Location<'_>,
        prefab: &str,
        at: Vec3,
        parent: Option<GameObjectId>,
    ) -> GameObjectId {
        match self.world.instantiate_at(prefab, at, Quat::IDENTITY, parent) {
            Ok(id) => id,
            Err(error) => {
                let me = self.name().to_string();
                teach(self.world, site, &format!("spawn:{prefab}"), || {
                    format!(
                        "{me}: ctx.spawn(\"{prefab}\") failed: {error}. The path is relative to the assets folder, \
                         e.g. \"prefabs/coin.prefab\"; the result is GameObjectId::NONE"
                    )
                });
                GameObjectId::NONE
            }
        }
    }

    /// Включить или выключить этот объект с детьми (Unity: `gameObject.SetActive`).
    pub fn set_active_self(&mut self, active: bool) {
        let id = self.id;
        self.world.scene.set_active(id, active);
    }

    // ----- таймеры с замыканием -----

    /// Выполнить `f` через `seconds` секунд для этого объекта: `ctx.delay(2.0, |ctx| ctx.destroy_self())`.
    /// Умрёт вместе с объектом. Длинная форма с сообщением — [`after`](Ctx::after).
    pub fn delay(&mut self, seconds: f32, f: impl FnOnce(&mut Ctx) + 'static) -> TimerId {
        let owner = self.id;
        self.call_after(seconds, move |world| {
            if world.scene.get(owner).is_some() {
                f(&mut Ctx { world, id: owner });
            }
        })
    }

    /// Выполнять `f` каждые `seconds` секунд: `ctx.repeat(1.0, |ctx| { ctx.spawn("prefabs/enemy.prefab"); })`.
    /// Остановить — [`cancel_timer`](Ctx::cancel_timer) с возвращённым номером.
    pub fn repeat(&mut self, seconds: f32, mut f: impl FnMut(&mut Ctx) + 'static) -> TimerId {
        let owner = self.id;
        self.call_every(seconds, move |world| {
            if world.scene.get(owner).is_some() {
                f(&mut Ctx { world, id: owner });
            }
        })
    }

    // ----- сохранения -----

    /// Запомнить значение между запусками игры (Unity: `PlayerPrefs`) и сразу записать на
    /// диск: `ctx.save("best", self.best)`. Вызывайте, когда значение меняется, а не каждый кадр.
    pub fn save<T: FieldValue>(&mut self, key: &str, value: T) {
        let data = self.save_data();
        data.set(key, &value);
        if let Err(error) = data.save() {
            log::warn!(target: "script", "ctx.save(\"{key}\"): {error}");
        }
    }

    /// Прочитать сохранённое значение или `default`: `self.best = ctx.load("best", 0);`.
    pub fn load<T: FieldValue>(&mut self, key: &str, default: T) -> T {
        self.save_data().get::<T>(key).unwrap_or(default)
    }

    // ----- случайность -----

    /// Случайное число 0..1.
    pub fn rand(&mut self) -> f32 {
        self.random().f32()
    }

    /// Случайное число в `min..max`: `ctx.rand_range(-3.0, 3.0)`.
    pub fn rand_range(&mut self, min: f32, max: f32) -> f32 {
        self.random().range(min, max)
    }

    /// Случайное целое `min..=max` (обе границы входят): кубик — `ctx.rand_int(1, 6)`.
    pub fn rand_int(&mut self, min: i32, max: i32) -> i32 {
        self.random().int_range_inclusive(min, max)
    }

    // ----- лог -----

    /// Строка в окно Console с именем объекта: `ctx.log(format!("hp = {}", self.hp))`.
    pub fn log(&self, text: impl std::fmt::Display) {
        log::info!(target: "script", "[{}] {text}", self.name());
    }

    /// Предупреждение в Console с именем объекта.
    pub fn warn(&self, text: impl std::fmt::Display) {
        log::warn!(target: "script", "[{}] {text}", self.name());
    }
}

/// Поставить объект `id` в мировую точку (с учётом родителя).
pub(crate) fn set_world_position(world: &mut World, id: GameObjectId, position: impl AsVec3) {
    let Some(object) = world.scene.get(id) else { return };
    let parent = object.parent();
    let current = world.scene.world_position(id);
    let target = position.with_z(current.z);
    let local = match parent {
        Some(parent) => world.scene.world_matrix(parent).inverse().transform_point3(target),
        None => target,
    };
    if let Some(object) = world.scene.get_mut(id) {
        object.transform.position = local;
    }
}

impl Message {
    /// Это сообщение с именем `name`: `if message.is("Caught") { … }`.
    pub fn is(&self, name: &str) -> bool {
        self.name == name
    }

    /// Значение сообщения нужного типа: `message.get::<f32>()` (`None`, если тип другой).
    pub fn get<T: FieldValue>(&self) -> Option<T> {
        T::from_value(&self.value)
    }
}

// ----------------------------------------------------------------------
// Шпаргалка: один источник для окна Help → Scripting Cheat Sheet и для таблицы в
// docs/api/gameplay.md (тест сверяет их, поэтому они не расходятся).
// ----------------------------------------------------------------------

/// Строка шпаргалки по коротким формам.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatEntry {
    /// Раздел: `Hooks`, `Moving`, `Input`…
    pub topic: &'static str,
    /// Короткая форма.
    pub short: &'static str,
    /// Пример в одну строку.
    pub example: &'static str,
    /// Длинная форма, которую она оборачивает (обе остаются рабочими).
    pub wraps: &'static str,
}

const fn e(topic: &'static str, short: &'static str, example: &'static str, wraps: &'static str) -> CheatEntry {
    CheatEntry { topic, short, example, wraps }
}

/// Все короткие формы — по разделам, в порядке «от первого скрипта к сложному».
pub const CHEAT_SHEET: &[CheatEntry] = &[
    e("Game", "engine::game! { name, scene, components }", "engine::game! { name: \"My Game\", scene: \"scenes/main.scene\", components: [Player] }", "fn register + fn build_with + fn build + engine::game_main!(..)"),
    e("Hooks", "fn start(&mut self, ctx)", "fn start(&mut self, ctx: &mut Ctx) { ctx.log(\"hi\"); }", "fn on_start(&mut self, ctx)"),
    e("Hooks", "fn update(&mut self, ctx)", "fn update(&mut self, ctx: &mut Ctx) { ctx.move_by(Vec2::X * ctx.dt()); }", "fn on_update(&mut self, ctx, dt)"),
    e("Hooks", "fn fixed_update(&mut self, ctx)", "fn fixed_update(&mut self, ctx: &mut Ctx) { ctx.set_velocity(Vec2::X); }", "fn on_fixed_update(&mut self, ctx, dt)"),
    e("Hooks", "fn late_update(&mut self, ctx)", "fn late_update(&mut self, ctx: &mut Ctx) { /* camera */ }", "fn on_late_update(&mut self, ctx, dt)"),
    e("Hooks", "fn on_collision(&mut self, ctx, other)", "if ctx.has_tag(other, \"Enemy\") { ctx.destroy_self(); }", "fn on_collision_enter(&mut self, ctx, other)"),
    e("Hooks", "fn on_trigger(&mut self, ctx, other)", "if ctx.has_tag(other, \"Player\") { ctx.play(\"coin\"); }", "fn on_trigger_enter(&mut self, ctx, other)"),
    e("Hooks", "fn on_click(&mut self, ctx)", "fn on_click(&mut self, ctx: &mut Ctx) { ctx.load_scene(\"level1\"); }", "on_message from a Button / ctx.ui().clicked(..)"),
    e("Hooks", "message.is(name) / message.get::<T>()", "if message.is(\"Damage\") { self.hp -= message.get::<i32>().unwrap_or(1); }", "message.name == .. / T::from_value(&message.value)"),
    e("Time", "ctx.dt()", "ctx.move_by(Vec2::X * 3.0 * ctx.dt())", "the dt parameter / ctx.time().delta"),
    e("Time", "ctx.elapsed()", "let wave = ctx.elapsed().sin();", "ctx.time().elapsed as f32"),
    e("Moving", "ctx.pos() / ctx.pos2()", "let p = ctx.pos2();", "ctx.world_position()"),
    e("Moving", "ctx.set_pos(p)", "ctx.set_pos(Vec2::new(0.0, 3.0))", "ctx.transform().position = .. (local)"),
    e("Moving", "ctx.move_by(delta)", "ctx.move_by(Vec2::new(speed * ctx.dt(), 0.0))", "ctx.transform().translate(..)"),
    e("Moving", "ctx.rotate_by(deg)", "ctx.rotate_by(90.0 * ctx.dt())", "ctx.transform().rotate_z(radians)"),
    e("Moving", "ctx.angle() / ctx.set_angle(deg)", "ctx.set_angle(45.0)", "transform().angle_z() / set_angle_z(radians)"),
    e("Moving", "ctx.scale() / ctx.set_scale(s)", "ctx.set_scale(Vec2::splat(2.0))", "ctx.transform().scale"),
    e("Moving", "ctx.look_at(point)", "ctx.look_at(target_pos)", "ctx.transform().look_at(point, Vec3::Y)"),
    e("Moving", "ctx.forward() / right() / up()", "ctx.move_by(ctx.forward() * ctx.dt())", "scene.world_rotation(id) * Vec3::NEG_Z"),
    e("Moving", "ctx.move_to(p, secs) / scale_to", "ctx.move_to(Vec2::new(4.0, 0.0), 0.5);", "ctx.start_tween(Tween::position(id, p).duration(secs))"),
    e("Input", "ctx.key(Key::W) / key_pressed / key_released", "if ctx.key_pressed(Key::Space) { .. }", "ctx.input().key(..) / key_down / key_up"),
    e("Input", "ctx.axis(\"Horizontal\")", "let run = ctx.axis(\"Horizontal\");", "ctx.input().axis(..)"),
    e("Input", "ctx.axis2(x, y)", "let dir = ctx.axis2(\"Horizontal\", \"Vertical\");", "Vec2::new(input.axis(x), input.axis(y))"),
    e("Input", "ctx.pressed(\"Jump\") / held / released", "if ctx.pressed(\"Jump\") { .. }", "ctx.input().button_down / button / button_up"),
    e("Input", "ctx.mouse() / mouse_pressed(b) / mouse_held(b)", "if ctx.mouse_pressed(MouseButton::Left) { .. }", "ctx.input().mouse_position() / mouse_button_down"),
    e("Screen", "ctx.view_rect()", "if ctx.pos().y < ctx.view_rect().y { ctx.destroy_self(); }", "camera.ortho_size × screen aspect, by hand"),
    e("Screen", "ctx.pointer_world() / pointer_held()", "if ctx.pointer_held() { let target = ctx.pointer_world(); }", "touches() / mouse_position() + camera math + ui().pointer_over_ui()"),
    e("Screen", "ctx.screen_to_world(pixels)", "let p = ctx.screen_to_world(ctx.mouse());", "Unity: Camera.main.ScreenToWorldPoint, by hand"),
    e("Components", "ctx.me::<T>() / ctx.me_mut::<T>()", "ctx.me_mut::<SpriteRenderer>().flip_x = true;", "ctx.get::<T>() / get_mut::<T>() (Option)"),
    e("Components", "ctx.with::<T, _>(|t| ..)", "ctx.with::<Light, _>(|l| l.intensity = 2.0);", "if let Some(t) = ctx.get_mut::<T>() { .. }"),
    e("Components", "ctx.has::<T>() / ctx.add(component)", "if !ctx.has::<Light>() { ctx.add(Light::default()); }", "ctx.object().has / object_mut().add"),
    e("Objects", "ctx.find(\"Name\")", "let game = ctx.find(\"Game\");", "ctx.find_by_name(..) (Option)"),
    e("Objects", "ctx.find_tag(\"Tag\") / find_all_tag", "let player = ctx.find_tag(\"Player\");", "ctx.find_by_tag / find_all_by_tag"),
    e("Objects", "ctx.first_with::<T>() / all_with::<T>()", "for coin in ctx.all_with::<Coin>() { .. }", "ctx.find_with::<T>()"),
    e("Objects", "ctx.obj(self.target)", "let target = ctx.obj(self.target);", "ctx.resolve(self.target) (Option)"),
    e("Objects", "ctx.on(id).…", "ctx.on(enemy).me_mut::<Health>().hp -= 1;", "ctx.get_on_mut::<T>(id), scene.get_mut(id)"),
    e("Objects", "ctx.exists(id) / has_tag(id, tag) / name_of(id)", "if ctx.exists(target) { .. }", "ctx.scene().get(id).is_some() / .tag == .."),
    e("Messages", "ctx.send(id, \"Name\")", "ctx.send(game, \"Caught\");", "ctx.send_message(id, name, Value::Nil)"),
    e("Messages", "ctx.send_value(id, \"Name\", v)", "ctx.send_value(player, \"Damage\", 2);", "ctx.send_message(id, name, Value::Int(2))"),
    e("Lifetime", "ctx.spawn(prefab) / spawn_at / spawn_child", "let coin = ctx.spawn_at(\"prefabs/coin.prefab\", ctx.pos());", "ctx.instantiate_at(..) (Result)"),
    e("Lifetime", "ctx.destroy_self() / destroy(id) / destroy_self_after(s)", "ctx.destroy_self_after(2.0);", "(unchanged)"),
    e("Lifetime", "ctx.set_active_self(on)", "ctx.on(door).set_active_self(false);", "ctx.set_active(ctx.id, on)"),
    e("Timers", "ctx.delay(secs, |ctx| ..)", "ctx.delay(1.0, |ctx| ctx.destroy_self());", "ctx.call_after(secs, |world| ..) / ctx.after(secs, msg, value)"),
    e("Timers", "ctx.repeat(secs, |ctx| ..)", "ctx.repeat(2.0, |ctx| { ctx.spawn(\"prefabs/enemy.prefab\"); });", "ctx.call_every / ctx.every(secs, msg, value)"),
    e("Timers", "Timer / Cooldown fields", "pub shot: Cooldown = Cooldown::new(0.3),", "(unchanged)"),
    e("Physics", "ctx.velocity() / velocity2() / set_velocity(v)", "ctx.set_velocity(Vec2::new(run * 5.0, ctx.velocity2().y));", "ctx.get_mut::<Rigidbody2D>().velocity"),
    e("Physics", "ctx.add_force(f)", "ctx.add_force(Vec3::Y * 20.0);", "physics3d.add_force(id, f)"),
    e("Physics", "ctx.grounded()", "if ctx.grounded() && ctx.pressed(\"Jump\") { .. }", "CharacterController::grounded / a raycast down"),
    e("Physics", "ctx.raycast(from, dir, max) / raycast2d", "if let Some(hit) = ctx.raycast2d(ctx.pos2(), Vec2::NEG_Y, 1.0) { .. }", "Physics2D / Physics3D::raycast(.., Some(id))"),
    e("Physics", "ctx.overlap_sphere(center, r)", "for id in ctx.overlap_sphere(ctx.pos(), 3.0) { .. }", "physics3d.overlap_sphere(..)"),
    e("Sound", "ctx.play(\"clip\") / play_at(clip, p)", "ctx.play(\"sounds/jump.wav\");", "ctx.resource_mut::<Audio>().play(..)"),
    e("Sound", "ctx.play(\"sfx:coin\") / music(\"music:calm\")", "ctx.play(\"sfx:hurt\");", "audio.add_clip(\"hurt\", synth::sfx::hurt()) + play(\"hurt\")"),
    e("Sound", "ctx.music(\"clip\") / stop_music()", "ctx.music(\"theme\");", "audio.play_music(clip, 1.0)"),
    e("UI", "ctx.ui_text(\"Name\", text)", "ctx.ui_text(\"Score\", format!(\"Score: {}\", self.score));", "find_by_name + get_component_mut::<Text>().set(..)"),
    e("UI", "ctx.ui_show(\"Name\") / ui_hide", "ctx.ui_show(\"GameOver\");", "scene.ui_set_visible(id, true)"),
    e("UI", "ctx.clicked(\"button\")", "if ctx.clicked(\"restart\") { ctx.reload_scene(); }", "ctx.ui().clicked(..)"),
    e("Shaders", "ctx.set_shader_param(name, v)", "ctx.set_shader_param(\"amount\", self.burn);", "ctx.me_mut::<MaterialParams>().set(..) / SpriteShader / PostEffect"),
    e("Scenes", "ctx.load_scene(name) / reload_scene() / quit()", "ctx.load_scene(\"level2\");", "(unchanged)"),
    e("Saving", "ctx.save(key, v) / ctx.load(key, default)", "self.best = ctx.load(\"best\", 0);", "ctx.save_data().set_int(..) + save() / int(..)"),
    e("Random", "ctx.rand() / rand_range(a, b) / rand_int(a, b)", "let x = ctx.rand_range(-3.0, 3.0);", "ctx.random().f32() / range / int_range_inclusive"),
    e("Log", "ctx.log(text) / ctx.warn(text)", "ctx.log(format!(\"hp = {}\", self.hp));", "log::info!(..)"),
];

/// Шпаргалка таблицей Markdown — ровно в таком виде она стоит в `docs/api/gameplay.md`.
pub fn cheat_sheet_markdown() -> String {
    let mut out = String::from("| раздел | короткая форма | пример | длинная форма (тоже работает) |\n|---|---|---|---|\n");
    for entry in CHEAT_SHEET {
        let cell = |s: &str| s.replace('|', "\\|");
        out.push_str(&format!(
            "| {} | `{}` | `{}` | `{}` |\n",
            entry.topic,
            cell(entry.short),
            cell(entry.example),
            cell(entry.wraps)
        ));
    }
    out
}
