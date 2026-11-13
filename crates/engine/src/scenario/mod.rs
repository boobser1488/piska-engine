//! Сценарии на языке **Scenario** (`.scenario`) поверх компонентов: катсцены, обучение,
//! диалоги, события уровня, волны врагов. Руководство — `docs/api/scenario.md`.
//!
//! * [`ScenarioRunner`] — компонент: какой сценарий запустить на объекте;
//! * подсистема (ставится в [`crate::app`]) компилирует сценарии (с кэшем), каждый кадр
//!   продолжает их задачи, доставляет сообщения, пишет ошибки в лог/Console как
//!   `file:line:col` и перезапускает сценарий, когда его файл изменился (горячая
//!   перезагрузка — без пересборки игры);
//! * [`register`] — своя функция для сценариев из Rust.
//!
//! Компоненты по-прежнему пишутся на Rust и C++; язык ядра — крейт `engine-scenario`.

mod api;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use engine_assets::{AssetDatabase, AssetHandle, AssetType};
use engine_core::prelude::*;
use engine_core::reflect::Value as EValue;
use engine_scenario::{Group, Vm};

pub use api::{ease_by_name, from_script, key_by_name, to_script};
pub use engine_scenario as lang;
pub use engine_scenario::{Args, ErrorKind, Limits, Script, ScriptError, Throw, Value as ScriptValue};

/// Маркер ассета-сценария для [`AssetHandle`]: импортёр `Scenario`, файлы `.scenario`.
#[derive(Debug)]
pub struct ScenarioAsset;

impl AssetType for ScenarioAsset {
    const IMPORTER: &'static str = "Scenario";
}

component! {
    /// Запускает сценарий Scenario (`.scenario`) на этом объекте. Сценарий перезапускается,
    /// когда его файл сохранён (горячая перезагрузка, в том числе в Play).
    pub struct ScenarioRunner {
        /// Файл сценария (обычно `assets/scenarios/*.scenario`).
        pub script: AssetHandle<ScenarioAsset> = AssetHandle::none(),
        /// Путь сценария относительно ассетов — вместо `script` (удобно из кода).
        pub path: String = String::new(),
        /// Запустить, как только объект появился. Иначе — `engine::scenario::start`.
        pub auto_start: bool = true,
        /// Перезапускать при изменении файла (на десктопе и в редакторе).
        pub hot_reload: bool = true,
    }
}

impl ScenarioRunner {
    /// Раннер сценария по пути относительно ассетов: `ScenarioRunner::new("scenarios/intro.scenario")`.
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string(), ..Self::default() }
    }
}

impl Component for ScenarioRunner {
    fn hooks(&self) -> Hooks {
        Hooks::MESSAGE
    }

    /// Сообщения объекту (от Rust, C++, интерфейса или других сценариев) идут в сценарий:
    /// их ждёт `wait_message(name)` и получают обработчики `on(name, fn)`.
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        let id = ctx.id;
        ctx.world.resources.get_or_insert_with(Inbox::default).0.push((id, message.name.clone(), message.value.clone()));
    }
}

/// Событие `broadcast(name, value)` из сценария — Rust читает его через
/// `world.events.read::<ScenarioMessage>()` в следующем кадре.
#[derive(Clone, Debug)]
pub struct ScenarioMessage {
    pub name: String,
    pub value: EValue,
}

#[derive(Default)]
struct Inbox(Vec<(GameObjectId, String, EValue)>);

/// Контекст своей функции сценария (см. [`register`]).
pub struct ScenarioCtx<'a> {
    pub world: &'a mut World,
    /// Объект, на котором работает вызвавший сценарий.
    pub object: GameObjectId,
}

/// Своя функция для сценариев:
///
/// ```no_run
/// # let mut app = engine::app();
/// engine::scenario::register(&mut app, "give_coins", |ctx, args| {
///     let n = args.int(0)?;
///     let coins = ctx.world.resources.get_or_insert_with(|| 0i64);
///     *coins += n;
///     Ok(engine::scenario::ScriptValue::Num(*coins as f64))
/// });
/// ```
///
/// Ошибка (`Err(Throw::msg("…"))` или неверный аргумент через `args.num(i)?`) показывается
/// в месте вызова в сценарии: `file:line:col`.
pub fn register(
    app: &mut App,
    name: &str,
    f: impl Fn(&mut ScenarioCtx, Args) -> Result<ScriptValue, Throw> + 'static,
) {
    let rt = runtime(&mut app.world);
    rt.vm.register(name, move |ctx, args| {
        let object = GameObjectId::from_bits(ctx.user());
        let world = api::world(ctx)?;
        let mut sc = ScenarioCtx { world, object };
        f(&mut sc, args)
    });
}

/// Своя константа или таблица для сценариев (`register_value(&mut app, "MAX_WAVES", 5.0.into())`).
pub fn register_value(app: &mut App, name: &str, value: ScriptValue) {
    runtime(&mut app.world).vm.register_value(name, value);
}

/// Запустить (или перезапустить) сценарий объекта — для раннеров с `auto_start = false`.
pub fn start(world: &mut World, object: GameObjectId) {
    runtime(world).requests.push((object, true));
}

/// Остановить сценарий объекта (до следующего [`start`]).
pub fn stop(world: &mut World, object: GameObjectId) {
    runtime(world).requests.push((object, false));
}

/// Сценарий объекта сейчас работает (есть живые задачи или обработчики сообщений)?
pub fn is_running(world: &World, object: GameObjectId) -> bool {
    world
        .resources
        .get::<ScenarioRuntime>()
        .and_then(|rt| rt.runners.get(&object.to_bits()))
        .is_some_and(|r| r.running && !r.group.is_idle())
}

/// Глобальная переменная работающего сценария объекта (для тестов и отладки).
pub fn global(world: &World, object: GameObjectId, name: &str) -> Option<ScriptValue> {
    let rt = world.resources.get::<ScenarioRuntime>()?;
    rt.runners.get(&object.to_bits())?.group.module()?.get(name)
}

/// Ошибки сценариев с прошлого вызова (они же уже в логе / Console).
pub fn take_errors(world: &mut World) -> Vec<ScriptError> {
    std::mem::take(&mut runtime(world).errors)
}

/// Проверить сценарий без запуска: синтаксис и неизвестные имена.
pub fn check(world: &mut World, path: &str) -> Result<(), String> {
    let text = read_source(world, path)?;
    let rt = runtime(world);
    let script = Script::compile(path, &text).map_err(|e| e.report())?;
    rt.vm.instantiate(&script).map(|_| ()).map_err(|e| e.report())
}

/// Пределы исполнения (инструкций за кадр, глубина вызовов, память).
pub fn limits(world: &mut World) -> &mut Limits {
    &mut runtime(world).vm.limits
}

/// Как часто проверять файлы сценариев на изменения (секунды реального времени).
const WATCH_INTERVAL: f64 = 0.25;

struct Runner {
    group: Group,
    path: String,
    hash: u64,
    running: bool,
    /// Хеш файла, который не скомпилировался: ждём исправления.
    failed: Option<u64>,
}

struct Compiled {
    hash: u64,
    script: Rc<Script>,
}

/// Состояние всех сценариев мира (ресурс).
pub struct ScenarioRuntime {
    vm: Vm,
    object_class: u32,
    runners: BTreeMap<u64, Runner>,
    compiled: HashMap<String, Compiled>,
    /// Какие версии файлов уже проверены (и, если с ошибкой, — показаны).
    checked: HashMap<String, u64>,
    reported: HashSet<(String, u64)>,
    clock: f64,
    idle_ticks: u32,
    last_frame: u64,
    requests: Vec<(GameObjectId, bool)>,
    errors: Vec<ScriptError>,
}

impl ScenarioRuntime {
    fn new() -> Self {
        let mut vm = Vm::new();
        vm.set_printer(|level, text| match level {
            engine_scenario::Level::Warn => log::warn!(target: "scenario", "{text}"),
            engine_scenario::Level::Info => log::info!(target: "scenario", "{text}"),
        });
        let object_class = api::install(&mut vm);
        vm.register_value("self", ScriptValue::Nil);
        Self {
            vm,
            object_class,
            runners: BTreeMap::new(),
            compiled: HashMap::new(),
            checked: HashMap::new(),
            reported: HashSet::new(),
            clock: 0.0,
            idle_ticks: 0,
            last_frame: 0,
            requests: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn report(&mut self, e: ScriptError) {
        log::error!(target: "scenario", "{}", e.report());
        if self.errors.len() < 64 {
            self.errors.push(e);
        }
    }
}

fn runtime(world: &mut World) -> &mut ScenarioRuntime {
    world.resources.get_or_insert_with(ScenarioRuntime::new)
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn read_source(world: &World, path: &str) -> Result<String, String> {
    let db = world
        .resources
        .get::<AssetDatabase>()
        .ok_or_else(|| format!("{path}: assets are not mounted (call engine::mount_assets first)"))?;
    let bytes = db.read_path(path).map_err(|e| format!("{path}: {e}"))?;
    String::from_utf8(bytes.into_owned()).map_err(|_| format!("{path}: the file is not UTF-8 text"))
}

fn runner_path(world: &World, runner: &ScenarioRunner) -> String {
    if !runner.script.is_none() {
        if let Some(path) = world.resources.get::<AssetDatabase>().and_then(|db| db.relative_path_of(runner.script.guid())) {
            return path.to_string();
        }
    }
    runner.path.replace('\\', "/")
}

/// Скомпилировать (с кэшем по содержимому). `Err(None)` — ошибка уже показана.
fn compile(rt: &mut ScenarioRuntime, world: &World, path: &str) -> Result<(Rc<Script>, u64), Option<u64>> {
    let text = match read_source(world, path) {
        Ok(t) => t,
        Err(e) => {
            if rt.reported.insert((path.to_string(), 0)) {
                log::error!(target: "scenario", "{e}");
            }
            return Err(Some(0));
        }
    };
    let h = hash(text.as_bytes());
    if let Some(c) = rt.compiled.get(path) {
        if c.hash == h {
            return Ok((c.script.clone(), h));
        }
    }
    match Script::compile(path, &text) {
        Ok(script) => {
            rt.compiled.insert(path.to_string(), Compiled { hash: h, script: script.clone() });
            Ok((script, h))
        }
        Err(e) => {
            if rt.reported.insert((path.to_string(), h)) {
                rt.report(e);
            }
            Err(Some(h))
        }
    }
}

/// Запустить сценарий объекта заново.
fn start_runner(rt: &mut ScenarioRuntime, world: &World, id: GameObjectId, path: String) {
    let bits = id.to_bits();
    if let Some(mut old) = rt.runners.remove(&bits) {
        old.group.stop_all();
    }
    let mut group = Group::new(bits);
    let (hash, running, failed) = match compile(rt, world, &path) {
        Ok((script, h)) => {
            rt.vm.register_value("self", api::handle(rt.object_class, id));
            match group.start(&rt.vm, &script) {
                Ok(()) => (h, true, None),
                Err(e) => {
                    if rt.reported.insert((path.clone(), h)) {
                        rt.report(e);
                    }
                    (h, false, Some(h))
                }
            }
        }
        Err(h) => (h.unwrap_or(0), false, h),
    };
    rt.runners.insert(bits, Runner { group, path, hash, running, failed });
}

/// Проверка всех `.scenario` из базы ассетов: ошибка видна в Console сразу после сохранения.
fn check_changed_files(rt: &mut ScenarioRuntime, world: &World) {
    let Some(db) = world.resources.get::<AssetDatabase>() else { return };
    if db.is_packed() {
        return;
    }
    let paths: Vec<String> = db
        .assets()
        .filter(|(_, p)| p.ends_with(".scenario"))
        .map(|(_, p)| p.to_string())
        .collect();
    for path in paths {
        let Ok(bytes) = db.read_path(&path) else { continue };
        let h = hash(&bytes);
        if rt.checked.get(&path) == Some(&h) {
            continue;
        }
        rt.checked.insert(path.clone(), h);
        let text = String::from_utf8_lossy(&bytes);
        let result = match Script::compile(&path, &text) {
            Ok(s) => rt.vm.instantiate(&s).map(|_| ()),
            Err(e) => Err(e),
        };
        match result {
            Err(e) => {
                if rt.reported.insert((path.clone(), h)) {
                    rt.report(e);
                }
            }
            Ok(()) => {
                // Был с ошибкой — сообщить, что исправлен.
                if rt.reported.iter().any(|(p, _)| *p == path) {
                    rt.reported.retain(|(p, _)| *p != path);
                    log::info!(target: "scenario", "{path}: compiles again");
                }
            }
        }
    }
}

/// Горячая перезагрузка: изменившиеся файлы работающих сценариев перезапускаются.
fn hot_reload(rt: &mut ScenarioRuntime, world: &World, settings: &HashMap<u64, (String, bool)>) {
    if world.resources.get::<AssetDatabase>().is_none_or(|db| db.is_packed()) {
        return;
    }
    let mut paths: Vec<String> = rt
        .runners
        .iter()
        .filter(|(bits, _)| settings.get(bits).is_some_and(|(_, hot)| *hot))
        .map(|(_, r)| r.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        let Ok(text) = read_source(world, &path) else { continue };
        let h = hash(text.as_bytes());
        let stale: Vec<u64> = rt
            .runners
            .iter()
            .filter(|(_, r)| r.path == path && r.hash != h && r.failed != Some(h))
            .map(|(b, _)| *b)
            .collect();
        if stale.is_empty() {
            continue;
        }
        match compile(rt, world, &path) {
            Ok(_) => {
                for bits in &stale {
                    start_runner(rt, world, GameObjectId::from_bits(*bits), path.clone());
                }
                rt.checked.insert(path.clone(), h);
                log::info!(target: "scenario", "{path}: reloaded ({} runner(s) restarted)", stale.len());
            }
            Err(_) => {
                // Старая версия продолжает работать; ошибка уже показана.
                for bits in stale {
                    if let Some(r) = rt.runners.get_mut(&bits) {
                        if r.running {
                            r.hash = h;
                        } else {
                            r.failed = Some(h);
                        }
                    }
                }
                rt.checked.insert(path.clone(), h);
            }
        }
    }
}

fn update(world: &mut World) {
    let Some(mut rt) = world.resources.remove::<ScenarioRuntime>() else { return };
    // Новый запуск игры (редактор: Play после Stop) — старые сценарии не продолжаются.
    if world.time.frame < rt.last_frame {
        for (_, mut r) in std::mem::take(&mut rt.runners) {
            r.group.stop_all();
        }
    }
    rt.last_frame = world.time.frame;

    // 1. Раннеры сцены.
    let found: Vec<(GameObjectId, ScenarioRunner)> =
        world.scene.iter_with::<ScenarioRunner>().map(|(id, c)| (id, c.clone())).collect();
    let mut settings: HashMap<u64, (String, bool)> = HashMap::new();
    for (id, comp) in &found {
        settings.insert(id.to_bits(), (runner_path(world, comp), comp.hot_reload));
    }
    let gone: Vec<u64> = rt.runners.keys().filter(|b| !settings.contains_key(b)).copied().collect();
    for bits in gone {
        if let Some(mut r) = rt.runners.remove(&bits) {
            r.group.stop_all();
        }
    }
    for (id, comp) in &found {
        let (path, _) = &settings[&id.to_bits()];
        if path.is_empty() {
            continue;
        }
        match rt.runners.get(&id.to_bits()) {
            None if comp.auto_start => start_runner(&mut rt, world, *id, path.clone()),
            // Другой файл в инспекторе — перезапуск.
            Some(r) if r.path != *path => start_runner(&mut rt, world, *id, path.clone()),
            _ => {}
        }
    }
    for (id, run) in std::mem::take(&mut rt.requests) {
        let bits = id.to_bits();
        match (run, settings.get(&bits)) {
            (true, Some((path, _))) if !path.is_empty() => start_runner(&mut rt, world, id, path.clone()),
            (false, _) => {
                if let Some(r) = rt.runners.get_mut(&bits) {
                    r.group.stop_all();
                    r.running = false;
                }
            }
            _ => {}
        }
    }

    // 2. Горячая перезагрузка и проверка файлов.
    rt.clock += world.time.unscaled_delta as f64;
    if rt.clock >= WATCH_INTERVAL {
        rt.clock = 0.0;
        hot_reload(&mut rt, world, &settings);
        check_changed_files(&mut rt, world);
    }

    // 3. Сообщения.
    if let Some(inbox) = world.resources.get_mut::<Inbox>() {
        for (id, name, value) in std::mem::take(&mut inbox.0) {
            if let Some(r) = rt.runners.get_mut(&id.to_bits()) {
                r.group.post(&name, to_script(&value));
            }
        }
    }

    // 4. Кадр сценариев (объекты, выключенные в иерархии, стоят на паузе).
    let dt = world.time.delta as f64;
    let ScenarioRuntime { vm, runners, .. } = &mut rt;
    let mut errors = Vec::new();
    for (bits, r) in runners.iter_mut() {
        if !r.running || !world.scene.is_active(GameObjectId::from_bits(*bits)) {
            continue;
        }
        let report = r.group.update(vm, world, dt, &mut |host, token| {
            host.downcast_mut::<World>().map_or(Some(ScriptValue::Nil), |w| api::poll(w, token))
        });
        for w in report.warnings {
            log::warn!(target: "scenario", "{w}");
        }
        errors.extend(report.errors);
    }
    for e in errors {
        rt.report(e);
    }
    world.resources.insert(rt);
}

fn idle(world: &mut World) {
    let Some(mut rt) = world.resources.remove::<ScenarioRuntime>() else { return };
    rt.idle_ticks += 1;
    if rt.idle_ticks.is_multiple_of(20) {
        check_changed_files(&mut rt, world);
        // Объекты исчезли (Stop в редакторе восстановил сцену) — их сценарии больше не нужны.
        let gone: Vec<u64> =
            rt.runners.keys().filter(|b| !world.scene.contains(GameObjectId::from_bits(**b))).copied().collect();
        for bits in gone {
            if let Some(mut r) = rt.runners.remove(&bits) {
                r.group.stop_all();
            }
        }
    }
    world.resources.insert(rt);
}

struct ScenarioSystem;

impl Subsystem for ScenarioSystem {
    fn name(&self) -> &'static str {
        "Scenario"
    }

    fn post_update(&mut self, world: &mut World) {
        update(world);
    }

    fn idle(&mut self, world: &mut World) {
        idle(world);
    }
}

/// Подключить сценарии к приложению (вызывается из [`crate::app`]).
pub fn install(app: &mut App) {
    app.world.registry.register::<ScenarioRunner>();
    app.world.registry.categorize(&["ScenarioRunner"], "Scripts (Scenario)");
    runtime(&mut app.world);
    app.add_subsystem(ScenarioSystem);
}
