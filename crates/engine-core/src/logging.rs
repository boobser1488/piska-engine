//! Лог движка: stderr + кольцевой буфер в памяти.
//!
//! Буфер читает окно Console редактора ([`records`]); в обычной игре он просто
//! хранит последние [`CAPACITY`] записей и ничего не стоит.

use std::collections::VecDeque;
use std::sync::Mutex;

pub use log::Level;

/// Сколько последних записей хранится в памяти.
pub const CAPACITY: usize = 2000;

/// Одна запись лога.
#[derive(Clone, Debug, PartialEq)]
pub struct LogRecord {
    /// Порядковый номер с запуска — по нему консоль понимает, что уже показано.
    pub seq: u64,
    pub level: Level,
    /// Откуда сообщение: путь модуля (`engine_physics`, `platformer::scripts`) или `plugin`.
    pub target: String,
    pub message: String,
}

struct Buffer {
    records: VecDeque<LogRecord>,
    next_seq: u64,
}

static BUFFER: Mutex<Buffer> = Mutex::new(Buffer {
    records: VecDeque::new(),
    next_seq: 0,
});

fn buffer() -> std::sync::MutexGuard<'static, Buffer> {
    // Паника в чужом потоке во время записи лога не должна ломать сам лог.
    BUFFER.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Записи с номером `seq >= since`, от старых к новым.
pub fn records_since(since: u64) -> Vec<LogRecord> {
    buffer()
        .records
        .iter()
        .filter(|record| record.seq >= since)
        .cloned()
        .collect()
}

/// Все записи, которые ещё помнит буфер.
pub fn records() -> Vec<LogRecord> {
    records_since(0)
}

/// Забыть все записи (кнопка Clear в консоли). Нумерация продолжается.
pub fn clear() {
    buffer().records.clear();
}

struct EngineLogger;

fn is_third_party(target: &str) -> bool {
    ["wgpu", "naga", "winit", "egui", "eframe"]
        .iter()
        .any(|prefix| target.starts_with(prefix))
}

impl log::Log for EngineLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Свои сообщения — от Info; сторонние библиотеки (wgpu, naga, winit, egui) — от Warn;
        // слой драйверов wgpu_hal — только ошибки (его предупреждения — про версии
        // драйверов и слои валидации, а не про игру).
        let target = metadata.target();
        if target.starts_with("wgpu_hal") {
            return metadata.level() <= Level::Error;
        }
        if is_third_party(target) {
            return metadata.level() <= Level::Warn;
        }
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            Level::Error => "error",
            Level::Warn => "warn ",
            Level::Info => "info ",
            Level::Debug => "debug",
            Level::Trace => "trace",
        };
        let message = record.args().to_string();
        eprintln!("[{level}] {}: {message}", record.target());

        let mut buffer = buffer();
        let seq = buffer.next_seq;
        buffer.next_seq += 1;
        if buffer.records.len() >= CAPACITY {
            buffer.records.pop_front();
        }
        buffer.records.push_back(LogRecord {
            seq,
            level: record.level(),
            target: record.target().to_string(),
            message,
        });
    }

    fn flush(&self) {}
}

static LOGGER: EngineLogger = EngineLogger;

/// Включить лог движка. Повторные вызовы безвредны; если приложение уже поставило
/// свой логгер, он остаётся (и консоль редактора тогда пуста).
pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
