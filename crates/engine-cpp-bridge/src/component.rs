//! Игровые компоненты на C++.
//!
//! Плагин описывает тип компонента структурой [`EngineComponentType`]: как создать
//! экземпляр, какие у него хуки и поля. Здесь этот тип оборачивается в
//! [`CppComponent`] — обычный `Component` движка: он живёт на объекте, показывается в
//! инспекторе и сохраняется в сцену, как компонент на Rust.
//!
//! Мир из C++ доступен через функции [`EngineApi`](crate::abi::EngineApi) (см. `api.rs`).
//! У них нет параметра «мир»: на время хука указатель на `World` кладётся в ячейку потока
//! (`scope.rs`).
//!
//! Поля читаются и пишутся через функции `get`/`set`, которые генерирует макрос
//! `ENGINE_FIELD` на стороне C++: Rust ничего не знает о раскладке класса в памяти, поэтому
//! поля могут быть любого вида — включая `std::string`, перечисления и ассеты.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::rc::Rc;

use engine_core::prelude::*;

use crate::abi::*;
use crate::scope::*;

pub(crate) use crate::api::HOST_API;

// ----------------------------------------------------------------------
// Класс компонента и его экземпляры
// ----------------------------------------------------------------------

type FieldGet = unsafe extern "C" fn(*const c_void, *mut EngineValue);
type FieldSet = unsafe extern "C" fn(*mut c_void, *const EngineValue) -> i32;

#[derive(Clone, Copy)]
struct FieldSlot {
    kind: u32,
    get: FieldGet,
    set: FieldSet,
}

/// Описание поля, прочитанное из плагина.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldDesc {
    pub name: String,
    pub kind: u32,
    pub variants: Vec<String>,
    pub asset_kind: String,
}

/// Подсказки инспектору и тексты ошибок. «Утекают» намеренно (как и имена полей):
/// reflection отдаёт `&'static`, а типов в плагине немного.
#[derive(Clone, Copy)]
struct FieldInfo {
    variants: &'static [&'static str],
    asset_kind: &'static str,
    expected: &'static str,
}

/// Копия таблицы функций типа из плагина. Действительна, пока плагин загружен.
#[derive(Clone)]
struct Vtable {
    create: unsafe extern "C" fn() -> *mut c_void,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    on_start: Option<unsafe extern "C" fn(*mut c_void, EngineObject)>,
    on_update: Option<unsafe extern "C" fn(*mut c_void, EngineObject, f32)>,
    on_fixed_update: Option<unsafe extern "C" fn(*mut c_void, EngineObject, f32)>,
    on_late_update: Option<unsafe extern "C" fn(*mut c_void, EngineObject, f32)>,
    on_collision_enter: Option<unsafe extern "C" fn(*mut c_void, EngineObject, EngineObject)>,
    on_trigger_enter: Option<unsafe extern "C" fn(*mut c_void, EngineObject, EngineObject)>,
    on_destroy: Option<unsafe extern "C" fn(*mut c_void, EngineObject)>,
    on_message: Option<unsafe extern "C" fn(*mut c_void, EngineObject, *const c_char, *const EngineValue)>,
    fields: Vec<FieldSlot>,
}

/// Тип C++-компонента. Переживает перезагрузку плагина: меняется только таблица функций.
pub(crate) struct CppClass {
    name: &'static str,
    /// Описания полей текущей версии плагина (для сравнения при перезагрузке).
    descs: RefCell<Vec<FieldDesc>>,
    field_names: Cell<&'static [&'static str]>,
    field_info: Cell<&'static [FieldInfo]>,
    /// `None` — плагин выгружен (или в новой версии такого типа нет).
    vtable: RefCell<Option<Vtable>>,
}

impl CppClass {
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Плагин выгружается: его функции больше нельзя вызывать.
    pub fn unbind(&self) {
        self.vtable.borrow_mut().take();
    }

    pub fn is_bound(&self) -> bool {
        self.vtable.borrow().is_some()
    }

    fn set_fields(&self, descs: &[FieldDesc]) {
        if *self.descs.borrow() == descs {
            return;
        }
        let names: Vec<&'static str> = descs.iter().map(|d| leak_str(&d.name)).collect();
        let info: Vec<FieldInfo> = descs
            .iter()
            .map(|d| FieldInfo {
                variants: Box::leak(d.variants.iter().map(|v| leak_str(v)).collect::<Vec<_>>().into_boxed_slice()),
                asset_kind: leak_str(&d.asset_kind),
                expected: expected_name(d),
            })
            .collect();
        self.field_names.set(Box::leak(names.into_boxed_slice()));
        self.field_info.set(Box::leak(info.into_boxed_slice()));
        *self.descs.borrow_mut() = descs.to_vec();
    }
}

fn leak_str(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

fn expected_name(desc: &FieldDesc) -> &'static str {
    match desc.kind {
        ENGINE_FIELD_F32 => "Float",
        ENGINE_FIELD_I32 => "Int (32 bit)",
        ENGINE_FIELD_BOOL => "Bool",
        ENGINE_FIELD_VEC2 => "Vec2",
        ENGINE_FIELD_VEC3 => "Vec3",
        ENGINE_FIELD_COLOR => "Color",
        ENGINE_FIELD_STRING => "Str",
        ENGINE_FIELD_ASSET => "Asset",
        ENGINE_FIELD_QUAT => "Quat",
        _ => leak_str(&format!("Enum({})", desc.variants.join(" | "))),
    }
}

/// Прочитать описание типа из плагина. `None` — описание некорректно (причина в логе).
///
/// # Safety
/// `ty` указывает на `EngineComponentType` загруженного плагина с совпавшей версией ABI;
/// его строки и массив полей статические (контракт ABI).
pub(crate) unsafe fn read_type(ty: &EngineComponentType) -> Option<(String, Vec<FieldDesc>, VtableHandle)> {
    // SAFETY: `name` — статическая C-строка плагина.
    let name = unsafe { text(ty.name) }?.to_string();
    let Some(create) = ty.create else {
        log::error!("C++ component `{name}` has no `create` function and cannot be used");
        return None;
    };
    let mut descs = Vec::new();
    let mut slots = Vec::new();
    for index in 0..ty.field_count as usize {
        if ty.fields.is_null() {
            break;
        }
        // SAFETY: `fields` — массив из `field_count` элементов по контракту ABI.
        let field = unsafe { &*ty.fields.add(index) };
        // SAFETY: `field.name` — статическая C-строка плагина.
        let Some(field_name) = (unsafe { text(field.name) }) else {
            continue;
        };
        if field.kind > ENGINE_FIELD_QUAT {
            log::warn!("C++ component `{name}`: field `{field_name}` has unknown kind {}", field.kind);
            continue;
        }
        let (Some(get), Some(set)) = (field.get, field.set) else {
            log::warn!("C++ component `{name}`: field `{field_name}` has no accessors and is skipped");
            continue;
        };
        let mut variants = Vec::new();
        if field.kind == ENGINE_FIELD_ENUM && !field.variants.is_null() {
            for v in 0..field.variant_count as usize {
                // SAFETY: `variants` — массив из `variant_count` статических C-строк (контракт ABI).
                if let Some(variant) = unsafe { text(*field.variants.add(v)) } {
                    variants.push(variant.to_string());
                }
            }
        }
        descs.push(FieldDesc {
            name: field_name.to_string(),
            kind: field.kind,
            variants,
            // SAFETY: `asset_kind` — NULL или статическая C-строка плагина.
            asset_kind: unsafe { text(field.asset_kind) }.unwrap_or("").to_string(),
        });
        slots.push(FieldSlot {
            kind: field.kind,
            get,
            set,
        });
    }
    Some((
        name,
        descs,
        VtableHandle(Vtable {
            create,
            destroy: ty.destroy,
            on_start: ty.on_start,
            on_update: ty.on_update,
            on_fixed_update: ty.on_fixed_update,
            on_late_update: ty.on_late_update,
            on_collision_enter: ty.on_collision_enter,
            on_trigger_enter: ty.on_trigger_enter,
            on_destroy: ty.on_destroy,
            on_message: ty.on_message,
            fields: slots,
        }),
    ))
}

/// Непрозрачная обёртка: таблицу функций можно только передать в [`bind_class`].
pub(crate) struct VtableHandle(Vtable);

/// Создать класс или привязать новую таблицу функций к уже известному (перезагрузка).
pub(crate) fn bind_class(
    existing: Option<Rc<CppClass>>,
    name: &str,
    fields: &[FieldDesc],
    vtable: VtableHandle,
) -> Rc<CppClass> {
    let class = existing.unwrap_or_else(|| {
        Rc::new(CppClass {
            name: leak_str(name),
            descs: RefCell::new(Vec::new()),
            field_names: Cell::new(&[]),
            field_info: Cell::new(&[]),
            vtable: RefCell::new(None),
        })
    });
    class.set_fields(fields);
    *class.vtable.borrow_mut() = Some(vtable.0);
    class
}

/// Экземпляр C++-компонента на объекте сцены.
pub struct CppComponent {
    class: Rc<CppClass>,
    /// Экземпляр, созданный `create` текущей версии плагина; null — плагин выгружен.
    instance: *mut c_void,
    /// Значения полей на время перезагрузки плагина.
    parked: Vec<(String, Value)>,
}

impl CppComponent {
    pub(crate) fn new(class: Rc<CppClass>) -> Self {
        let mut component = Self {
            class,
            instance: std::ptr::null_mut(),
            parked: Vec::new(),
        };
        component.attach();
        component
    }

    /// Создать экземпляр в (новой) версии плагина и вернуть в него значения полей.
    pub(crate) fn attach(&mut self) {
        if !self.instance.is_null() {
            return;
        }
        let Some(create) = self.class.vtable.borrow().as_ref().map(|v| v.create) else {
            return;
        };
        // SAFETY: `create` — функция загруженного плагина (таблица привязана), без аргументов.
        self.instance = unsafe { create() };
        if self.instance.is_null() {
            log::error!(
                "C++ component `{}`: its constructor failed (threw an exception?); \
                 the component stays inert",
                self.class.name
            );
        }
        for (name, value) in std::mem::take(&mut self.parked) {
            // Поле могло исчезнуть или сменить тип в новой версии — тогда оно остаётся по умолчанию.
            let _ = self.set_field(&name, &value);
        }
    }

    /// Запомнить значения полей и уничтожить экземпляр — перед выгрузкой плагина.
    pub(crate) fn detach(&mut self) {
        if self.instance.is_null() {
            return;
        }
        self.parked = self
            .field_names()
            .iter()
            .filter_map(|name| Some((name.to_string(), self.get_field(name)?)))
            .collect();
        let destroy = self.class.vtable.borrow().as_ref().and_then(|v| v.destroy);
        if let Some(destroy) = destroy {
            // SAFETY: экземпляр создан `create` той же (ещё загруженной) версии плагина.
            unsafe { destroy(self.instance) };
        }
        self.instance = std::ptr::null_mut();
    }

    fn slot(&self, name: &str) -> Option<(usize, FieldSlot)> {
        let index = self.class.field_names.get().iter().position(|n| *n == name)?;
        let slot = self.class.vtable.borrow().as_ref()?.fields.get(index).copied()?;
        Some((index, slot))
    }

    /// Вызвать хук плагина над этим экземпляром.
    fn call(&self, world: &mut World, run: impl FnOnce(&Vtable, *mut c_void)) {
        if self.instance.is_null() {
            return;
        }
        // Копия таблицы: хук может косвенно обратиться к классу, заём держать нельзя.
        let Some(vtable) = self.class.vtable.borrow().clone() else {
            return;
        };
        let _scope = WorldScope::enter(world);
        run(&vtable, self.instance);
    }
}

impl Drop for CppComponent {
    fn drop(&mut self) {
        // Если плагин уже выгружен, его `destroy` вызывать нельзя: память экземпляра
        // ушла вместе с библиотекой.
        if self.class.is_bound() {
            self.detach();
        }
    }
}

impl Reflect for CppComponent {
    fn type_name(&self) -> &'static str {
        self.class.name
    }

    fn field_names(&self) -> &'static [&'static str] {
        self.class.field_names.get()
    }

    fn get_field(&self, name: &str) -> Option<Value> {
        if self.instance.is_null() {
            return self.parked.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        }
        let (index, slot) = self.slot(name)?;
        let mut out = nil_value();
        // SAFETY: экземпляр жив (создан `create` загруженной версии плагина), `get` описывает
        // его поле (контракт `ENGINE_FIELD`), `out` — валидное место для значения.
        unsafe { (slot.get)(self.instance, &mut out) };
        if slot.kind == ENGINE_FIELD_ENUM {
            // Перечисление идёт через границу номером варианта.
            let info = self.class.field_info.get()[index];
            return info.variants.get(usize::try_from(out.i).ok()?).map(|v| Value::Enum((*v).to_string()));
        }
        // SAFETY: строка в `out` (если есть) принадлежит экземпляру и жива до следующего
        // изменения поля; `from_engine_value` копирует её сразу.
        unsafe { from_engine_value(&out) }
    }

    fn set_field(&mut self, name: &str, value: &Value) -> Result<(), ReflectError> {
        let no_field = || ReflectError::NoSuchField {
            component: self.class.name,
            field: name.to_string(),
            available: self.class.field_names.get().join(", "),
        };
        if self.instance.is_null() {
            // Плагин выгружен: значение дождётся его возвращения.
            self.parked.retain(|(n, _)| n != name);
            self.parked.push((name.to_string(), value.clone()));
            return Ok(());
        }
        let (index, slot) = self.slot(name).ok_or_else(no_field)?;
        let field: &'static str = self.class.field_names.get()[index];
        let info = self.class.field_info.get()[index];
        let mismatch = |expected: &'static str| ReflectError::TypeMismatch {
            component: self.class.name,
            field,
            expected,
            got: value.kind_name(),
        };

        // Приводим значение к виду поля и упаковываем в ABI. Строки живут в `keep`
        // до конца вызова `set`.
        let keep: Option<CString>;
        let mut packed = nil_value();
        match (slot.kind, value) {
            (ENGINE_FIELD_F32, Value::Float(v)) => {
                packed.kind = ENGINE_VALUE_FLOAT;
                packed.f[0] = *v;
                keep = None;
            }
            (ENGINE_FIELD_F32, Value::Int(v)) => {
                packed.kind = ENGINE_VALUE_FLOAT;
                packed.f[0] = *v as f32;
                keep = None;
            }
            (ENGINE_FIELD_I32, Value::Int(v)) => {
                let v = i32::try_from(*v).map_err(|_| mismatch("Int (32 bit)"))?;
                packed.kind = ENGINE_VALUE_INT;
                packed.i = v as i64;
                keep = None;
            }
            (ENGINE_FIELD_BOOL, Value::Bool(v)) => {
                packed.kind = ENGINE_VALUE_BOOL;
                packed.i = *v as i64;
                keep = None;
            }
            (ENGINE_FIELD_VEC2, Value::Vec2(x, y)) => {
                packed.kind = ENGINE_VALUE_VEC2;
                packed.f = [*x, *y, 0.0, 0.0];
                keep = None;
            }
            (ENGINE_FIELD_VEC3, Value::Vec3(x, y, z)) => {
                packed.kind = ENGINE_VALUE_VEC3;
                packed.f = [*x, *y, *z, 0.0];
                keep = None;
            }
            (ENGINE_FIELD_COLOR, Value::Color(r, g, b, a)) => {
                packed.kind = ENGINE_VALUE_COLOR;
                packed.f = [*r, *g, *b, *a];
                keep = None;
            }
            (ENGINE_FIELD_QUAT, Value::Quat(x, y, z, w)) => {
                packed.kind = ENGINE_VALUE_QUAT;
                packed.f = [*x, *y, *z, *w];
                keep = None;
            }
            (ENGINE_FIELD_STRING, Value::Str(s)) => {
                packed.kind = ENGINE_VALUE_STR;
                keep = Some(c_string(s));
            }
            (ENGINE_FIELD_ASSET, Value::Asset(s) | Value::Str(s)) => {
                packed.kind = ENGINE_VALUE_ASSET;
                keep = Some(c_string(s));
            }
            (ENGINE_FIELD_ENUM, Value::Enum(s) | Value::Str(s)) => {
                let Some(position) = info.variants.iter().position(|v| *v == s.as_str()) else {
                    return Err(mismatch(info.expected));
                };
                packed.kind = ENGINE_VALUE_INT;
                packed.i = position as i64;
                keep = None;
            }
            _ => return Err(mismatch(info.expected)),
        }
        if let Some(text) = &keep {
            packed.s = text.as_ptr();
        }
        // SAFETY: экземпляр жив, `set` описывает его поле (контракт `ENGINE_FIELD`),
        // `packed` валиден, строка в нём жива до конца вызова (`keep`).
        let rejected = unsafe { (slot.set)(self.instance, &packed) };
        if rejected != 0 {
            return Err(mismatch(info.expected));
        }
        Ok(())
    }

    fn field_meta(&self, name: &str) -> FieldMeta {
        match self.slot(name) {
            Some((index, _)) => {
                let info = self.class.field_info.get()[index];
                FieldMeta {
                    variants: info.variants,
                    asset_kind: info.asset_kind,
                }
            }
            None => FieldMeta::default(),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Все хуки: область мира + вызов функции плагина с сигнатурой из ABI.
// SAFETY (общий для блоков ниже): `instance` создан `create` этой же версии плагина,
// плагин загружен (таблица привязана), аргументы — значения. Исключения C++ гасит
// переходник в engine_component.hpp, поэтому через границу ничего не летит.
impl Component for CppComponent {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let object = to_object(ctx.id);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_start {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object) }
            }
        });
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let object = to_object(ctx.id);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_update {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object, dt) }
            }
        });
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let object = to_object(ctx.id);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_fixed_update {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object, dt) }
            }
        });
    }

    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let object = to_object(ctx.id);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_late_update {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object, dt) }
            }
        });
    }

    fn on_collision_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        let (object, other) = (to_object(ctx.id), to_object(other));
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_collision_enter {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object, other) }
            }
        });
    }

    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        let (object, other) = (to_object(ctx.id), to_object(other));
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_trigger_enter {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object, other) }
            }
        });
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        let object = to_object(ctx.id);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_destroy {
                // SAFETY: см. комментарий над `impl`.
                unsafe { hook(instance, object) }
            }
        });
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        let object = to_object(ctx.id);
        let name = c_string(&message.name);
        let value = to_engine_value(&message.value);
        self.call(ctx.world, |vtable, instance| {
            if let Some(hook) = vtable.on_message {
                // SAFETY: см. комментарий над `impl`; `name` и `value` живут до конца вызова.
                unsafe { hook(instance, object, name.as_ptr(), &value) }
            }
        });
    }
}

/// C-строка из Rust-строки (нулевые байты заменяются пробелами).
pub(crate) fn c_string(text: &str) -> CString {
    CString::new(text.replace('\0', " ")).unwrap_or_default()
}
