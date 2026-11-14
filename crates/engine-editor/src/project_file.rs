//! File → Export Project File…: сохраняет исходники открытого проекта в один переносимый
//! файл (`engine-project`, формат — `docs/api/project-file.md`). То же делает «Save Project As
//! File…» в Piska Hub и `piska-export project-pack`: код один — крейт `engine-project`.
//!
//! Запись идёт в отдельном потоке: окно показывает число файлов и размер, есть «Cancel»,
//! редактор остаётся рабочим. Расширение файла и название инструмента берутся из
//! `engine_i18n::brand`. Файл пишется во временный файл рядом и переименовывается — отмена и
//! ошибка не оставляют половину файла.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::vec2;
use engine_i18n::brand;
use engine_project::{CollectOptions, Control, PackOptions, Phase, Progress};

use crate::i18n::{tr_s, trf};
use crate::icons::Icon;
use crate::state::Editor;
use crate::theme;

/// Что показывает окно.
#[derive(Clone, Default)]
enum Run {
    #[default]
    Idle,
    Working {
        progress: Option<Progress>,
        control: Control,
    },
    Done {
        out: PathBuf,
        files: u64,
        bytes: u64,
        archive: u64,
    },
    Failed(String),
}

/// Состояние окна: общее для окна и потока записи.
#[derive(Default)]
struct State {
    open: bool,
    project: PathBuf,
    title: String,
    out: String,
    tests: bool,
    cpp: bool,
    builds: bool,
    run: Run,
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<State>>);

fn shared(ctx: &egui::Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("project_file_window"), Shared::default).clone())
}

fn human(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// Записать файл проекта в потоке; результат кладётся в `state.run`.
fn start(shared: &Shared, ctx: &egui::Context) {
    let (project, out, options, control) = {
        let mut state = shared.0.lock().expect("project file state");
        let mut out = PathBuf::from(state.out.trim());
        if out.extension().is_none() {
            out.set_extension(brand::PROJECT_EXTENSION);
        }
        let control = Control::new();
        state.run = Run::Working {
            progress: None,
            control: control.clone(),
        };
        let options = PackOptions {
            collect: CollectOptions {
                tests: state.tests,
                cpp: state.cpp,
                builds: state.builds,
                ..CollectOptions::default()
            },
            tool: brand::EDITOR_NAME.to_lowercase().replace(' ', "-"),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
            ..PackOptions::default()
        };
        (state.project.clone(), out, options, control)
    };
    let (shared, ctx) = (shared.clone(), ctx.clone());
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let result = engine_project::pack(&project, &out, &options, &control, &mut |p: &Progress| {
            if last.elapsed() < std::time::Duration::from_millis(60) {
                return;
            }
            last = std::time::Instant::now();
            if let Ok(mut state) = shared.0.lock() {
                if let Run::Working { progress, .. } = &mut state.run {
                    *progress = Some(p.clone());
                }
            }
            ctx.request_repaint();
        });
        if let Ok(mut state) = shared.0.lock() {
            state.run = match result {
                Ok(summary) => Run::Done {
                    out,
                    files: summary.files,
                    bytes: summary.bytes,
                    archive: summary.archive_size,
                },
                Err(engine_project::Error::Cancelled) => Run::Idle,
                Err(err) => Run::Failed(err.to_string()),
            };
        }
        ctx.request_repaint();
    });
}

impl Editor {
    /// Открыть окно Export Project File для проекта `project` (то же, что пункт меню File).
    pub fn show_project_file_export(&self, ctx: &egui::Context, project: std::path::PathBuf) {
        open_for(ctx, project);
    }

    /// File → Export Project File…: открыть окно для проекта, чьи ассеты подключены.
    pub(crate) fn project_file_menu_item(&mut self, ui: &mut egui::Ui) {
        let project = self.assets_root().map(|assets| crate::instance::project_dir_of_assets(&assets));
        let has_manifest = project.as_ref().is_some_and(|dir| dir.join("Cargo.toml").is_file());
        if theme::item(tr_s("Export Project File…"))
            .icon(Icon::Save)
            .enabled_if(has_manifest, tr_s("The project folder with Cargo.toml was not found next to the assets"))
            .show(ui)
        {
            if let Some(project) = project {
                open_for(ui.ctx(), project);
            }
        }
    }

    /// Окно (вызывается каждый кадр из `Editor::ui`).
    pub(crate) fn project_file_window(&mut self, ctx: &egui::Context) {
        let shared = shared(ctx);
        let (mut open, running) = {
            let state = shared.0.lock().expect("project file state");
            (state.open, matches!(state.run, Run::Working { .. }))
        };
        if !open {
            return;
        }
        let mut start_now = false;
        let mut cancel = false;
        let mut reveal: Option<PathBuf> = None;
        // Заголовок (с именем проекта) рисует `dialog_title` внутри.
        crate::window_kit::tool_window(egui::Id::new("project_file_window_frame"), "")
            .resizable(false)
            .default_size(vec2(480.0, 300.0))
            .show(ctx, &mut open, |ui| {
                let mut state = shared.0.lock().expect("project file state");
                ui.set_width(460.0);
                theme::dialog_title(ui, Icon::Save, theme::accent(), &trf!("Save “{}” as one file", state.title));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(trf!(
                            "Everything needed to rebuild the project in one portable .{} file: Cargo.toml, src, assets with .meta, settings. No target/, .git/ or caches. The same file opens in the hub (Import).",
                            brand::PROJECT_EXTENSION
                        ))
                        .color(theme::TEXT_MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(theme::SP_M);
                theme::section_label(ui, tr_s("Save to"));
                ui.add_enabled_ui(!running, |ui| crate::controls::text_field(ui, &mut state.out, "", f32::INFINITY));
                ui.add_space(theme::SP_S);
                ui.add_enabled_ui(!running, |ui| {
                    ui.checkbox(&mut state.tests, tr_s("Tests (tests/)"));
                    ui.checkbox(&mut state.cpp, tr_s("C++ sources (cpp/)"));
                    ui.checkbox(&mut state.builds, tr_s("The last exported builds (dist/)"));
                });
                ui.label(
                    egui::RichText::new(tr_s("Always left out: target/, .git/, caches, plugin build folders, editor temp files and links. .gitignore is respected."))
                        .font(theme::caption())
                        .color(theme::TEXT_DISABLED),
                );
                ui.add_space(theme::SP_M);
                match state.run.clone() {
                    Run::Idle => {}
                    Run::Working { progress, .. } => {
                        let (fraction, text) = match &progress {
                            Some(p) if p.phase == Phase::Working && p.bytes_total > 0 => (
                                Some((p.bytes_done as f64 / p.bytes_total as f64) as f32),
                                trf!(
                                    "{done} / {total} files  ·  {bytes} of {all}",
                                    done = p.files_done,
                                    total = p.files_total,
                                    bytes = human(p.bytes_done),
                                    all = human(p.bytes_total)
                                ),
                            ),
                            _ => (None, tr_s("Looking at the files…").to_string()),
                        };
                        theme::progress_bar(ui, fraction, &text);
                        if let Some(p) = &progress {
                            ui.label(egui::RichText::new(&p.current).font(theme::caption()).color(theme::TEXT_DISABLED));
                        }
                    }
                    Run::Done { out, files, bytes, archive } => {
                        ui.label(
                            egui::RichText::new(trf!(
                                "Saved {name}: {files} files, {bytes} packed into {archive}",
                                name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                                files = files,
                                bytes = human(bytes),
                                archive = human(archive)
                            ))
                            .color(theme::OK),
                        );
                        if theme::button(ui, tr_s("Show in Folder")).clicked() {
                            reveal = Some(out);
                        }
                    }
                    Run::Failed(error) => {
                        ui.add(egui::Label::new(egui::RichText::new(error).color(theme::ERROR)).wrap());
                    }
                }
                crate::window_kit::dialog_actions(ui, |ui| {
                    if running {
                        cancel = theme::secondary_button(ui, tr_s("Cancel")).clicked();
                    } else {
                        let ready = !state.out.trim().is_empty();
                        let export = ui.add_enabled_ui(ready, |ui| theme::primary_button(ui, tr_s("Export"))).inner;
                        start_now = export.clicked();
                    }
                });
            });
        let mut state = shared.0.lock().expect("project file state");
        if cancel {
            if let Run::Working { control, .. } = &state.run {
                control.cancel();
            }
        }
        if !open && !running {
            state.open = false;
        } else if !open && running {
            // Закрыли окно, пока идёт запись: останавливаем её.
            if let Run::Working { control, .. } = &state.run {
                control.cancel();
            }
            state.open = false;
        }
        drop(state);
        if start_now {
            start(&shared, ctx);
        }
        if let Some(out) = reveal {
            crate::state::reveal(&out);
        }
    }
}

/// Открыть окно для проекта `project`: путь по умолчанию — рядом с проектом.
fn open_for(ctx: &egui::Context, project: PathBuf) {
    let shared = shared(ctx);
    let mut state = shared.0.lock().expect("project file state");
    if !matches!(state.run, Run::Working { .. }) {
        state.run = Run::Idle;
    }
    if state.project != project || state.out.is_empty() {
        let name = project_name(&project);
        state.title = name.clone();
        let parent = project.parent().map(Path::to_path_buf).unwrap_or_default();
        state.out = parent.join(brand::project_file_name(&name)).display().to_string();
        state.tests = true;
        state.cpp = true;
        state.builds = false;
        state.project = project;
    }
    state.open = true;
}

/// Имя проекта: `[package] name` из `Cargo.toml`, иначе имя каталога.
fn project_name(project: &Path) -> String {
    std::fs::read_to_string(project.join("Cargo.toml"))
        .ok()
        .and_then(|text| {
            let line = text.lines().find(|l| l.trim_start().starts_with("name") && l.contains('"'))?;
            line.split('"').nth(1).map(str::to_string)
        })
        .or_else(|| project.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "project".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska-editor-project-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_window_opens_for_a_project_and_exports_it_in_the_background() {
        let root = temp("export");
        let project = root.join("my_game");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("target")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"my_game\"\n").unwrap();
        std::fs::write(project.join("src/lib.rs"), "pub fn build() {}\n").unwrap();
        std::fs::write(project.join("target/junk"), "x").unwrap();
        let ctx = egui::Context::default();
        open_for(&ctx, project.clone());
        let shared = shared(&ctx);
        {
            let state = shared.0.lock().unwrap();
            assert!(state.open && state.tests && state.cpp && !state.builds);
            assert!(state.out.ends_with(&format!("my_game.{}", brand::PROJECT_EXTENSION)), "{}", state.out);
            assert_eq!(state.title, "my_game");
        }
        shared.0.lock().unwrap().out = root.join("saved").display().to_string();
        std::fs::create_dir_all(root.join("saved").parent().unwrap()).unwrap();
        start(&shared, &ctx);
        let started = std::time::Instant::now();
        loop {
            if !matches!(shared.0.lock().unwrap().run, Run::Working { .. }) || started.elapsed().as_secs() > 30 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let state = shared.0.lock().unwrap();
        let Run::Done { out, files, .. } = &state.run else {
            panic!("the export finishes");
        };
        assert_eq!(out.extension().and_then(|e| e.to_str()), Some(brand::PROJECT_EXTENSION), "the extension is added");
        assert_eq!(*files, 2);
        let info = engine_project::read_info(out).unwrap();
        assert_eq!(info.manifest.project.name, "my_game");
        assert!(!info.manifest.files.iter().any(|f| f.path.starts_with("target/")));
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failure_is_shown_in_the_window() {
        let root = temp("fail");
        let ctx = egui::Context::default();
        open_for(&ctx, root.join("not_a_project"));
        let shared = shared(&ctx);
        start(&shared, &ctx);
        let started = std::time::Instant::now();
        while matches!(shared.0.lock().unwrap().run, Run::Working { .. }) && started.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(&shared.0.lock().unwrap().run, Run::Failed(text) if text.contains("Cargo.toml")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
