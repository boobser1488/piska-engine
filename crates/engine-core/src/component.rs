//! Component — поведение и данные на GameObject'е. Аналог `MonoBehaviour`.

use crate::error::SceneError;
use crate::input::Input;
use crate::math::{Quat, Vec3};
use crate::object::{GameObject, GameObjectId};
use crate::random::Random;
use crate::reflect::{Reflect, Value};
use crate::save::SaveData;
use crate::scene::Scene;
use crate::schedule::TimerId;
use crate::transform::Transform;
use crate::tween::{Sequence, Tween, TweenId};
use crate::world::{Time, World};

/// Сообщение для `send_message`: имя + произвольное значение.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub name: String,
    pub value: Value,
}

/// Набор хуков компонента — ответ [`Component::hooks`]. Битовая маска:
/// `Hooks::UPDATE | Hooks::LATE_UPDATE`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hooks(u16);

impl Hooks {
    /// Ни одного хука: «чистые данные» (спрайт, коллайдер, свет…).
    pub const NONE: Hooks = Hooks(0);
    pub const START: Hooks = Hooks(1);
    pub const UPDATE: Hooks = Hooks(1 << 1);
    pub const FIXED_UPDATE: Hooks = Hooks(1 << 2);
    pub const LATE_UPDATE: Hooks = Hooks(1 << 3);
    pub const DESTROY: Hooks = Hooks(1 << 4);
    pub const MESSAGE: Hooks = Hooks(1 << 5);
    pub const COLLISION_ENTER: Hooks = Hooks(1 << 6);
    pub const TRIGGER_ENTER: Hooks = Hooks(1 << 7);
    /// Все хуки, в том числе те, что появятся в будущих версиях. Значение по умолчанию.
    pub const ALL: Hooks = Hooks(u16::MAX);

    const NAMES: [(Hooks, &'static str); 8] = [
        (Hooks::START, "START"),
        (Hooks::UPDATE, "UPDATE"),
        (Hooks::FIXED_UPDATE, "FIXED_UPDATE"),
        (Hooks::LATE_UPDATE, "LATE_UPDATE"),
        (Hooks::DESTROY, "DESTROY"),
        (Hooks::MESSAGE, "MESSAGE"),
        (Hooks::COLLISION_ENTER, "COLLISION_ENTER"),
        (Hooks::TRIGGER_ENTER, "TRIGGER_ENTER"),
    ];

    /// Есть ли в наборе все хуки из `other`.
    pub const fn contains(self, other: Hooks) -> bool {
        self.0 & other.0 == other.0
    }

    /// Есть ли в наборе хоть один хук из `other`.
    pub const fn intersects(self, other: Hooks) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: Hooks) -> Hooks {
        Hooks(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl Default for Hooks {
    /// Как у компонента, который ничего о себе не сказал: все хуки.
    fn default() -> Self {
        Hooks::ALL
    }
}

impl std::ops::BitOr for Hooks {
    type Output = Hooks;
    fn bitor(self, other: Hooks) -> Hooks {
        self.union(other)
    }
}

impl std::ops::BitOrAssign for Hooks {
    fn bitor_assign(&mut self, other: Hooks) {
        *self = self.union(other);
    }
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == Hooks::ALL {
            return f.write_str("Hooks(ALL)");
        }
        let names: Vec<&str> = Hooks::NAMES
            .iter()
            .filter(|(hook, _)| self.contains(*hook))
            .map(|(_, name)| *name)
            .collect();
        if names.is_empty() {
            f.write_str("Hooks(NONE)")
        } else {
            write!(f, "Hooks({})", names.join(" | "))
        }
    }
}

/// Компонент. Данные объявляются макросом `component!`, поведение — хуками ниже.
/// Все хуки необязательны: для «чистых данных» достаточно `impl Component for T {}`.
///
/// Порядок в кадре: `on_start` (один раз) → `on_fixed_update` (0..N раз, шаг физики)
/// → `on_update` → `on_late_update` → отложенное уничтожение (`on_destroy`).
#[allow(unused_variables)]
pub trait Component: Reflect + 'static {
    /// Какие хуки компонент реализует. Движок не вызывает остальные — так тысячи
    /// спрайтов и коллайдеров не стоят ничего в фазах `on_update` и др. По умолчанию
    /// [`Hooks::ALL`]: компонент, который ничего не сказал, получает всё, как раньше.
    ///
    /// ```
    /// # use engine_core::prelude::*;
    /// # use engine_core::component::Hooks;
    /// # component! { pub struct Health { pub hp: i32 = 3 } }
    /// impl Component for Health {
    ///     fn hooks(&self) -> Hooks { Hooks::NONE } // только данные
    /// }
    /// ```
    ///
    /// Спрашивается один раз — когда компонент добавляют на объект. Хук, которого нет
    /// в ответе, не будет вызван, даже если он переопределён: перечисляйте все свои.
    fn hooks(&self) -> Hooks {
        Hooks::ALL
    }

    /// Порядок выполнения (Unity: Script Execution Order). Чем меньше число, тем раньше
    /// компонент получает `on_start`, `on_fixed_update`, `on_update` и `on_late_update`:
    /// все компоненты с порядком −100 идут до всех с порядком 0, те — до +100. При равном
    /// порядке остаётся прежний: иерархия, затем порядок компонентов на объекте.
    /// По умолчанию 0. Значение из настроек проекта (`assets/project.settings`,
    /// [`ProjectSettings`](crate::settings::ProjectSettings)) перекрывает это, а оно —
    /// ответ по умолчанию. Спрашивается один раз — когда компонент добавляют на объект.
    ///
    /// ```
    /// # use engine_core::prelude::*;
    /// # component! { pub struct InputReader {} }
    /// impl Component for InputReader {
    ///     fn execution_order(&self) -> i32 { -50 } // раньше обычных скриптов
    /// }
    /// ```
    fn execution_order(&self) -> i32 {
        0
    }

    /// Один раз, перед первым обновлением активного объекта.
    fn on_start(&mut self, ctx: &mut Ctx) {}
    /// Каждый кадр.
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {}
    /// С фиксированным шагом, прямо перед шагом физики.
    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {}
    /// Каждый кадр после всех `on_update` — для камер и слежения.
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {}
    /// Перед удалением объекта (в конце кадра, в котором вызван `destroy`).
    fn on_destroy(&mut self, ctx: &mut Ctx) {}
    /// Сообщение от `send_message`.
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {}
    /// Началось столкновение с твёрдым коллайдером объекта `other`.
    fn on_collision_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {}
    /// Объект `other` вошёл в триггер (или мы вошли в его триггер).
    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {}
}

/// Контекст хука: «я» (`id`) и весь мир.
///
/// Пока хук выполняется, сам компонент временно вынут из объекта, поэтому у скрипта
/// есть честный `&mut World` без `RefCell` и без `unsafe`. Следствие: `ctx.get::<Self>()`
/// внутри собственного хука вернёт `None` — используйте `self`.
pub struct Ctx<'a> {
    pub world: &'a mut World,
    pub id: GameObjectId,
}

impl Ctx<'_> {
    pub fn object(&self) -> &GameObject {
        self.world
            .scene
            .get(self.id)
            .expect("the object running a hook is alive: destruction is deferred to the end of the frame")
    }

    pub fn object_mut(&mut self) -> &mut GameObject {
        self.world
            .scene
            .get_mut(self.id)
            .expect("the object running a hook is alive: destruction is deferred to the end of the frame")
    }

    /// Unity: `transform`.
    pub fn transform(&mut self) -> &mut Transform {
        &mut self.object_mut().transform
    }

    pub fn world_position(&self) -> Vec3 {
        self.world.scene.world_position(self.id)
    }

    /// Unity: `GetComponent<T>()` на своём объекте.
    pub fn get<T: Component>(&self) -> Option<&T> {
        self.object().get::<T>()
    }

    pub fn get_mut<T: Component>(&mut self) -> Option<&mut T> {
        self.object_mut().get_mut::<T>()
    }

    pub fn input(&self) -> &Input {
        &self.world.input
    }

    pub fn time(&self) -> &Time {
        &self.world.time
    }

    pub fn scene(&self) -> &Scene {
        &self.world.scene
    }

    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.world.scene
    }

    /// Глобальный ресурс (физика, база ассетов, ваши синглтоны).
    pub fn resource<T: 'static>(&self) -> Option<&T> {
        self.world.resources.get::<T>()
    }

    pub fn resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.world.resources.get_mut::<T>()
    }

    /// Unity: `GameObject.FindWithTag`.
    pub fn find_by_tag(&self, tag: &str) -> Option<GameObjectId> {
        self.world.scene.find_by_tag(tag)
    }

    /// Unity: `SendMessage`. Доставляется всем компонентам `target` в конце текущей фазы кадра.
    pub fn send_message(&mut self, target: GameObjectId, name: &str, value: Value) {
        self.world.send_message(target, name, value);
    }

    /// Типизированное событие; читатели увидят его в следующем кадре.
    pub fn emit<E: 'static>(&mut self, event: E) {
        self.world.events.emit(event);
    }

    /// Unity: `Destroy(obj)`. Объект и его дети исчезнут в конце кадра.
    pub fn destroy(&mut self, id: GameObjectId) {
        self.world.scene.destroy(id);
    }

    pub fn destroy_self(&mut self) {
        self.world.scene.destroy(self.id);
    }

    /// Unity: `Instantiate(prefab)`. `reference` — путь или GUID префаба.
    pub fn instantiate(&mut self, reference: &str) -> Result<GameObjectId, SceneError> {
        self.world.instantiate(reference)
    }

    /// Unity: `Instantiate(prefab, position, Quaternion.identity)` — в мировой позиции.
    pub fn instantiate_at(&mut self, reference: &str, position: Vec3) -> Result<GameObjectId, SceneError> {
        self.world.instantiate_at(reference, position, Quat::IDENTITY, None)
    }

    /// Unity: `Instantiate(prefab, parent)` — ребёнком `parent`, в его начале координат.
    pub fn instantiate_under(
        &mut self,
        reference: &str,
        parent: GameObjectId,
    ) -> Result<GameObjectId, SceneError> {
        self.world
            .instantiate_at(reference, Vec3::ZERO, Quat::IDENTITY, Some(parent))
    }

    /// Полная форма: поза (мировая без `parent`, локальная с ним) и родитель.
    pub fn instantiate_with(
        &mut self,
        reference: &str,
        position: Vec3,
        rotation: Quat,
        parent: Option<GameObjectId>,
    ) -> Result<GameObjectId, SceneError> {
        self.world.instantiate_at(reference, position, rotation, parent)
    }

    /// Unity: `Instantiate(obj)` — копия живого объекта с потомками.
    pub fn duplicate(&mut self, id: GameObjectId) -> Result<GameObjectId, SceneError> {
        self.world.duplicate(id)
    }

    // ----- запросы -----

    /// Unity: `GameObject.Find`.
    pub fn find_by_name(&self, name: &str) -> Option<GameObjectId> {
        self.world.scene.find_by_name(name)
    }

    /// Unity: `GameObject.FindGameObjectsWithTag`.
    pub fn find_all_by_tag(&self, tag: &str) -> Vec<GameObjectId> {
        self.world.scene.find_all_by_tag(tag)
    }

    /// Unity: `FindObjectsOfType<T>()` — объекты с компонентом `T`. Свой объект в
    /// список не попадёт, пока выполняется его собственный хук (компонент «вынут»).
    /// Стоит пропорционально числу таких объектов, а не размеру сцены.
    pub fn find_with<T: Component>(&self) -> Vec<GameObjectId> {
        self.world.scene.find_with::<T>()
    }

    pub fn parent(&self) -> Option<GameObjectId> {
        self.object().parent()
    }

    pub fn children(&self) -> Vec<GameObjectId> {
        self.object().children().to_vec()
    }

    /// Unity: `GetComponentInChildren<T>()` на другом объекте (или на `self.id`).
    pub fn component_in_children<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        self.world.scene.component_in_children::<T>(id)
    }

    pub fn component_in_children_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        self.world.scene.component_in_children_mut::<T>(id)
    }

    /// Unity: `GetComponentInParent<T>()`.
    pub fn component_in_parent<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        self.world.scene.component_in_parent::<T>(id)
    }

    pub fn component_in_parent_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        self.world.scene.component_in_parent_mut::<T>(id)
    }

    /// Unity: `GetComponent<T>()` на чужом объекте.
    pub fn get_on<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        self.world.scene.get_component::<T>(id)
    }

    pub fn get_on_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        self.world.scene.get_component_mut::<T>(id)
    }

    // ----- активность -----

    /// Unity: `SetActive`. Объект с потомками перестаёт обновляться, рисоваться и сталкиваться.
    pub fn set_active(&mut self, id: GameObjectId, active: bool) {
        self.world.scene.set_active(id, active);
    }

    /// Unity: `activeInHierarchy`.
    pub fn is_active(&self, id: GameObjectId) -> bool {
        self.world.scene.is_active(id)
    }

    // ----- время -----

    /// Unity: `Time.timeScale`.
    pub fn time_scale(&self) -> f32 {
        self.world.time.time_scale
    }

    /// 0 — пауза, 0.3 — замедление, 1 — норма. Действует со следующего кадра.
    pub fn set_time_scale(&mut self, scale: f32) {
        self.world.time.time_scale = scale.max(0.0);
    }

    // ----- качество графики -----

    /// Unity: `QualitySettings.SetQualityLevel`. Заменяет ресурс
    /// [`GraphicsSettings`](crate::settings::GraphicsSettings) значениями пресета; рендер
    /// читает его каждый кадр, поэтому смена видна со следующего кадра.
    pub fn set_quality(&mut self, level: crate::settings::QualityLevel) {
        self.world.resources.insert(crate::settings::GraphicsSettings::preset(level));
    }

    /// Текущий пресет качества (без ресурса — `High`, как рисует движок по умолчанию).
    pub fn quality(&self) -> crate::settings::QualityLevel {
        self.world
            .resources
            .get::<crate::settings::GraphicsSettings>()
            .map_or_else(crate::settings::QualityLevel::default, |g| g.quality)
    }

    /// Закрыть игру (Unity: `Application.Quit`). В редакторе — выйти из режима Play.
    pub fn quit(&mut self) {
        self.world.request_quit();
    }

    // ----- таймеры -----

    /// «Перезвони через `delay` секунд»: этому объекту придёт сообщение `name` (`on_message`).
    pub fn after(&mut self, delay: f32, name: &str, value: Value) -> TimerId {
        self.world.scheduler().send_after(self.id, delay, name, value)
    }

    /// Присылать себе сообщение каждые `interval` секунд (Unity: `InvokeRepeating`).
    pub fn every(&mut self, interval: f32, name: &str, value: Value) -> TimerId {
        self.world.scheduler().send_every(self.id, interval, name, value)
    }

    /// Отправить сообщение другому объекту через `delay` секунд.
    pub fn send_after(&mut self, target: GameObjectId, delay: f32, name: &str, value: Value) -> TimerId {
        self.world.scheduler().send_after(target, delay, name, value)
    }

    /// Вызвать замыкание через `delay` секунд. Таймер привязан к этому объекту: умрёт вместе с ним.
    pub fn call_after(&mut self, delay: f32, f: impl FnOnce(&mut World) + 'static) -> TimerId {
        let owner = self.id;
        let scheduler = self.world.scheduler();
        let id = scheduler.call_after(delay, f);
        scheduler.owned_by(id, owner)
    }

    /// Вызывать замыкание каждые `interval` секунд; умрёт вместе с этим объектом.
    pub fn call_every(&mut self, interval: f32, f: impl FnMut(&mut World) + 'static) -> TimerId {
        let owner = self.id;
        let scheduler = self.world.scheduler();
        let id = scheduler.call_every(interval, f);
        scheduler.owned_by(id, owner)
    }

    /// Unity: `Destroy(obj, delay)`.
    pub fn destroy_after(&mut self, target: GameObjectId, delay: f32) -> TimerId {
        self.world.scheduler().destroy_after(target, delay)
    }

    pub fn destroy_self_after(&mut self, delay: f32) -> TimerId {
        let id = self.id;
        self.world.scheduler().destroy_after(id, delay)
    }

    /// Unity: `CancelInvoke(name)` для одного таймера.
    pub fn cancel_timer(&mut self, timer: TimerId) -> bool {
        self.world.scheduler().cancel(timer)
    }

    /// Unity: `CancelInvoke()` — все таймеры этого объекта.
    pub fn cancel_my_timers(&mut self) -> usize {
        let id = self.id;
        self.world.scheduler().cancel_owned_by(id)
    }

    // ----- твины -----

    /// Запустить анимацию; см. [`Tween`].
    pub fn start_tween(&mut self, tween: Tween) -> TweenId {
        self.world.start_tween(tween)
    }

    pub fn start_sequence(&mut self, sequence: Sequence) -> TweenId {
        self.world.start_sequence(sequence)
    }

    pub fn cancel_tween(&mut self, tween: TweenId) -> bool {
        self.world.tweens().cancel(tween)
    }

    /// Снять все твины объекта `target`.
    pub fn cancel_tweens_of(&mut self, target: GameObjectId) -> usize {
        self.world.tweens().cancel_all_for(target)
    }

    // ----- сцены -----

    /// Unity: `SceneManager.LoadScene(name)`. Выполняется в **конце кадра**; текущий кадр
    /// доживает в старой сцене. Не уничтожаются только объекты после `dont_destroy_on_load`.
    pub fn load_scene(&mut self, name: &str) {
        self.world.scenes().load(name);
    }

    /// Загрузка поверх текущей сцены (Unity: `LoadSceneMode.Additive`).
    pub fn load_scene_additive(&mut self, name: &str) {
        self.world.scenes().load_additive(name);
    }

    /// Перезагрузить сцену, загруженную последней через `load_scene`.
    pub fn reload_scene(&mut self) {
        self.world.scenes().reload();
    }

    /// Убрать объекты, добавленные `load_scene_additive(name)`.
    pub fn unload_scene(&mut self, name: &str) {
        self.world.scenes().unload(name);
    }

    /// Имя текущей сцены (поле `name` из файла сцены).
    pub fn scene_name(&self) -> &str {
        &self.world.scene.name
    }

    /// Unity: `DontDestroyOnLoad(obj)`: см. [`Scene::set_persistent`].
    pub fn dont_destroy_on_load(&mut self, id: GameObjectId) {
        self.world.scene.set_persistent(id, true);
    }

    // ----- ресурсы и события -----

    /// Генератор случайных чисел игры: `ctx.random().range(-1.0, 1.0)`.
    pub fn random(&mut self) -> &mut Random {
        self.world.random()
    }

    /// Сохранения игрока. Если игра не подключила [`SaveData`], создаётся набор в памяти
    /// (ничего не переживёт перезапуск) и в лог идёт предупреждение.
    pub fn save_data(&mut self) -> &mut SaveData {
        if !self.world.resources.contains::<SaveData>() {
            log::warn!(
                "SaveData is not installed, using an in-memory one that is lost on exit;                  insert `SaveData::open(\"YourGame\")` into `world.resources` at startup"
            );
        }
        self.world.resources.get_or_insert_with(SaveData::in_memory)
    }

    /// Ресурс типа `T`; если его ещё нет — создаётся значением по умолчанию.
    /// Удобно для глобального состояния игры (счёт, жизни, настройки).
    pub fn resource_or_default<T: Default + 'static>(&mut self) -> &mut T {
        self.world.resources.get_or_insert_with(T::default)
    }

    /// События типа `E`, отправленные в прошлом кадре.
    pub fn events<E: 'static>(&self) -> impl Iterator<Item = &E> + '_ {
        self.world.events.read::<E>()
    }
}
