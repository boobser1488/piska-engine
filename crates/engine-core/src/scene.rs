//! Scene — граф GameObject'ов.

use std::any::TypeId;
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::component::{Component, Hooks};
use crate::error::SceneError;
use crate::index::SceneIndex;
use crate::math::{Mat4, Quat, Vec3};
use crate::object::{GameObject, GameObjectId, Journal};

/// Счётчик для [`Scene::instance_id`].
static NEXT_SCENE: AtomicU64 = AtomicU64::new(1);

struct Slot {
    generation: u32,
    obj: Option<GameObject>,
}

/// Сцена владеет всеми объектами. Доступ — по [`GameObjectId`].
pub struct Scene {
    pub name: String,
    slots: Vec<Slot>,
    free: Vec<u32>,
    roots: Vec<GameObjectId>,
    pending_destroy: Vec<GameObjectId>,
    /// Поколение для новых слотов. У сцены-преемницы оно выше любого поколения
    /// прежней сцены, поэтому старые id никогда не совпадут с новыми.
    first_generation: u32,
    /// Журнал изменений состава компонентов (общий с объектами, см. `crate::index`).
    journal: Journal,
    /// Индексы «тип → объекты», порядок иерархии, списки для диспетчера хуков.
    index: RefCell<SceneIndex>,
    /// Версия иерархии: растёт при создании объектов и смене родителя / порядка братьев.
    hierarchy: u64,
    /// Сколько объектов создано за жизнь сцены.
    spawned: u64,
    instance: u64,
    /// В `roots` остались уничтоженные объекты (см. `remove_subtree_lazy`).
    dead_roots: bool,
    /// Стабильный номер объекта → объект (поля-ссылки, см. `crate::objref`).
    uids: HashMap<u64, GameObjectId>,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Scene {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            slots: Vec::new(),
            free: Vec::new(),
            roots: Vec::new(),
            pending_destroy: Vec::new(),
            first_generation: 1,
            journal: Rc::new(RefCell::new(Vec::new())),
            index: RefCell::new(SceneIndex::default()),
            hierarchy: 0,
            spawned: 0,
            dead_roots: false,
            uids: HashMap::new(),
            instance: NEXT_SCENE.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Число, уникальное для каждой сцены в процессе: у сцены-преемницы (загрузка
    /// сцены, Stop в редакторе) оно другое. Подсистемы по нему замечают, что сцену заменили.
    pub fn instance_id(&self) -> u64 {
        self.instance
    }

    /// Пустая сцена, которая заменит эту (загрузка сцены, выход из Play в редакторе).
    /// Все id прежней сцены в ней гарантированно «протухшие»: подсистемы, которые
    /// помнят объекты по id (физика), увидят, что тех больше нет.
    pub fn successor(&self, name: impl Into<String>) -> Scene {
        let newest = self
            .slots
            .iter()
            .map(|slot| slot.generation)
            .max()
            .unwrap_or(0)
            .max(self.first_generation);
        let mut scene = Scene {
            first_generation: newest + 1,
            ..Scene::new(name)
        };
        // Порядок выполнения из настроек проекта переживает смену сцены.
        scene.index.get_mut().overrides = self.index.borrow().overrides.clone();
        scene
    }

    /// Создать пустой объект в корне сцены.
    pub fn spawn(&mut self, name: impl Into<String>) -> GameObjectId {
        let mut obj = GameObject::new(name.into());
        obj.journal = Some(self.journal.clone());
        obj.uid = loop {
            let uid = crate::objref::fresh_uid();
            if !self.uids.contains_key(&uid) {
                break uid;
            }
        };
        let uid = obj.uid;
        let id = match self.free.pop() {
            Some(index) => {
                obj.slot = index;
                let slot = &mut self.slots[index as usize];
                slot.obj = Some(obj);
                GameObjectId {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                obj.slot = self.slots.len() as u32;
                self.slots.push(Slot {
                    generation: self.first_generation,
                    obj: Some(obj),
                });
                GameObjectId {
                    index: (self.slots.len() - 1) as u32,
                    generation: self.first_generation,
                }
            }
        };
        self.uids.insert(uid, id);
        self.roots.push(id);
        self.spawned += 1;
        let spawned = self.spawned;
        let hierarchy = self.hierarchy;
        let index = self.index.get_mut();
        index.born(id, spawned);
        // Новый корень — последний в порядке иерархии: порядок остаётся верным.
        let still_valid = index.order_for == Some(hierarchy);
        if still_valid {
            index.append_to_order(id);
        }
        self.hierarchy += 1;
        if still_valid {
            self.index.get_mut().order_for = Some(self.hierarchy);
        }
        id
    }

    /// Создать объект «цепочкой»:
    /// `scene.build("Player").tag("Player").at(0.0, 2.0).with(Health::default()).id()`.
    pub fn build(&mut self, name: impl Into<String>) -> ObjectBuilder<'_> {
        let id = self.spawn(name);
        ObjectBuilder { scene: self, id }
    }

    pub fn get(&self, id: GameObjectId) -> Option<&GameObject> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation == id.generation {
            slot.obj.as_ref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, id: GameObjectId) -> Option<&mut GameObject> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation == id.generation {
            slot.obj.as_mut()
        } else {
            None
        }
    }

    pub fn contains(&self, id: GameObjectId) -> bool {
        self.get(id).is_some()
    }

    /// Живой объект по стабильному номеру ([`GameObject::uid`]); `None` для 0, для
    /// уничтоженного объекта и для номера из другой сцены.
    pub fn find_by_uid(&self, uid: u64) -> Option<GameObjectId> {
        if uid == 0 {
            return None;
        }
        let id = *self.uids.get(&uid)?;
        self.get(id).filter(|o| o.uid == uid).map(|_| id)
    }

    /// Объект, на который ссылается поле (см. [`ObjectRef`](crate::objref::ObjectRef)).
    pub fn resolve(&self, reference: crate::objref::ObjectRef) -> Option<GameObjectId> {
        self.find_by_uid(reference.uid())
    }

    /// Задать объекту стабильный номер (загрузчик сцен ставит номер из файла).
    /// `false` — объекта нет, номер 0 или его уже носит другой объект этой сцены.
    pub fn set_uid(&mut self, id: GameObjectId, uid: u64) -> bool {
        let uid = uid & crate::objref::UID_MASK;
        if uid == 0 || self.find_by_uid(uid).is_some_and(|other| other != id) {
            return false;
        }
        let Some(obj) = self.get_mut(id) else {
            return false;
        };
        let old = std::mem::replace(&mut obj.uid, uid);
        if self.uids.get(&old) == Some(&id) {
            self.uids.remove(&old);
        }
        self.uids.insert(uid, id);
        true
    }

    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Объекты верхнего уровня, в порядке окна иерархии.
    pub fn roots(&self) -> &[GameObjectId] {
        &self.roots
    }

    /// Все объекты в порядке иерархии (родитель раньше детей).
    pub fn ids(&self) -> Vec<GameObjectId> {
        let mut out = Vec::with_capacity(self.len());
        for &root in &self.roots {
            self.collect(root, &mut out);
        }
        out
    }

    /// Как [`ids`](Self::ids), но в готовый вектор (он очищается): без выделения памяти,
    /// если его ёмкости хватает. Так каждый кадр обходит сцену диспетчер хуков.
    pub fn ids_into(&self, out: &mut Vec<GameObjectId>) {
        out.clear();
        out.reserve(self.len());
        for &root in &self.roots {
            self.collect(root, out);
        }
    }

    /// Обойти объекты, активные в иерархии (Unity: `activeInHierarchy`), в порядке
    /// иерархии — как `ids()` + `is_active`, но без списка всех id и без подъёма к
    /// родителям для каждого объекта: неактивное поддерево пропускается целиком.
    /// Подсистемы, которые каждый кадр ищут «свои» компоненты, обходят сцену так.
    pub fn visit_active(&self, mut f: impl FnMut(GameObjectId, &GameObject)) {
        let mut stack: Vec<GameObjectId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(obj) = self.get(id) else { continue };
            if !obj.active {
                continue;
            }
            f(id, obj);
            stack.extend(obj.children.iter().rev().copied());
        }
    }

    /// Объект и все его потомки (родитель раньше детей).
    pub fn subtree(&self, root: GameObjectId) -> Vec<GameObjectId> {
        let mut out = Vec::new();
        self.collect(root, &mut out);
        out
    }

    fn collect(&self, id: GameObjectId, out: &mut Vec<GameObjectId>) {
        if let Some(obj) = self.get(id) {
            out.push(id);
            for &child in &obj.children {
                self.collect(child, out);
            }
        }
    }

    /// Unity: `transform.SetParent(parent, worldPositionStays: false)`.
    /// `None` — вынести в корень сцены.
    pub fn set_parent(
        &mut self,
        child: GameObjectId,
        parent: Option<GameObjectId>,
    ) -> Result<(), SceneError> {
        let old_parent = self.get(child).ok_or(SceneError::DeadObject(child))?.parent;
        if let Some(new_parent) = parent {
            let mut cursor = Some(new_parent);
            while let Some(current) = cursor {
                let obj = self.get(current).ok_or(SceneError::DeadObject(current))?;
                if current == child {
                    return Err(SceneError::Cycle {
                        child: self.get(child).map(|o| o.name.clone()).unwrap_or_default(),
                        parent: self
                            .get(new_parent)
                            .map(|o| o.name.clone())
                            .unwrap_or_default(),
                    });
                }
                cursor = obj.parent;
            }
        }
        match old_parent {
            Some(old) => {
                if let Some(obj) = self.get_mut(old) {
                    obj.children.retain(|c| *c != child);
                }
            }
            None => self.roots.retain(|r| *r != child),
        }
        let keeps_order = self.reparent_keeps_order(child, parent);
        match parent {
            Some(new_parent) => {
                if let Some(obj) = self.get_mut(new_parent) {
                    obj.children.push(child);
                }
            }
            None => self.roots.push(child),
        }
        if let Some(obj) = self.get_mut(child) {
            obj.parent = parent;
        }
        self.hierarchy += 1;
        if keeps_order {
            self.index.get_mut().order_for = Some(self.hierarchy);
        }
        Ok(())
    }

    /// Останется ли порядок иерархии верным, если `child` станет последним ребёнком
    /// `parent` (или последним корнем). Так бывает, когда `child` — только что созданный
    /// объект без детей (он последний в порядке), а поддерево `parent` тоже в самом
    /// конце: префабы, `build(..).child_of(..)`. Тогда порядок не пересчитывается.
    fn reparent_keeps_order(&mut self, child: GameObjectId, parent: Option<GameObjectId>) -> bool {
        let hierarchy = self.hierarchy;
        let childless = self.get(child).is_some_and(|o| o.children.is_empty());
        let index = self.index.get_mut();
        if index.order_for != Some(hierarchy) || !childless || index.order.last() != Some(&child) {
            return false;
        }
        let Some(parent) = parent else {
            return true;
        };
        let Some(previous) = index.order.len().checked_sub(2).map(|i| index.order[i]) else {
            return false;
        };
        // Предыдущий в порядке — сам `parent` или его потомок: значит, поддерево
        // `parent` заканчивается прямо перед `child`.
        let mut cursor = Some(previous);
        while let Some(current) = cursor {
            if current == parent {
                return true;
            }
            cursor = self.get(current).and_then(|o| o.parent);
        }
        false
    }

    /// Переставить объект среди «братьев» (порядок окна иерархии).
    pub fn set_sibling_index(&mut self, id: GameObjectId, index: usize) {
        let Some(parent) = self.get(id).map(|o| o.parent) else {
            return;
        };
        let siblings = match parent {
            Some(parent) => match self.get_mut(parent) {
                Some(obj) => &mut obj.children,
                None => return,
            },
            None => &mut self.roots,
        };
        if let Some(current) = siblings.iter().position(|s| *s == id) {
            siblings.remove(current);
            siblings.insert(index.min(siblings.len()), id);
        }
        self.hierarchy += 1;
    }

    /// Unity: `transform.SetParent(parent, worldPositionStays: true)` — сменить
    /// родителя, сохранив мировую позу объекта.
    pub fn set_parent_keep_world(
        &mut self,
        child: GameObjectId,
        parent: Option<GameObjectId>,
    ) -> Result<(), SceneError> {
        let world = self.world_matrix(child);
        self.set_parent(child, parent)?;
        let parent_world = parent.map_or(Mat4::IDENTITY, |p| self.world_matrix(p));
        let (scale, rotation, position) =
            (parent_world.inverse() * world).to_scale_rotation_translation();
        if let Some(obj) = self.get_mut(child) {
            obj.transform.position = position;
            obj.transform.rotation = rotation;
            obj.transform.scale = scale;
        }
        Ok(())
    }

    /// Матрица «локальное → мировое» с учётом всех предков.
    pub fn world_matrix(&self, id: GameObjectId) -> Mat4 {
        let mut matrix = Mat4::IDENTITY;
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            let Some(obj) = self.get(current) else { break };
            matrix = obj.transform.local_matrix() * matrix;
            cursor = obj.parent;
        }
        matrix
    }

    pub fn world_position(&self, id: GameObjectId) -> Vec3 {
        self.world_matrix(id).w_axis.truncate()
    }

    pub fn world_rotation(&self, id: GameObjectId) -> Quat {
        self.world_matrix(id).to_scale_rotation_translation().1
    }

    /// Unity: `activeInHierarchy` — активен сам объект и все его предки.
    pub fn is_active(&self, id: GameObjectId) -> bool {
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            match self.get(current) {
                Some(obj) if obj.active => cursor = obj.parent,
                _ => return false,
            }
        }
        true
    }

    /// Unity: `GameObject.Find(name)`.
    pub fn find_by_name(&self, name: &str) -> Option<GameObjectId> {
        self.ids()
            .into_iter()
            .find(|id| self.get(*id).is_some_and(|o| o.name == name))
    }

    /// Unity: `GameObject.FindWithTag(tag)`.
    pub fn find_by_tag(&self, tag: &str) -> Option<GameObjectId> {
        self.ids()
            .into_iter()
            .find(|id| self.get(*id).is_some_and(|o| o.tag == tag))
    }

    /// Unity: `GameObject.FindGameObjectsWithTag(tag)`.
    pub fn find_all_by_tag(&self, tag: &str) -> Vec<GameObjectId> {
        self.ids()
            .into_iter()
            .filter(|id| self.get(*id).is_some_and(|o| o.tag == tag))
            .collect()
    }

    /// Unity: `FindObjectsOfType<T>()` — все объекты с компонентом `T` (и неактивные),
    /// в порядке иерархии. Стоит пропорционально числу таких объектов, а не размеру
    /// сцены: у сцены есть индекс «тип компонента → объекты».
    pub fn find_with<T: Component>(&self) -> Vec<GameObjectId> {
        let mut ids = self.indexed::<T>();
        ids.retain(|id| self.get(*id).is_some_and(|o| o.has::<T>()));
        ids
    }

    /// Сколько объектов несут компонент `T` (и неактивные) — без обхода сцены.
    /// Подсистема может сразу выйти, если «её» компонентов нет.
    pub fn count_with<T: Component>(&self) -> usize {
        self.refresh_index();
        self.index.borrow().count_of(TypeId::of::<T>())
    }

    /// Вызвать `f` для каждого объекта с компонентом `T` (и неактивного) в порядке
    /// иерархии. Как [`find_with`](Self::find_with), но без списка.
    pub fn visit_with<T: Component>(&self, mut f: impl FnMut(GameObjectId, &T)) {
        for id in self.indexed::<T>() {
            if let Some(component) = self.get_component::<T>(id) {
                f(id, component);
            }
        }
    }

    /// Вызвать `f` для каждого объекта с компонентом `T`, активного в иерархии, в
    /// порядке иерархии. Так подсистемы находят «свои» компоненты каждый кадр.
    pub fn visit_active_with<T: Component>(&self, mut f: impl FnMut(GameObjectId, &T)) {
        for id in self.indexed::<T>() {
            if !self.is_active(id) {
                continue;
            }
            if let Some(component) = self.get_component::<T>(id) {
                f(id, component);
            }
        }
    }

    /// Упорядочить объекты как в окне иерархии (родитель раньше детей); уничтоженные — в конец.
    pub fn sort_by_hierarchy(&self, ids: &mut [GameObjectId]) {
        let index = self.prepared_index();
        ids.sort_by_key(|id| index.rank_of(*id));
    }

    /// Объекты, у которых в индексе есть компонент `T`, в порядке иерархии.
    fn indexed<T: Component>(&self) -> Vec<GameObjectId> {
        let mut index = self.prepared_index();
        index.ordered_of(TypeId::of::<T>()).to_vec()
    }

    /// Все объекты с тегом `tag`, которые активны в иерархии.
    pub fn find_all_active_by_tag(&self, tag: &str) -> Vec<GameObjectId> {
        self.ids()
            .into_iter()
            .filter(|id| self.get(*id).is_some_and(|o| o.tag == tag) && self.is_active(*id))
            .collect()
    }

    /// Сколько объектов с тегом `tag` (включая неактивные).
    pub fn count_with_tag(&self, tag: &str) -> usize {
        self.ids()
            .into_iter()
            .filter(|id| self.get(*id).is_some_and(|o| o.tag == tag))
            .count()
    }

    /// Пары «объект, компонент `T`» по всей сцене, в порядке иерархии (включая неактивные).
    ///
    /// ```
    /// # use engine_core::prelude::*;
    /// # component! { pub struct Enemy { pub hp: i32 = 3 } }
    /// # impl Component for Enemy {}
    /// let mut scene = Scene::new("s");
    /// scene.build("A").with(Enemy::default());
    /// scene.build("B").with(Enemy { hp: 5 });
    /// let total: i32 = scene.iter_with::<Enemy>().map(|(_, e)| e.hp).sum();
    /// assert_eq!(total, 8);
    /// ```
    pub fn iter_with<T: Component>(&self) -> impl Iterator<Item = (GameObjectId, &T)> + '_ {
        self.indexed::<T>()
            .into_iter()
            .filter_map(move |id| Some((id, self.get(id)?.get::<T>()?)))
    }

    /// То же, но только для объектов, активных в иерархии (Unity: `FindObjectsOfType<T>()`).
    pub fn iter_active_with<T: Component>(&self) -> impl Iterator<Item = (GameObjectId, &T)> + '_ {
        self.iter_with::<T>().filter(move |(id, _)| self.is_active(*id))
    }

    /// Вызвать `f` для каждого компонента `T` в сцене (включая неактивные объекты).
    pub fn for_each_with_mut<T: Component>(&mut self, mut f: impl FnMut(GameObjectId, &mut T)) {
        for id in self.indexed::<T>() {
            if let Some(component) = self.get_mut(id).and_then(|o| o.get_mut::<T>()) {
                f(id, component);
            }
        }
    }

    /// Родитель объекта.
    pub fn parent_of(&self, id: GameObjectId) -> Option<GameObjectId> {
        self.get(id)?.parent
    }

    /// Корень иерархии, в которой лежит объект (сам объект, если он в корне).
    pub fn root_of(&self, id: GameObjectId) -> Option<GameObjectId> {
        let mut current = id;
        self.get(current)?;
        while let Some(parent) = self.get(current)?.parent {
            current = parent;
        }
        Some(current)
    }

    /// Прямой ребёнок с именем `name` (Unity: `transform.Find`).
    pub fn find_child(&self, parent: GameObjectId, name: &str) -> Option<GameObjectId> {
        self.get(parent)?
            .children
            .iter()
            .copied()
            .find(|c| self.get(*c).is_some_and(|o| o.name == name))
    }

    /// Потомок по пути из имён через `/`: `scene.find_path(player, "Arm/Hand")`.
    pub fn find_path(&self, root: GameObjectId, path: &str) -> Option<GameObjectId> {
        path.split('/')
            .filter(|p| !p.is_empty())
            .try_fold(root, |current, part| self.find_child(current, part))
    }

    /// Unity: `GetComponentInChildren<T>()` — сам объект, затем потомки в глубину.
    /// Возвращает объект, на котором нашёлся компонент.
    pub fn find_in_children<T: Component>(&self, id: GameObjectId) -> Option<GameObjectId> {
        self.subtree(id)
            .into_iter()
            .find(|c| self.get(*c).is_some_and(|o| o.has::<T>()))
    }

    /// Unity: `GetComponentsInChildren<T>()` — объекты поддерева (с самим собой), у которых есть `T`.
    pub fn find_all_in_children<T: Component>(&self, id: GameObjectId) -> Vec<GameObjectId> {
        self.subtree(id)
            .into_iter()
            .filter(|c| self.get(*c).is_some_and(|o| o.has::<T>()))
            .collect()
    }

    /// Unity: `GetComponentInParent<T>()` — сам объект, затем предки вверх.
    pub fn find_in_parent<T: Component>(&self, id: GameObjectId) -> Option<GameObjectId> {
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            let obj = self.get(current)?;
            if obj.has::<T>() {
                return Some(current);
            }
            cursor = obj.parent;
        }
        None
    }

    pub fn component_in_children<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        let found = self.find_in_children::<T>(id)?;
        self.get_component::<T>(found)
    }

    pub fn component_in_children_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        let found = self.find_in_children::<T>(id)?;
        self.get_component_mut::<T>(found)
    }

    pub fn component_in_parent<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        let found = self.find_in_parent::<T>(id)?;
        self.get_component::<T>(found)
    }

    pub fn component_in_parent_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        let found = self.find_in_parent::<T>(id)?;
        self.get_component_mut::<T>(found)
    }

    /// Unity: `SetActive`. Неактивный объект и всё его поддерево не получают `on_update`,
    /// не рисуются и не участвуют в физике (рендер и физика спрашивают [`is_active`](Self::is_active)).
    /// `false`, если объекта нет.
    pub fn set_active(&mut self, id: GameObjectId, active: bool) -> bool {
        match self.get_mut(id) {
            Some(obj) => {
                obj.active = active;
                true
            }
            None => false,
        }
    }

    /// Unity: `DontDestroyOnLoad`. Объект переживёт замену сцены (`ctx.load_scene` и
    /// `World::load_scene`), и его `GameObjectId` останется действительным. Флаг ставится
    /// на **корень** иерархии, в которой лежит объект, — переживает вся иерархия.
    pub fn set_persistent(&mut self, id: GameObjectId, persistent: bool) -> bool {
        match self.root_of(id) {
            Some(root) => {
                if let Some(obj) = self.get_mut(root) {
                    obj.persistent = persistent;
                }
                true
            }
            None => false,
        }
    }

    /// Unity: `GetComponent<T>()` по id объекта.
    pub fn get_component<T: Component>(&self, id: GameObjectId) -> Option<&T> {
        self.get(id)?.get::<T>()
    }

    pub fn get_component_mut<T: Component>(&mut self, id: GameObjectId) -> Option<&mut T> {
        self.get_mut(id)?.get_mut::<T>()
    }

    /// Unity: `Destroy(obj)`. Отложено до конца кадра; дети уничтожаются вместе с родителем.
    pub fn destroy(&mut self, id: GameObjectId) {
        if self.contains(id) && !self.pending_destroy.contains(&id) {
            self.pending_destroy.push(id);
        }
    }

    pub(crate) fn take_pending_destroy(&mut self) -> Vec<GameObjectId> {
        std::mem::take(&mut self.pending_destroy)
    }

    /// Немедленно удалить объект с потомками, без хуков. Хуки вызывает `World`.
    pub(crate) fn remove_subtree(&mut self, root: GameObjectId) {
        self.remove_subtree_lazy(root);
        self.prune_roots();
    }

    /// Как `remove_subtree`, но уничтоженный корень может остаться в списке корней до
    /// [`prune_roots`](Self::prune_roots): удаление сотни корней из сцены в 20 000
    /// объектов — один проход по списку, а не сотня.
    pub(crate) fn remove_subtree_lazy(&mut self, root: GameObjectId) {
        let Some(parent) = self.get(root).map(|o| o.parent) else {
            return;
        };
        match parent {
            Some(parent) => {
                if let Some(obj) = self.get_mut(parent) {
                    obj.children.retain(|c| *c != root);
                }
            }
            None if self.roots.last() == Some(&root) => {
                self.roots.pop();
            }
            None => self.dead_roots = true,
        }
        let doomed = self.subtree(root);
        let index = self.index.get_mut();
        for id in doomed {
            index.forget(id);
            let slot = &mut self.slots[id.index as usize];
            if let Some(uid) = slot.obj.as_ref().map(|o| o.uid) {
                if self.uids.get(&uid) == Some(&id) {
                    self.uids.remove(&uid);
                }
            }
            slot.obj = None;
            slot.generation += 1;
            self.free.push(id.index);
        }
        // Порядок остальных объектов не изменился; уничтоженные остаются в нём «дырами»,
        // пока их не станет слишком много.
        let index = self.index.get_mut();
        if index.dead_in_order > 1024 && index.dead_in_order * 2 > index.order.len() {
            self.hierarchy += 1;
        }
    }

    /// Убрать из списка корней уничтоженные объекты (см. `remove_subtree_lazy`).
    pub(crate) fn prune_roots(&mut self) {
        if std::mem::take(&mut self.dead_roots) {
            let slots = &self.slots;
            self.roots.retain(|id| {
                slots
                    .get(id.index as usize)
                    .is_some_and(|slot| slot.generation == id.generation && slot.obj.is_some())
            });
        }
    }

    /// Сжать списки компонентов после `remove::<T>()`. Вызывается в конце кадра и
    /// смотрит только на объекты, у которых что-то удаляли.
    pub(crate) fn compact_components(&mut self) {
        self.refresh_index();
        let tombstones = std::mem::take(&mut self.index.get_mut().tombstones);
        for slot in tombstones {
            let Some(obj) = self.slots.get_mut(slot as usize).and_then(|s| s.obj.as_mut()) else {
                continue;
            };
            if obj.has_tombstones {
                obj.components.retain(|slot| !slot.removed);
                obj.has_tombstones = false;
                // Номера компонентов сдвинулись: кэши порядка выполнения устарели.
                self.index.get_mut().comps_version += 1;
            }
        }
    }

    // ------------------------------------------------------------------
    // Индекс (см. `crate::index`)
    // ------------------------------------------------------------------

    /// Разобрать журнал: обновить индекс у объектов, где менялся состав компонентов.
    fn refresh_index(&self) {
        if self.journal.borrow().is_empty() {
            return;
        }
        let changed = std::mem::take(&mut *self.journal.borrow_mut());
        let mut index = self.index.borrow_mut();
        for slot in changed {
            let Some(entry) = self.slots.get(slot as usize) else {
                continue;
            };
            let Some(obj) = entry.obj.as_ref() else {
                continue;
            };
            obj.queued.set(false);
            let id = GameObjectId {
                index: slot,
                generation: entry.generation,
            };
            let spawned = index.slots.get(slot as usize).map_or(0, |info| info.spawned);
            index.update(id, obj, spawned);
        }
    }

    /// Индекс, в котором разобран журнал и посчитан порядок иерархии.
    fn prepared_index(&self) -> RefMut<'_, SceneIndex> {
        self.refresh_index();
        let mut index = self.index.borrow_mut();
        if index.order_for != Some(self.hierarchy) {
            self.rebuild_order(&mut index);
        }
        index
    }

    fn rebuild_order(&self, index: &mut SceneIndex) {
        let mut stack = std::mem::take(&mut index.stack);
        index.order.clear();
        index.order.reserve(self.len());
        if index.rank.len() < self.slots.len() {
            index.rank.resize(self.slots.len(), u32::MAX);
        }
        stack.clear();
        stack.extend(self.roots.iter().rev().copied());
        while let Some(id) = stack.pop() {
            let Some(obj) = self.get(id) else { continue };
            index.rank[id.index as usize] = index.order.len() as u32;
            index.order.push(id);
            stack.extend(obj.children.iter().rev().copied());
        }
        index.stack = stack;
        index.order_for = Some(self.hierarchy);
        index.order_epoch += 1;
        index.dead_in_order = 0;
    }

    /// Объекты, у которых есть компонент с хуком `hook`, в порядке иерархии (в `out`).
    pub(crate) fn hook_targets(&self, hook: Hooks, out: &mut Vec<GameObjectId>) {
        let mut index = self.prepared_index();
        out.clear();
        out.extend_from_slice(index.hook_list(hook));
    }

    // ------------------------------------------------------------------
    // Порядок выполнения (Unity: Script Execution Order)
    // ------------------------------------------------------------------

    /// Порядок выполнения по имени типа компонента, который сейчас действует
    /// (настройки проекта). Пусто — у всех порядок из кода (`Component::execution_order`).
    pub fn execution_orders(&self) -> std::collections::HashMap<String, i32> {
        self.index.borrow().overrides.clone()
    }

    /// Задать порядок выполнения по именам типов: значение перекрывает
    /// `Component::execution_order`. Действует на уже созданные компоненты сразу (список
    /// вызова пересортируется один раз, а не каждый кадр) и на созданные позже.
    pub fn set_execution_orders(&mut self, orders: std::collections::HashMap<String, i32>) {
        self.index.get_mut().overrides = orders;
        // Пересчитать порядок у всех компонентов: объекты уходят в журнал индекса.
        for (slot, entry) in self.slots.iter().enumerate() {
            let Some(obj) = entry.obj.as_ref() else { continue };
            if obj.components.is_empty() || obj.queued.get() {
                continue;
            }
            self.journal.borrow_mut().push(slot as u32);
            obj.queued.set(true);
        }
    }

    /// Действующий порядок выполнения компонента `index` объекта `id`
    /// (`None` — нет такого компонента).
    pub fn execution_order_of(&self, id: GameObjectId, index: usize) -> Option<i32> {
        self.refresh_index();
        let slot = self.get(id)?.components.get(index)?;
        (!slot.removed).then(|| slot.order.get())
    }

    /// Компоненты с хуком `hook` в порядке выполнения. `false` — в сцене нет компонентов
    /// с порядком не 0, и вызывающий идёт по обычному списку объектов (`hook_targets`).
    /// Иначе `out` — пары «объект, номер компонента на объекте».
    pub(crate) fn hook_entries(&self, hook: Hooks, out: &mut Vec<(GameObjectId, u32)>) -> bool {
        let mut index = self.prepared_index();
        if index.ordered_objects == 0 {
            return false;
        }
        let stamp = index.ordered_stamp();
        if index.ordered_list_mut(hook).stamp != Some(stamp) {
            let ids: Vec<GameObjectId> = index.hook_list(hook).to_vec();
            let mut keyed: Vec<(i32, GameObjectId, u32)> = Vec::with_capacity(ids.len());
            for id in ids {
                let Some(obj) = self.get(id) else { continue };
                for (slot_index, slot) in obj.components.iter().enumerate() {
                    if !slot.removed && slot.hooks.intersects(hook) {
                        keyed.push((slot.order.get(), id, slot_index as u32));
                    }
                }
            }
            // Сортировка устойчива: при равном порядке остаётся иерархия, потом слот.
            keyed.sort_by_key(|entry| entry.0);
            let list = index.ordered_list_mut(hook);
            list.entries.clear();
            list.entries.extend(keyed.into_iter().map(|(_, id, slot)| (id, slot)));
            list.stamp = Some(stamp);
        }
        out.clear();
        out.extend_from_slice(&index.ordered_list_mut(hook).entries);
        true
    }

    /// Для фазы `on_start` в сцене с порядком выполнения: не стартовавшие компоненты
    /// объектов `ids` (в порядке иерархии) — по порядку выполнения.
    pub(crate) fn start_entries(&self, ids: &[GameObjectId], out: &mut Vec<(GameObjectId, u32)>) {
        let mut keyed: Vec<(i32, GameObjectId, u32)> = Vec::new();
        for &id in ids {
            let Some(obj) = self.get(id) else { continue };
            for (slot_index, slot) in obj.components.iter().enumerate() {
                if !slot.removed && !slot.started && slot.comp.is_some() {
                    keyed.push((slot.order.get(), id, slot_index as u32));
                }
            }
        }
        keyed.sort_by_key(|entry| entry.0);
        out.clear();
        out.extend(keyed.into_iter().map(|(_, id, slot)| (id, slot)));
    }

    /// Как `start_targets`, но только объекты, созданные не позже `spawned_before`.
    pub(crate) fn start_targets_before(&self, spawned_before: u64, out: &mut Vec<GameObjectId>) {
        self.start_targets(out);
        let index = self.index.borrow();
        out.retain(|id| index.slots[id.index as usize].spawned <= spawned_before);
    }

    /// Есть ли в сцене компоненты с порядком выполнения не 0.
    pub(crate) fn has_ordered_components(&self) -> bool {
        self.refresh_index();
        self.index.borrow().ordered_objects > 0
    }

    /// Объекты с компонентами, которые ещё не получили `on_start`, в порядке иерархии.
    pub(crate) fn start_targets(&self, out: &mut Vec<GameObjectId>) {
        let index = self.prepared_index();
        out.clear();
        out.extend(index.pending_start.iter().copied());
        out.sort_unstable_by_key(|id| index.rank_of(*id));
    }

    /// Фаза `on_start` идёт по очереди `queue`, сейчас — по `queue[position]`. Если хуки
    /// добавили компоненты объектам, созданным до начала фазы (`spawned_before` —
    /// счётчик создания на её начало) и стоящим в иерархии дальше текущего, они
    /// вставляются в хвост очереди: как при обходе всей сцены, они стартуют в этой же фазе.
    /// Порядок иерархии здесь не пересчитывается — места таких объектов известны с
    /// начала фазы.
    pub(crate) fn merge_late_starts(&self, queue: &mut Vec<GameObjectId>, position: usize, spawned_before: u64) {
        if self.journal.borrow().is_empty() {
            return;
        }
        self.refresh_index();
        let index = self.index.borrow();
        let after = index.rank_of(queue[position]);
        let rest = &queue[position + 1..];
        let found: Vec<GameObjectId> = index
            .pending_start
            .iter()
            .copied()
            .filter(|id| {
                let info = &index.slots[id.index as usize];
                info.spawned <= spawned_before
                    && index.rank_of(*id) > after
                    && index.rank_of(*id) != u32::MAX
                    && !rest.contains(id)
            })
            .collect();
        if found.is_empty() {
            return;
        }
        queue.extend(found);
        queue[position + 1..].sort_by_key(|id| index.rank_of(*id));
    }

    /// Сколько объектов создано за жизнь сцены (метка начала фазы для `merge_late_starts`).
    pub(crate) fn spawn_count(&self) -> u64 {
        self.spawned
    }

    /// После фазы `on_start`: объекты, у которых все компоненты стартовали, больше не ждут.
    pub(crate) fn settle_started(&mut self, ids: &[GameObjectId]) {
        self.refresh_index();
        for &id in ids {
            let done = self
                .get(id)
                .is_none_or(|o| o.components.iter().all(|slot| slot.removed || slot.started));
            if done {
                self.index.get_mut().pending_start.remove(&id);
            }
        }
    }
}

/// «Цепочный» конструктор объекта, см. [`Scene::build`].
pub struct ObjectBuilder<'a> {
    scene: &'a mut Scene,
    id: GameObjectId,
}

impl ObjectBuilder<'_> {
    fn obj(&mut self) -> &mut GameObject {
        self.scene
            .get_mut(self.id)
            .expect("builder owns a freshly spawned object")
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.obj().tag = tag.into();
        self
    }

    pub fn layer(mut self, layer: u32) -> Self {
        self.obj().layer = layer.min(31);
        self
    }

    /// Позиция в 2D (z = 0).
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.obj().transform.position = Vec3::new(x, y, 0.0);
        self
    }

    pub fn at3(mut self, x: f32, y: f32, z: f32) -> Self {
        self.obj().transform.position = Vec3::new(x, y, z);
        self
    }

    pub fn rotation(mut self, rotation: Quat) -> Self {
        self.obj().transform.rotation = rotation;
        self
    }

    pub fn scale(mut self, x: f32, y: f32, z: f32) -> Self {
        self.obj().transform.scale = Vec3::new(x, y, z);
        self
    }

    pub fn inactive(mut self) -> Self {
        self.obj().active = false;
        self
    }

    /// Unity: `AddComponent<T>()`.
    pub fn with<T: Component>(mut self, component: T) -> Self {
        self.obj().add(component);
        self
    }

    /// Сделать дочерним для `parent`.
    ///
    /// Паникует, если `parent` уже уничтожен: это ошибка в коде, создающем сцену.
    pub fn child_of(self, parent: GameObjectId) -> Self {
        if let Err(err) = self.scene.set_parent(self.id, Some(parent)) {
            panic!("ObjectBuilder::child_of: {err}");
        }
        self
    }

    pub fn id(self) -> GameObjectId {
        self.id
    }
}
