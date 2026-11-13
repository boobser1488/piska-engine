//! GameObject — базовый объект сцены.

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::component::{Component, Hooks};
use crate::transform::Transform;

/// Лёгкий копируемый идентификатор объекта (индекс + поколение).
/// После уничтожения объекта его id «протухает»: `scene.get(id)` вернёт `None`,
/// даже если слот уже занят новым объектом.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GameObjectId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl GameObjectId {
    /// Идентификатор одним числом (поколение в старших битах) — для передачи через
    /// границу с C++ и в сетевых сообщениях.
    pub fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }

    /// Обратно из [`to_bits`](Self::to_bits). Произвольное число даёт «протухший» id.
    pub fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

impl fmt::Debug for GameObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GameObject#{}v{}", self.index, self.generation)
    }
}

pub(crate) struct ComponentSlot {
    /// `None`, пока компонент «вынут» для вызова хука (см. `World::dispatch`).
    pub comp: Option<Box<dyn Component>>,
    pub started: bool,
    /// Надгробие: компонент удалён, слот будет сжат в конце кадра.
    pub removed: bool,
    /// Ответ `Component::hooks` в момент добавления.
    pub hooks: Hooks,
    /// Тип компонента: по нему сцена ведёт индекс «тип → объекты» (и пока компонент вынут).
    pub type_id: TypeId,
    /// Имя типа (`Reflect::type_name`): по нему настройки проекта перекрывают порядок.
    pub name: &'static str,
    /// Порядок из кода (`Component::execution_order`) в момент добавления.
    pub default_order: i32,
    /// Действующий порядок выполнения: `default_order` или значение из настроек проекта.
    /// Пересчитывается индексом сцены (поэтому `Cell`).
    pub order: Cell<i32>,
}

/// Журнал изменений состава компонентов: номера слотов сцены, у объектов которых
/// добавили или удалили компонент. Общий у сцены и её объектов; сцена разбирает его
/// лениво — при первом запросе к индексу.
pub(crate) type Journal = Rc<RefCell<Vec<u32>>>;

/// Объект сцены: имя, тег, слой, Transform и список компонентов.
pub struct GameObject {
    pub name: String,
    pub tag: String,
    /// Слой 0..=31: маски камер и матрица столкновений.
    pub layer: u32,
    /// Собственный флаг активности. Объект «жив» в кадре, только если активны и все предки.
    pub active: bool,
    /// «Не уничтожать при загрузке сцены» (Unity: `DontDestroyOnLoad`). Выставляется через
    /// `Scene::set_persistent`; в файл сцены не пишется.
    pub persistent: bool,
    pub transform: Transform,
    pub(crate) parent: Option<GameObjectId>,
    pub(crate) children: Vec<GameObjectId>,
    pub(crate) components: Vec<ComponentSlot>,
    pub(crate) has_tombstones: bool,
    /// Ссылка на префаб, из которого создан объект (для сохранения переопределений).
    pub(crate) prefab: Option<String>,
    /// Номер слота в сцене и журнал сцены (см. [`Journal`]).
    pub(crate) slot: u32,
    pub(crate) journal: Option<Journal>,
    /// Объект уже записан в журнал и ждёт разбора.
    pub(crate) queued: Cell<bool>,
}

impl GameObject {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            tag: String::new(),
            layer: 0,
            active: true,
            persistent: false,
            transform: Transform::IDENTITY,
            parent: None,
            children: Vec::new(),
            components: Vec::new(),
            has_tombstones: false,
            prefab: None,
            slot: 0,
            journal: None,
            queued: Cell::new(false),
        }
    }

    /// Сообщить сцене, что состав компонентов изменился.
    fn touch(&self) {
        if self.queued.get() {
            return;
        }
        if let Some(journal) = &self.journal {
            journal.borrow_mut().push(self.slot);
            self.queued.set(true);
        }
    }

    pub fn parent(&self) -> Option<GameObjectId> {
        self.parent
    }

    pub fn children(&self) -> &[GameObjectId] {
        &self.children
    }

    /// Префаб-источник, если объект — экземпляр префаба.
    pub fn prefab(&self) -> Option<&str> {
        self.prefab.as_deref()
    }

    /// Unity: `GetComponent<T>()`.
    pub fn get<T: Component>(&self) -> Option<&T> {
        // Тип сравнивается по записи в слоте — без виртуального вызова на каждый компонент.
        let wanted = TypeId::of::<T>();
        self.components
            .iter()
            .filter(|slot| !slot.removed && slot.type_id == wanted)
            .find_map(|slot| slot.comp.as_deref()?.as_any().downcast_ref::<T>())
    }

    pub fn get_mut<T: Component>(&mut self) -> Option<&mut T> {
        let wanted = TypeId::of::<T>();
        self.components
            .iter_mut()
            .filter(|slot| !slot.removed && slot.type_id == wanted)
            .find_map(|slot| slot.comp.as_deref_mut()?.as_any_mut().downcast_mut::<T>())
    }

    pub fn has<T: Component>(&self) -> bool {
        self.get::<T>().is_some()
    }

    /// Unity: `AddComponent<T>()`. `on_start` вызовется перед ближайшим обновлением.
    pub fn add<T: Component>(&mut self, component: T) -> &mut T {
        self.add_boxed(Box::new(component));
        self.components
            .last_mut()
            .and_then(|slot| slot.comp.as_deref_mut())
            .and_then(|c| c.as_any_mut().downcast_mut::<T>())
            .expect("component was just added")
    }

    pub fn add_boxed(&mut self, component: Box<dyn Component>) {
        let hooks = component.hooks();
        let type_id = (*component.as_any()).type_id();
        let name = component.type_name();
        let default_order = component.execution_order();
        self.components.push(ComponentSlot {
            comp: Some(component),
            started: false,
            removed: false,
            hooks,
            type_id,
            name,
            default_order,
            order: Cell::new(default_order),
        });
        self.touch();
    }

    /// Удалить первый компонент типа `T`. Возвращает `false`, если такого нет.
    pub fn remove<T: Component>(&mut self) -> bool {
        let wanted = TypeId::of::<T>();
        let found = self.components.iter_mut().find(|slot| {
            !slot.removed && slot.type_id == wanted && slot.comp.is_some()
        });
        match found {
            Some(slot) => {
                slot.removed = true;
                slot.comp = None;
                self.has_tombstones = true;
                self.touch();
                true
            }
            None => false,
        }
    }

    /// Привязать объект к префабу (или отвязать: `None` — «Unpack» в редакторе).
    pub fn set_prefab(&mut self, reference: Option<String>) {
        self.prefab = reference;
    }

    /// Удалить компонент по порядковому номеру в [`components`](Self::components) —
    /// так компоненты адресует инспектор.
    pub fn remove_at(&mut self, index: usize) -> bool {
        let found = self
            .components
            .iter_mut()
            .filter(|slot| !slot.removed && slot.comp.is_some())
            .nth(index);
        match found {
            Some(slot) => {
                slot.removed = true;
                slot.comp = None;
                self.has_tombstones = true;
                self.touch();
                true
            }
            None => false,
        }
    }

    /// Переставить компонент: с места `from` на место `to` в списке
    /// [`components`](Self::components) (так же нумерует инспектор). Меняет порядок при
    /// равном порядке выполнения и порядок записи в сцене. `false` — номера вне списка
    /// или `from == to`.
    pub fn move_component(&mut self, from: usize, to: usize) -> bool {
        let live: Vec<usize> = self
            .components
            .iter()
            .enumerate()
            .filter(|(_, slot)| !slot.removed && slot.comp.is_some())
            .map(|(raw, _)| raw)
            .collect();
        if from == to || from >= live.len() || to >= live.len() {
            return false;
        }
        let slot = self.components.remove(live[from]);
        // Вниз — после компонента `to` (он сдвинулся на 1), вверх — перед ним.
        self.components.insert(live[to], slot);
        self.touch();
        true
    }

    /// Все компоненты как `dyn Component` — так их видит инспектор и сериализатор.
    pub fn components(&self) -> impl Iterator<Item = &dyn Component> {
        self.components
            .iter()
            .filter(|slot| !slot.removed)
            .filter_map(|slot| slot.comp.as_deref())
    }

    pub fn components_mut(&mut self) -> impl Iterator<Item = &mut (dyn Component + 'static)> {
        self.components
            .iter_mut()
            .filter(|slot| !slot.removed)
            .filter_map(|slot| slot.comp.as_deref_mut())
    }
}
