//! Индексы сцены: «тип компонента → объекты», порядок иерархии и списки объектов,
//! у которых есть нужный хук. По ним подсистемы и диспетчер хуков находят «свои»
//! объекты за время, пропорциональное их числу, а не размеру сцены.
//!
//! Индекс обновляется лениво. Объект пишет номер своего слота в общий журнал, когда у
//! него добавляют или удаляют компонент (`GameObject::add`, `remove`); сцена разбирает
//! журнал при первом запросе. Порядок иерархии пересчитывается, только если иерархия
//! менялась (создание объекта в корне и «ребёнок только что созданного родителя» —
//! дописываются в конец без пересчёта).

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use crate::component::Hooks;
use crate::object::{GameObject, GameObjectId};

/// Что индекс знает об одном слоте сцены.
pub(crate) struct SlotInfo {
    pub generation: u32,
    pub live: bool,
    /// Типы компонентов объекта (без повторов).
    pub types: Vec<TypeId>,
    /// Объединение хуков всех компонентов объекта.
    pub hooks: Hooks,
    /// Номер создания объекта (см. `Scene::spawn`).
    pub spawned: u64,
    /// У объекта есть компонент с порядком выполнения не 0.
    pub ordered: bool,
}

impl Default for SlotInfo {
    fn default() -> Self {
        Self {
            generation: 0,
            live: false,
            types: Vec::new(),
            hooks: Hooks::NONE,
            spawned: 0,
            ordered: false,
        }
    }
}

/// Объекты с компонентом одного типа.
#[derive(Default)]
pub(crate) struct TypeEntry {
    pub members: HashSet<GameObjectId>,
    /// Растёт при каждом изменении `members`.
    pub version: u64,
    /// Те же объекты в порядке иерархии (кэш) и для какого состояния он посчитан.
    pub ordered: Vec<GameObjectId>,
    pub ordered_stamp: Option<(u64, u64)>,
}

/// Объекты (в порядке иерархии), у которых есть хук `hook`, — кэш для диспетчера.
pub(crate) struct HookList {
    pub hook: Hooks,
    pub stamp: Option<(u64, u64)>,
    pub ids: Vec<GameObjectId>,
}

/// Компоненты с хуком `hook` в порядке выполнения: `(объект, номер компонента на объекте)`.
/// Кэш для сцен, где у какого-нибудь компонента порядок не 0.
pub(crate) struct OrderedList {
    pub hook: Hooks,
    pub stamp: Option<(u64, u64, u64)>,
    pub entries: Vec<(GameObjectId, u32)>,
}

#[derive(Default)]
pub(crate) struct SceneIndex {
    pub slots: Vec<SlotInfo>,
    pub types: HashMap<TypeId, TypeEntry>,
    /// Объекты, у которых есть компоненты без `on_start`.
    pub pending_start: HashSet<GameObjectId>,
    /// Растёт, когда меняется набор хуков хоть одного объекта.
    pub hooks_version: u64,
    /// Все объекты в порядке иерархии (могут попадаться уже уничтоженные — их
    /// отличает поколение) и место каждого слота в этом порядке.
    pub order: Vec<GameObjectId>,
    pub rank: Vec<u32>,
    /// Для какой версии иерархии сцены посчитан `order` (`None` — ни для какой).
    pub order_for: Option<u64>,
    /// Растёт при каждом полном пересчёте `order`.
    pub order_epoch: u64,
    /// Сколько уничтоженных объектов лежит в `order`.
    pub dead_in_order: usize,
    pub hook_lists: Vec<HookList>,
    /// Слоты объектов с удалёнными компонентами (сжать в конце кадра).
    pub tombstones: Vec<u32>,
    /// Буфер обхода при пересчёте порядка.
    pub stack: Vec<GameObjectId>,
    /// Порядок выполнения по имени типа компонента (из настроек проекта).
    pub overrides: HashMap<String, i32>,
    /// Сколько живых объектов имеют компонент с порядком не 0. Ноль — диспетчер идёт по
    /// быстрому пути, как до появления порядка выполнения.
    pub ordered_objects: usize,
    /// Растёт при любом изменении состава компонентов (в том числе сжатии надгробий).
    pub comps_version: u64,
    pub ordered_lists: Vec<OrderedList>,
}

impl SceneIndex {
    fn info_mut(&mut self, slot: u32) -> &mut SlotInfo {
        let slot = slot as usize;
        if self.slots.len() <= slot {
            self.slots.resize_with(slot + 1, SlotInfo::default);
        }
        &mut self.slots[slot]
    }

    /// Жив ли `id` с точки зрения индекса.
    pub fn is_live(&self, id: GameObjectId) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|info| info.live && info.generation == id.generation)
    }

    /// Новый объект в слоте `id.index`.
    pub fn born(&mut self, id: GameObjectId, spawned: u64) {
        let info = self.info_mut(id.index);
        info.generation = id.generation;
        info.live = true;
        info.types.clear();
        info.hooks = Hooks::NONE;
        info.spawned = spawned;
        if info.ordered {
            info.ordered = false;
            self.ordered_objects -= 1;
        }
    }

    /// Объект уничтожен: убрать его отовсюду.
    pub fn forget(&mut self, id: GameObjectId) {
        if !self.is_live(id) {
            return;
        }
        let info = &mut self.slots[id.index as usize];
        info.live = false;
        if info.ordered {
            info.ordered = false;
            self.ordered_objects -= 1;
        }
        self.comps_version += 1;
        let types = std::mem::take(&mut info.types);
        let had_hooks = !info.hooks.is_empty();
        info.hooks = Hooks::NONE;
        for type_id in &types {
            if let Some(entry) = self.types.get_mut(type_id) {
                entry.members.remove(&id);
                entry.version += 1;
            }
        }
        self.slots[id.index as usize].types = types;
        self.slots[id.index as usize].types.clear();
        if had_hooks {
            self.hooks_version += 1;
        }
        self.pending_start.remove(&id);
        if self.order_for.is_some() {
            self.dead_in_order += 1;
        }
    }

    /// Пересчитать то, что индекс знает об объекте `id`, по его компонентам.
    pub fn update(&mut self, id: GameObjectId, object: &GameObject, spawned: u64) {
        if !self.is_live(id) {
            self.born(id, spawned);
        }
        let mut types: Vec<TypeId> = Vec::with_capacity(object.components.len());
        let mut hooks = Hooks::NONE;
        let mut unstarted = false;
        let mut ordered = false;
        for slot in object.components.iter().filter(|slot| !slot.removed) {
            if !types.contains(&slot.type_id) {
                types.push(slot.type_id);
            }
            hooks |= slot.hooks;
            unstarted |= !slot.started;
            // Порядок выполнения: настройка проекта перекрывает значение из кода.
            let order = if self.overrides.is_empty() {
                slot.default_order
            } else {
                self.overrides.get(slot.name).copied().unwrap_or(slot.default_order)
            };
            slot.order.set(order);
            ordered |= order != 0;
        }
        self.comps_version += 1;

        let info = &mut self.slots[id.index as usize];
        if info.ordered != ordered {
            info.ordered = ordered;
            if ordered {
                self.ordered_objects += 1;
            } else {
                self.ordered_objects -= 1;
            }
        }
        let old = std::mem::replace(&mut info.types, types);
        let old_hooks = std::mem::replace(&mut info.hooks, hooks);
        let new = &self.slots[id.index as usize].types;
        for type_id in old.iter().filter(|t| !new.contains(t)) {
            if let Some(entry) = self.types.get_mut(type_id) {
                entry.members.remove(&id);
                entry.version += 1;
            }
        }
        for type_id in new.iter().filter(|t| !old.contains(t)) {
            let entry = self.types.entry(*type_id).or_default();
            entry.members.insert(id);
            entry.version += 1;
        }
        if old_hooks != hooks {
            self.hooks_version += 1;
        }
        if unstarted {
            self.pending_start.insert(id);
        }
        if object.has_tombstones {
            self.tombstones.push(id.index);
        }
    }

    /// Новый объект дописан в конец порядка иерархии (порядок остаётся верным).
    pub fn append_to_order(&mut self, id: GameObjectId) {
        let slot = id.index as usize;
        if self.rank.len() <= slot {
            self.rank.resize(slot + 1, u32::MAX);
        }
        self.rank[slot] = self.order.len() as u32;
        self.order.push(id);
    }

    /// Место объекта в порядке иерархии (`u32::MAX` — неизвестно или уничтожен).
    pub fn rank_of(&self, id: GameObjectId) -> u32 {
        if !self.is_live(id) {
            return u32::MAX;
        }
        self.rank.get(id.index as usize).copied().unwrap_or(u32::MAX)
    }

    /// Объекты с компонентом типа `type_id` в порядке иерархии. Порядок (`order`)
    /// должен быть актуален.
    pub fn ordered_of(&mut self, type_id: TypeId) -> &[GameObjectId] {
        let SceneIndex {
            types,
            order,
            rank,
            slots,
            order_epoch,
            ..
        } = self;
        let Some(entry) = types.get_mut(&type_id) else {
            return &[];
        };
        let stamp = (*order_epoch, entry.version);
        if entry.ordered_stamp != Some(stamp) {
            entry.ordered.clear();
            let live = |id: &GameObjectId| {
                slots
                    .get(id.index as usize)
                    .is_some_and(|info| info.live && info.generation == id.generation)
            };
            if entry.members.len() * 8 > order.len() {
                // Объектов с этим компонентом много: дешевле пройти порядок целиком.
                entry.ordered.extend(order.iter().copied().filter(|id| {
                    live(id) && slots[id.index as usize].types.contains(&type_id)
                }));
            } else {
                entry.ordered.extend(entry.members.iter().copied());
                entry
                    .ordered
                    .sort_unstable_by_key(|id| rank.get(id.index as usize).copied().unwrap_or(u32::MAX));
            }
            entry.ordered_stamp = Some(stamp);
        }
        &entry.ordered
    }

    /// Объекты, у которых есть хук `hook`, в порядке иерархии. `order` должен быть актуален.
    pub fn hook_list(&mut self, hook: Hooks) -> &[GameObjectId] {
        let position = match self.hook_lists.iter().position(|list| list.hook == hook) {
            Some(position) => position,
            None => {
                self.hook_lists.push(HookList {
                    hook,
                    stamp: None,
                    ids: Vec::new(),
                });
                self.hook_lists.len() - 1
            }
        };
        let stamp = (self.order_epoch, self.hooks_version);
        let SceneIndex {
            hook_lists,
            order,
            slots,
            ..
        } = self;
        let list = &mut hook_lists[position];
        if list.stamp != Some(stamp) {
            list.ids.clear();
            list.ids.extend(order.iter().copied().filter(|id| {
                slots.get(id.index as usize).is_some_and(|info| {
                    info.live && info.generation == id.generation && info.hooks.intersects(hook)
                })
            }));
            list.stamp = Some(stamp);
        }
        &list.ids
    }

    /// Сколько объектов с компонентом типа `type_id`.
    pub fn count_of(&self, type_id: TypeId) -> usize {
        self.types.get(&type_id).map_or(0, |entry| entry.members.len())
    }
}

impl SceneIndex {
    /// Кэш порядка выполнения для хука `hook` (создаётся при первом обращении).
    pub fn ordered_list_mut(&mut self, hook: Hooks) -> &mut OrderedList {
        let position = match self.ordered_lists.iter().position(|list| list.hook == hook) {
            Some(position) => position,
            None => {
                self.ordered_lists.push(OrderedList {
                    hook,
                    stamp: None,
                    entries: Vec::new(),
                });
                self.ordered_lists.len() - 1
            }
        };
        &mut self.ordered_lists[position]
    }

    /// Отметка актуальности кэшей порядка выполнения.
    pub fn ordered_stamp(&self) -> (u64, u64, u64) {
        (self.order_epoch, self.hooks_version, self.comps_version)
    }
}
