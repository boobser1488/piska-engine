//! Ссылки на объекты сцены в полях компонентов (Unity: поле типа `GameObject` /
//! `Rigidbody` в инспекторе).
//!
//! У каждого объекта сцены есть **стабильный идентификатор** (`uid`): он пишется в файл
//! сцены (`id: …` у узла), переживает сохранение, загрузку, переименование и
//! перестановку объектов. Поле [`ObjectRef`] хранит именно его, а не `GameObjectId`,
//! поэтому ссылка из файла снова находит свой объект после загрузки. Сцена держит
//! таблицу «uid → объект», так что [`ObjectRef::get`] — один поиск в хеш-таблице.
//!
//! Копирование группы объектов (Duplicate, вставка, повторная аддитивная загрузка)
//! перенумеровывает её одним ключом XOR ([`rekey_nodes`]): ссылки внутри группы
//! переходят на копии, ссылки наружу остаются на оригиналах. Экземпляр префаба выводит
//! номера своих детей как `uid экземпляра ^ номер в префабе` (как `fileID` в Unity) —
//! номера детей не нужно хранить в сцене, и они одинаковы при каждой загрузке.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use crate::component::Component;
use crate::object::GameObjectId;
use crate::reflect::{FieldMeta, FieldValue, TypeName, Value};
use crate::scene::Scene;
use crate::serialize::NodeData;

/// Номера объектов занимают 48 бит: короткие числа в файле, а случайное совпадение в
/// сцене из десятков тысяч объектов практически невозможно (и всё равно обрабатывается).
pub(crate) const UID_MASK: u64 = (1 << 48) - 1;

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Новый случайный номер объекта (не 0).
pub fn fresh_uid() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seed = *SEED.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(0x5049_534B_4120_4F42);
        hasher.finish()
    });
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let uid = splitmix(seed ^ n.wrapping_mul(0xD1B5_4A32_D192_ED03)) & UID_MASK;
        if uid != 0 {
            return uid;
        }
    }
}

/// Номер узла префаба без `id` (старые файлы): из пути имён — одинаковый при каждой загрузке.
pub(crate) fn path_uid(path: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let uid = splitmix(hash) & UID_MASK;
    if uid == 0 { 1 } else { uid }
}

/// Ссылка на объект сцены — поле компонента, которое назначают в инспекторе
/// (перетаскиванием из Hierarchy или из списка объектов).
///
/// ```
/// use engine_core::prelude::*;
///
/// component! {
///     pub struct Door {
///         /// Рычаг, который открывает дверь.
///         pub lever: ObjectRef = ObjectRef::NONE,
///     }
/// }
/// impl Component for Door {
///     fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
///         if let Some(lever) = ctx.resolve(self.lever) {
///             let _pulled = ctx.scene().get(lever).is_some_and(|o| o.active);
///         }
///     }
/// }
/// ```
///
/// В файле сцены: `"lever": Object(81230412339)`; `Object(0)` — «не назначено».
/// Уничтоженный объект или объект из другой сцены разрешается в `None`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectRef {
    uid: u64,
}

impl ObjectRef {
    /// «Не назначено» (Unity: `None (GameObject)`).
    pub const NONE: ObjectRef = ObjectRef { uid: 0 };

    /// Ссылка на живой объект `id` сцены `scene` (`NONE`, если его нет).
    pub fn new(scene: &Scene, id: GameObjectId) -> Self {
        Self {
            uid: scene.get(id).map_or(0, |o| o.uid()),
        }
    }

    /// Ссылка по стабильному номеру объекта (см. [`GameObject::uid`](crate::object::GameObject::uid)).
    pub const fn from_uid(uid: u64) -> Self {
        Self { uid }
    }

    /// Стабильный номер объекта, на который ссылается поле (0 — не назначено).
    pub const fn uid(self) -> u64 {
        self.uid
    }

    pub const fn is_none(self) -> bool {
        self.uid == 0
    }

    pub const fn is_some(self) -> bool {
        self.uid != 0
    }

    /// Найти объект в сцене: `None`, если поле не назначено или объекта больше нет.
    pub fn get(self, scene: &Scene) -> Option<GameObjectId> {
        scene.find_by_uid(self.uid)
    }
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.uid == 0 {
            write!(f, "ObjectRef(None)")
        } else {
            write!(f, "ObjectRef({})", self.uid)
        }
    }
}

impl FieldValue for ObjectRef {
    const EXPECTED: &'static str = "Object";
    fn to_value(&self) -> Value {
        Value::Object(self.uid)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Object(uid) => Some(Self { uid: *uid }),
            Value::Nil => Some(Self::NONE),
            _ => None,
        }
    }
}

/// Ссылка на объект, у которого есть компонент `T` (Unity: поле типа `Rigidbody`).
/// В файле — то же `Object(…)`, что и [`ObjectRef`]; инспектор предлагает только
/// объекты с компонентом `T`.
///
/// ```
/// use engine_core::prelude::*;
///
/// component! { pub struct Health { pub hp: i32 = 3 } }
/// impl Component for Health {}
///
/// component! {
///     pub struct HealthBar {
///         pub target: ComponentRef<Health> = ComponentRef::NONE,
///     }
/// }
/// impl Component for HealthBar {
///     fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
///         let _hp = self.target.component(ctx.scene()).map_or(0, |h| h.hp);
///     }
/// }
/// ```
pub struct ComponentRef<T> {
    pub object: ObjectRef,
    marker: PhantomData<fn() -> T>,
}

impl<T> ComponentRef<T> {
    pub const NONE: Self = Self {
        object: ObjectRef::NONE,
        marker: PhantomData,
    };

    pub const fn from_object(object: ObjectRef) -> Self {
        Self {
            object,
            marker: PhantomData,
        }
    }

    pub const fn is_none(&self) -> bool {
        self.object.is_none()
    }
}

impl<T: Component> ComponentRef<T> {
    /// Ссылка на объект `id`, если у него есть `T` (иначе `NONE`).
    pub fn new(scene: &Scene, id: GameObjectId) -> Self {
        if scene.get(id).is_some_and(|o| o.has::<T>()) {
            Self::from_object(ObjectRef::new(scene, id))
        } else {
            Self::NONE
        }
    }

    /// Объект — если он жив и на нём всё ещё есть `T`.
    pub fn get(&self, scene: &Scene) -> Option<GameObjectId> {
        let id = self.object.get(scene)?;
        scene.get(id)?.has::<T>().then_some(id)
    }

    pub fn component<'s>(&self, scene: &'s Scene) -> Option<&'s T> {
        scene.get_component::<T>(self.object.get(scene)?)
    }

    pub fn component_mut<'s>(&self, scene: &'s mut Scene) -> Option<&'s mut T> {
        let id = self.object.get(scene)?;
        scene.get_component_mut::<T>(id)
    }
}

impl<T> Clone for ComponentRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ComponentRef<T> {}
impl<T> Default for ComponentRef<T> {
    fn default() -> Self {
        Self::NONE
    }
}
impl<T> PartialEq for ComponentRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.object == other.object
    }
}
impl<T> Eq for ComponentRef<T> {}
impl<T> fmt::Debug for ComponentRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ComponentRef<{}>({:?})", std::any::type_name::<T>(), self.object)
    }
}
impl<T> From<ComponentRef<T>> for ObjectRef {
    fn from(r: ComponentRef<T>) -> Self {
        r.object
    }
}

impl<T: TypeName> FieldValue for ComponentRef<T> {
    const EXPECTED: &'static str = "Object";
    fn to_value(&self) -> Value {
        Value::Object(self.object.uid())
    }
    fn from_value(value: &Value) -> Option<Self> {
        ObjectRef::from_value(value).map(Self::from_object)
    }
    /// У поля-ссылки `asset_kind` — имя требуемого компонента (см. [`object_requirement`]).
    fn meta() -> FieldMeta {
        FieldMeta {
            variants: &[],
            asset_kind: T::TYPE_NAME,
        }
    }
}

/// Какой компонент должен быть у объекта в поле-ссылке: для `Value::Object` поле
/// [`FieldMeta::asset_kind`] хранит имя типа компонента (`ComponentRef<T>`), пусто — любой
/// объект (`ObjectRef`).
pub fn object_requirement(meta: &FieldMeta) -> Option<&'static str> {
    (!meta.asset_kind.is_empty()).then_some(meta.asset_kind)
}

// ----------------------------------------------------------------------
// Ссылки внутри данных сцены: обход, перенумерация, проверка
// ----------------------------------------------------------------------

/// Вызвать `f` для каждой ссылки в значении (включая элементы списков).
pub fn visit_value_refs(value: &mut Value, f: &mut impl FnMut(&mut u64)) {
    match value {
        Value::Object(uid) => f(uid),
        Value::List(items) => {
            for item in items {
                visit_value_refs(item, f);
            }
        }
        _ => {}
    }
}

/// Вызвать `f(узел, «Компонент.поле», ссылка)` для каждой ссылки в полях компонентов узла
/// и потомков (и в переопределениях экземпляров префабов).
pub fn visit_node_refs(node: &mut NodeData, f: &mut impl FnMut(&str, &str, &mut u64)) {
    let name = node.name.clone();
    for component in &mut node.components {
        for (field, value) in &mut component.fields {
            let label = format!("{}.{}", component.kind, field);
            visit_value_refs(value, &mut |uid| f(&name, &label, uid));
        }
    }
    for change in &mut node.child_components {
        for component in &mut change.added {
            for (field, value) in &mut component.fields {
                let label = format!("{}/{}.{}", change.path, component.kind, field);
                visit_value_refs(value, &mut |uid| f(&name, &label, uid));
            }
        }
    }
    for ov in &mut node.overrides {
        if ov.component == crate::serialize::GAME_OBJECT {
            continue; // `GameObject.id` — номер самого объекта, а не ссылка
        }
        let label = format!("{}.{}", ov.component, ov.field);
        visit_value_refs(&mut ov.value, &mut |uid| f(&name, &label, uid));
    }
    for child in &mut node.children {
        visit_node_refs(child, f);
    }
}

/// Номера всех узлов дерева (0 не считается).
pub fn collect_node_ids(node: &NodeData, out: &mut HashSet<u64>) {
    if node.id != 0 {
        out.insert(node.id);
    }
    for child in &node.children {
        collect_node_ids(child, out);
    }
}

/// Перенумеровать группу узлов одним ключом: `id ^ key` у каждого узла; ссылки на узлы
/// группы переходят на новые номера, ссылки наружу не меняются. Так копируют группу:
/// внутренние ссылки копии указывают на копии, производные номера детей экземпляров
/// префабов (`uid экземпляра ^ номер в префабе`) остаются производными.
pub fn rekey_nodes(nodes: &mut [NodeData], key: u64) {
    let key = key & UID_MASK;
    if key == 0 {
        return;
    }
    let mut inside = HashSet::new();
    for node in nodes.iter() {
        collect_node_ids(node, &mut inside);
    }
    fn walk(node: &mut NodeData, key: u64) {
        if node.id != 0 {
            node.id ^= key;
        }
        for child in &mut node.children {
            walk(child, key);
        }
    }
    for node in nodes.iter_mut() {
        walk(node, key);
        visit_node_refs(node, &mut |_, _, uid| {
            if inside.contains(uid) {
                *uid ^= key;
            }
        });
    }
}

/// Ключ для [`rekey_nodes`], с которым номера группы не совпадут с занятыми (`taken`).
pub fn fresh_key(nodes: &[NodeData], taken: impl Fn(u64) -> bool) -> u64 {
    let mut ids = HashSet::new();
    for node in nodes {
        collect_node_ids(node, &mut ids);
    }
    for _ in 0..64 {
        let key = fresh_uid();
        if ids.iter().all(|id| !taken(id ^ key) && id ^ key != 0) {
            return key;
        }
    }
    fresh_uid()
}

/// Ссылки на объекты, которых нет ни среди узлов, ни среди `known`, — в «не назначено»,
/// с предупреждением, где именно (Unity: `Missing` → `None`).
pub fn clear_dangling_refs(nodes: &mut [NodeData], known: impl Fn(u64) -> bool, origin: &str) {
    let mut ids = HashSet::new();
    for node in nodes.iter() {
        collect_node_ids(node, &mut ids);
    }
    for node in nodes.iter_mut() {
        visit_node_refs(node, &mut |object, field, uid| {
            if *uid != 0 && !ids.contains(uid) && !known(*uid) {
                log::warn!(
                    "{origin}: object `{object}`, field `{field}` refers to an object that does not exist (id {uid}); the reference is set to None"
                );
                *uid = 0;
            }
        });
    }
}

/// Узлы префаба без номеров (старые файлы) получают номера из пути имён.
pub(crate) fn fill_path_ids(node: &mut NodeData, path: &str) {
    if node.id == 0 {
        node.id = path_uid(path);
    }
    for child in &mut node.children {
        let child_path = format!("{path}/{}", child.name);
        fill_path_ids(child, &child_path);
    }
}

/// Развёрнутый префаб становится экземпляром с номером `instance`: корень получает
/// `instance`, остальные узлы — `instance ^ номер в префабе`; ссылки внутри префаба
/// переводятся на новые номера, ссылки наружу (в файле префаба их быть не должно) —
/// в «не назначено».
pub(crate) fn derive_instance_ids(root: &mut NodeData, instance: u64, reference: &str) {
    let mut map: HashMap<u64, u64> = HashMap::new();
    fn walk(node: &mut NodeData, instance: u64, is_root: bool, map: &mut HashMap<u64, u64>) {
        let derived = if is_root { instance } else { (node.id ^ instance) & UID_MASK };
        let derived = if derived == 0 { instance } else { derived };
        if node.id != 0 {
            map.insert(node.id, derived);
        }
        node.id = derived;
        for child in &mut node.children {
            walk(child, instance, false, map);
        }
    }
    walk(root, instance, true, &mut map);
    visit_node_refs(root, &mut |object, field, uid| {
        if *uid == 0 {
            return;
        }
        match map.get(uid) {
            Some(new) => *uid = *new,
            None => {
                log::warn!(
                    "prefab `{reference}`: object `{object}`, field `{field}` refers to an object outside the prefab; the reference is set to None"
                );
                *uid = 0;
            }
        }
    });
}

/// Снятый с объекта сцены префаб: номера потомков — `uid ^ uid корня` (для экземпляра
/// это ровно номера в префабе), ссылки внутрь переводятся так же, ссылки наружу
/// становятся «не назначено» (у ассета не может быть ссылок на объекты сцены).
/// Возвращает описания очищенных ссылок — редактор показывает их пользователю.
pub(crate) fn localize_prefab_ids(root: &mut NodeData) -> Vec<String> {
    let root_uid = root.id;
    let mut inside = HashSet::new();
    collect_node_ids(root, &mut inside);
    fn walk(node: &mut NodeData, root_uid: u64, is_root: bool) {
        if !is_root && node.id != 0 && root_uid != 0 {
            node.id ^= root_uid;
        }
        for child in &mut node.children {
            walk(child, root_uid, false);
        }
    }
    walk(root, root_uid, true);
    let mut cleared = Vec::new();
    visit_node_refs(root, &mut |object, field, uid| {
        if *uid == 0 {
            return;
        }
        if inside.contains(uid) {
            if *uid != root_uid {
                *uid ^= root_uid;
            }
        } else {
            cleared.push(format!("`{object}`.{field}"));
            *uid = 0;
        }
    });
    cleared
}
