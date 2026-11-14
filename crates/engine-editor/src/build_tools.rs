//! Tools → Check Build Tools…: окно с результатом `piska-export doctor --json` (схема —
//! `docs/install.md`): что найдено, каких версий, чего не хватает, и кнопка «Copy fix
//! command» у каждой проблемы. Окно только читает: ничего не устанавливает и не запускает
//! установщики. Если инструмент `piska-export` не собран, окно объясняет, как его собрать.
//!
//! `doctor` запускает `--version` у найденных программ — это может занять секунды, поэтому
//! он идёт в отдельном потоке, а окно показывает «Checking…».

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{vec2, Color32};
use serde::Deserialize;

use crate::i18n::{tr_s, trf};
use crate::icons::Icon;
use crate::state::Editor;
use crate::theme;

/// Одна проверка из отчёта `doctor --json`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ToolCheck {
    pub component: String,
    pub id: String,
    pub label: String,
    /// `ok`, `old`, `missing` или `info`.
    pub status: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    /// Команда установщика, которая исправит проблему (`None` — исправлять нечего).
    #[serde(default)]
    pub fix: Option<String>,
}

impl ToolCheck {
    /// Проверка не прошла: инструмента нет или он слишком старый.
    pub fn failed(&self) -> bool {
        self.status == "old" || self.status == "missing"
    }
}

/// Отчёт `piska-export doctor --json`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct DoctorReport {
    pub os: String,
    pub ok: bool,
    #[serde(default)]
    pub failed: u32,
    pub checks: Vec<ToolCheck>,
}

impl DoctorReport {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("`piska-export doctor --json` printed something unexpected: {e}"))
    }
}

/// Состояние окна.
#[derive(Clone, Debug)]
enum Doctor {
    /// `piska-export` не найден рядом с редактором.
    NotBuilt,
    Running,
    Done(DoctorReport),
    Failed(String),
}

/// Окно открыто и что в нём (общие для потока проверки и окна).
#[derive(Clone)]
struct Shared(Arc<Mutex<Option<Doctor>>>);

fn shared(ctx: &egui::Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("build_tools_doctor"), || Shared(Arc::new(Mutex::new(None)))).clone())
}

/// Программа `piska-export`: `PISKA_EXPORT`, иначе рядом с программой редактора (или
/// уровнем выше), иначе в `CARGO_TARGET_DIR/{debug,release}`.
pub(crate) fn export_executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PISKA_EXPORT").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let name = format!("piska-export{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().ok()?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    exe.ancestors()
        .skip(1)
        .take(3)
        .map(|dir| dir.join(&name))
        .chain(target.into_iter().flat_map(|t| [t.join("debug").join(&name), t.join("release").join(&name)]))
        .find(|candidate| candidate.is_file())
}

fn run_doctor(exe: PathBuf) -> Doctor {
    let output = engine_build::host::command(&exe).args(["doctor", "--json"]).stdin(std::process::Stdio::null()).output();
    match output {
        // Код выхода 1 — «есть проблемы»: отчёт всё равно напечатан.
        Ok(output) if !output.stdout.is_empty() => match DoctorReport::parse(&String::from_utf8_lossy(&output.stdout)) {
            Ok(report) => Doctor::Done(report),
            Err(err) => Doctor::Failed(err),
        },
        Ok(output) => Doctor::Failed(trf!(
            "`{exe}` printed nothing: {err}",
            exe = exe.display(),
            err = String::from_utf8_lossy(&output.stderr).trim().to_string()
        )),
        Err(err) => Doctor::Failed(trf!("cannot start `{exe}`: {err}", exe = exe.display(), err = err)),
    }
}

impl Editor {
    /// Tools → Check Build Tools…: открыть окно и запустить `piska-export doctor --json`.
    pub fn check_build_tools(&mut self, ctx: &egui::Context) {
        let state = shared(ctx);
        let Some(exe) = export_executable() else {
            *state.0.lock().expect("doctor state") = Some(Doctor::NotBuilt);
            return;
        };
        *state.0.lock().expect("doctor state") = Some(Doctor::Running);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = run_doctor(exe);
            *state.0.lock().expect("doctor state") = Some(result);
            ctx.request_repaint();
        });
    }

    /// Отчёт последней проверки (`None` — окно закрыто, проверка идёт или не удалась).
    pub fn build_tools_report(&self, ctx: &egui::Context) -> Option<DoctorReport> {
        match shared(ctx).0.lock().expect("doctor state").clone() {
            Some(Doctor::Done(report)) => Some(report),
            _ => None,
        }
    }

    /// Окно показывает «piska-export не собран» (для тестов).
    pub fn build_tools_not_built(&self, ctx: &egui::Context) -> bool {
        matches!(*shared(ctx).0.lock().expect("doctor state"), Some(Doctor::NotBuilt))
    }

    /// Идентификатор кнопки «Copy fix command» строки проверки `id`.
    pub fn build_tools_copy_fix_id(id: &str) -> egui::Id {
        egui::Id::new(("build_tools_copy_fix", id.to_string()))
    }

    /// Пункт меню Tools.
    pub(crate) fn build_tools_menu_item(&mut self, ui: &mut egui::Ui) {
        theme::menu_section(ui, tr_s("Build"));
        if theme::item(tr_s("Check Build Tools…")).icon(Icon::Build).show(ui) {
            self.check_build_tools(ui.ctx());
        }
    }

    /// Окно (вызывается каждый кадр из `Editor::ui`).
    pub(crate) fn build_tools_window(&mut self, ctx: &egui::Context) {
        let state = shared(ctx);
        let Some(current) = state.0.lock().expect("doctor state").clone() else {
            return;
        };
        let mut open = true;
        let mut again = false;
        crate::window_kit::tool_window(egui::Id::new("build_tools_window"), tr_s("Build Tools"))
            .default_size(vec2(760.0, 420.0))
            .show(ctx, &mut open, |ui| match &current {
                Doctor::NotBuilt => not_built(ui),
                Doctor::Running => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(tr_s("Checking the build tools…")).color(theme::TEXT_MUTED));
                    });
                }
                Doctor::Failed(error) => {
                    ui.label(egui::RichText::new(error).color(theme::ERROR));
                    ui.add_space(theme::SP_M);
                    again = theme::button(ui, tr_s("Check again")).clicked();
                }
                Doctor::Done(report) => again = report_table(ui, report),
            });
        if !open {
            *state.0.lock().expect("doctor state") = None;
        } else if again {
            self.check_build_tools(ctx);
        }
    }
}

/// «piska-export не собран»: как собрать (как окно «Piska Modeler is not installed here»).
fn not_built(ui: &mut egui::Ui) {
    ui.set_max_width(520.0);
    theme::dialog_title(ui, Icon::Build, theme::TEXT_MUTED, tr_s("piska-export is not built here"));
    ui.label(
        egui::RichText::new(tr_s(
            "The build tools check runs `piska-export doctor`, a separate program of the engine workspace. Build it once from the engine folder:",
        ))
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(theme::SP_S);
    ui.label(egui::RichText::new("cargo build -p piska-export").font(theme::mono()).color(theme::TEXT));
    ui.add_space(theme::SP_S);
    ui.label(
        egui::RichText::new(tr_s("or point the PISKA_EXPORT environment variable at the program, then open this window again."))
            .color(theme::TEXT_MUTED),
    );
}

fn status_color(status: &str) -> Color32 {
    match status {
        "ok" => theme::OK,
        "old" => theme::WARN,
        "missing" => theme::ERROR,
        _ => theme::TEXT_MUTED,
    }
}

/// Таблица отчёта. Возвращает «проверить снова».
fn report_table(ui: &mut egui::Ui, report: &DoctorReport) -> bool {
    let summary = if report.ok {
        tr_s("Everything needed to build and export games is installed.").to_string()
    } else {
        trf!("{n} problem(s) found. Run the fix command of each row in a terminal (the editor never installs anything itself).", n = report.failed)
    };
    ui.label(egui::RichText::new(summary).color(if report.ok { theme::OK } else { theme::WARN }));
    ui.add_space(theme::SP_S);
    egui::ScrollArea::vertical().id_salt("build_tools_rows").max_height(ui.available_height() - 40.0).show(ui, |ui| {
        egui::Grid::new("build_tools_grid").striped(true).num_columns(5).spacing(vec2(theme::SP_M, theme::SP_S)).show(ui, |ui| {
            for title in [tr_s("Component"), tr_s("Tool"), tr_s("Status"), tr_s("Version"), ""] {
                ui.label(egui::RichText::new(title).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            }
            ui.end_row();
            for check in &report.checks {
                ui.label(egui::RichText::new(&check.component).color(theme::TEXT_MUTED));
                let tool = ui.label(&check.label);
                let place = [check.path.as_deref(), check.detail.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("\n");
                if !place.is_empty() {
                    tool.on_hover_text(place);
                }
                ui.label(egui::RichText::new(&check.status).color(status_color(&check.status)));
                ui.label(check.version.as_deref().unwrap_or("—"));
                match check.fix.as_deref().filter(|_| check.failed()) {
                    Some(fix) => {
                        let button = theme::button(ui, tr_s("Copy fix command"));
                        let button = ui.interact(button.rect, Editor::build_tools_copy_fix_id(&check.id), egui::Sense::click());
                        if button.on_hover_text(fix).clicked() {
                            ui.ctx().copy_text(fix.to_string());
                        }
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    });
    ui.add_space(theme::SP_S);
    let again = theme::button(ui, tr_s("Check again")).clicked();
    ui.label(egui::RichText::new(trf!("System: {os}", os = report.os)).size(theme::FONT_CAPTION).color(theme::TEXT_DISABLED));
    again
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_doctor_json_schema_parses() {
        let json = r#"{ "os": "windows", "ok": false, "failed": 1,
            "checks": [
                { "component": "core", "id": "rustc", "label": "rustc", "status": "ok", "version": "1.97.1", "path": "C:/x/rustc.exe", "detail": null, "fix": null },
                { "component": "android", "id": "ndk", "label": "Android NDK", "status": "missing", "version": null, "path": null, "detail": "not found", "fix": "install/install.ps1 -Components android" }
            ] }"#;
        let report = DoctorReport::parse(json).unwrap();
        assert!(!report.ok);
        assert_eq!(report.checks.len(), 2);
        assert!(!report.checks[0].failed());
        assert!(report.checks[1].failed());
        assert_eq!(report.checks[1].fix.as_deref(), Some("install/install.ps1 -Components android"));
        assert!(DoctorReport::parse("not json").unwrap_err().contains("doctor --json"));
    }
}
