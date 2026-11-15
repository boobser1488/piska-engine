//! Запись обзора мышью в виде Scene в файл — для разбора жалоб «камера трясётся».
//!
//! Включается переменной окружения `PISKA_TRACE_LOOK=<файл>` (или файлом-флажком
//! `piska-trace-look.on` во временной папке системы): пока зажата правая кнопка,
//! каждый кадр в файл уходит строка CSV. По ней видно, что пришло от мыши (сырое движение
//! и сдвиг курсора), сколько длился кадр и куда в итоге повернулась камера — то есть где
//! именно рождается неровность: в мыши, в темпе кадров или в самой камере.

use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

fn sink() -> Option<&'static Mutex<File>> {
    static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        // Без переменной окружения — по файлу-флажку во временной папке (редактор обычно
        // запускает хаб, и передать ему переменную неудобно): есть `piska-trace-look.on` —
        // пишем рядом в `piska-trace-look.csv`.
        let path = std::env::var_os("PISKA_TRACE_LOOK").map(std::path::PathBuf::from).or_else(|| {
            let dir = std::env::temp_dir();
            dir.join("piska-trace-look.on").is_file().then(|| dir.join("piska-trace-look.csv"))
        })?;
        let mut file = File::create(path).ok()?;
        writeln!(file, "time_s,raw_dt_ms,camera_dt_ms,pointer_events,cursor_dx,cursor_dy,raw_dx,raw_dy,used_dx,used_dy,yaw_deg,pitch_deg").ok()?;
        Some(Mutex::new(file))
    })
    .as_ref()
}

/// Запись включена.
pub(crate) fn enabled() -> bool {
    sink().is_some()
}

/// Один кадр обзора.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record(time: f64, raw_dt: f32, camera_dt: f32, pointer_events: usize, cursor: egui::Vec2, raw: egui::Vec2, used: egui::Vec2, yaw: f32, pitch: f32) {
    let Some(sink) = sink() else {
        return;
    };
    if let Ok(mut file) = sink.lock() {
        let _ = writeln!(
            file,
            "{time:.4},{:.3},{:.3},{pointer_events},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.4},{:.4}",
            raw_dt * 1000.0,
            camera_dt * 1000.0,
            cursor.x,
            cursor.y,
            raw.x,
            raw.y,
            used.x,
            used.y,
            yaw.to_degrees(),
            pitch.to_degrees()
        );
    }
}
