//! Плагины на C++: динамические (`.dll` / `.so` / `.dylib`) и вкомпилированные статически.
//!
//! Весь `unsafe` работы с библиотекой сосредоточен в этом файле; наружу —
//! безопасный тип [`Plugin`] и подсистема [`CppPlugins`].

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

use engine_core::prelude::*;
use crate::dylib::Library;

use crate::abi::{EnginePlugin, EnginePluginEntry, ENGINE_ABI_VERSION};
use crate::component::{bind_class, read_type, CppClass, CppComponent, FieldDesc, VtableHandle, HOST_API};
use crate::scope::WorldScope;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error(
        "cannot load plugin `{path}`: {source}. Check that the file exists, was built for \
         this platform/architecture, and that its own dependencies are next to it"
    )]
    Load {
        path: String,
        #[source]
        source: crate::dylib::Error,
    },
    #[error("cannot copy plugin `{path}` for hot reload: {source}")]
    Copy {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "`{path}` is not an engine plugin: symbol `engine_plugin_entry` is missing. \
         Export it with `extern \"C\"` (and `__declspec(dllexport)` on Windows)"
    )]
    NoEntry { path: String },
    #[error("plugin `{path}` returned a null descriptor from `engine_plugin_entry`")]
    NullDescriptor { path: String },
    #[error(
        "plugin `{path}` was built for plugin ABI v{found}, the engine uses v{expected}. \
         Rebuild the plugin against the current include/engine_api.h"
    )]
    AbiMismatch {
        path: String,
        found: u32,
        expected: u32,
    },
    #[error(
        "plugin `{name}` refused to load: on_load returned {code} (1 means the plugin's own          ABI check failed: rebuild it against the current headers)"
    )]
    InitFailed { name: String, code: i32 },
    #[error("plugin `{name}` does not provide `{capability}`")]
    Unsupported {
        name: String,
        capability: &'static str,
    },
    #[error(
        "the C++ scripts of `{name}` are not available on this platform: C++ is not built for the web          (wasm32). Run the game on desktop or Android, or guard the call with cfg(engine_game_cpp)"
    )]
    StaticUnavailable { name: String },
    #[error("buffer has {actual} bytes, but a {width}x{height} RGBA image needs {expected}")]
    BufferSize {
        actual: usize,
        expected: usize,
        width: u32,
        height: u32,
    },
}

/// Точка входа C++-скриптов игры, вкомпилированных в её крейт (`engine-cpp-build`).
///
/// Создаётся макросом `engine::cpp_scripts!()`, передаётся в
/// [`CppPlugins::add_static_scripts`] или `engine::add_static_cpp`.
#[derive(Clone, Copy)]
pub struct StaticScripts {
    name: &'static str,
    entry: Option<unsafe extern "C" fn() -> *const EnginePlugin>,
}

impl StaticScripts {
    /// Скрипты с точкой входа `entry`.
    ///
    /// # Safety
    /// `entry` возвращает null или указатель на `EnginePlugin` со статическим временем
    /// жизни, оформленный по правилам ABI (макрос `ENGINE_PLUGIN_AUTO_ENTRY`).
    pub unsafe fn from_entry(name: &'static str, entry: unsafe extern "C" fn() -> *const EnginePlugin) -> Self {
        Self { name, entry: Some(entry) }
    }

    /// Скриптов на этой платформе нет (веб): подключение вернёт [`PluginError::StaticUnavailable`].
    pub fn unavailable(name: &'static str) -> Self {
        Self { name, entry: None }
    }

    /// Имя пакета игры.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Скрипты вкомпилированы в эту сборку.
    pub fn is_available(&self) -> bool {
        self.entry.is_some()
    }
}

impl std::fmt::Debug for StaticScripts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticScripts").field("name", &self.name).field("available", &self.is_available()).finish()
    }
}

/// Имя файла динамической библиотеки на текущей платформе:
/// `plugin_file_name("vignette")` → `vignette.dll` / `libvignette.so` / `libvignette.dylib`.
pub fn plugin_file_name(stem: &str) -> String {
    format!(
        "{}{stem}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

/// Откуда загружена «горячая» копия плагина.
struct HotSource {
    /// Файл, который пересобирает пользователь.
    original: PathBuf,
    /// Копия, которая загружена на самом деле: оригинал остаётся свободным для записи.
    copy: PathBuf,
    /// Время изменения оригинала, когда мы его копировали (или пытались).
    modified: Option<SystemTime>,
}

/// Загруженный плагин. При уничтожении вызывает `on_unload` и выгружает библиотеку.
pub struct Plugin {
    name: String,
    path: PathBuf,
    /// Указатель на статическую структуру плагина.
    /// Инвариант: валиден, пока жив `library` (для статических модулей — всегда).
    descriptor: *const EnginePlugin,
    /// `None` — модуль вкомпилирован в программу.
    library: Option<Library>,
    /// Типы компонентов, прочитанные из плагина и ещё не привязанные к классам.
    pending: Vec<(String, Vec<FieldDesc>, VtableHandle)>,
    classes: Vec<Rc<CppClass>>,
    hot: Option<HotSource>,
}

impl Plugin {
    /// Загрузить динамический плагин.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PluginError> {
        Self::open(path.as_ref(), path.as_ref(), None)
    }

    /// Загрузить копию плагина, оставив сам файл свободным: его можно пересобрать,
    /// пока программа работает, и [`CppPlugins`] подхватит новую версию.
    pub fn load_hot(path: impl AsRef<Path>) -> Result<Self, PluginError> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let original = path.as_ref().to_path_buf();
        let modified = std::fs::metadata(&original).and_then(|m| m.modified()).ok();
        let stem = original.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin");
        let extension = original.extension().and_then(|s| s.to_str()).unwrap_or("bin");
        let copy = original.with_file_name(format!(
            "{stem}.live-{}-{}.{extension}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::copy(&original, &copy).map_err(|source| PluginError::Copy {
            path: original.display().to_string(),
            source,
        })?;
        let hot = HotSource {
            original: original.clone(),
            copy: copy.clone(),
            modified,
        };
        Self::open(&copy, &original, Some(hot)).inspect_err(|_| {
            let _ = std::fs::remove_file(&copy);
        })
    }

    fn open(file: &Path, shown_path: &Path, hot: Option<HotSource>) -> Result<Self, PluginError> {
        let shown = shown_path.display().to_string();

        // SAFETY: загрузка библиотеки исполняет её статические инициализаторы —
        // произвольный нативный код. Плагины — доверенный код проекта, как и
        // любая слинкованная библиотека; это задокументированное допущение.
        let library = unsafe { Library::new(file) }.map_err(|source| PluginError::Load {
            path: shown.clone(),
            source,
        })?;

        // SAFETY: тип символа зафиксирован ABI (`EnginePluginEntry`); несовпадение
        // версий ловится в `from_descriptor` по `abi_version` — первому полю структуры.
        let descriptor = unsafe {
            let entry = library
                .get::<EnginePluginEntry>(b"engine_plugin_entry\0")
                .map_err(|_| PluginError::NoEntry {
                    path: shown.clone(),
                })?;
            entry()
        };
        // SAFETY: указатель получен из точки входа загруженной библиотеки, которая
        // передаётся во владение создаваемому плагину.
        unsafe { Self::from_descriptor(descriptor, shown_path.to_path_buf(), Some(library), hot) }
    }

    /// Подключить модуль, вкомпилированный в программу (например, через `cc` в `build.rs`).
    ///
    /// # Safety
    /// `descriptor` — null или указатель на `EnginePlugin` со статическим временем
    /// жизни, оформленный по правилам ABI (как у динамического плагина).
    pub unsafe fn from_static(descriptor: *const EnginePlugin) -> Result<Self, PluginError> {
        // SAFETY: предусловие функции; библиотеки нет — код живёт всё время работы программы.
        unsafe { Self::from_descriptor(descriptor, PathBuf::from("<static module>"), None, None) }
    }

    /// # Safety
    /// `descriptor` — null или указатель на `EnginePlugin`, живой, пока жив `library`
    /// (или всегда, если `library` — `None`).
    unsafe fn from_descriptor(
        descriptor: *const EnginePlugin,
        path: PathBuf,
        library: Option<Library>,
        hot: Option<HotSource>,
    ) -> Result<Self, PluginError> {
        let shown = path.display().to_string();
        if descriptor.is_null() {
            return Err(PluginError::NullDescriptor { path: shown });
        }

        // SAFETY: указатель не нулевой и по контракту указывает на `EnginePlugin`.
        // Сначала читаем только `abi_version`: при несовпадении остальные поля не трогаем.
        let abi_version = unsafe { (*descriptor).abi_version };
        if abi_version != ENGINE_ABI_VERSION {
            return Err(PluginError::AbiMismatch {
                path: shown,
                found: abi_version,
                expected: ENGINE_ABI_VERSION,
            });
        }

        // SAFETY: версия совпала, значит раскладка структуры та же, что у нас.
        let desc = unsafe { &*descriptor };
        let name = if desc.name.is_null() {
            shown.clone()
        } else {
            // SAFETY: `name` — статическая C-строка плагина (контракт ABI).
            unsafe { CStr::from_ptr(desc.name) }
                .to_string_lossy()
                .into_owned()
        };

        if let Some(on_load) = desc.on_load {
            // SAFETY: колбэк плагина с сигнатурой из ABI; `HOST_API` — `static`,
            // живёт дольше любого плагина.
            let code = unsafe { on_load(&HOST_API) };
            if code != 0 {
                return Err(PluginError::InitFailed { name, code });
            }
        }

        let mut pending = Vec::new();
        if !desc.components.is_null() {
            for index in 0..desc.component_count as usize {
                // SAFETY: `components` — массив из `component_count` описаний (контракт ABI),
                // живой столько же, сколько сам дескриптор.
                let ty = unsafe { &*desc.components.add(index) };
                // SAFETY: описание принадлежит плагину с совпавшей версией ABI.
                if let Some(parsed) = unsafe { read_type(ty) } {
                    pending.push(parsed);
                }
            }
        }

        Ok(Self {
            name,
            path,
            descriptor,
            library,
            pending,
            classes: Vec::new(),
            hot,
        })
    }

    fn descriptor(&self) -> &EnginePlugin {
        // SAFETY: инвариант структуры — `descriptor` валиден, пока жив `self.library`.
        unsafe { &*self.descriptor }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Имена игровых компонентов, которые даёт плагин.
    pub fn component_names(&self) -> Vec<&'static str> {
        self.classes.iter().map(|class| class.name()).collect()
    }

    /// Шаг симуляции плагина. Ничего не делает, если плагин не задал колбэк.
    pub fn fixed_update(&self, dt: f32) {
        if let Some(callback) = self.descriptor().on_fixed_update {
            // SAFETY: колбэк плагина с сигнатурой из ABI, аргумент — значение.
            unsafe { callback(dt) }
        }
    }

    /// То же, но с доступом к миру: из `on_fixed_update(dt)` плагина можно звать функции API.
    pub fn fixed_update_in(&self, world: &mut World, dt: f32) {
        if let Some(callback) = self.descriptor().on_fixed_update {
            let _scope = WorldScope::enter(world);
            // SAFETY: колбэк плагина с сигнатурой из ABI, аргумент — значение.
            unsafe { callback(dt) }
        }
    }

    /// CPU пост-эффект над RGBA8-изображением на месте.
    pub fn process_rgba(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
    ) -> Result<(), PluginError> {
        let callback = self
            .descriptor()
            .process_rgba
            .ok_or_else(|| PluginError::Unsupported {
                name: self.name.clone(),
                capability: "process_rgba",
            })?;
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(PluginError::BufferSize {
                actual: pixels.len(),
                expected,
                width,
                height,
            });
        }
        // SAFETY: срез эксклюзивный и ровно `width * height * 4` байт (проверено
        // выше) — это и есть предусловие колбэка. Плагин не сохраняет указатель.
        unsafe { callback(pixels.as_mut_ptr(), width, height) };
        Ok(())
    }

    /// Изменился ли файл плагина с момента загрузки (только для «горячих»).
    fn source_changed(&self) -> bool {
        let Some(hot) = &self.hot else {
            return false;
        };
        let now = std::fs::metadata(&hot.original).and_then(|m| m.modified()).ok();
        now.is_some() && now != hot.modified
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        // Экземпляры компонентов, пережившие плагин, не должны звать его функции.
        for class in &self.classes {
            class.unbind();
        }
        if let Some(on_unload) = self.descriptor().on_unload {
            // SAFETY: код плагина ещё загружен — библиотека выгружается ниже.
            unsafe { on_unload() }
        }
        log::info!("plugin `{}` unloaded ({})", self.name, self.path.display());
        // Сначала выгрузить библиотеку, потом удалить её «горячую» копию.
        drop(self.library.take());
        if let Some(hot) = &self.hot {
            let _ = std::fs::remove_file(&hot.copy);
        }
    }
}

/// Подсистема: держит плагины, регистрирует их компоненты, зовёт `on_fixed_update`
/// и перезагружает «горячие» плагины, когда их файл пересобран.
#[derive(Default)]
pub struct CppPlugins {
    plugins: Vec<Plugin>,
    /// Все известные типы компонентов по имени; переживают перезагрузку плагина.
    classes: HashMap<String, Rc<CppClass>>,
    frame: u64,
    /// Сколько проверок подряд новая версия плагина не загрузилась.
    failed_reloads: u32,
}

impl CppPlugins {
    /// Загрузить плагин. Его компоненты станут доступны после
    /// [`register_components`](Self::register_components).
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<&Plugin, PluginError> {
        let plugin = Plugin::load(path)?;
        Ok(self.adopt(plugin))
    }

    /// Загрузить плагин с горячей перезагрузкой: пересоберите файл — и новая версия
    /// подхватится сама, значения полей компонентов сохранятся.
    pub fn load_hot(&mut self, path: impl AsRef<Path>) -> Result<&Plugin, PluginError> {
        let plugin = Plugin::load_hot(path)?;
        Ok(self.adopt(plugin))
    }

    /// Подключить модуль, вкомпилированный в программу.
    ///
    /// # Safety
    /// См. [`Plugin::from_static`].
    pub unsafe fn add_static(&mut self, descriptor: *const EnginePlugin) -> Result<&Plugin, PluginError> {
        // SAFETY: предусловие передано вызывающему.
        let plugin = unsafe { Plugin::from_static(descriptor) }?;
        Ok(self.adopt(plugin))
    }

    /// Подключить C++-скрипты игры, собранные `engine-cpp-build` (см. `engine::cpp_scripts!()`).
    pub fn add_static_scripts(&mut self, scripts: StaticScripts) -> Result<&Plugin, PluginError> {
        let Some(entry) = scripts.entry else {
            return Err(PluginError::StaticUnavailable { name: scripts.name.to_string() });
        };
        // SAFETY: `StaticScripts::from_entry` приняла точку входа под то же предусловие.
        unsafe { self.add_static(entry()) }
    }

    fn adopt(&mut self, mut plugin: Plugin) -> &Plugin {
        self.bind(&mut plugin);
        log::info!(
            "plugin `{}` loaded{}",
            plugin.name(),
            if plugin.classes.is_empty() {
                String::new()
            } else {
                format!(": components {}", plugin.component_names().join(", "))
            }
        );
        self.plugins.push(plugin);
        self.plugins.last().expect("just pushed")
    }

    /// Привязать типы плагина к классам (новым или уже известным по имени).
    fn bind(&mut self, plugin: &mut Plugin) {
        for (name, fields, vtable) in std::mem::take(&mut plugin.pending) {
            let class = bind_class(self.classes.get(&name).cloned(), &name, &fields, vtable);
            self.classes.insert(name, class.clone());
            plugin.classes.push(class);
        }
    }

    /// Зарегистрировать компоненты всех плагинов: после этого их можно загружать из
    /// сцен и добавлять в редакторе наравне с компонентами на Rust.
    pub fn register_components(&self, registry: &mut TypeRegistry) {
        for class in self.classes.values() {
            let class = class.clone();
            let class_name = class.name();
            registry.register_factory(class.name(), move || Box::new(CppComponent::new(class.clone())));
            registry.categorize(&[class_name], "Scripts (C++)");
        }
    }

    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.name() == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Plugin> {
        self.plugins.iter()
    }

    /// Перезагрузить «горячие» плагины, файл которых изменился. Возвращает их число.
    pub fn reload_changed(&mut self, world: &mut World) -> usize {
        let mut reloaded = 0;
        for index in 0..self.plugins.len() {
            if !self.plugins[index].source_changed() {
                continue;
            }
            let Some(original) = self.plugins[index].hot.as_ref().map(|hot| hot.original.clone()) else {
                continue;
            };
            // Сначала пробуем загрузить новую версию: если файл ещё дописывается или
            // сломан, старая продолжает работать.
            let mut fresh = match Plugin::load_hot(&original) {
                Ok(plugin) => plugin,
                Err(err) => {
                    // Чаще всего файл просто ещё дописывает компоновщик: молча пробуем
                    // снова при следующей проверке и сообщаем, только если не выходит долго.
                    self.failed_reloads += 1;
                    if self.failed_reloads == 6 {
                        log::error!("{err}; keeping the previous version of the plugin");
                    }
                    continue;
                }
            };

            // Экземпляры старой версии сохраняют поля и уничтожаются её же кодом…
            self.failed_reloads = 0;
            for_each_cpp_component(world, |component| component.detach());
            // …старая версия выгружается (её классы отвязываются в `Drop`)…
            self.bind(&mut fresh);
            let old = std::mem::replace(&mut self.plugins[index], fresh);
            let kept: Vec<Rc<CppClass>> = self.plugins[index].classes.clone();
            drop_keeping_bound(old, &kept);
            // …и экземпляры создаются заново кодом новой версии.
            self.register_components(&mut world.registry);
            for_each_cpp_component(world, |component| component.attach());
            // Новая версия класса могла получить или потерять хуки.
            for id in world.scene.ids() {
                if let Some(object) = world.scene.get_mut(id) {
                    if object.components().any(|c| c.as_any().is::<CppComponent>()) {
                        object.refresh_hooks();
                    }
                }
            }
            log::info!(
                "plugin `{}` reloaded: {}",
                self.plugins[index].name(),
                self.plugins[index].component_names().join(", ")
            );
            reloaded += 1;
        }
        reloaded
    }
}

/// Выгрузить старую версию плагина, не отвязывая классы, которые уже привязаны к новой.
fn drop_keeping_bound(mut old: Plugin, kept: &[Rc<CppClass>]) {
    old.classes.retain(|class| !kept.iter().any(|k| Rc::ptr_eq(k, class)));
    drop(old);
}

fn for_each_cpp_component(world: &mut World, mut f: impl FnMut(&mut CppComponent)) {
    for id in world.scene.ids() {
        let Some(object) = world.scene.get_mut(id) else {
            continue;
        };
        for component in object.components_mut() {
            if let Some(component) = component.as_any_mut().downcast_mut::<CppComponent>() {
                f(component);
            }
        }
    }
}

impl Subsystem for CppPlugins {
    fn name(&self) -> &'static str {
        "cpp-plugins"
    }

    fn init(&mut self, world: &mut World) {
        self.register_components(&mut world.registry);
    }

    fn pre_update(&mut self, world: &mut World) {
        // Раз в полсекунды (при 60 кадрах) смотрим, не пересобран ли плагин.
        self.frame += 1;
        if self.frame.is_multiple_of(30) {
            self.reload_changed(world);
        }
    }

    fn idle(&mut self, world: &mut World) {
        self.pre_update(world);
    }

    fn fixed_update(&mut self, world: &mut World, dt: f32) {
        for plugin in &self.plugins {
            plugin.fixed_update_in(world, dt);
        }
    }
}

/// Игровые компоненты из `plugins/cpp-gameplay/gameplay.cpp`, вкомпилированные в
/// крейт: `Spinner`, `Bobber`, `Mover`. Тот же код, что в динамическом плагине, —
/// удобно для тестов и для игр, которым не нужна горячая перезагрузка.
#[cfg(engine_static_cpp)]
pub fn builtin_gameplay(plugins: &mut CppPlugins) -> Result<&Plugin, PluginError> {
    // SAFETY: функция определена в `gameplay.cpp`, слинкованном `build.rs`, и возвращает
    // указатель на статическую `EnginePlugin`, оформленную макросом из engine_component.hpp.
    unsafe { plugins.add_static(crate::ffi::engine_builtin_gameplay_entry()) }
}
