//! Сериализация сцен и префабов в человекочитаемый RON.
//!
//! Формат один и тот же для сцены и префаба: дерево узлов [`NodeData`]. Сцена — это
//! список корневых узлов, префаб — один корневой узел. Узел либо описан целиком
//! (компоненты + дети), либо ссылается на префаб (`prefab: Some("…")`) и хранит
//! только отличия от него (`overrides`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::component::Component;
use crate::error::{did_you_mean, SceneError};
use crate::math::Vec3;
use crate::object::GameObjectId;
use crate::reflect::{TypeRegistry, Value};
use crate::scene::Scene;
use crate::transform::Transform;

// ----------------------------------------------------------------------
// Данные на диске
// ----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneData {
    pub name: String,
    #[serde(default)]
    pub objects: Vec<NodeData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeData {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub layer: u32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub active: bool,
    /// Ссылка на префаб (путь или GUID). Тогда отличия от него — в `overrides`,
    /// `removed` и `components`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    /// Только для экземпляра префаба: компоненты префаба, удалённые у экземпляра.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(default)]
    pub transform: TransformData,
    /// Компоненты. Для экземпляра префаба — только *добавленные* поверх префаба.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<ComponentData>,
    /// Дети. Для экземпляра префаба — только *добавленные* поверх префаба.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeData>,
}

impl Default for NodeData {
    fn default() -> Self {
        Self {
            name: String::new(),
            tag: String::new(),
            layer: 0,
            active: true,
            prefab: None,
            overrides: Vec::new(),
            removed: Vec::new(),
            transform: TransformData::default(),
            components: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Transform в файле: поворот — углы Эйлера в градусах, как в инспекторе.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl From<&Transform> for TransformData {
    fn from(t: &Transform) -> Self {
        // Округляем градусы до 1e-4, чтобы в файле не было `-0.0` и `89.99999`.
        let clean = |v: f32| {
            let r = (v * 1e4).round() / 1e4;
            if r == 0.0 { 0.0 } else { r }
        };
        let euler = t.euler_deg();
        Self {
            position: t.position.to_array(),
            rotation: [clean(euler.x), clean(euler.y), clean(euler.z)],
            scale: t.scale.to_array(),
        }
    }
}

impl From<&TransformData> for Transform {
    fn from(d: &TransformData) -> Self {
        let mut t = Transform {
            position: Vec3::from_array(d.position),
            scale: Vec3::from_array(d.scale),
            ..Transform::IDENTITY
        };
        t.set_euler_deg(Vec3::from_array(d.rotation));
        t
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentData {
    /// Имя типа компонента, как в `component! { struct … }`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

/// Переопределение одного поля в экземпляре префаба.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    /// Путь до дочернего объекта по именам: `"Body/Wheel"`. Пусто — корень префаба.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Имя компонента или `"Transform"` (поля `position`, `rotation`, `scale`).
    pub component: String,
    pub field: String,
    pub value: Value,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}
fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}

fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::new()
        .indentor("    ".to_string())
        .struct_names(false)
}

impl SceneData {
    pub fn from_ron(text: &str, origin: &str) -> Result<Self, SceneError> {
        ron::from_str(text).map_err(|e| SceneError::Parse {
            path: origin.to_string(),
            message: e.to_string(),
        })
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, pretty()).expect("scene data is always serializable")
    }
}

// ----------------------------------------------------------------------
// Префабы
// ----------------------------------------------------------------------

/// Префаб — шаблон объекта (с детьми). Файл `.prefab` = один [`NodeData`].
#[derive(Clone, Debug, PartialEq)]
pub struct Prefab {
    pub root: NodeData,
}

impl Prefab {
    /// Снять префаб с живого объекта сцены.
    pub fn from_object(scene: &Scene, id: GameObjectId) -> Option<Self> {
        scene.get(id)?;
        let mut root = snapshot_node(scene, id);
        root.transform.position = [0.0; 3]; // позиция принадлежит экземпляру
        Some(Self { root })
    }

    pub fn from_ron(text: &str, origin: &str) -> Result<Self, SceneError> {
        let root = ron::from_str(text).map_err(|e| SceneError::Parse {
            path: origin.to_string(),
            message: e.to_string(),
        })?;
        Ok(Self { root })
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(&self.root, pretty()).expect("prefab data is always serializable")
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, SceneError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_ron(&text, &path.display().to_string())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let path = path.as_ref();
        std::fs::write(path, self.to_ron()).map_err(|source| SceneError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

/// Откуда загрузчик берёт префаб по ссылке из сцены.
pub trait PrefabSource {
    fn load(&self, reference: &str) -> Result<Prefab, SceneError>;
}

/// Префабы из каталога: ссылка — путь относительно корня.
pub struct DirPrefabs {
    pub root: PathBuf,
}

impl DirPrefabs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl PrefabSource for DirPrefabs {
    fn load(&self, reference: &str) -> Result<Prefab, SceneError> {
        let path = self.root.join(reference);
        if !path.is_file() {
            return Err(SceneError::PrefabNotFound {
                reference: reference.to_string(),
                reason: format!("no such file `{}`", path.display()),
            });
        }
        Prefab::load(path)
    }
}

/// Префабы в памяти: для тестов и префабов, созданных кодом.
#[derive(Default)]
pub struct MemoryPrefabs {
    pub prefabs: HashMap<String, Prefab>,
}

impl MemoryPrefabs {
    pub fn with(mut self, reference: impl Into<String>, prefab: Prefab) -> Self {
        self.prefabs.insert(reference.into(), prefab);
        self
    }
}

impl PrefabSource for MemoryPrefabs {
    fn load(&self, reference: &str) -> Result<Prefab, SceneError> {
        self.prefabs
            .get(reference)
            .cloned()
            .ok_or_else(|| SceneError::PrefabNotFound {
                reference: reference.to_string(),
                reason: format!(
                    "not registered in memory.{}",
                    did_you_mean(reference, self.prefabs.keys().map(String::as_str))
                ),
            })
    }
}

// ----------------------------------------------------------------------
// Сцена → данные
// ----------------------------------------------------------------------

fn component_data(component: &dyn Component) -> ComponentData {
    ComponentData {
        kind: component.type_name().to_string(),
        fields: component
            .field_names()
            .iter()
            .filter_map(|name| Some((name.to_string(), component.get_field(name)?)))
            .collect(),
    }
}

/// Полный «развёрнутый» снимок объекта с потомками (без ссылок на префабы).
pub fn snapshot_node(scene: &Scene, id: GameObjectId) -> NodeData {
    let Some(obj) = scene.get(id) else {
        return NodeData::default();
    };
    NodeData {
        name: obj.name.clone(),
        tag: obj.tag.clone(),
        layer: obj.layer,
        active: obj.active,
        prefab: None,
        overrides: Vec::new(),
        removed: Vec::new(),
        transform: TransformData::from(&obj.transform),
        components: obj.components().map(component_data).collect(),
        children: obj
            .children()
            .iter()
            .map(|child| snapshot_node(scene, *child))
            .collect(),
    }
}

/// Точный снимок объекта с потомками: всё развёрнуто, но метка «из какого префаба»
/// сохранена. Такой узел создаётся обратно через [`spawn_resolved`] без потерь —
/// на этом стоят Play/Stop, Undo и Duplicate в редакторе.
pub fn exact_node(scene: &Scene, id: GameObjectId) -> NodeData {
    let Some(obj) = scene.get(id) else {
        return NodeData::default();
    };
    // Transform копируется без округления углов: снимок должен вернуть ровно ту же позу.
    let euler = obj.transform.euler_deg();
    NodeData {
        prefab: obj.prefab().map(str::to_string),
        transform: TransformData {
            position: obj.transform.position.to_array(),
            rotation: euler.to_array(),
            scale: obj.transform.scale.to_array(),
        },
        children: obj
            .children()
            .iter()
            .map(|child| exact_node(scene, *child))
            .collect(),
        ..snapshot_node_shallow(scene, id)
    }
}

/// Снимок всей сцены в памяти, см. [`exact_node`].
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    pub name: String,
    pub objects: Vec<NodeData>,
}

impl SceneSnapshot {
    pub fn capture(scene: &Scene) -> Self {
        Self {
            name: scene.name.clone(),
            objects: scene
                .roots()
                .iter()
                .map(|root| exact_node(scene, *root))
                .collect(),
        }
    }
}

/// Дописать в узел поля, которые в файле опущены (то есть равны значениям по
/// умолчанию). Без этого сравнение «экземпляр против префаба» приняло бы каждое
/// опущенное поле за переопределение.
fn complete_node(node: &mut NodeData, registry: &TypeRegistry) {
    for data in &mut node.components {
        let Ok(mut component) = registry.create(&data.kind) else {
            continue; // неизвестный тип: оставляем как есть, об этом скажет загрузчик
        };
        for (field, value) in &data.fields {
            let _ = component.set_field(field, value);
        }
        *data = component_data(component.as_ref());
    }
    for child in &mut node.children {
        complete_node(child, registry);
    }
}

/// Узел для записи в файл: экземпляр префаба превращается в ссылку + переопределения.
pub fn save_node(
    scene: &Scene,
    id: GameObjectId,
    prefabs: &dyn PrefabSource,
    registry: &TypeRegistry,
) -> NodeData {
    let Some(obj) = scene.get(id) else {
        return NodeData::default();
    };

    if let Some(reference) = obj.prefab() {
        let base = prefabs
            .load(reference)
            .and_then(|prefab| resolve_node(&prefab.root, prefabs, 0))
            .map(|mut base| {
                complete_node(&mut base, registry);
                base
            });
        match base {
            Ok(base) => {
                let instance = snapshot_node(scene, id);
                let mut overrides = Vec::new();
                diff_nodes(&base, &instance, "", true, &mut overrides);
                // Компоненты корня, которых нет в префабе, и наоборот.
                let added = instance
                    .components
                    .iter()
                    .filter(|c| !base.components.iter().any(|b| b.kind == c.kind))
                    .cloned()
                    .collect();
                let removed = base
                    .components
                    .iter()
                    .filter(|b| !instance.components.iter().any(|c| c.kind == b.kind))
                    .map(|b| b.kind.clone())
                    .collect();
                let added_children = obj
                    .children()
                    .iter()
                    .filter(|child| {
                        scene
                            .get(**child)
                            .is_some_and(|c| !base.children.iter().any(|b| b.name == c.name))
                    })
                    .map(|child| save_node(scene, *child, prefabs, registry))
                    .collect();
                return NodeData {
                    name: instance.name,
                    tag: if instance.tag == base.tag { String::new() } else { instance.tag },
                    layer: if instance.layer == base.layer { 0 } else { instance.layer },
                    active: instance.active,
                    prefab: Some(reference.to_string()),
                    overrides,
                    removed,
                    transform: instance.transform,
                    components: added,
                    children: added_children,
                };
            }
            Err(err) => log::warn!(
                "`{}`: prefab link `{reference}` is broken ({err}); saving the object inline",
                obj.name
            ),
        }
    }

    NodeData {
        children: obj
            .children()
            .iter()
            .map(|child| save_node(scene, *child, prefabs, registry))
            .collect(),
        ..snapshot_node_shallow(scene, id)
    }
}

fn snapshot_node_shallow(scene: &Scene, id: GameObjectId) -> NodeData {
    let Some(obj) = scene.get(id) else {
        return NodeData::default();
    };
    NodeData {
        name: obj.name.clone(),
        tag: obj.tag.clone(),
        layer: obj.layer,
        active: obj.active,
        transform: TransformData::from(&obj.transform),
        components: obj.components().map(component_data).collect(),
        ..NodeData::default()
    }
}

pub fn scene_data(scene: &Scene, prefabs: &dyn PrefabSource, registry: &TypeRegistry) -> SceneData {
    SceneData {
        name: scene.name.clone(),
        objects: scene
            .roots()
            .iter()
            .map(|root| save_node(scene, *root, prefabs, registry))
            .collect(),
    }
}

fn approx_eq(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-4)
}

fn vec3_value(v: [f32; 3]) -> Value {
    Value::Vec3(v[0], v[1], v[2])
}

/// Найти отличия экземпляра от префаба: значения полей и Transform'ы детей.
/// Добавленные и удалённые компоненты корня учитывает [`save_node`]; у детей
/// префаба они пока не отслеживаются (см. DESIGN.md).
fn diff_nodes(base: &NodeData, instance: &NodeData, path: &str, is_root: bool, out: &mut Vec<Override>) {
    // Transform корня всегда принадлежит экземпляру и хранится в самом узле.
    if !is_root {
        let pairs = [
            ("position", base.transform.position, instance.transform.position),
            ("rotation", base.transform.rotation, instance.transform.rotation),
            ("scale", base.transform.scale, instance.transform.scale),
        ];
        for (field, b, i) in pairs {
            if !approx_eq(b, i) {
                out.push(Override {
                    path: path.to_string(),
                    component: "Transform".to_string(),
                    field: field.to_string(),
                    value: vec3_value(i),
                });
            }
        }
    }

    for base_component in &base.components {
        let Some(inst_component) = instance
            .components
            .iter()
            .find(|c| c.kind == base_component.kind)
        else {
            continue;
        };
        for (field, value) in &inst_component.fields {
            if base_component.fields.get(field) != Some(value) {
                out.push(Override {
                    path: path.to_string(),
                    component: base_component.kind.clone(),
                    field: field.clone(),
                    value: value.clone(),
                });
            }
        }
    }

    for base_child in &base.children {
        if let Some(inst_child) = instance.children.iter().find(|c| c.name == base_child.name) {
            let child_path = if path.is_empty() {
                base_child.name.clone()
            } else {
                format!("{path}/{}", base_child.name)
            };
            diff_nodes(base_child, inst_child, &child_path, false, out);
        }
    }
}

// ----------------------------------------------------------------------
// Данные → сцена
// ----------------------------------------------------------------------

/// Развернуть ссылки на префабы и применить переопределения. Результат полностью
/// «встроенный»; поле `prefab` остаётся как метка происхождения.
pub fn resolve_node(
    node: &NodeData,
    prefabs: &dyn PrefabSource,
    depth: u32,
) -> Result<NodeData, SceneError> {
    let mut out = match &node.prefab {
        Some(reference) => {
            if depth > 16 {
                return Err(SceneError::PrefabCycle {
                    reference: reference.clone(),
                });
            }
            let prefab = prefabs.load(reference)?;
            let mut root = resolve_node(&prefab.root, prefabs, depth + 1)?;
            for ov in &node.overrides {
                apply_override(&mut root, ov, reference, &node.name)?;
            }
            root.components
                .retain(|c| !node.removed.contains(&c.kind));
            root.components.extend(node.components.iter().cloned());
            root.name = node.name.clone();
            root.transform = node.transform;
            if !node.tag.is_empty() {
                root.tag = node.tag.clone();
            }
            if node.layer != 0 {
                root.layer = node.layer;
            }
            root.active = node.active;
            root.prefab = Some(reference.clone());
            root.overrides.clear();
            root.removed.clear();
            root
        }
        None => NodeData {
            children: Vec::new(),
            ..node.clone()
        },
    };
    for child in &node.children {
        out.children.push(resolve_node(child, prefabs, depth + 1)?);
    }
    Ok(out)
}

fn apply_override(
    root: &mut NodeData,
    ov: &Override,
    prefab: &str,
    object: &str,
) -> Result<(), SceneError> {
    let bad = |target: String, hint: String| SceneError::BadOverride {
        object: object.to_string(),
        prefab: prefab.to_string(),
        target,
        hint,
    };

    let mut node = root;
    for part in ov.path.split('/').filter(|p| !p.is_empty()) {
        let hint = did_you_mean(part, node.children.iter().map(|c| c.name.as_str()));
        node = node
            .children
            .iter_mut()
            .find(|c| c.name == part)
            .ok_or_else(|| bad(format!("child `{part}` (path `{}`)", ov.path), hint))?;
    }

    if ov.component == "Transform" {
        let Value::Vec3(x, y, z) = ov.value else {
            return Err(bad(
                format!("Transform.{} = {}", ov.field, ov.value.kind_name()),
                " Transform fields take `Vec3(x, y, z)`.".to_string(),
            ));
        };
        let slot = match ov.field.as_str() {
            "position" => &mut node.transform.position,
            "rotation" => &mut node.transform.rotation,
            "scale" => &mut node.transform.scale,
            other => {
                return Err(bad(
                    format!("Transform.{other}"),
                    did_you_mean(other, ["position", "rotation", "scale"]),
                ))
            }
        };
        *slot = [x, y, z];
        return Ok(());
    }

    let hint = did_you_mean(&ov.component, node.components.iter().map(|c| c.kind.as_str()));
    let component = node
        .components
        .iter_mut()
        .find(|c| c.kind == ov.component)
        .ok_or_else(|| bad(format!("component `{}`", ov.component), hint))?;
    // Существование и тип поля проверит `set_field` при создании компонента.
    component.fields.insert(ov.field.clone(), ov.value.clone());
    Ok(())
}

/// Создать объекты из уже развёрнутого узла (см. [`resolve_node`]).
/// При ошибке ничего не остаётся в сцене.
pub fn spawn_resolved(
    scene: &mut Scene,
    registry: &TypeRegistry,
    node: &NodeData,
    parent: Option<GameObjectId>,
) -> Result<GameObjectId, SceneError> {
    let id = scene.spawn(node.name.clone());
    match fill_object(scene, registry, node, id, parent) {
        Ok(()) => Ok(id),
        Err(err) => {
            scene.remove_subtree(id);
            Err(err)
        }
    }
}

fn fill_object(
    scene: &mut Scene,
    registry: &TypeRegistry,
    node: &NodeData,
    id: GameObjectId,
    parent: Option<GameObjectId>,
) -> Result<(), SceneError> {
    if parent.is_some() {
        scene.set_parent(id, parent)?;
    }
    let obj = scene.get_mut(id).ok_or(SceneError::DeadObject(id))?;
    obj.tag = node.tag.clone();
    obj.layer = node.layer.min(31);
    obj.active = node.active;
    obj.transform = Transform::from(&node.transform);
    obj.prefab = node.prefab.clone();

    for data in &node.components {
        let mut component = match registry.create(&data.kind) {
            Ok(component) => component,
            // Редактор: незарегистрированный тип не губит сцену, а хранится как есть.
            Err(_) if crate::missing::keeps_missing_scripts() => {
                obj.add_boxed(Box::new(crate::missing::MissingScript::new(&data.kind, &data.fields)));
                continue;
            }
            Err(e) => return Err(e.in_object(&node.name)),
        };
        for (field, value) in &data.fields {
            match component.set_field(field, value) {
                Ok(()) => {}
                // Поля больше нет в коде (или сцену сохранили с другим компонентом того же
                // имени): как в Unity, значение отбрасывается, а сцена открывается.
                Err(error @ crate::error::ReflectError::NoSuchField { .. }) => {
                    log::warn!("object `{}`: {error}; the saved value is ignored", node.name);
                }
                Err(error) => return Err(SceneError::from(error).in_object(&node.name)),
            }
        }
        obj.add_boxed(component);
    }

    for child in &node.children {
        spawn_resolved(scene, registry, child, Some(id))?;
    }
    Ok(())
}
