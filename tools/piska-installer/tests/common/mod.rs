//! Общее для тестов установщика: «руки» (настоящие события egui без окна и GPU), поддельные
//! скрипты (записанные потоки событий) и временные каталоги.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use piska_installer::runner::{Backend, FakeLog, FakeScript, Purpose, RunRequest};
use piska_installer::{Installer, InstallerOptions};
use serde_json::json;

/// Язык интерфейса — на весь процесс: тесты одного файла идут по очереди.
static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    engine_editor::kit::i18n::set(engine_editor::kit::i18n::Language::English);
    guard
}

pub const SCREEN: egui::Vec2 = egui::vec2(1060.0, 700.0);
const DT: f32 = 1.0 / 60.0;

/// Мышь и клавиатура без окна.
pub struct Hands {
    pub ctx: egui::Context,
    time: f64,
    pub shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    pub fn new(installer: &mut Installer) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        hands.frames(installer, 3);
        hands
    }

    pub fn frame(&mut self, installer: &mut Installer, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += f64::from(DT);
        installer.update();
        self.shapes = self.ctx.run(input, |ctx| installer.ui(ctx)).shapes;
    }

    pub fn frames(&mut self, installer: &mut Installer, n: usize) {
        for _ in 0..n {
            self.frame(installer, vec![]);
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    pub fn click_at(&mut self, installer: &mut Installer, pos: egui::Pos2) {
        self.frame(installer, vec![egui::Event::PointerMoved(pos)]);
        self.frame(installer, vec![Self::button(pos, true)]);
        self.frame(installer, vec![Self::button(pos, false)]);
        self.frames(installer, 2);
    }

    /// Нажать именованную кнопку (она должна быть нарисована в последнем кадре).
    pub fn click(&mut self, installer: &mut Installer, name: &str) {
        let pos = installer.hit(name).unwrap_or_else(|| panic!("no `{name}` on the page {:?}; there are: {:?}", installer.page, names(installer)));
        self.click_at(installer, pos);
    }

    pub fn key(&mut self, installer: &mut Installer, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        self.frame(installer, vec![ev(true)]);
        self.frame(installer, vec![ev(false)]);
        self.frames(installer, 2);
    }

    /// Кадры, пока не выполнится условие (фон работает в своих потоках).
    pub fn until(&mut self, installer: &mut Installer, limit: Duration, what: &str, mut done: impl FnMut(&Installer) -> bool) {
        let end = std::time::Instant::now() + limit;
        while !done(installer) {
            assert!(std::time::Instant::now() < end, "timed out waiting for: {what} (page {:?})", installer.page);
            self.frames(installer, 1);
            std::thread::sleep(Duration::from_millis(15));
        }
        self.frames(installer, 2);
    }

    /// Все надписи последнего кадра: (текст, моноширинный ли).
    pub fn texts(&self) -> Vec<(String, bool)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, bool)>) {
            match shape {
                egui::Shape::Text(t) => {
                    let mono = t.galley.job.sections.first().is_some_and(|s| s.format.font_id.family == egui::FontFamily::Monospace);
                    out.push((t.galley.text().to_string(), mono));
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &self.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    pub fn has_text(&self, needle: &str) -> bool {
        self.texts().iter().any(|(t, _)| t.contains(needle))
    }
}

/// Имена кнопок последнего кадра (для сообщений об ошибке).
pub fn names(installer: &Installer) -> Vec<String> {
    let mut v: Vec<String> = ["next", "back", "install", "cancel", "retry", "skip", "dialog-ok", "dialog-cancel", "close"]
        .iter()
        .filter(|n| installer.hit(n).is_some())
        .map(|s| s.to_string())
        .collect();
    v.sort();
    v
}

pub fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-installer-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn repo() -> PathBuf {
    // Без canonicalize: на Windows он даёт путь с префиксом `\\?\`, который скрипты не ждут.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

// ---------------------------------------------------------------------------
// Поддельные скрипты
// ---------------------------------------------------------------------------

/// Шаг поддельной машины: id, компонент, заголовок, уже установлен, права, лицензия, скачивание.
#[derive(Clone)]
pub struct FakeStep {
    pub id: &'static str,
    pub component: &'static str,
    pub title: &'static str,
    pub installed: bool,
    pub elevation: Option<&'static str>,
    pub licence: Option<(&'static str, &'static str)>,
    pub download_mb: u64,
    pub skippable: bool,
}

const fn step(id: &'static str, component: &'static str, title: &'static str, installed: bool) -> FakeStep {
    FakeStep { id, component, title, installed, elevation: None, licence: None, download_mb: 0, skippable: false }
}

/// Машина без Build Tools и Android, остальное есть.
pub fn machine() -> Vec<FakeStep> {
    let mut steps = vec![
        step("core.git", "core", "Git", true),
        FakeStep {
            elevation: Some("Visual Studio Build Tools install system-wide: Windows will show ONE administrator (UAC) prompt for it."),
            licence: Some(("vs-buildtools", "Visual Studio Build Tools")),
            ..step("core.buildtools", "core", "Visual Studio Build Tools (C++ workload + Windows SDK)", false)
        },
        FakeStep { download_mb: 14, ..step("core.rustup", "core", "Rust (rustup, stable, MSVC)", true) },
        FakeStep { skippable: true, ..step("core.rust-components", "core", "Rust components (clippy, rustfmt)", true) },
        FakeStep { skippable: true, ..step("core.vulkan", "core", "Vulkan runtime (GPU driver)", false) },
        step("cpp.cmake", "cpp", "CMake", true),
        step("cpp.ninja", "cpp", "Ninja", false),
        step("web.target", "web", "Rust target wasm32-unknown-unknown", false),
        step("android.jdk", "android", "JDK 17", false),
        FakeStep { download_mb: 156, ..step("android.cmdline-tools", "android", "Android command-line tools (sdkmanager)", false) },
        FakeStep { licence: Some(("android-sdk", "the Android SDK")), ..step("android.licenses", "android", "Android SDK licences", false) },
        step("android.packages", "android", "Android SDK packages", false),
        step("android.env", "android", "Environment: ANDROID_HOME, ANDROID_NDK_HOME (and JAVA_HOME if unset)", false),
        FakeStep { download_mb: 96, ..step("linux-cross.zig", "linux-cross", "zig 0.17.1", false) },
        step("linux-cross.env", "linux-cross", "Environment: PISKA_ZIG", false),
        step("engine.source", "engine", "Engine sources (with the modeler submodule)", true),
        step("engine.build", "engine", "Build hub, editor, export tool and modeler (release)", false),
        FakeStep { skippable: true, ..step("engine.shortcuts", "engine", "Start-menu shortcuts", false) },
        FakeStep { skippable: true, ..step("config.preferences", "config", "Editor preferences: Build & Tools paths", false) },
    ];
    for s in &mut steps {
        if s.id == "core.git" {
            s.elevation = Some("The Git installer may ask Windows for administrator rights (one UAC prompt).");
        }
    }
    steps
}

pub fn hello(dry_run: bool, free: u64, components: &[String]) -> String {
    json!({"event":"hello","protocol":1,"script":"install.ps1","shell":"PowerShell 5.1 Desktop","os":"windows","mode":"normal","dry_run":dry_run,
           "assume_clean":false,"components":components,"install_dir":"C:\\Fake\\Install","repo_dir":"C:\\Fake\\engine","in_checkout":true,
           "cache":"C:\\Fake\\Install\\cache","state_file":"C:\\Fake\\state.json","manifest_file":"C:\\Fake\\manifest.json","prefs_file":"C:\\Fake\\editor.ron",
           "log": if dry_run { serde_json::Value::Null } else { json!("C:\\Fake\\logs\\install.log") },"free_bytes":free,"is_admin":false})
    .to_string()
}

pub fn plan_line(s: &FakeStep, n: usize, total: usize, skip: bool) -> String {
    let mut lines = vec![json!({"kind":"run","text":format!("do {}", s.id)})];
    let mut downloads = vec![];
    if s.download_mb > 0 {
        let url = format!("https://example.invalid/{}.zip", s.id);
        lines.insert(0, json!({"kind":"download","text":format!("{url} (~{} MB)", s.download_mb)}));
        lines.insert(1, json!({"kind":"verify","text":"sha256 from https://example.invalid/x.sha256"}));
        downloads.push(json!({"name":s.id,"url":url,"size_mb":s.download_mb,"sha256":null,"sha256_url":"https://example.invalid/x.sha256","pinned":false,"pin_status":null,"cross_check":null}));
    }
    if s.id.ends_with(".env") {
        lines = vec![json!({"kind":"env","text":"ANDROID_HOME=C:\\Fake\\Sdk (user)"})];
    }
    json!({"event":"plan","n":n,"total":total,"id":s.id,"component":s.component,"title":s.title,"skip":skip,
           "reason": if skip { json!("found") } else { serde_json::Value::Null },
           "found": if skip { json!(format!("{} 1.2.3", s.id)) } else { serde_json::Value::Null },
           "version": if skip { json!("1.2.3") } else { serde_json::Value::Null }, "path": null,
           "elevation": s.elevation, "licence": s.licence.map(|(id, name)| json!({"id":id,"name":name,"url":format!("https://example.invalid/licence/{id}")})),
           "skippable": s.skippable, "download_mb": s.download_mb, "downloads": downloads, "lines": lines})
    .to_string()
}

pub fn summary(exit: i32, dry_run: bool, failed: Option<&str>, counts: (u32, u32, u32, u32)) -> String {
    json!({"event":"summary","exit":exit,"ok":exit == 0,"dry_run":dry_run,"failed_step":failed,"error":failed.map(|_| "boom"),"reboot":false,
           "elevation_refused":false,"steps":{"total":counts.0,"done":counts.1,"skipped":counts.2,"failed":counts.3},
           "log": if dry_run { serde_json::Value::Null } else { json!("C:\\Fake\\logs\\install.log") },"state_file":"C:\\Fake\\state.json",
           "manifest_file":"C:\\Fake\\manifest.json","repo_dir":"C:\\Fake\\engine","path":"C:\\Fake\\bin","env":{}})
    .to_string()
}

pub fn step_event(phase: &str, s: &FakeStep, n: usize, total: usize) -> String {
    json!({"event":"step","phase":phase,"step":s.id,"n":n,"total":total,"title":s.title,"component":s.component}).to_string()
}

/// Сухой прогон по поддельной машине: план для выбранных компонентов.
pub fn dry(request: &RunRequest, free: u64) -> FakeScript {
    let comps = &request.options.components;
    let steps: Vec<FakeStep> = machine().into_iter().filter(|s| comps.iter().any(|c| c == s.component) || s.component == "config").collect();
    let mut script = FakeScript::new().out(hello(true, free, comps)).out(r#"{"event":"log","level":"info","step":null,"text":"DRY RUN"}"#);
    for (i, s) in steps.iter().enumerate() {
        script = script.out(plan_line(s, i + 1, steps.len(), s.installed));
    }
    script.out(summary(0, true, None, (steps.len() as u32, 0, 0, 0))).exit(0)
}

/// Поведение установки по сценарию.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Success,
    /// Шаг посередине падает (ninja); на повтор с -Resume — удача.
    FailInMiddle,
    /// Вопрос о лицензии, которую не приняли заранее.
    LicencePrompt,
    /// UAC отклонён.
    ElevationRefused,
    /// Долгий шаг, который отменяют.
    Cancel,
    /// Мусор в выводе, установка удачна.
    Malformed,
    /// Тишина без конца.
    Hang,
    /// Процесс падает без события summary.
    Crash,
}

/// Поддельный скрипт установки.
pub fn install(request: &RunRequest, scenario: Scenario) -> FakeScript {
    let comps = &request.options.components;
    let steps: Vec<FakeStep> = machine().into_iter().filter(|s| comps.iter().any(|c| c == s.component) || s.component == "config").collect();
    let total = steps.len();
    let mut script = FakeScript::new().out(hello(false, 500 << 30, comps));
    let resumed = request.options.resume;
    for (i, s) in steps.iter().enumerate() {
        let n = i + 1;
        if s.installed || request.options.skip_steps.iter().any(|k| k == s.id) {
            script = script.out(json!({"event":"step","phase":"skip","step":s.id,"n":n,"total":total,"title":s.title,"reason":"found","found":"x 1.0"}).to_string());
            continue;
        }
        script = script.out(step_event("start", s, n, total));
        if s.elevation.is_some() {
            script = script.out(json!({"event":"elevation","step":s.id,"title":s.title,"reason":s.elevation,"confirmed":true}).to_string());
            if scenario == Scenario::ElevationRefused {
                return script
                    .out(json!({"event":"step","phase":"failed","step":s.id,"n":n,"total":total,"title":s.title,"error":"the administrator rights prompt was refused (exit code 1223)","log":"C:\\Fake\\logs\\install.log","skippable":false,"elevation_refused":true}).to_string())
                    .out(summary(1, false, Some(s.id), (total as u32, 0, 0, 1)))
                    .exit(1);
            }
        }
        if let Some((id, name)) = s.licence {
            if scenario == Scenario::LicencePrompt && id == "vs-buildtools" {
                script = script.out(json!({"event":"prompt","kind":"licence","step":s.id,"licence":"other-licence","name":name,"url":"https://example.invalid/l"}).to_string()).answer(
                    FakeScript::new(),
                    FakeScript::new()
                        .out(json!({"event":"step","phase":"declined","step":s.id,"title":s.title,"reason":"licence","licence":id}).to_string())
                        .out(summary(4, false, None, (total as u32, 0, 0, 0)))
                        .exit(4),
                );
            }
        }
        if s.download_mb > 0 {
            let total_bytes = s.download_mb << 20;
            for part in 1..=4u64 {
                script = script
                    .out(json!({"event":"progress","step":s.id,"url":format!("https://example.invalid/{}.zip", s.id),"bytes":total_bytes * part / 4,"total":total_bytes}).to_string())
                    .sleep_ms(20);
            }
        }
        script = script.out(json!({"event":"log","level":"output","step":s.id,"text":format!("working on {}", s.id)}).to_string());
        match scenario {
            Scenario::FailInMiddle if s.id == "cpp.ninja" && !resumed => {
                return script
                    .out(json!({"event":"log","level":"output","step":s.id,"text":"winget: download failed"}).to_string())
                    .out(json!({"event":"step","phase":"failed","step":s.id,"n":n,"total":total,"title":s.title,"error":"winget install Ninja-build.Ninja failed with exit code 5","log":"C:\\Fake\\logs\\install.log","skippable":false,"elevation_refused":false}).to_string())
                    .out(summary(1, false, Some(s.id), (total as u32, 1, 2, 1)))
                    .exit(1);
            }
            Scenario::Cancel if s.id == "cpp.ninja" => return script.sleep_ms(60_000).exit(0),
            Scenario::Hang if s.id == "cpp.ninja" => return script.hang(),
            Scenario::Crash if s.id == "cpp.ninja" => return script.err("Exception: something broke inside PowerShell").exit(-1),
            Scenario::Malformed if s.id == "cpp.ninja" => {
                script = script.out("WARNING: this is not JSON").out("{\"event\": broken").out("{\"no\":\"event\"}").err("a line on stderr");
            }
            _ => {}
        }
        script = script.out(step_event("done", s, n, total));
    }
    script.out(summary(0, false, None, (total as u32, 3, 2, 0))).exit(0)
}

/// Поддельный установщик: проверка, план и установка по сценарию.
pub fn fake_installer(scenario: Scenario, free: u64, mut options: InstallerOptions) -> (Installer, Arc<Mutex<FakeLog>>) {
    let (backend, log) = Backend::fake(move |request: &RunRequest| match request.purpose {
        Purpose::Detect | Purpose::Plan => dry(request, free),
        Purpose::Install => install(request, scenario),
        Purpose::UninstallPlan => FakeScript::new()
            .out(r#"{"event":"uninstall-plan","manifest":"C:\\Fake\\manifest.json","dry_run":true,"items":[{"action":"delete-file","target":"C:\\Fake\\Piska Hub.lnk","ask":false},{"action":"winget-uninstall","target":"Kitware.CMake","ask":true}]}"#)
            .out(summary(0, true, None, (0, 0, 0, 0)))
            .exit(0),
        Purpose::Uninstall => FakeScript::new()
            .out(r#"{"event":"prompt","kind":"confirm","step":null,"key":"uninstall-proceed","subject":"","question":"Proceed?","default":false}"#)
            .answer(
                FakeScript::new()
                    .out(r#"{"event":"prompt","kind":"confirm","step":null,"key":"uninstall-item","subject":"Kitware.CMake","question":"Remove Kitware.CMake (winget-uninstall)?","default":false}"#)
                    .answer(FakeScript::new().out(r#"{"event":"log","level":"info","step":null,"text":"  done: winget-uninstall Kitware.CMake"}"#), FakeScript::new().out(r#"{"event":"log","level":"info","step":null,"text":"  kept Kitware.CMake"}"#))
                    .out(summary(0, false, None, (0, 0, 0, 0))),
                FakeScript::new().out(summary(4, false, None, (0, 0, 0, 0))).exit(4),
            )
            .exit(0),
    });
    options.backend = Some(backend);
    options.sandbox = true;
    let keep_language = options.config_dir.is_some() && options.language.is_none();
    if options.config_dir.is_none() {
        options.config_dir = Some(temp(&format!("cfg-{scenario:?}-{}", COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst))));
    }
    if options.doctor.is_none() {
        options.doctor = Some(Vec::new());
    }
    if options.engine_root.is_none() {
        options.engine_root = Some(repo());
    }
    // Свой каталог настроек — язык из них (тест перезапуска); иначе — английский.
    if !keep_language {
        options.language = options.language.or(Some(engine_editor::kit::i18n::Language::English));
    }
    (Installer::new(options), log)
}

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Пройти мастер до страницы Review нажатиями (Typical).
pub fn walk_to_review(hands: &mut Hands, installer: &mut Installer) {
    hands.until(installer, Duration::from_secs(60), "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    hands.click(installer, "next");
    assert_eq!(installer.page, piska_installer::Page::Components);
    hands.click(installer, "preset-typical");
    hands.click(installer, "next");
    assert_eq!(installer.page, piska_installer::Page::Options);
    hands.click(installer, "next");
    assert_eq!(installer.page, piska_installer::Page::Licences);
    for l in installer.licences_needed() {
        let key = Installer::licence_key(&l);
        hands.click(installer, &format!("licence-{key}"));
    }
    hands.click(installer, "next");
    assert_eq!(installer.page, piska_installer::Page::Review);
    hands.until(installer, Duration::from_secs(120), "the plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}
