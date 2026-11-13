//! Группа задач одного экземпляра скрипта: главная задача, задачи из `start(fn)`,
//! обработчики `on(name, fn)`, ожидания по времени, кадрам и сообщениям.
//!
//! Хост (движок) вызывает [`Group::update`] раз в кадр с шагом времени. Ожидания,
//! которые понимает только хост (конец твина, клик по кнопке), передаются как
//! [`Suspend::Host`] и проверяются функцией `poll`.

use std::any::Any;
use std::rc::Rc;

use crate::compiler::Script;
use crate::error::ScriptError;
use crate::task::Task;
use crate::value::{Module, Value};
use crate::vm::{SchedCtx, Status, Suspend, Vm};

enum Wait {
    Ready(Value),
    Seconds(f64),
    Frames(u32),
    Message(Rc<str>),
    Task(u64),
    Host(Value),
    /// Вытеснена по бюджету инструкций — продолжить в следующем кадре.
    Busy,
}

struct Entry {
    id: u64,
    task: Task,
    wait: Wait,
    busy_frames: u32,
    dead: bool,
}

/// Что произошло за кадр.
#[derive(Default)]
pub struct Report {
    /// Ошибки задач (задача с ошибкой остановлена, остальные продолжают работать).
    pub errors: Vec<ScriptError>,
    /// Предупреждения (долгая работа без ожидания и т. п.).
    pub warnings: Vec<String>,
}

/// Группа задач одного экземпляра скрипта.
pub struct Group {
    sched: SchedCtx,
    tasks: Vec<Entry>,
    inbox: Vec<(Rc<str>, Value)>,
    module: Option<Rc<Module>>,
    main_id: u64,
    pub(crate) main_result: Option<Value>,
    warned: bool,
    name: Rc<str>,
}

impl Group {
    /// `user` — данные хоста, доступные встроенным функциям через `ctx.user()`.
    pub fn new(user: u64) -> Self {
        let sched = SchedCtx { user, ..Default::default() };
        Group { sched, tasks: Vec::new(), inbox: Vec::new(), module: None, main_id: 0, main_result: None, warned: false, name: "".into() }
    }

    /// Связывает скрипт и ставит его главную задачу в очередь (она начнёт работу в
    /// ближайшем `update`). Ошибка связывания (неизвестное имя) возвращается сразу.
    pub fn start(&mut self, vm: &Vm, script: &Rc<Script>) -> Result<(), ScriptError> {
        let inst = vm.instantiate(script)?;
        self.name = script.name.clone();
        self.module = Some(inst.module);
        self.main_result = None;
        self.main_id = self.spawn(inst.main, Vec::new()).unwrap_or(0);
        Ok(())
    }

    /// Запускает функцию скрипта отдельной задачей; возвращает id задачи.
    pub fn spawn(&mut self, func: Value, args: Vec<Value>) -> Result<u64, String> {
        let task = Task::new(&func, args)?;
        self.sched.next_id += 1;
        let id = self.sched.next_id;
        self.sched.live.insert(id);
        self.tasks.push(Entry { id, task, wait: Wait::Ready(Value::Nil), busy_frames: 0, dead: false });
        Ok(id)
    }

    /// Сообщение группе: будит `wait_message(name)` и запускает обработчики `on(name, …)`
    /// в ближайшем `update`.
    pub fn post(&mut self, name: &str, value: Value) {
        if self.inbox.len() < 10_000 {
            self.inbox.push((name.into(), value));
        }
    }

    /// Глобальные переменные экземпляра скрипта.
    pub fn module(&self) -> Option<&Rc<Module>> {
        self.module.as_ref()
    }

    /// Нет ни задач, ни обработчиков сообщений.
    pub fn is_idle(&self) -> bool {
        self.tasks.iter().all(|t| t.dead) && self.sched.handlers.is_empty() && self.inbox.is_empty()
    }

    /// Работающих задач.
    pub fn task_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.dead).count()
    }

    /// Обработчики сообщения `name` подписаны?
    pub fn listens(&self, name: &str) -> bool {
        self.sched.handlers.iter().any(|(n, _)| &**n == name)
    }

    /// Время группы: сумма шагов `update`.
    pub fn time(&self) -> f64 {
        self.sched.time
    }

    /// Результат главной задачи (значение `return` верхнего уровня), если она завершилась.
    pub fn main_result(&self) -> Option<&Value> {
        self.main_result.as_ref()
    }

    /// Останавливает всё и освобождает экземпляр.
    pub fn stop_all(&mut self) {
        self.tasks.clear();
        self.inbox.clear();
        self.sched.handlers.clear();
        self.sched.live.clear();
        if let Some(m) = self.module.take() {
            m.clear();
        }
    }

    /// Один кадр: доставка сообщений, продвижение ожиданий, возобновление готовых задач.
    pub fn update(
        &mut self,
        vm: &mut Vm,
        host: &mut dyn Any,
        dt: f64,
        poll: &mut dyn FnMut(&mut dyn Any, &Value) -> Option<Value>,
    ) -> Report {
        let mut report = Report::default();
        self.sched.dt = dt;
        self.sched.time += dt;
        self.sched.frame += 1;

        // 1. Сообщения.
        let inbox = std::mem::take(&mut self.inbox);
        for (name, value) in inbox {
            for e in &mut self.tasks {
                if let Wait::Message(n) = &e.wait {
                    if *n == name {
                        e.wait = Wait::Ready(value.clone());
                    }
                }
            }
            let handlers: Vec<Value> =
                self.sched.handlers.iter().filter(|(n, _)| *n == name).map(|(_, h)| h.clone()).collect();
            for h in handlers {
                if let Err(e) = self.spawn(h, vec![value.clone()]) {
                    report.warnings.push(format!("{}: handler for `{name}`: {e}", self.name));
                }
            }
        }

        // 2. Ожидания.
        for e in &mut self.tasks {
            if e.dead {
                continue;
            }
            let ready = match &mut e.wait {
                Wait::Ready(_) => None,
                Wait::Seconds(t) => {
                    *t -= dt;
                    (*t <= 1e-9).then_some(Value::Nil)
                }
                Wait::Frames(n) => {
                    *n = n.saturating_sub(1);
                    (*n == 0).then_some(Value::Nil)
                }
                Wait::Message(_) => None,
                Wait::Task(id) => (!self.sched.live.contains(id)).then_some(Value::Nil),
                Wait::Host(v) => poll(host, v),
                Wait::Busy => Some(Value::Nil),
            };
            if let Some(v) = ready {
                e.wait = Wait::Ready(v);
            }
        }

        // 3. Готовые задачи (и запущенные по ходу дела — в этом же кадре).
        let mut i = 0;
        while i < self.tasks.len() {
            if self.tasks[i].dead || !matches!(self.tasks[i].wait, Wait::Ready(_)) {
                i += 1;
                continue;
            }
            let was_busy = self.tasks[i].busy_frames > 0;
            let input = match std::mem::replace(&mut self.tasks[i].wait, Wait::Busy) {
                Wait::Ready(v) => v,
                _ => Value::Nil,
            };
            let id = self.tasks[i].id;
            self.sched.current = id;
            let result = self.tasks[i].task.resume(vm, host, &mut self.sched, input);
            let entry = &mut self.tasks[i];
            match result {
                Ok(Status::Done(v)) => {
                    entry.dead = true;
                    if id == self.main_id {
                        self.main_result = Some(v);
                    }
                }
                Ok(Status::Suspended(s)) => {
                    entry.busy_frames = 0;
                    entry.wait = match s {
                        Suspend::Seconds(t) => Wait::Seconds(t),
                        Suspend::Frames(n) => Wait::Frames(n.max(1)),
                        Suspend::Message(n) => Wait::Message(n),
                        Suspend::Task(t) => Wait::Task(t),
                        Suspend::Host(v) => Wait::Host(v),
                    };
                }
                Ok(Status::Preempted) => {
                    entry.wait = Wait::Busy;
                    entry.busy_frames += 1;
                    if !was_busy && !self.warned {
                        self.warned = true;
                        report.warnings.push(format!(
                            "{}: a task ran {} instructions without waiting; it continues next frame (add `wait()` or `wait_frame()` inside long loops)",
                            self.name, vm.limits.instructions_per_resume
                        ));
                    }
                    if entry.busy_frames > vm.limits.max_busy_frames {
                        entry.dead = true;
                        report.errors.push(entry.task.limit_error(&format!(
                            "stopped: this task ran for {} frames without waiting (an infinite loop?)",
                            entry.busy_frames
                        )));
                    }
                }
                Err(e) => {
                    entry.dead = true;
                    report.errors.push(e);
                }
            }
            if self.tasks[i].dead {
                self.sched.live.remove(&id);
            }
            self.apply_effects();
            i += 1;
        }
        self.tasks.retain(|t| !t.dead);
        self.sched.live = self.tasks.iter().map(|t| t.id).collect();
        // Сообщения себе (`emit`) — в следующем кадре.
        let posts = std::mem::take(&mut self.sched.posts);
        self.inbox.extend(posts);
        report
    }

    fn apply_effects(&mut self) {
        for (id, func, args) in std::mem::take(&mut self.sched.spawned) {
            if let Ok(task) = Task::new(&func, args) {
                self.tasks.push(Entry { id, task, wait: Wait::Ready(Value::Nil), busy_frames: 0, dead: false });
            }
        }
        for id in std::mem::take(&mut self.sched.stopped) {
            for e in &mut self.tasks {
                if e.id == id {
                    e.dead = true;
                }
            }
            self.sched.live.remove(&id);
        }
        if std::mem::take(&mut self.sched.stop_others) {
            let current = self.sched.current;
            for e in &mut self.tasks {
                if e.id != current {
                    e.dead = true;
                    self.sched.live.remove(&e.id);
                }
            }
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(m) = self.module.take() {
            // Разрывает циклы «функция ↔ глобальные» — иначе экземпляр не освободится.
            m.clear();
        }
    }
}
