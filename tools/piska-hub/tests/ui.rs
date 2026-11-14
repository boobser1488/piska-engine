//! Хаб работает мышью и клавиатурой без окна и GPU: список, поиск, выбор, открытие
//! (в режиме `dry_run` — только команда), мастер нового проекта, убрать / вернуть,
//! удаление с диска, списки переживают перезапуск.

use std::path::{Path, PathBuf};

use piska_hub::config::HubConfig;
use piska_hub::jobs::JobState;
use piska_hub::{paths, Hub, HubAction, HubOptions, Page};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 900.0);
const DT: f32 = 1.0 / 60.0;

struct Hands {
    ctx: egui::Context,
    time: f64,
}

impl Hands {
    fn new(hub: &mut Hub) -> Self {
        let mut hands = Self {
            ctx: egui::Context::default(),
            time: 0.0,
        };
        hands.frames(hub, 3);
        hands
    }

    fn frame(&mut self, hub: &mut Hub, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += f64::from(DT);
        hub.update();
        let _ = self.ctx.run(input, |ctx| hub.ui(ctx));
    }

    fn frames(&mut self, hub: &mut Hub, count: usize) {
        for _ in 0..count {
            self.frame(hub, vec![]);
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(&mut self, hub: &mut Hub, pos: egui::Pos2) {
        self.frame(hub, vec![egui::Event::PointerMoved(pos)]);
        self.frame(hub, vec![Self::button(pos, true)]);
        self.frame(hub, vec![Self::button(pos, false)]);
        self.frames(hub, 2);
    }

    fn double_click(&mut self, hub: &mut Hub, pos: egui::Pos2) {
        self.frame(hub, vec![egui::Event::PointerMoved(pos)]);
        for _ in 0..2 {
            self.frame(hub, vec![Self::button(pos, true)]);
            self.frame(hub, vec![Self::button(pos, false)]);
        }
        self.frames(hub, 2);
    }

    fn type_text(&mut self, hub: &mut Hub, text: &str) {
        self.frame(hub, vec![egui::Event::Text(text.to_string())]);
        self.frames(hub, 2);
    }

    /// Центр виджета, если он нарисован в последнем кадре.
    fn center(&self, id: egui::Id) -> Option<egui::Pos2> {
        self.ctx.read_response(id).map(|r| r.rect.center())
    }

    fn click_id(&mut self, hub: &mut Hub, id: egui::Id) {
        let pos = self.center(id).unwrap_or_else(|| panic!("widget {id:?} is on screen"));
        self.click(hub, pos);
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn hub(config_dir: &Path) -> Hub {
    // Свой пустой каталог программ движка: piska-editor «не собран» — проверки не зависят
    // от того, что лежит в общем каталоге сборки.
    let tools = config_dir.join("tools");
    let _ = std::fs::create_dir_all(&tools);
    Hub::new(HubOptions {
        config_dir: Some(config_dir.to_path_buf()),
        dry_run: true,
        tools_dir: Some(tools),
        ..HubOptions::default()
    })
}

fn root() -> PathBuf {
    paths::find_engine_root().expect("the tests run inside the engine workspace")
}

#[test]
fn the_list_shows_engine_games_and_search_filters_it() {
    let config = temp("search");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    let template = root().join("games").join("template");
    assert!(hands.center(Hub::row_id(&template)).is_some(), "games/template is listed");
    assert!(hub.projects().iter().any(|l| l.location.as_deref() == Some("games/template")));

    // Щелчок в поле поиска и ввод.
    let search = hub.search_field_id().expect("the search field is drawn");
    hands.click_id(&mut hub, search);
    hands.type_text(&mut hub, "zzz-no-such");
    assert_eq!(hub.search(), "zzz-no-such");
    assert!(hands.center(Hub::row_id(&template)).is_none(), "filtered out");
    assert!(hub.visible_projects().is_empty());

    hub.set_search("");
    hands.frames(&mut hub, 2);
    hands.click_id(&mut hub, search);
    hands.type_text(&mut hub, "templ");
    assert!(hands.center(Hub::row_id(&template)).is_some());
    assert!(hub.visible_projects().iter().all(|l| l.path.to_string_lossy().contains("templ") || l.title().to_lowercase().contains("templ")));
    std::fs::remove_dir_all(&config).unwrap();
}

#[test]
fn a_click_selects_a_project_and_a_double_click_opens_it_with_build_progress() {
    let config = temp("open");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    let template = root().join("games").join("template");
    let row = hands.center(Hub::row_id(&template)).unwrap();
    hands.click(&mut hub, row);
    assert!(hub.selected().is_some_and(|s| paths::same(s, &template)));
    assert!(hub.job().is_none());

    hands.double_click(&mut hub, row);
    // Свежая программа редактора в общем каталоге сборки открывается сразу (без cargo);
    // тогда дальше проверяем ход сборки через «Rebuild and Open».
    if hub.job().is_none() {
        assert!(hub.dry_log.last().unwrap().starts_with("launch "), "{:?}", hub.dry_log);
        hub.apply(HubAction::RebuildAndOpen(template.clone()));
        hands.frames(&mut hub, 2);
    }
    let job = hub.job().expect("double-click starts the editor build");
    assert_eq!(job.state, JobState::Building);
    assert!(hub.dry_log.last().unwrap().ends_with("build -p template --features editor --bin template-editor --message-format=json"));
    // Открытие записано: «последний раз» и файл недавних для редактора.
    let (saved, _) = HubConfig::load(&config.join(paths::CONFIG_FILE));
    assert!(saved.entry(&template).and_then(|e| e.last_opened).is_some());
    let recent = std::fs::read_to_string(config.join(paths::RECENT_FILE)).unwrap();
    assert!(recent.lines().nth(1).unwrap().ends_with(&template.display().to_string()), "{recent}");

    // Вывод cargo попадает в окно хода сборки.
    let job = hub.job_mut().unwrap();
    job.feed_stderr("   Compiling engine-core v0.0.1");
    job.feed_stdout(r#"{"reason":"compiler-artifact","package_id":"path+file:///x/crates/engine-core#0.0.1","target":{"name":"engine_core"},"fresh":false}"#);
    job.feed_stdout(r#"{"reason":"compiler-message","message":{"level":"error","message":"expected `;`","rendered":"error: expected `;`\n --> games/template/src/lib.rs:3:9\n","spans":[{"file_name":"games/template/src/lib.rs","line_start":3,"column_start":9,"is_primary":true}]}}"#);
    hands.frames(&mut hub, 2);
    let progress = &hub.job().unwrap().progress;
    assert_eq!(progress.errors.len(), 1);
    assert_eq!(progress.errors[0].location(), "games/template/src/lib.rs:3:9");
    // Сборка идёт в фоне; подробности открываются из полосы задач.
    assert!(hands.center(egui::Id::new("hub-error").with(0)).is_none(), "the details are not forced on the user");
    hands.click_id(&mut hub, egui::Id::new(Hub::TASK_DETAILS_ID));
    assert!(hands.center(egui::Id::new("hub-error").with(0)).is_some(), "errors are listed with their location");

    // Отмена и возврат к списку.
    hands.click_id(&mut hub, egui::Id::new("hub-job-cancel"));
    assert_eq!(hub.job().unwrap().state, JobState::Cancelled);
    hands.click_id(&mut hub, egui::Id::new("hub-job-back"));
    assert!(hub.job().is_none());
    assert!(hands.center(Hub::row_id(&template)).is_some());
    std::fs::remove_dir_all(&config).unwrap();
}

#[test]
fn the_wizard_creates_a_project_that_appears_in_the_list() {
    let config = temp("wizard");
    let location = temp("wizard-location");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    hands.click_id(&mut hub, Hub::nav_id(Page::NewProject));
    assert_eq!(hub.page(), Page::NewProject);

    hands.click_id(&mut hub, egui::Id::new(Hub::WIZARD_NAME_ID));
    hands.type_text(&mut hub, "Bad Name");
    // Неверное имя — кнопка недоступна, проект не создаётся.
    hands.click_id(&mut hub, egui::Id::new(Hub::CREATE_BUTTON_ID));
    assert!(hub.wizard().created.is_none());

    hub.wizard_mut().name.clear();
    hands.frames(&mut hub, 1);
    hands.click_id(&mut hub, egui::Id::new(Hub::WIZARD_NAME_ID));
    hands.type_text(&mut hub, "hub_ui_probe");
    hub.wizard_mut().location = location.display().to_string();
    hands.frames(&mut hub, 2);
    // Второй шаблон — Empty 3D.
    hands.click_id(&mut hub, egui::Id::new("hub-template").with(1));
    assert_eq!(hub.wizard().template, piska_hub::templates::Template::Empty3D);
    hands.click_id(&mut hub, egui::Id::new(Hub::CREATE_BUTTON_ID));
    let created = hub.wizard().created.clone().expect("the project was created");
    assert!(paths::same(&created, &location.join("hub_ui_probe")));
    assert!(created.join("src").join("bin").join("hub_ui_probe-editor.rs").is_file());
    assert!(created.join("assets").join("scenes").join("main.scene.meta").is_file());
    let listed = hub.projects().iter().find(|l| paths::same(&l.path, &created)).expect("listed");
    let info = listed.info.as_ref().unwrap();
    assert_eq!(info.dimension, piska_hub::project::Dimension::ThreeD);
    assert!(info.standalone);

    // «Open Now» — сборка редактора нового проекта.
    hands.click_id(&mut hub, egui::Id::new("hub-open-now"));
    // Без своего кода — общий редактор; он ещё не собран, поэтому одна сборка piska-editor.
    assert!(hub.dry_log.last().unwrap().contains("build -p piska-editor --bin piska-editor"), "{:?}", hub.dry_log);
    assert_eq!(hub.job().unwrap().command.launch_args, ["--project".to_string(), created.display().to_string()]);
    hub.apply(HubAction::CancelJob);
    hub.apply(HubAction::CloseJob);

    // Проект создан там, где хаб создаёт проекты, — его можно удалить с диска.
    assert_eq!(hub.can_delete(&created), Ok(()));
    hub.apply(HubAction::Delete(created.clone()));
    assert!(!created.exists());
    assert!(hub.projects().iter().all(|l| !paths::same(&l.path, &created)));
    std::fs::remove_dir_all(&config).unwrap();
    std::fs::remove_dir_all(&location).unwrap();
}

#[test]
fn remove_pin_and_missing_projects_survive_a_restart() {
    let config = temp("persist");
    let template = root().join("games").join("template");
    let gone = temp("persist-gone").join("old_game");
    {
        let mut hub = hub(&config);
        hub.apply(HubAction::TogglePin(template.clone()));
        assert!(hub.projects()[0].favorite, "pinned projects go first");
        // Пропавший каталог: строка остаётся серой с «Locate…».
        let mut cfg = hub.config.clone();
        cfg.entry_mut(&gone).last_opened = Some(1);
        cfg.save(&config.join(paths::CONFIG_FILE)).unwrap();
    }
    let mut hub = hub(&config);
    assert!(hub.projects().iter().find(|l| paths::same(&l.path, &template)).unwrap().favorite, "the pin was saved");
    let missing = hub.projects().iter().find(|l| paths::same(&l.path, &gone)).expect("missing projects stay listed");
    assert!(missing.missing());
    let mut hands = Hands::new(&mut hub);
    assert!(hands.center(Hub::row_id(&gone).with("locate")).is_some(), "a missing row offers Locate…");
    hands.click_id(&mut hub, Hub::row_id(&gone).with("locate"));
    assert!(matches!(hub.dialog(), Some(piska_hub::Dialog::Add { locate: Some(_), .. })));
    hub.apply(HubAction::CloseDialog);

    // Убрать игру движка из списка: прячется, файлы на месте; Restore возвращает.
    hub.apply(HubAction::Remove(template.clone()));
    assert!(template.join("Cargo.toml").is_file());
    assert!(hub.projects().iter().all(|l| !paths::same(&l.path, &template)));
    let mut again = Hub::new(HubOptions {
        config_dir: Some(config.clone()),
        dry_run: true,
        ..HubOptions::default()
    });
    assert!(again.projects().iter().all(|l| !paths::same(&l.path, &template)), "removal survives a restart");
    again.apply(HubAction::RestoreHidden);
    assert!(again.projects().iter().any(|l| paths::same(&l.path, &template)));

    // Удалять с диска можно не всё: примеры движка и чужие каталоги — нет.
    assert!(again.can_delete(&template).unwrap_err().contains("samples"));
    let stray = temp("stray");
    assert!(again.can_delete(&stray).is_err());
    std::fs::remove_dir_all(&stray).unwrap();
    std::fs::remove_dir_all(&config).unwrap();
    std::fs::remove_dir_all(gone.parent().unwrap()).unwrap();
}

#[test]
fn the_hub_stays_usable_while_a_build_runs() {
    let config = temp("busy");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    let template = root().join("games").join("template");
    // Медленная («вечная») сборка в фоне.
    hub.apply(HubAction::RebuildAndOpen(template.clone()));
    assert!(hub.job().is_some_and(|j| j.is_building()));
    hands.frames(&mut hub, 2);
    assert!(hands.center(egui::Id::new(Hub::TASK_DETAILS_ID)).is_some(), "the task bar shows the build");

    // Список, поиск и выбор работают как обычно.
    let search = hub.search_field_id().expect("the projects page is shown, not a progress screen");
    hands.click_id(&mut hub, search);
    hands.type_text(&mut hub, "templ");
    assert_eq!(hub.search(), "templ");
    hub.set_search("");
    hands.frames(&mut hub, 2);
    if let Some(other) = hub.projects().iter().find(|l| l.folder() == "swarm").map(|l| l.path.clone()) {
        hands.click_id(&mut hub, Hub::row_id(&other));
        assert!(hub.selected().is_some_and(|s| paths::same(s, &other)));
        // Второй проект в очередь, а не «подождите».
        hub.apply(HubAction::RebuildAndOpen(other.clone()));
        assert_eq!(hub.queued().count(), 1);
        assert!(hub.job().is_some_and(|j| paths::same(&j.project, &template)), "the running build is not replaced");
    }
    // Настройки, мастер — тоже.
    hands.click_id(&mut hub, Hub::nav_id(Page::Settings));
    assert_eq!(hub.page(), Page::Settings);
    hands.click_id(&mut hub, Hub::nav_id(Page::NewProject));
    assert_eq!(hub.page(), Page::NewProject);
    assert!(hub.job().is_some_and(|j| j.is_building()), "the build goes on in the background");

    // Подробности — по кнопке, и из них назад, не останавливая сборку.
    hands.click_id(&mut hub, egui::Id::new(Hub::TASK_DETAILS_ID));
    hands.click_id(&mut hub, egui::Id::new("hub-job-hide"));
    assert!(hub.job().is_some_and(|j| j.is_building()));
    assert!(hub.search_field_id().is_some() || hub.page() == Page::NewProject);

    // Сборка кончилась неудачей — следующая из очереди начинается сама.
    if hub.queued().count() == 1 {
        let job = hub.job_mut().unwrap();
        job.feed_stdout(r#"{"reason":"build-finished","success":false}"#);
        job.simulate_exit(false);
        hands.frames(&mut hub, 2);
        assert_eq!(hub.queued().count(), 0);
        assert!(hub.job().is_some_and(|j| j.is_building() && j.title != "template"));
    }
    std::fs::remove_dir_all(&config).unwrap();
}

#[test]
fn the_wizard_works_from_the_keyboard() {
    let config = temp("keys");
    let location = temp("keys-location");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    hands.click_id(&mut hub, Hub::nav_id(Page::NewProject));
    hands.frames(&mut hub, 2);
    let focused = hands.ctx.memory(|m| m.focused());
    assert_eq!(focused, Some(egui::Id::new(Hub::WIZARD_NAME_ID)), "the name field has focus when the page opens");
    // Esc — назад к списку.
    hands.frame(&mut hub, vec![key(egui::Key::Escape)]);
    hands.frames(&mut hub, 2);
    assert_eq!(hub.page(), Page::Projects);

    hands.click_id(&mut hub, Hub::nav_id(Page::NewProject));
    hands.frames(&mut hub, 2);
    hub.wizard_mut().location = location.display().to_string();
    hands.type_text(&mut hub, "keyboard_game");
    assert_eq!(hub.wizard().name, "keyboard_game", "typing goes into the focused name field");
    let started = std::time::Instant::now();
    hands.frame(&mut hub, vec![key(egui::Key::Enter)]);
    hands.frames(&mut hub, 2);
    let created = location.join("keyboard_game");
    assert!(created.join("Cargo.toml").is_file(), "Enter creates the project");
    eprintln!("create + open request took {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    assert!(hub.dry_log.iter().any(|l| l.contains("piska-editor")), "…and opens it in the generic editor: {:?}", hub.dry_log);
    assert_eq!(hub.page(), Page::Projects);
    // Мастер помнит место и шаблон.
    let again = Hub::new(HubOptions {
        config_dir: Some(config.clone()),
        dry_run: true,
        ..HubOptions::default()
    });
    assert!(paths::same(std::path::Path::new(&again.wizard().location), &location));
    assert_eq!(again.wizard().template, hub.wizard().template);
    std::fs::remove_dir_all(&config).unwrap();
    std::fs::remove_dir_all(&location).unwrap();
}

#[test]
fn the_tools_page_starts_the_model_editor() {
    let config = temp("tools");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    hands.click_id(&mut hub, Hub::nav_id(Page::Tools));
    assert_eq!(hub.page(), Page::Tools);
    hands.click_id(&mut hub, egui::Id::new("hub-tool").with(0).with("open"));
    // Modeler живёт в своём рабочем пространстве `modeler/`: его собирают по манифесту оттуда.
    let command = hub.dry_log.last().unwrap();
    assert!(command.contains("-p piska-modeler --bin piska-modeler"), "{:?}", hub.dry_log);
    assert!(command.contains("build --manifest-path") && command.contains("modeler"), "{:?}", hub.dry_log);
    std::fs::remove_dir_all(&config).unwrap();
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn add_existing_validates_the_folder() {
    let config = temp("add");
    let mut hub = hub(&config);
    let mut hands = Hands::new(&mut hub);
    // «Add Existing…» стало страницей «Import Project» (docs/api/hub.md); диалог остался только для Locate.
    hands.click_id(&mut hub, egui::Id::new("hub-import"));
    assert_eq!(hub.page(), Page::Import);
    assert!(hub.dialog().is_none());
    // Не проект — объяснение и ничего не добавлено.
    let not_project = temp("add-empty");
    hub.apply(HubAction::Add(not_project.clone()));
    assert!(hub.status().unwrap().text.contains("no Cargo.toml"));
    assert!(hub.projects().iter().all(|l| !paths::same(&l.path, &not_project)));
    std::fs::remove_dir_all(&not_project).unwrap();
    std::fs::remove_dir_all(&config).unwrap();
}
