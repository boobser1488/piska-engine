//! World — сцена + всё глобальное состояние кадра. Здесь же диспетчеризация хуков.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::Path;

use crate::component::{Component, Ctx, Hooks, Message};
use crate::error::SceneError;
use crate::input::Input;
use crate::object::GameObjectId;
use crate::reflect::{TypeRegistry, Value};
use crate::scene::Scene;
use crate::math::{Quat, Vec3};
use crate::random::Random;
use crate::scenes::{Request, SceneLoadFailed, SceneLoaded, SceneManager};
use crate::schedule::{self, Scheduler};
use crate::serialize::{
    self, DirPrefabs, NodeData, PrefabSource, SceneData, SceneSnapshot, TransformData,
};
use crate::tween::{self, Sequence, Tween, TweenId, Tweens};

/// Время кадра. Unity: `Time.deltaTime`, `Time.time`, `Time.fixedDeltaTime`, `Time.timeScale`.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    /// Длительность текущего кадра в секундах (уже с учётом `time_scale`).
    pub delta: f32,
    /// Секунд игрового времени с запуска (идёт с учётом `time_scale`).
    pub elapsed: f64,
    pub frame: u64,
    /// Длительность кадра в секундах **без** `time_scale` — для интерфейса и таймеров на паузе.
    pub unscaled_delta: f32,
    /// Секунд реального времени с запуска (не зависит от `time_scale`).
    pub unscaled_elapsed: f64,
    /// Шаг `on_fixed_update` и физики.
    pub fixed_delta: f32,
    /// 0 — пауза, 1 — нормальная скорость.
    pub time_scale: f32,
    /// Где текущий кадр находится между двумя последними фиксированными шагами (0..=1).
    pub fixed_alpha: f32,
    /// Самый длинный кадр, который видит игра (`Time.maximumDeltaTime`): подвисание дольше
    /// обрезается. По умолчанию без ограничения; проект задаёт его в Project Settings → Time.
    pub max_delta: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta: 0.0,
            elapsed: 0.0,
            frame: 0,
            unscaled_delta: 0.0,
            unscaled_elapsed: 0.0,
            fixed_delta: 1.0 / 60.0,
            time_scale: 1.0,
            fixed_alpha: 1.0,
            max_delta: f32::INFINITY,
        }
    }
}

impl Time {
    /// Кадров в секунду по реальному времени (0, пока не было ни одного кадра).
    pub fn fps(&self) -> f32 {
        if self.unscaled_delta > 0.0 {
            1.0 / self.unscaled_delta
        } else {
            0.0
        }
    }

    /// `time_scale == 0`: игра на паузе.
    pub fn is_paused(&self) -> bool {
        self.time_scale <= 0.0
    }
}

/// Глобальные синглтоны по типу: физика, база ассетов, статистика рендера, ваши менеджеры.
#[derive(Default)]
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn insert<T: 'static>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|old| old.downcast::<T>().ok())
            .map(|old| *old)
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref::<T>()
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut::<T>()
    }

    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|old| old.downcast::<T>().ok())
            .map(|old| *old)
    }

    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Ресурс типа `T`; если его нет — создаёт через `make`.
    pub fn get_or_insert_with<T: 'static>(&mut self, make: impl FnOnce() -> T) -> &mut T {
        self.map
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(make()))
            .downcast_mut::<T>()
            .expect("resource map is keyed by TypeId")
    }
}

/// Типизированные события. Событие, отправленное в кадре N, читается в кадре N+1 —
/// так каждый читатель видит его ровно один раз, независимо от порядка скриптов.
#[derive(Default)]
pub struct Events {
    pending: HashMap<TypeId, Vec<Box<dyn Any>>>,
    readable: HashMap<TypeId, Vec<Box<dyn Any>>>,
}

impl Events {
    pub fn emit<E: 'static>(&mut self, event: E) {
        self.pending
            .entry(TypeId::of::<E>())
            .or_default()
            .push(Box::new(event));
    }

    pub fn read<E: 'static>(&self) -> impl Iterator<Item = &E> + '_ {
        self.readable
            .get(&TypeId::of::<E>())
            .into_iter()
            .flatten()
            .filter_map(|event| event.downcast_ref::<E>())
    }

    pub(crate) fn swap(&mut self) {
        self.readable = std::mem::take(&mut self.pending);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// Только компоненты, у которых ещё не было `on_start`.
    Start,
    /// Только уже стартовавшие компоненты.
    Running,
}

/// Игровой мир: активная сцена и глобальное состояние.
pub struct World {
    pub scene: Scene,
    pub time: Time,
    pub input: Input,
    pub events: Events,
    pub resources: Resources,
    /// Типы компонентов, известные загрузчику сцен.
    pub registry: TypeRegistry,
    /// Откуда брать префабы по ссылке из сцены (каталог, база ассетов, память).
    pub prefabs: Box<dyn PrefabSource>,
    messages: Vec<(GameObjectId, Message)>,
    quit_requested: bool,
    /// Буфер порядка обхода для фаз `on_update` и др. (см. `for_each_active`).
    dispatch_ids: Vec<GameObjectId>,
    /// То же для сцен с порядком выполнения: пары «объект, номер компонента».
    dispatch_entries: Vec<(GameObjectId, u32)>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            scene: Scene::default(),
            time: Time::default(),
            input: Input::default(),
            events: Events::default(),
            resources: Resources::default(),
            registry: TypeRegistry::default(),
            prefabs: Box::new(DirPrefabs::new(".")),
            messages: Vec::new(),
            quit_requested: false,
            dispatch_ids: Vec::new(),
            dispatch_entries: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Диспетчеризация хуков
    // ------------------------------------------------------------------

    /// Вызвать `f` для каждого (стартовавшего) компонента объекта.
    ///
    /// Это единственный механизм вызова пользовательского кода. Компонент на время
    /// вызова вынимается из объекта (`Option::take`), поэтому хук получает полный
    /// `&mut World` в безопасном Rust. Подсистемы (физика и т.п.) используют этот же
    /// метод для своих колбэков: `world.dispatch(id, |c, ctx| c.on_trigger_enter(ctx, other))`.
    /// Если известно, какой хук зовёт `f`, лучше [`dispatch_hook`](Self::dispatch_hook):
    /// он пропускает компоненты без этого хука (см. [`Component::hooks`]).
    pub fn dispatch(
        &mut self,
        id: GameObjectId,
        mut f: impl FnMut(&mut dyn Component, &mut Ctx<'_>),
    ) {
        self.run_hook(id, Phase::Running, Hooks::ALL, &mut f);
    }

    /// Как [`dispatch`](Self::dispatch), но только для компонентов, у которых есть хук
    /// `hook`: `world.dispatch_hook(id, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, other))`.
    pub fn dispatch_hook(
        &mut self,
        id: GameObjectId,
        hook: Hooks,
        mut f: impl FnMut(&mut dyn Component, &mut Ctx<'_>),
    ) {
        self.run_hook(id, Phase::Running, hook, &mut f);
    }

    fn run_hook(
        &mut self,
        id: GameObjectId,
        phase: Phase,
        hook: Hooks,
        f: &mut dyn FnMut(&mut dyn Component, &mut Ctx<'_>),
    ) {
        // Компоненты, добавленные хуком на этот же объект, обходятся в этом же проходе.
        let mut index = 0;
        while self.run_slot(id, index, phase, hook, f) {
            index += 1;
        }
    }

    /// Хук `hook` у одного компонента объекта (`index` — номер в списке компонентов).
    /// `false` — объекта нет или номер за концом списка.
    fn run_slot(
        &mut self,
        id: GameObjectId,
        index: usize,
        phase: Phase,
        hook: Hooks,
        f: &mut dyn FnMut(&mut dyn Component, &mut Ctx<'_>),
    ) -> bool {
        let Some(obj) = self.scene.get_mut(id) else {
            return false;
        };
        let Some(slot) = obj.components.get_mut(index) else {
            return false;
        };
        let eligible = !slot.removed
            && slot.comp.is_some()
            && match phase {
                Phase::Start => !slot.started,
                Phase::Running => slot.started,
            };
        // Компонент без этого хука пропускаем (в фазе старта он всё равно
        // считается стартовавшим — дальше получит остальные свои хуки).
        if !eligible || !slot.hooks.intersects(hook) {
            if eligible {
                slot.started = true;
            }
            return true;
        }
        slot.started = true;
        let Some(mut component) = slot.comp.take() else {
            return true;
        };

        f(component.as_mut(), &mut Ctx { world: self, id });

        // Вернуть компонент на место, если объект жив и компонент не удалили из хука.
        if let Some(slot) = self
            .scene
            .get_mut(id)
            .and_then(|obj| obj.components.get_mut(index))
        {
            if !slot.removed && slot.comp.is_none() {
                slot.comp = Some(component);
            }
        }
        true
    }

    /// Фаза `hook` для объектов, у которых он есть, в порядке иерархии.
    fn for_each_active(&mut self, hook: Hooks, f: &mut dyn FnMut(&mut dyn Component, &mut Ctx<'_>)) {
        // Снимок на начало фазы (созданные в хуках объекты ждут следующей фазы), в
        // переиспользуемом буфере. Список берётся из индекса сцены: объекты без этого
        // хука (спрайты, коллайдеры…) не стоят ничего. Хук может снова запустить фазу —
        // тогда вложенный вызов возьмёт новый буфер.
        // Если у каких-то компонентов порядок выполнения не 0, идём по компонентам, а не
        // по объектам: список отсортирован и кэшируется (см. `Scene::hook_entries`).
        let mut entries = std::mem::take(&mut self.dispatch_entries);
        if self.scene.hook_entries(hook, &mut entries) {
            for &(id, slot) in &entries {
                if self.scene.is_active(id) {
                    self.run_slot(id, slot as usize, Phase::Running, hook, f);
                }
            }
            if entries.capacity() > self.dispatch_entries.capacity() {
                self.dispatch_entries = entries;
            }
            return;
        }
        self.dispatch_entries = entries;
        let mut ids = std::mem::take(&mut self.dispatch_ids);
        self.scene.hook_targets(hook, &mut ids);
        for &id in &ids {
            // Активность проверяется в момент вызова: хук мог выключить кого-то ниже.
            if self.scene.is_active(id) {
                self.run_hook(id, Phase::Running, hook, f);
            }
        }
        if ids.capacity() > self.dispatch_ids.capacity() {
            self.dispatch_ids = ids;
        }
    }

    /// `on_start` для всех компонентов, которые его ещё не получали.
    pub fn start_pending(&mut self) {
        if self.scene.has_ordered_components() {
            self.start_pending_ordered();
            return;
        }
        let mut ids = std::mem::take(&mut self.dispatch_ids);
        self.scene.start_targets(&mut ids);
        let spawned_before = self.scene.spawn_count();
        let mut position = 0;
        while position < ids.len() {
            let id = ids[position];
            if self.scene.is_active(id) {
                self.run_hook(id, Phase::Start, Hooks::START, &mut |c, ctx| c.on_start(ctx));
            }
            // Хук мог добавить компоненты объектам ниже по иерархии: как и раньше,
            // они стартуют в этой же фазе.
            self.scene.merge_late_starts(&mut ids, position, spawned_before);
            position += 1;
        }
        self.scene.settle_started(&ids);
        if ids.capacity() > self.dispatch_ids.capacity() {
            self.dispatch_ids = ids;
        }
    }

    /// `on_start` в сцене с порядком выполнения: по компонентам, а не по объектам.
    /// Компоненты, которые хуки `on_start` добавили объектам, созданным до начала фазы,
    /// стартуют в этой же фазе — следующим «кругом» (не более 8 кругов).
    fn start_pending_ordered(&mut self) {
        let spawned_before = self.scene.spawn_count();
        let mut ids = std::mem::take(&mut self.dispatch_ids);
        let mut entries = std::mem::take(&mut self.dispatch_entries);
        for _ in 0..8 {
            self.scene.start_targets_before(spawned_before, &mut ids);
            self.scene.start_entries(&ids, &mut entries);
            let mut ran = false;
            for &(id, slot) in &entries {
                if self.scene.is_active(id) {
                    ran = true;
                    self.run_slot(id, slot as usize, Phase::Start, Hooks::START, &mut |c, ctx| c.on_start(ctx));
                }
            }
            self.scene.settle_started(&ids);
            if !ran {
                break;
            }
        }
        if ids.capacity() > self.dispatch_ids.capacity() {
            self.dispatch_ids = ids;
        }
        if entries.capacity() > self.dispatch_entries.capacity() {
            self.dispatch_entries = entries;
        }
    }

    pub fn fixed_update(&mut self, dt: f32) {
        self.for_each_active(Hooks::FIXED_UPDATE, &mut |c, ctx| c.on_fixed_update(ctx, dt));
        self.flush_messages();
    }

    pub fn update(&mut self, dt: f32) {
        self.for_each_active(Hooks::UPDATE, &mut |c, ctx| c.on_update(ctx, dt));
        self.flush_messages();
    }

    pub fn late_update(&mut self, dt: f32) {
        self.for_each_active(Hooks::LATE_UPDATE, &mut |c, ctx| c.on_late_update(ctx, dt));
        self.flush_messages();
    }

    // ------------------------------------------------------------------
    // Сообщения
    // ------------------------------------------------------------------

    /// Unity: `SendMessage`. Сообщение получат все компоненты `target` (хук `on_message`)
    /// в конце текущей фазы кадра.
    pub fn send_message(&mut self, target: GameObjectId, name: &str, value: Value) {
        self.messages.push((
            target,
            Message {
                name: name.to_string(),
                value,
            },
        ));
    }

    /// Доставить накопленные сообщения. Ответные сообщения доставляются тут же,
    /// но не более 8 «кругов» — бесконечный пинг-понг растягивается по кадрам, а не вешает игру.
    pub fn flush_messages(&mut self) {
        for _ in 0..8 {
            if self.messages.is_empty() {
                return;
            }
            for (target, message) in std::mem::take(&mut self.messages) {
                self.run_hook(target, Phase::Running, Hooks::MESSAGE, &mut |c, ctx| {
                    c.on_message(ctx, &message)
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Уничтожение
    // ------------------------------------------------------------------

    /// Выполнить отложенные `destroy`: `on_destroy` у всего поддерева, затем удаление.
    pub fn flush_destroyed(&mut self) {
        loop {
            let pending = self.scene.take_pending_destroy();
            if pending.is_empty() {
                break;
            }
            for root in pending {
                for id in self.scene.subtree(root) {
                    self.run_hook(id, Phase::Running, Hooks::DESTROY, &mut |c, ctx| c.on_destroy(ctx));
                }
                self.scene.remove_subtree_lazy(root);
            }
            self.scene.prune_roots();
        }
        self.scene.compact_components();
    }

    /// Конец кадра: уничтожение, события, сброс «нажато в этом кадре».
    pub fn end_frame(&mut self) {
        self.flush_destroyed();
        self.apply_scene_requests();
        self.events.swap();
        self.input.end_frame();
    }

    // ------------------------------------------------------------------
    // Сцены и префабы
    // ------------------------------------------------------------------

    /// Unity: `Instantiate(prefab)`. `reference` — путь или GUID, как его понимает `self.prefabs`.
    pub fn instantiate(&mut self, reference: &str) -> Result<GameObjectId, SceneError> {
        let name = Path::new(reference)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(reference)
            .to_string();
        let node = NodeData {
            name,
            prefab: Some(reference.to_string()),
            ..NodeData::default()
        };
        self.spawn_node(&node, None)
    }

    /// Unity: `Instantiate(prefab, position, rotation, parent)`. Без `parent` поза мировая,
    /// с `parent` — локальная. Масштаб берётся из корня префаба.
    pub fn instantiate_at(
        &mut self,
        reference: &str,
        position: Vec3,
        rotation: Quat,
        parent: Option<GameObjectId>,
    ) -> Result<GameObjectId, SceneError> {
        if let Some(parent) = parent {
            if !self.scene.contains(parent) {
                return Err(SceneError::DeadObject(parent));
            }
        }
        let name = Path::new(reference)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(reference)
            .to_string();
        let prefab = self.prefabs.load(reference)?;
        let node = NodeData {
            name,
            prefab: Some(reference.to_string()),
            transform: TransformData {
                scale: prefab.root.transform.scale,
                ..TransformData::default()
            },
            ..NodeData::default()
        };
        let id = self.spawn_node(&node, parent)?;
        if let Some(obj) = self.scene.get_mut(id) {
            obj.transform.position = position;
            obj.transform.rotation = rotation;
        }
        Ok(id)
    }

    /// Создать объект(ы) из описания (узел сцены или префаба).
    pub fn spawn_node(
        &mut self,
        node: &NodeData,
        parent: Option<GameObjectId>,
    ) -> Result<GameObjectId, SceneError> {
        let resolved = serialize::resolve_node(node, self.prefabs.as_ref(), 0)?;
        serialize::spawn_resolved(&mut self.scene, &self.registry, &resolved, parent)
    }

    /// Заменить текущую сцену сценой из RON-текста. При ошибке текущая сцена не трогается.
    pub fn load_scene_str(&mut self, text: &str, origin: &str) -> Result<(), SceneError> {
        let data = SceneData::from_ron(text, origin)?;
        let resolved = data
            .objects
            .iter()
            .map(|node| serialize::resolve_node(node, self.prefabs.as_ref(), 0))
            .collect::<Result<Vec<_>, _>>()?;

        // Есть «не уничтожаемые при загрузке» объекты — меняем сцену на месте, чтобы их
        // id остались действительными. Иначе — прежний путь: новая сцена-преемница.
        let has_persistent = self
            .scene
            .roots()
            .iter()
            .any(|id| self.scene.get(*id).is_some_and(|o| o.persistent));
        if has_persistent {
            self.install_scene(Some(data.name), &resolved, false)?;
            return Ok(());
        }
        self.replace_scene(data.name, &resolved)
    }

    /// Unity: `LoadScene(.., LoadSceneMode.Additive)` — добавить объекты сцены из RON-текста
    /// к текущей, ничего не удаляя. Возвращает корни добавленных объектов; при ошибке
    /// в сцене ничего не меняется.
    pub fn load_scene_additive_str(
        &mut self,
        text: &str,
        origin: &str,
    ) -> Result<Vec<GameObjectId>, SceneError> {
        let data = SceneData::from_ron(text, origin)?;
        let resolved = data
            .objects
            .iter()
            .map(|node| serialize::resolve_node(node, self.prefabs.as_ref(), 0))
            .collect::<Result<Vec<_>, _>>()?;
        self.install_scene(None, &resolved, true)
    }

    pub fn load_scene_additive(&mut self, path: impl AsRef<Path>) -> Result<Vec<GameObjectId>, SceneError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.load_scene_additive_str(&text, &path.display().to_string())
    }

    /// Поставить сцену «на месте»: проверить, что она собирается, убрать старые объекты
    /// (кроме `persistent`, если `keep_existing == false`), создать новые в этой же сцене.
    fn install_scene(
        &mut self,
        name: Option<String>,
        nodes: &[NodeData],
        keep_existing: bool,
    ) -> Result<Vec<GameObjectId>, SceneError> {
        // Пробная сборка: если компонент не зарегистрирован или поле не того типа,
        // текущая сцена остаётся нетронутой.
        let mut scratch = Scene::new("validate");
        for node in nodes {
            serialize::spawn_resolved(&mut scratch, &self.registry, node, None)?;
        }

        if !keep_existing {
            for root in self.scene.roots().to_vec() {
                if !self.scene.get(root).is_some_and(|o| o.persistent) {
                    self.scene.destroy(root);
                }
            }
            self.flush_destroyed();
            if let Some(name) = name {
                self.scene.name = name;
            }
        }

        let mut roots = Vec::with_capacity(nodes.len());
        for node in nodes {
            roots.push(serialize::spawn_resolved(&mut self.scene, &self.registry, node, None)?);
        }
        Ok(roots)
    }

    /// Выполнить запросы `SceneManager` (загрузка, перезагрузка, выгрузка). Зовётся из `end_frame`.
    pub(crate) fn apply_scene_requests(&mut self) {
        let Some(mut manager) = self.resources.remove::<SceneManager>() else {
            return;
        };
        for request in std::mem::take(&mut manager.pending) {
            match request {
                Request::Load { name, additive } => self.manager_load(&mut manager, &name, additive),
                Request::Reload => match manager.loaded_name.clone() {
                    Some(name) => self.manager_load(&mut manager, &name, false),
                    None => log::warn!(
                        "reload_scene: no scene was loaded by name yet; use `load_scene(name)` first"
                    ),
                },
                Request::Unload(name) => match manager.additive.remove(&name) {
                    Some(roots) => {
                        for root in roots {
                            self.scene.destroy(root);
                        }
                        self.flush_destroyed();
                    }
                    None => log::warn!("unload_scene: `{name}` was not loaded additively"),
                },
            }
        }
        // Хуки `on_destroy` могли попросить новую загрузку: она попала во «временный»
        // менеджер, перенесём её в основной — выполнится в конце следующего кадра.
        if let Some(mut fresh) = self.resources.remove::<SceneManager>() {
            manager.pending.append(&mut fresh.pending);
        }
        self.resources.insert(manager);
    }

    fn manager_load(&mut self, manager: &mut SceneManager, name: &str, additive: bool) {
        let result = manager.read(name).and_then(|text| {
            if additive {
                self.load_scene_additive_str(&text, name).map(Some)
            } else {
                self.load_scene_str(&text, name).map(|()| None)
            }
        });
        match result {
            Ok(roots) => {
                match roots {
                    Some(roots) => {
                        manager.additive.insert(name.to_string(), roots);
                    }
                    None => {
                        manager.loaded_name = Some(name.to_string());
                        manager.additive.clear();
                    }
                }
                log::info!("scene `{name}` loaded{}", if additive { " (additive)" } else { "" });
                self.events.emit(SceneLoaded {
                    name: name.to_string(),
                    additive,
                });
            }
            Err(error) => {
                log::error!("cannot load scene `{name}`, the current scene is kept: {error}");
                self.events.emit(SceneLoadFailed {
                    name: name.to_string(),
                    error: error.to_string(),
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Ресурсы «геймплея»: создаются при первом обращении
    // ------------------------------------------------------------------

    /// Планировщик таймеров (`ctx.after(..)` и др. работают через него).
    pub fn scheduler(&mut self) -> &mut Scheduler {
        self.resources.get_or_insert_with(Scheduler::default)
    }

    /// Работающие твины.
    pub fn tweens(&mut self) -> &mut Tweens {
        self.resources.get_or_insert_with(Tweens::default)
    }

    /// Очередь смены сцены и источник сцен по имени.
    pub fn scenes(&mut self) -> &mut SceneManager {
        self.resources.get_or_insert_with(SceneManager::default)
    }

    /// Генератор случайных чисел игры (зерно по умолчанию фиксировано: см. [`Random`]).
    pub fn random(&mut self) -> &mut Random {
        self.resources.get_or_insert_with(Random::default)
    }

    /// Попросить закрыть игру (Unity: `Application.Quit`). Окно закроется после текущего
    /// кадра; редактор вместо этого выйдет из режима Play.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Просил ли кто-нибудь закрыть игру в этом запуске.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Сбросить всё, что копится в ресурсах за время игры: таймеры, твины, отложенные
    /// запросы сцены, `time_scale`. Зовёт редактор при выходе из Play: иначе таймеры без
    /// владельца и пауза пережили бы остановку. Сохранения и генератор случайных чисел не трогаются.
    pub fn reset_gameplay_state(&mut self) {
        self.quit_requested = false;
        if let Some(scheduler) = self.resources.get_mut::<Scheduler>() {
            scheduler.cancel_all();
        }
        if let Some(tweens) = self.resources.get_mut::<Tweens>() {
            tweens.cancel_all();
        }
        if let Some(scenes) = self.resources.get_mut::<SceneManager>() {
            scenes.pending.clear();
            scenes.additive.clear();
            scenes.loaded_name = None;
        }
        self.time.time_scale = 1.0;
    }

    pub fn start_tween(&mut self, tween: Tween) -> TweenId {
        self.tweens().start(tween)
    }

    pub fn start_sequence(&mut self, sequence: Sequence) -> TweenId {
        self.tweens().start_sequence(sequence)
    }

    /// Шаг таймеров и твинов; `App::tick` зовёт его раз в кадр перед `on_update`.
    /// Тест, который гоняет `world.update` напрямую, может вызвать его сам.
    pub fn run_scheduled(&mut self) {
        schedule::run(self);
        tween::run(self);
    }

    /// Собрать новую сцену из развёрнутых узлов и заменить ею текущую.
    fn replace_scene(&mut self, name: String, nodes: &[NodeData]) -> Result<(), SceneError> {
        // Сначала собираем новую сцену отдельно: если компонент не зарегистрирован
        // или поле не того типа, старая сцена останется нетронутой.
        let mut scene = self.scene.successor(name);
        for node in nodes {
            serialize::spawn_resolved(&mut scene, &self.registry, node, None)?;
        }

        for root in self.scene.roots().to_vec() {
            self.scene.destroy(root);
        }
        self.flush_destroyed();
        self.scene = scene;
        Ok(())
    }

    /// Точный снимок сцены в памяти (редактор: Play/Stop, Undo).
    pub fn snapshot(&self) -> SceneSnapshot {
        SceneSnapshot::capture(&self.scene)
    }

    /// Вернуть сцену к снимку. Все прежние `GameObjectId` перестают действовать.
    pub fn restore(&mut self, snapshot: &SceneSnapshot) -> Result<(), SceneError> {
        self.replace_scene(snapshot.name.clone(), &snapshot.objects)
    }

    /// Unity: `Instantiate(obj)` — копия объекта с потомками рядом с оригиналом.
    pub fn duplicate(&mut self, id: GameObjectId) -> Result<GameObjectId, SceneError> {
        let parent = self
            .scene
            .get(id)
            .ok_or(SceneError::DeadObject(id))?
            .parent();
        let node = serialize::exact_node(&self.scene, id);
        serialize::spawn_resolved(&mut self.scene, &self.registry, &node, parent)
    }

    pub fn load_scene(&mut self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.load_scene_str(&text, &path.display().to_string())
    }

    /// Текущая сцена как RON. Экземпляры префабов сохраняются ссылкой + переопределениями.
    pub fn scene_to_string(&self) -> String {
        serialize::scene_data(&self.scene, self.prefabs.as_ref(), &self.registry).to_ron()
    }

    pub fn save_scene(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let path = path.as_ref();
        std::fs::write(path, self.scene_to_string()).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}
