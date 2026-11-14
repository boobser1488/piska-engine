//! Open без лишней сборки, честные состояния ожидания и ошибок, «Prebuild all».
//! Программы — поддельные файлы с заданным временем изменения; хаб в режиме `dry_run`
//! ничего не запускает, а пишет в `dry_log` (`launch <exe>` или команду cargo).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use piska_hub::builds::BuildState;
use piska_hub::config::HubConfig;
use piska_hub::Diagnosis;
use piska_hub::jobs::JobState;
use piska_hub::templates::{self, NewProject, Template};
use piska_hub::{paths, Hub, HubAction, HubOptions};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-builds-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_mtime(path: &Path, time: SystemTime) {
    std::fs::File::options().write(true).open(path).unwrap().set_modified(time).unwrap();
}

/// Все собственные входы проекта — к одному времени.
fn age_inputs(dir: &Path, time: SystemTime) {
    fn walk(dir: &Path, time: SystemTime) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, time);
            } else {
                set_mtime(&path, time);
            }
        }
    }
    set_mtime(&dir.join("Cargo.toml"), time);
    walk(&dir.join("src"), time);
}

struct Setup {
    root: PathBuf,
    config: PathBuf,
    base: SystemTime,
}

impl Setup {
    fn new(name: &str) -> Self {
        Self {
            root: temp(name),
            config: temp(&format!("{name}-config")),
            base: SystemTime::now() - Duration::from_secs(3600),
        }
    }

    /// Проект с входами на `base`; `exe_age` — программа редактора на `base + exe_age` секунд.
    fn project(&self, name: &str, exe_age: Option<i64>) -> (PathBuf, Option<PathBuf>) {
        let engine = paths::find_engine_root().unwrap();
        let plan = templates::plan(
            &NewProject {
                name: name.into(),
                location: self.root.clone(),
                template: Template::Empty2D,
                cpp: false,
            },
            &engine,
        )
        .unwrap();
        templates::create(&plan, &engine).unwrap();
        // Свой код на Rust — проекту нужна своя программа редактора.
        std::fs::write(
            plan.dir.join("src").join("scripts").join("mover.rs"),
            "use engine::prelude::*;\ncomponent! { pub struct Mover {} }\nimpl Component for Mover {}\n",
        )
        .unwrap();
        age_inputs(&plan.dir, self.base);
        let exe = exe_age.map(|age| {
            let exe = self.root.join("bin").join(format!("{name}-editor.exe"));
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, "fake").unwrap();
            let time = if age >= 0 {
                self.base + Duration::from_secs(age as u64)
            } else {
                self.base - Duration::from_secs((-age) as u64)
            };
            set_mtime(&exe, time);
            exe
        });
        let file = self.config.join(paths::CONFIG_FILE);
        let (mut config, _) = HubConfig::load(&file);
        config.entry_mut(&plan.dir).editor_exe = exe.clone();
        config.save(&file).unwrap();
        (paths::clean(&plan.dir), exe)
    }

    fn hub(&self) -> Hub {
        self.hub_with_tools(false)
    }

    /// `tools` — в каталоге программ движка есть (поддельные) piska-editor и piska-player.
    fn hub_with_tools(&self, tools: bool) -> Hub {
        let dir = self.root.join("tools");
        std::fs::create_dir_all(&dir).unwrap();
        if tools {
            for name in ["piska-editor", "piska-player"] {
                std::fs::write(dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)), "fake").unwrap();
            }
        }
        Hub::new(HubOptions {
            config_dir: Some(self.config.clone()),
            dry_run: true,
            tools_dir: Some(dir),
            ..HubOptions::default()
        })
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.config);
    }
}

fn state(hub: &Hub, dir: &Path) -> BuildState {
    hub.projects().iter().find(|l| paths::same(&l.path, dir)).expect("listed").builds.editor
}

#[test]
fn a_fresh_build_opens_at_once_without_cargo() {
    let setup = Setup::new("fresh");
    let (dir, exe) = setup.project("hub_fresh", Some(60));
    let mut hub = setup.hub();
    assert!(matches!(state(&hub, &dir), BuildState::Fresh { .. }));
    hub.apply(HubAction::Open(dir.clone()));
    assert_eq!(hub.dry_log, [format!("launch {}", exe.unwrap().display())], "no cargo when nothing changed");
    assert!(hub.job().is_none());
    assert!(hub.status().unwrap().text.contains("open in the editor"));

    // «Rebuild and Open» собирает всегда.
    hub.apply(HubAction::RebuildAndOpen(dir.clone()));
    assert!(hub.dry_log.last().unwrap().contains("build -p hub_fresh --features editor --bin hub_fresh-editor"));
    assert!(hub.job().is_some_and(|j| j.is_building()));
}

#[test]
fn a_changed_project_builds_first_but_the_last_build_can_still_be_opened() {
    let setup = Setup::new("stale");
    let (dir, exe) = setup.project("hub_stale", Some(60));
    // Правка скрипта после сборки.
    set_mtime(&dir.join("src").join("lib.rs"), setup.base + Duration::from_secs(120));
    let mut hub = setup.hub();
    assert!(matches!(state(&hub, &dir), BuildState::Stale { .. }));
    hub.apply(HubAction::Open(dir.clone()));
    assert!(hub.dry_log[0].contains("build -p hub_stale"), "{:?}", hub.dry_log);
    assert!(hub.job().is_some_and(|j| j.is_building()));

    // Во время сборки — «Open Last Build»: сборка отменяется, запускается прошлая программа.
    hub.apply(HubAction::OpenLastBuild(dir.clone()));
    assert!(hub.job().is_none());
    assert_eq!(hub.dry_log.last().unwrap(), &format!("launch {}", exe.unwrap().display()));
}

#[test]
fn a_project_never_built_builds_and_has_no_last_build() {
    let setup = Setup::new("never");
    let (dir, _) = setup.project("hub_never", None);
    let mut hub = setup.hub();
    assert_eq!(state(&hub, &dir), BuildState::Never);
    hub.apply(HubAction::OpenLastBuild(dir.clone()));
    assert!(hub.dry_log.is_empty());
    assert!(hub.status().unwrap().text.contains("no previous build"));
    hub.apply(HubAction::Open(dir.clone()));
    assert!(hub.dry_log[0].contains("--bin hub_never-editor"));

    // Удачная сборка запоминает программу; в следующий раз Open — без cargo.
    let built = setup.root.join("bin").join("hub_never-editor.exe");
    std::fs::create_dir_all(built.parent().unwrap()).unwrap();
    std::fs::write(&built, "fake").unwrap();
    {
        let job = hub.job_mut().unwrap();
        job.feed_stdout(&format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file:///x/hub_never#0.1.0","target":{{"name":"hub_never-editor"}},"executable":{},"fresh":false}}"#,
            serde_json_string(&built)
        ));
        job.feed_stdout(r#"{"reason":"build-finished","success":true}"#);
        job.simulate_exit(true);
    }
    hub.update();
    assert_eq!(hub.dry_log.last().unwrap(), &format!("launch {}", built.display()));
    let (saved, _) = HubConfig::load(&setup.config.join(paths::CONFIG_FILE));
    assert_eq!(saved.entry(&dir).unwrap().editor_exe.as_deref(), Some(built.as_path()));
    let again = setup.hub();
    assert!(matches!(state(&again, &dir), BuildState::Fresh { .. }), "the new build is remembered");
}

#[test]
fn projects_without_own_code_open_in_the_generic_editor_at_once() {
    let setup = Setup::new("generic");
    let engine = paths::find_engine_root().unwrap();
    let plan = templates::plan(
        &NewProject {
            name: "hub_plain".into(),
            location: setup.root.clone(),
            template: Template::Empty3D,
            cpp: false,
        },
        &engine,
    )
    .unwrap();
    templates::create(&plan, &engine).unwrap();
    let dir = paths::clean(&plan.dir);
    let file = setup.config.join(paths::CONFIG_FILE);
    let (mut config, _) = HubConfig::load(&file);
    config.entry_mut(&dir);
    config.save(&file).unwrap();
    let mut hub = setup.hub_with_tools(true);
    let info = hub.projects().iter().find(|l| paths::same(&l.path, &dir)).unwrap().info.clone().unwrap();
    assert!(!info.native, "a new project has no code to compile");
    hub.apply(HubAction::Open(dir.clone()));
    let tools = setup.root.join("tools");
    let editor = tools.join(format!("piska-editor{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(hub.dry_log, [format!("launch {} --project {}", editor.display(), dir.display())], "no cargo at all");
    assert!(hub.job().is_none());
    hub.apply(HubAction::Play(dir.clone()));
    let player = tools.join(format!("piska-player{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(hub.dry_log.last().unwrap(), &format!("launch {} --project {}", player.display(), dir.display()));

    // Свой код появился, программа проекта не собрана — общий редактор сразу, с `--build`.
    let (native, _) = setup.project("hub_native_new", None);
    hub.apply(HubAction::Refresh);
    hub.apply(HubAction::Open(native.clone()));
    assert_eq!(hub.dry_log.last().unwrap(), &format!("launch {} --project {} --build", editor.display(), native.display()));
    assert!(hub.job().is_none(), "the build happens inside the editor, the hub stays free");
}

fn serde_json_string(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('\\', "\\\\"))
}

#[test]
fn waiting_and_failures_are_explained_in_plain_words() {
    let setup = Setup::new("diagnosis");
    let (dir, _) = setup.project("hub_diag", Some(-60));
    let mut hub = setup.hub();
    hub.apply(HubAction::Open(dir.clone()));

    hub.job_mut().unwrap().feed_stderr("    Blocking waiting for file lock on build directory");
    match hub.job_diagnosis() {
        Some(Diagnosis::WaitingForLock { what, .. }) => assert_eq!(what, "build directory"),
        other => panic!("waiting for the lock is explained: {other:?}"),
    }
    hub.job_mut().unwrap().feed_stderr("    Blocking waiting for file lock on package cache");
    assert!(matches!(hub.job_diagnosis(), Some(Diagnosis::WaitingForLock { what, .. }) if what == "package cache"));

    // Ошибка в движке, а не в проекте.
    hub.job_mut().unwrap().feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/crates/engine-render#0.0.1","message":{"level":"error","message":"expected `;`","rendered":"error: expected `;`","spans":[{"file_name":"crates/engine-render/src/renderer.rs","line_start":10,"column_start":5,"is_primary":true}]}}"#);
    match hub.job_diagnosis() {
        Some(Diagnosis::EngineBroken { package, error }) => {
            assert_eq!(package, "engine-render");
            assert_eq!(error.location(), "crates/engine-render/src/renderer.rs:10:5");
        }
        other => panic!("an engine error is named as such: {other:?}"),
    }
    let job = hub.job_mut().unwrap();
    job.feed_stdout(r#"{"reason":"build-finished","success":false}"#);
    job.simulate_exit(false);
    hub.update();
    assert!(matches!(hub.job().unwrap().state, JobState::Failed(_)));
    assert!(hub.last_build(&dir, &piska_hub::jobs::JobKind::Open).is_some(), "the last build is offered");

    // Ошибка в своём коде.
    hub.apply(HubAction::RetryJob);
    hub.job_mut().unwrap().feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///x/hub_diag#0.1.0","message":{"level":"error","message":"cannot find value `speeed`","rendered":"error","spans":[{"file_name":"src/lib.rs","line_start":12,"column_start":9,"is_primary":true}]}}"#);
    match hub.job_diagnosis() {
        Some(Diagnosis::ProjectError { error }) => assert_eq!(error.location(), "src/lib.rs:12:9"),
        other => panic!("a project error is the project's: {other:?}"),
    }
}

#[test]
fn prebuild_all_builds_what_needs_it_one_after_another() {
    let setup = Setup::new("prebuild");
    let (fresh, _) = setup.project("hub_pb_fresh", Some(60));
    let (first, _) = setup.project("hub_pb_one", None);
    let (second, _) = setup.project("hub_pb_two", Some(-60));
    let mut hub = setup.hub();
    let _ = fresh;
    // Только эти три проекта: игры движка в списке тоже есть, считаем по своим.
    hub.apply(HubAction::PrebuildAll);
    let prebuild = hub.prebuild().expect("prebuild started").clone();
    let queued: Vec<&PathBuf> = prebuild.queue.iter().map(|(p, _)| p).collect();
    assert!(queued.iter().any(|p| paths::same(p, &first)));
    assert!(queued.iter().any(|p| paths::same(p, &second)));
    assert!(!queued.iter().any(|p| paths::same(p, &fresh)), "fresh editors are skipped");
    assert!(prebuild.skipped >= 1);

    // Проходим очередь: первая сборка падает, остальные удаются; ничего не запускается.
    let mut steps = 0;
    while hub.prebuild().is_some_and(|p| !p.finished) {
        steps += 1;
        assert!(steps < 100);
        let job = hub.job_mut().expect("a build per project");
        assert!(!job.launch, "prebuild never starts the editor");
        let ok = steps != 1;
        if ok {
            let exe = setup.root.join("bin").join(format!("built-{steps}.exe"));
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, "fake").unwrap();
            let target = job.command.launch_target.clone().unwrap();
            job.feed_stdout(&format!(
                r#"{{"reason":"compiler-artifact","package_id":"x#p@0.1.0","target":{{"name":"{target}"}},"executable":{},"fresh":false}}"#,
                serde_json_string(&exe)
            ));
        } else {
            job.feed_stdout(r#"{"reason":"compiler-message","package_id":"x#p@0.1.0","message":{"level":"error","message":"boom","rendered":"error: boom","spans":[]}}"#);
        }
        job.feed_stdout(&format!(r#"{{"reason":"build-finished","success":{ok}}}"#));
        job.simulate_exit(ok);
        hub.update();
    }
    let prebuild = hub.prebuild().unwrap();
    assert_eq!(prebuild.results.len(), prebuild.queue.len());
    assert!(!prebuild.results[0].1 && prebuild.results[0].2.contains("boom"), "{:?}", prebuild.results[0]);
    assert!(prebuild.results[1..].iter().all(|r| r.1));
    assert!(hub.dry_log.iter().all(|line| !line.starts_with("launch")), "{:?}", hub.dry_log);
    // Дальше в очереди — только программы движка, которых нет (общий редактор, Piska Modeler).
    assert!(hub.job().is_none_or(|j| j.tool && !j.launch), "after the project editors only engine tools build");
    assert!(hub.queued().all(|q| q.tool.is_some()));
    hub.apply(HubAction::CloseJob);
}

#[test]
fn open_does_not_start_a_second_editor_for_a_project_that_is_already_open() {
    let setup = Setup::new("open-twice");
    let (dir, _exe) = setup.project("hub_open_twice", Some(60));
    let mut hub = setup.hub();
    // Редактор проекта запущен не этим хабом: метка проекта — у него.
    let mut other = if cfg!(windows) {
        std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn()
    } else {
        std::process::Command::new("sleep").arg("30").spawn()
    }
    .unwrap();
    engine_editor::instance::acquire(&dir, other.id()).unwrap();
    hub.apply(HubAction::Open(dir.clone()));
    let result = (hub.dry_log.clone(), hub.status().map(|s| s.text.clone()));
    let _ = other.kill();
    let _ = other.wait();
    engine_editor::instance::release(&dir, other.id());
    assert!(result.0.is_empty(), "nothing is launched: {:?}", result.0);
    let status = result.1.unwrap_or_default();
    assert!(status.contains("already open in the editor") && status.contains(&other.id().to_string()), "{status}");
    // Того редактора больше нет (метки нет или она устарела) — Open снова работает.
    hub.apply(HubAction::Open(dir.clone()));
    assert_eq!(hub.dry_log.len(), 1, "{:?}", hub.dry_log);
}
