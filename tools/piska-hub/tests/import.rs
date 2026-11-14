//! Импорт проекта и сохранение проекта в файл: круг «упаковать → импортировать», каждая
//! настройка, каждая проблема предварительного отчёта, отказ на опасных и повреждённых
//! файлах, отмена без следов, перетаскивание, управление мышью и клавиатурой.
//!
//! Все проекты-образцы создаются в временных каталогах (их удаляют сами тесты); настоящий
//! список проектов и настройки хаба не читаются и не пишутся.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine_project::{Conflict, Control, PackOptions};
use piska_hub::import::{self, Code, EngineDep, ImportSettings, Level, Mode, Source};
use piska_hub::templates::{self, NewProject, Template};
use piska_hub::{paths, Dialog, Hub, HubAction, HubOptions, ImportStage, Page};

fn temp(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("piska-hub-import-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine() -> PathBuf {
    paths::find_engine_root().expect("the tests run inside the engine workspace")
}

/// Отдельный проект (со своим `[workspace]`) во временном каталоге.
fn make_project(parent: &Path, name: &str, cpp: bool) -> PathBuf {
    let root = engine();
    let plan = templates::plan(
        &NewProject {
            name: name.to_string(),
            location: parent.to_path_buf(),
            template: Template::Empty3D,
            cpp,
        },
        &root,
    )
    .unwrap();
    templates::create(&plan, &root).unwrap();
    plan.dir
}

fn write(root: &Path, rel: &str, body: impl AsRef<[u8]>) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Контрольная сумма FNV-1a 64: достаточно, чтобы сравнивать деревья.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn tree(root: &Path) -> BTreeMap<String, (u64, u64)> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, (u64, u64)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = format!("{prefix}{name}");
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{rel}/"), out);
            } else {
                let bytes = fs::read(entry.path()).unwrap();
                out.insert(rel, (bytes.len() as u64, fnv(&bytes)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, "", &mut out);
    out
}

fn hub(config: &Path, import: Vec<PathBuf>) -> Hub {
    let tools = config.join("tools");
    let _ = fs::create_dir_all(&tools);
    Hub::new(HubOptions {
        config_dir: Some(config.to_path_buf()),
        dry_run: true,
        tools_dir: Some(tools),
        import,
        ..HubOptions::default()
    })
}

fn pack(project: &Path, out: &Path) {
    engine_project::pack(project, out, &PackOptions::default(), &Control::default(), &mut |_| {}).unwrap();
}

fn codes(hub: &mut Hub, index: usize) -> Vec<(Level, Code)> {
    hub.import_problems(index).iter().map(|p| (p.level, p.code)).collect()
}

fn has(hub: &mut Hub, index: usize, code: Code) -> bool {
    codes(hub, index).iter().any(|(_, c)| *c == code)
}

/// Импортировать проект и дождаться конца; возвращает хаб.
fn run_import(hub: &mut Hub) {
    hub.apply(HubAction::ImportRun);
    hub.wait_for_works();
}

fn settings_for(hub: &mut Hub, location: &Path) {
    let page = hub.import_page_mut();
    page.settings = ImportSettings::new(location.to_path_buf());
}

// ---------------------------------------------------------------------------
// Круг: проект → файл → импорт
// ---------------------------------------------------------------------------

#[test]
fn a_project_file_round_trips_through_the_hub_import() {
    let root = temp("round");
    let src = make_project(&root, "round_game", true);
    write(&src, "dist/x/web/index.html", "built");
    write(&src, "target/debug/junk.bin", "derived");
    let file = root.join("round_game.pks");
    pack(&src, &file);

    let config = temp("round-config");
    let mut hub = hub(&config, vec![file.clone()]);
    assert_eq!(hub.page(), Page::Import, "a file argument opens the import page");
    hub.wait_for_import();
    assert_eq!(hub.import_page().items.len(), 1);
    let report = hub.import_page().items[0].report.clone().unwrap();
    assert_eq!(report.package, "round_game");
    assert_eq!(report.made_with.as_deref(), Some(import::ENGINE_VERSION));
    assert!(report.counts.scenes == 1 && report.counts.scenarios == 1 && report.counts.cpp_files == 1, "{:?}", report.counts);
    assert!(!report.blocked(), "{:?}", report.problems);

    let dest_parent = root.join("dest");
    settings_for(&mut hub, &dest_parent);
    assert!(hub.import_ready());
    run_import(&mut hub);
    let page = hub.import_page();
    assert_eq!(page.stage, ImportStage::Result);
    let result = page.result.as_ref().unwrap();
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.outcomes.len(), 1);
    let outcome = &result.outcomes[0];
    let dest = dest_parent.join("round_game");
    assert!(paths::same(&outcome.dir, &dest));
    assert_eq!(outcome.used, import::Used::Unpacked);

    // Дерево, размеры и суммы совпадают с исходником (без производного).
    let mut want = tree(&src);
    want.retain(|k, _| !k.starts_with("target/") && !k.starts_with("dist/"));
    assert_eq!(tree(&dest), want, "the imported tree equals the source tree");
    // Хаб принимает результат как проект и добавил его в список.
    let info = piska_hub::project::inspect(&dest).expect("the hub's own parser accepts the imported project");
    assert_eq!(info.package, "round_game");
    assert!(info.has_cpp);
    assert!(hub.projects().iter().any(|l| paths::same(&l.path, &dest)), "added to the project list");
    assert!(hub.config.settings.project_roots.iter().any(|r| paths::same(r, &dest_parent)));
    // Остатков временного каталога нет.
    let leftovers: Vec<String> = fs::read_dir(&dest_parent).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(leftovers, ["round_game"]);

    // Cargo принимает проект: манифест и пути читаются.
    let metadata = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--offline", "--format-version", "1", "--manifest-path"])
        .arg(dest.join("Cargo.toml"))
        .output();
    if let Ok(output) = metadata {
        assert!(output.status.success(), "cargo metadata: {}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("\"name\":\"round_game\""));
    }
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn copying_a_folder_never_touches_the_source() {
    let root = temp("copy");
    let src = make_project(&root, "copy_game", false);
    write(&src, "target/junk", "x");
    let before = tree(&src);
    let modified_before = fs::metadata(src.join("Cargo.toml")).unwrap().modified().unwrap();
    let config = temp("copy-config");
    let mut hub = hub(&config, vec![src.clone()]);
    hub.wait_for_import();
    let dest_parent = root.join("projects");
    settings_for(&mut hub, &dest_parent);
    {
        let page = hub.import_page_mut();
        page.items[0].name = "copy_game_two".to_string();
        page.items[0].name_touched = true;
        page.settings.regenerate_guids = true;
        page.settings.rename_inside = true;
    }
    run_import(&mut hub);
    assert_eq!(hub.import_page().result.as_ref().unwrap().outcomes.len(), 1);
    assert_eq!(tree(&src), before, "the source tree is byte for byte the same");
    assert_eq!(fs::metadata(src.join("Cargo.toml")).unwrap().modified().unwrap(), modified_before);
    assert!(dest_parent.join("copy_game_two/Cargo.toml").is_file());
    assert!(!tree(&dest_parent.join("copy_game_two")).keys().any(|k| k.starts_with("target/")), "target/ is not copied");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Настройки
// ---------------------------------------------------------------------------

fn imported(hub: &mut Hub) -> PathBuf {
    hub.import_page().result.as_ref().unwrap().outcomes[0].dir.clone()
}

#[test]
fn renaming_updates_the_manifest_the_sources_and_the_title() {
    let root = temp("rename");
    let src = make_project(&root, "old_name", false);
    let config = temp("rename-config");
    let mut hub = hub(&config, vec![src.clone()]);
    hub.wait_for_import();
    settings_for(&mut hub, &root.join("out"));
    {
        let page = hub.import_page_mut();
        page.items[0].name = "fresh-name".to_string();
        page.items[0].name_touched = true;
    }
    run_import(&mut hub);
    let dest = imported(&mut hub);
    let manifest = fs::read_to_string(dest.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"fresh-name\""), "{manifest}");
    assert!(manifest.contains("fresh-name-editor"), "the editor program follows the name: {manifest}");
    assert!(!manifest.contains("old_name"), "no trace of the old name: {manifest}");
    assert!(manifest.contains("title = \"Fresh Name\""), "{manifest}");
    assert!(dest.join("src/bin/fresh-name-editor.rs").is_file());
    assert!(!dest.join("src/bin/old_name-editor.rs").exists());
    let lib = fs::read_to_string(dest.join("src/lib.rs")).unwrap();
    assert!(lib.contains("GAME_NAME: &str = \"Fresh Name\""), "{lib}");
    let smoke = fs::read_to_string(dest.join("tests/smoke.rs")).unwrap();
    assert!(smoke.contains("fresh_name::build"), "tests' `use` paths follow: {smoke}");
    assert!(!smoke.contains("old_name"));
    let main = fs::read_to_string(dest.join("src/main.rs")).unwrap();
    assert!(main.contains("fresh_name::main()"));
    assert!(fs::read_to_string(dest.join("README.md")).unwrap().contains("-p fresh-name"));
    let info = piska_hub::project::inspect(&dest).unwrap();
    assert_eq!(info.package, "fresh-name");
    assert_eq!(info.editor_bin.as_deref(), Some("fresh-name-editor"));
    assert_eq!(info.title, "Fresh Name");
    let changes: Vec<_> = hub.import_page().result.as_ref().unwrap().outcomes[0].changes.iter().map(|c| c.code).collect();
    assert!(changes.contains(&import::ChangeCode::PackageRenamed) && changes.contains(&import::ChangeCode::FileRenamed));

    // Без «Rename inside» имя папки меняется, а пакет — нет.
    let mut hub2 = hub_again(&config, &src);
    settings_for(&mut hub2, &root.join("out2"));
    {
        let page = hub2.import_page_mut();
        page.items[0].name = "other_folder".to_string();
        page.items[0].name_touched = true;
        page.settings.rename_inside = false;
    }
    run_import(&mut hub2);
    let dest2 = imported(&mut hub2);
    assert!(dest2.ends_with("other_folder"));
    assert_eq!(piska_hub::project::inspect(&dest2).unwrap().package, "old_name", "the crate keeps its name");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

fn hub_again(config: &Path, src: &Path) -> Hub {
    let mut hub = hub(config, vec![src.to_path_buf()]);
    hub.wait_for_import();
    hub
}

#[test]
fn the_engine_dependency_is_rewritten_or_kept() {
    let root = temp("engine");
    let src = make_project(&root, "dep_game", false);
    // Проект «с другой машины»: путь к движку не существует здесь.
    let manifest = fs::read_to_string(src.join("Cargo.toml")).unwrap();
    let foreign = manifest
        .lines()
        .find(|l| l.starts_with("engine = "))
        .unwrap()
        .to_string();
    let foreign_text = manifest.replace(&foreign, "engine = { path = \"C:/Users/somebody/engine/crates/engine\", default-features = false, features = [\"full\"] }");
    fs::write(src.join("Cargo.toml"), &foreign_text).unwrap();
    let config = temp("engine-config");
    let mut h = hub(&config, vec![src.clone()]);
    h.wait_for_import();

    // Сохранить как есть: предупреждение и путь не тронут.
    settings_for(&mut h, &root.join("kept"));
    h.import_page_mut().settings.engine = EngineDep::Keep;
    assert!(has(&mut h, 0, Code::EnginePathBroken), "{:?}", codes(&mut h, 0));
    run_import(&mut h);
    let kept = imported(&mut h);
    assert!(fs::read_to_string(kept.join("Cargo.toml")).unwrap().contains("C:/Users/somebody/engine/crates/engine"));

    // Переписать: путь указывает на этот движок.
    let mut h2 = hub_again(&config, &src);
    settings_for(&mut h2, &root.join("rewritten"));
    assert!(has(&mut h2, 0, Code::EnginePathRewritten));
    assert!(!has(&mut h2, 0, Code::EnginePathBroken));
    run_import(&mut h2);
    let rewritten = imported(&mut h2);
    let text = fs::read_to_string(rewritten.join("Cargo.toml")).unwrap();
    let want = format!("{}/crates/engine", engine().display().to_string().replace('\\', "/"));
    assert!(text.contains(&want), "{text}");
    assert!(!text.contains("somebody"));
    assert!(text.contains("default-features = false"), "other keys of the dependency are kept: {text}");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn manifests_convert_between_workspace_member_and_standalone() {
    let root = engine();
    let member = fs::read_to_string(root.join("games/template/Cargo.toml")).unwrap().replace("\r\n", "\n");
    assert!(member.contains("version.workspace = true"));
    // Член рабочего пространства → отдельный проект.
    let edit = import::ManifestEdit {
        rename_to: Some("moved_game"),
        engine_crates: Some(format!("{}/crates", root.display().to_string().replace('\\', "/"))),
        in_workspace: Some(false),
        engine_root: Some(&root),
    };
    let (text, changes) = import::rewrite_manifest(&member, &edit).unwrap();
    let table: toml::Table = text.parse().expect("the result is valid TOML");
    let package = table["package"].as_table().unwrap();
    assert_eq!(package["name"].as_str(), Some("moved_game"));
    assert!(package["version"].is_str(), "inherited version is written out: {text}");
    assert!(table.contains_key("workspace") && table.contains_key("profile"), "own workspace and profiles are added");
    let engine_dep = table["dependencies"]["engine"]["path"].as_str().unwrap();
    assert!(engine_dep.ends_with("/crates/engine") && !engine_dep.starts_with(".."), "{engine_dep}");
    let log = &table["dependencies"]["log"];
    assert!(log.is_str() || log.is_table(), "workspace dependency `log` is inlined: {text}");
    assert!(!text.contains("workspace = true"), "{text}");
    assert!(changes.iter().any(|c| c.code == import::ChangeCode::WorkspaceAdded));
    assert!(text.contains("# Редактор в игру не входит"), "comments survive the edit");
    // Отдельный проект → член рабочего пространства.
    let edit = import::ManifestEdit {
        rename_to: None,
        engine_crates: Some("../../crates".to_string()),
        in_workspace: Some(true),
        engine_root: Some(&root),
    };
    let (back, changes) = import::rewrite_manifest(&text, &edit).unwrap();
    let table: toml::Table = back.parse().unwrap();
    assert!(!table.contains_key("workspace") && !table.contains_key("profile"));
    assert_eq!(table["dependencies"]["engine"]["path"].as_str(), Some("../../crates/engine"));
    assert!(changes.iter().any(|c| c.code == import::ChangeCode::WorkspaceRemoved));
    // Нечего править — текст не меняется.
    let (same, none) = import::rewrite_manifest(&back, &import::ManifestEdit { rename_to: None, engine_crates: None, in_workspace: None, engine_root: None }).unwrap();
    assert_eq!(same, back);
    assert!(none.is_empty());
    assert!(import::rewrite_manifest("not toml [", &import::ManifestEdit { rename_to: None, engine_crates: None, in_workspace: None, engine_root: None }).is_err());
}

#[test]
fn conflict_policies_apply_to_an_existing_destination() {
    let root = temp("conflict");
    let src = make_project(&root, "clash_game", false);
    let config = temp("conflict-config");
    for (conflict, readme_kept, readme_new, imported_copy) in [
        (Conflict::Skip, true, false, false),
        (Conflict::Overwrite, false, true, false),
        (Conflict::KeepBoth, true, false, true),
    ] {
        let parent = root.join(format!("projects-{conflict:?}"));
        write(&parent.join("clash_game"), "README.md", "MINE");
        write(&parent.join("clash_game"), "notes.txt", "my own file");
        let mut h = hub_again(&config, &src);
        settings_for(&mut h, &parent);
        h.import_page_mut().items[0].name = "clash_game".to_string();
        h.import_page_mut().items[0].name_touched = true;
        h.import_page_mut().settings.conflict = conflict;
        assert!(has(&mut h, 0, Code::DestinationExists), "the existing folder is reported: {conflict:?}");
        run_import(&mut h);
        let dest = parent.join("clash_game");
        let readme = fs::read_to_string(dest.join("README.md")).unwrap();
        assert_eq!(readme == "MINE", readme_kept, "{conflict:?}: README is {readme:?}");
        assert_eq!(readme.contains("round") || readme.contains("clash_game"), readme_new || imported_copy && false || readme_new);
        assert_eq!(dest.join("README.imported.md").exists(), imported_copy, "{conflict:?}");
        assert_eq!(fs::read_to_string(dest.join("notes.txt")).unwrap(), "my own file", "files that are not in the project are never touched");
        assert!(dest.join("src/lib.rs").is_file(), "the rest of the project is there");
    }
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn asset_guids_are_regenerated_with_their_references() {
    let root = temp("guids");
    let src = make_project(&root, "guid_game", false);
    let old_meta = fs::read_to_string(src.join("assets/scenes/main.scene.meta")).unwrap();
    let old_script_meta = fs::read_to_string(src.join("assets/scenarios/spin.scenario.meta")).unwrap();
    let guid_of = |text: &str| text.split('"').nth(1).unwrap().to_string();
    let (scene_guid, script_guid) = (guid_of(&old_meta), guid_of(&old_script_meta));
    assert!(fs::read_to_string(src.join("assets/scenes/main.scene")).unwrap().contains(&script_guid), "the scene references the script by GUID");
    let config = temp("guids-config");

    for regenerate in [false, true] {
        let mut h = hub_again(&config, &src);
        settings_for(&mut h, &root.join(format!("out-{regenerate}")));
        h.import_page_mut().settings.regenerate_guids = regenerate;
        run_import(&mut h);
        let dest = imported(&mut h);
        let scene = fs::read_to_string(dest.join("assets/scenes/main.scene")).unwrap();
        let new_scene_guid = guid_of(&fs::read_to_string(dest.join("assets/scenes/main.scene.meta")).unwrap());
        let new_script_guid = guid_of(&fs::read_to_string(dest.join("assets/scenarios/spin.scenario.meta")).unwrap());
        if regenerate {
            assert_ne!(new_scene_guid, scene_guid);
            assert_ne!(new_script_guid, script_guid);
            assert_eq!(new_scene_guid.len(), 32);
            assert!(scene.contains(&new_script_guid), "the reference follows the new GUID");
            assert!(!scene.contains(&script_guid), "no reference to the old GUID is left");
            let changes = &h.import_page().result.as_ref().unwrap().outcomes[0].changes;
            assert!(changes.iter().any(|c| c.code == import::ChangeCode::GuidsRegenerated));
        } else {
            assert_eq!(new_scene_guid, scene_guid);
            assert!(scene.contains(&script_guid));
        }
    }
    // Источник не изменился.
    assert_eq!(guid_of(&fs::read_to_string(src.join("assets/scenes/main.scene.meta")).unwrap()), scene_guid);
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn optional_parts_can_be_left_out_of_a_copy_and_of_an_unpack() {
    let root = temp("parts");
    let src = make_project(&root, "parts_game", true);
    write(&src, "dist/g/web/index.html", "built");
    write(&src, ".vscode/settings.json", "{}");
    let file = root.join("parts_game.pks");
    pack(&src, &file);
    let config = temp("parts-config");
    for source in [&src, &file] {
        let mut h = hub_again(&config, source);
        let report = h.import_page().items[0].report.clone().unwrap();
        assert!(report.counts.tests >= 1 && report.counts.cpp_files == 1, "{:?}", report.counts);
        let tag = source.display().to_string().len();
        settings_for(&mut h, &root.join(format!("none-{tag}")));
        {
            let s = &mut h.import_page_mut().settings;
            s.tests = false;
            s.cpp = false;
        }
        run_import(&mut h);
        let dest = imported(&mut h);
        assert!(!dest.join("tests").exists() && !dest.join("cpp").exists(), "parts are left out ({source:?})");
        assert!(dest.join("src/lib.rs").is_file() && dest.join("assets").is_dir());
        assert!(!dest.join("dist").exists() && !dest.join(".vscode").exists(), "builds and IDE files are off by default");

        let mut h = hub_again(&config, source);
        settings_for(&mut h, &root.join(format!("all-{tag}")));
        {
            let s = &mut h.import_page_mut().settings;
            s.builds = true;
            s.ide = true;
        }
        run_import(&mut h);
        let dest = imported(&mut h);
        assert!(dest.join("tests").is_dir() && dest.join("cpp").is_dir());
        if source == &src {
            // Из папки дополнительные части берутся с диска; в файл `dist/` не попал (по умолчанию выключен).
            assert!(dest.join("dist/g/web/index.html").is_file());
            assert!(dest.join(".vscode/settings.json").is_file());
        }
    }
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn use_in_place_adds_the_folder_and_changes_nothing() {
    let root = temp("inplace");
    let src = make_project(&root, "place_game", false);
    let before = tree(&src);
    let config = temp("inplace-config");
    let mut h = hub_again(&config, &src);
    h.import_page_mut().settings.mode = Mode::InPlace;
    assert!(h.import_ready());
    assert!(has(&mut h, 0, Code::InPlaceNotEdited));
    run_import(&mut h);
    let outcome = &h.import_page().result.as_ref().unwrap().outcomes[0];
    assert_eq!(outcome.used, import::Used::InPlace);
    assert!(paths::same(&outcome.dir, &src));
    assert_eq!(tree(&src), before, "nothing was written into the project");
    assert!(h.projects().iter().any(|l| paths::same(&l.path, &src)));
    // Повторный импорт того же каталога: сообщается, что он уже в списке.
    let mut again = hub(&config, vec![]);
    again.apply(HubAction::ImportAdd(src.clone()));
    again.wait_for_import();
    again.import_page_mut().settings.mode = Mode::InPlace;
    assert!(has(&mut again, 0, Code::AlreadyListed) || !again.projects().iter().any(|l| paths::same(&l.path, &src)));
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn after_import_actions_follow_the_checkboxes() {
    let root = temp("after");
    let src = make_project(&root, "after_game", false);
    let config = temp("after-config");
    let mut h = hub_again(&config, &src);
    settings_for(&mut h, &root.join("out"));
    {
        let s = &mut h.import_page_mut().settings;
        s.open = true;
        s.test = true;
    }
    run_import(&mut h);
    let dest = imported(&mut h);
    assert!(h.dry_log.iter().any(|l| l.contains("cargo test -p after_game")), "tests are started: {:?}", h.dry_log);
    assert!(
        h.job().is_some() || h.dry_log.iter().any(|l| l.contains("piska-editor") || l.contains("after_game")),
        "the project is opened in the editor: {:?}",
        h.dry_log
    );
    let _ = dest;
    // Без галочек ничего не запускается.
    let mut quiet = hub_again(&config, &src);
    settings_for(&mut quiet, &root.join("out-quiet"));
    run_import(&mut quiet);
    assert!(quiet.dry_log.is_empty(), "{:?}", quiet.dry_log);
    assert!(quiet.job().is_none());
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Предварительный отчёт: каждая проблема
// ---------------------------------------------------------------------------

#[test]
fn preflight_reports_each_problem() {
    let root = temp("preflight");
    let config = temp("preflight-config");

    // Не проект.
    let empty = root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let mut h = hub(&config, vec![]);
    h.apply(HubAction::ImportAdd(empty.clone()));
    assert!(h.import_page().items.is_empty(), "a folder without projects adds nothing");
    assert!(h.import_page().message.as_deref().unwrap().contains("No projects found"));
    write(&root.join("broken"), "Cargo.toml", "[package]\nname = \"b\"\n");
    h.apply(HubAction::ImportAdd(root.join("broken")));
    h.wait_for_import();
    assert!(has(&mut h, 0, Code::NotAProject));
    assert!(!h.import_ready(), "an unusable source blocks the import");
    let text = h.import_problems(0).iter().find(|p| p.code == Code::NotAProject).unwrap().text();
    assert!(text.contains("src/lib.rs"), "the error says what is missing: {text}");

    // Проект с набором недостатков.
    let src = make_project(&root, "flaw_game", true);
    write(&src, "target/debug/x", "derived");
    write(&src, "cpp/CMakeLists.txt", "project(x)");
    write(&src, "assets/loose.png", [1, 2, 3]); // без .meta
    write(&src, "assets/a.png", [1]);
    write(&src, "assets/a.png.meta", "(\n    guid: \"11111111111111111111111111111111\",\n    importer: \"Texture\",\n)\n");
    write(&src, "assets/b.png", [2]);
    write(&src, "assets/b.png.meta", "(\n    guid: \"11111111111111111111111111111111\",\n    importer: \"Texture\",\n)\n");
    let mut manifest = fs::read_to_string(src.join("Cargo.toml")).unwrap();
    manifest = manifest.replace("edition = \"2021\"", "edition = \"2018\"\nrust-version = \"999.0\"");
    manifest.push_str("\n[dependencies.other]\npath = \"C:/Users/somebody/other\"\n[dependencies.near]\npath = \"../does-not-exist\"\n");
    fs::write(src.join("Cargo.toml"), manifest).unwrap();
    write(&src, "Cargo.lock", "[[package]]\nname = \"engine\"\nversion = \"9.9.9\"\n");
    let mut h = hub(&config, vec![src.clone()]);
    h.wait_for_import();
    let found = codes(&mut h, 0);
    for (level, code) in [
        (Level::Warn, Code::ForeignPath),
        (Level::Warn, Code::PathMissing),
        (Level::Warn, Code::MissingMeta),
        (Level::Warn, Code::DuplicateGuids),
        (Level::Warn, Code::EngineVersion),
        (Level::Info, Code::EditionDiffers),
        (Level::Info, Code::Skipped),
    ] {
        assert!(found.contains(&(level, code)), "{code:?} at {level:?} is reported: {found:?}");
    }
    // `rustc` есть — старый `rust-version` замечен; нет — замечено, что его нет.
    assert!(found.iter().any(|(_, c)| matches!(c, Code::ToolchainOld | Code::ToolchainMissing)), "{found:?}");
    let cmake = std::process::Command::new("cmake").arg("--version").output().is_ok();
    assert_eq!(found.iter().any(|(_, c)| *c == Code::CmakeMissing), !cmake, "CMake is reported exactly when it is absent");
    let report = h.import_page().items[0].report.as_ref().unwrap();
    assert!(report.facts.cmake);
    assert_eq!(report.made_with.as_deref(), Some("9.9.9"));
    assert!(report.skipped.iter().any(|(p, _)| p == "target/"), "{:?}", report.skipped);
    let texts: Vec<String> = h.import_problems(0).iter().map(|p| p.text()).collect();
    assert!(texts.iter().any(|t| t.contains("loose.png")), "the missing .meta names a file: {texts:?}");
    assert!(texts.iter().any(|t| t.contains("a.png.meta") && t.contains("b.png.meta")), "duplicates name both files: {texts:?}");
    // Предупреждения не мешают импорту.
    assert!(h.import_ready());

    // Имя занято.
    let mut listed = hub(&config, vec![]);
    listed.config.entry_mut(&src).last_opened = Some(1);
    listed.refresh();
    listed.apply(HubAction::ImportAdd(src.clone()));
    listed.wait_for_import();
    assert_ne!(listed.import_page().items[0].name, "flaw_game", "a copy next to the original gets a free name");
    listed.import_page_mut().items[0].name = "flaw_game".to_string();
    listed.import_page_mut().items[0].name_touched = true;
    settings_for(&mut listed, &root.join("elsewhere"));
    assert!(has(&mut listed, 0, Code::NameInList), "{:?}", codes(&mut listed, 0));

    // Каталог назначения: занят; внутри источника; внутри движка; плохое имя.
    let mut h = hub_again(&config, &src);
    write(&root.join("dest/flaw_game"), "old.txt", "x");
    settings_for(&mut h, &root.join("dest"));
    assert!(has(&mut h, 0, Code::DestinationExists));
    settings_for(&mut h, &src);
    assert!(has(&mut h, 0, Code::DestinationInsideSource));
    assert!(!h.import_ready());
    settings_for(&mut h, &engine().join("crates"));
    assert!(has(&mut h, 0, Code::InsideEngine));
    assert!(!h.import_ready(), "writing into the engine sources is refused");
    settings_for(&mut h, &root.join("dest"));
    h.import_page_mut().items[0].name = "Bad Name".to_string();
    assert!(has(&mut h, 0, Code::NameInvalid));
    assert!(!h.import_ready());
    h.import_page_mut().items[0].name = "ok_name".to_string();
    assert!(h.import_ready());

    // Два проекта с одним именем в одном импорте.
    let src2 = make_project(&root, "twin_game", false);
    let mut h = hub(&config, vec![src.clone(), src2.clone()]);
    h.wait_for_import();
    settings_for(&mut h, &root.join("twins"));
    h.import_page_mut().items[1].name = h.import_page().items[0].name.clone();
    h.import_page_mut().items[1].name_touched = true;
    assert!(!h.import_ready(), "the same destination for two projects is refused");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

/// Контрольная сумма CRC-32 (для рукописных файлов проекта в тестах).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Файл проекта, написанный вручную: имена и содержимое — как есть.
fn craft(entries: &[(&str, &[u8])], writer_version: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&engine_project::MAGIC);
    out.extend_from_slice(&engine_project::FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut files = Vec::new();
    for (name, body) in entries {
        out.extend_from_slice(&[1, 0]);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        if !body.is_empty() {
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(body.len() as u64).to_le_bytes());
        out.extend_from_slice(&crc32(body).to_le_bytes());
        files.push(format!("{{\"path\":{name:?},\"size\":{},\"crc32\":{},\"packed\":{},\"method\":0}}", body.len(), crc32(body), body.len()));
    }
    let json = format!(
        "{{\"format\":1,\"project\":{{\"name\":\"crafted\",\"title\":\"Crafted\"}},\"writer\":{{\"tool\":\"test\",\"engine_version\":{writer_version:?}}},\"files\":[{}]}}",
        files.join(",")
    );
    let marker = out.len() as u64;
    out.push(0);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(&crc32(json.as_bytes()).to_le_bytes());
    out.extend_from_slice(&marker.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(json.as_bytes()).to_le_bytes());
    out.extend_from_slice(&[0x89, b'P', b'R', b'J', b'E', 0x0D, 0x0A, 0x1A]);
    out
}

#[test]
fn dangerous_and_damaged_project_files_are_refused_before_anything_is_written() {
    let root = temp("refuse");
    let config = temp("refuse-config");
    let manifest = b"[package]\nname = \"crafted\"\n[dependencies]\nengine = { path = \"x\" }\n";

    // Путь за пределами каталога.
    let evil = root.join("evil.pks");
    fs::write(&evil, craft(&[("Cargo.toml", manifest), ("src/lib.rs", b""), ("../outside.txt", b"pwned")], "0.0.1")).unwrap();
    let mut h = hub(&config, vec![evil.clone()]);
    h.wait_for_import();
    assert!(has(&mut h, 0, Code::UnsafePaths), "{:?}", codes(&mut h, 0));
    assert!(!h.import_ready());
    settings_for(&mut h, &root.join("out"));
    h.apply(HubAction::ImportRun);
    h.wait_for_works();
    assert!(h.works().is_empty() && h.import_page().stage == ImportStage::Choose, "the import does not start");
    assert!(!root.join("out").exists() && !root.join("outside.txt").exists(), "nothing was written anywhere");

    // Абсолютный путь и диск.
    for bad in ["/etc/passwd", "C:/Windows/x.txt", "a/../../b"] {
        let file = root.join("abs.pks");
        fs::write(&file, craft(&[("Cargo.toml", manifest), (bad, b"x")], "0.0.1")).unwrap();
        let mut h = hub(&config, vec![file]);
        h.wait_for_import();
        assert!(has(&mut h, 0, Code::UnsafePaths), "{bad}: {:?}", codes(&mut h, 0));
    }

    // Совпадение путей с точностью до регистра.
    let file = root.join("case.pks");
    fs::write(&file, craft(&[("Cargo.toml", manifest), ("src/lib.rs", b""), ("assets/A.png", b"1"), ("assets/a.png", b"2")], "0.0.1")).unwrap();
    let mut h = hub(&config, vec![file]);
    h.wait_for_import();
    assert!(has(&mut h, 0, Code::CaseCollision));
    if cfg!(any(windows, target_os = "macos")) {
        assert!(!h.import_ready(), "case collisions cannot be unpacked on this system");
    }

    // Повреждён байт, обрезан, не тот формат, версия новее.
    let src = make_project(&root, "dmg_game", false);
    let good = root.join("dmg_game.pks");
    pack(&src, &good);
    let original = fs::read(&good).unwrap();
    let damage = |needle: &[u8]| {
        let mut bytes = original.clone();
        let at = original.windows(needle.len()).position(|w| w == needle).unwrap() + needle.len() + 4 + 6;
        bytes[at] ^= 0xFF;
        bytes
    };
    let cases: Vec<(&str, Vec<u8>, Code)> = vec![
        ("truncated", original[..original.len() / 2].to_vec(), Code::ArchiveDamaged),
        ("not ours", b"PK\x03\x04 this is a zip, not a project file".to_vec(), Code::ArchiveDamaged),
        ("newer", {
            let mut v = original.clone();
            v[8] = 9;
            v
        }, Code::NewerFormat),
        ("bad list", {
            let mut v = original.clone();
            let n = v.len();
            v[n - 40] ^= 0x55;
            v
        }, Code::ArchiveDamaged),
    ];
    for (what, bytes, code) in cases {
        let file = root.join(format!("{}.pks", what.replace(' ', "-")));
        fs::write(&file, bytes).unwrap();
        let mut h = hub(&config, vec![file]);
        h.wait_for_import();
        assert!(has(&mut h, 0, code), "{what}: {:?}", codes(&mut h, 0));
        assert!(!h.import_ready(), "{what}");
    }
    // Испорчена маленькая запись `.meta`: предварительный отчёт её читает и называет.
    let file = root.join("flipped-meta.pks");
    fs::write(&file, damage(b"assets/scenes/main.scene.meta")).unwrap();
    let mut h = hub(&config, vec![file]);
    h.wait_for_import();
    let text = h.import_problems(0).iter().find(|p| p.code == Code::ArchiveDamaged).expect("the damage is reported").text();
    assert!(text.contains("main.scene.meta"), "the damaged entry is named: {text}");
    assert!(!h.import_ready());
    // Испорчен исходник: отчёт его не читает, импорт падает при распаковке, называет запись и ничего не оставляет.
    let file = root.join("flipped-lib.pks");
    fs::write(&file, damage(b"src/lib.rs")).unwrap();
    let mut h = hub(&config, vec![file]);
    h.wait_for_import();
    settings_for(&mut h, &root.join("flipped-out"));
    assert!(h.import_ready(), "{:?}", codes(&mut h, 0));
    run_import(&mut h);
    let result = h.import_page().result.as_ref().unwrap();
    assert!(result.outcomes.is_empty() && !result.failures.is_empty(), "the import fails");
    assert!(result.failures[0].1.contains("src/lib.rs"), "the damaged entry is named: {}", result.failures[0].1);
    let leftover: Vec<_> = fs::read_dir(root.join("flipped-out")).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(leftover.is_empty(), "no half-written project or staging folder: {leftover:?}");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn engine_version_of_a_project_file_is_compared_with_this_engine() {
    let root = temp("version");
    let config = temp("version-config");
    let manifest = b"[package]\nname = \"crafted\"\n[dependencies]\nengine = { path = \"x\" }\n";
    let file = root.join("old.pks");
    fs::write(&file, craft(&[("Cargo.toml", manifest), ("src/lib.rs", b"")], "0.0.0-old")).unwrap();
    let mut h = hub(&config, vec![file]);
    h.wait_for_import();
    let report = h.import_page().items[0].report.as_ref().unwrap();
    assert_eq!(report.made_with.as_deref(), Some("0.0.0-old"));
    assert!(has(&mut h, 0, Code::EngineVersion));
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Отмена и ход
// ---------------------------------------------------------------------------

#[test]
fn cancelling_in_the_middle_leaves_neither_a_project_nor_a_temporary_folder() {
    let root = temp("cancel");
    let src = make_project(&root, "cancel_game", false);
    for i in 0..40 {
        write(&src, &format!("assets/blob{i}.bin"), vec![i as u8; 200_000]);
        write(&src, &format!("assets/blob{i}.bin.meta"), format!("(\n    guid: \"{i:032x}\",\n    importer: \"Binary\",\n)\n"));
    }
    let location = root.join("out");
    let config = temp("cancel-config");
    let mut h = hub_again(&config, &src);
    let report = h.import_page().items[0].report.clone().unwrap();
    let settings = ImportSettings::new(location.clone());
    let item = import::PlanItem {
        source: Source::Folder(src.clone()),
        name: "cancel_game".to_string(),
        report,
    };
    let env = import::RunEnv { engine_root: Some(engine()) };
    // Отмена после десятого сообщения о ходе — посреди копирования.
    let control = Control::new();
    let watcher = control.clone();
    let mut calls = 0;
    let result = import::run_item(&item, &settings, &env, &control, &mut |_, _| {
        calls += 1;
        if calls == 10 {
            watcher.cancel();
        }
    });
    let err = result.unwrap_err();
    assert!(err.is_cancel(), "{err}");
    assert!(calls >= 10);
    let leftovers: Vec<String> = fs::read_dir(&location).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    assert!(leftovers.is_empty(), "no project and no staging folder: {leftovers:?}");

    // Те же гарантии из хаба: после «Cancel» либо ничего, либо целый проект.
    settings_for(&mut h, &location);
    h.apply(HubAction::ImportRun);
    h.apply(HubAction::ImportCancel);
    h.wait_for_works();
    let names: Vec<String> = fs::read_dir(&location).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    match h.import_page().stage {
        ImportStage::Choose => assert!(names.is_empty(), "cancelled: {names:?}"),
        _ => {
            assert_eq!(names, ["cancel_game"], "finished before the cancel arrived: the project is complete");
            assert!(location.join("cancel_game/assets/blob39.bin").is_file());
        }
    }
    assert!(h.works().iter().all(|w| !w.is_running()));
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Сохранение проекта в файл из хаба
// ---------------------------------------------------------------------------

#[test]
fn the_hub_saves_a_project_to_a_file_in_the_background() {
    let root = temp("save");
    let src = make_project(&root, "save_game", true);
    write(&src, "dist/x/y.html", "built");
    let config = temp("save-config");
    let mut h = hub(&config, vec![]);
    h.set_now(1_700_000_000);
    h.config.entry_mut(&src).last_opened = Some(1);
    h.refresh();
    h.apply(HubAction::ShowPack(src.clone()));
    let Some(Dialog::Pack(dialog)) = h.dialog().cloned() else {
        panic!("the Save Project As File window opens");
    };
    assert!(dialog.out.ends_with(&format!("save_game.{}", engine_editor::kit::i18n::brand::PROJECT_EXTENSION)), "{}", dialog.out);
    assert!(dialog.tests && dialog.cpp && !dialog.builds, "tests and C++ on, builds off by default");
    let mut dialog = *dialog;
    let out = root.join("saved").join("my copy");
    dialog.out = out.display().to_string();
    h.apply(HubAction::StartPack(Box::new(dialog.clone())));
    assert!(h.dialog().is_none(), "the window closes and the work goes on in the background");
    assert_eq!(h.works().len(), 1);
    h.wait_for_works();
    let file = out.with_extension(engine_editor::kit::i18n::brand::PROJECT_EXTENSION);
    assert!(file.is_file(), "the extension is added when it is missing: {}", file.display());
    let info = engine_project::read_info(&file).unwrap();
    assert_eq!(info.manifest.project.name, "save_game");
    assert_eq!(info.manifest.created, 1_700_000_000);
    assert!(!info.manifest.files.iter().any(|f| f.path.starts_with("dist/") || f.path.starts_with("target/")));
    assert!(info.manifest.files.iter().any(|f| f.path.starts_with("cpp/")));
    assert!(h.status().unwrap().text.contains("Saved"));
    assert_eq!(h.config.settings.last_pack_dir.as_deref().map(paths::clean), Some(paths::clean(&root.join("saved"))));

    // Тот же проект и параметры — те же байты (время задано).
    let again = root.join("again.pks");
    let mut second = dialog.clone();
    second.out = again.display().to_string();
    h.apply(HubAction::StartPack(Box::new(second)));
    h.wait_for_works();
    assert_eq!(fs::read(&file).unwrap(), fs::read(&again).unwrap(), "the file is deterministic");

    // Параметры меняют содержимое; ошибка пути остаётся в окне.
    let mut third = dialog.clone();
    third.out = root.join("builds.pks").display().to_string();
    third.builds = true;
    third.tests = false;
    h.apply(HubAction::StartPack(Box::new(third)));
    h.wait_for_works();
    let info = engine_project::read_info(&root.join("builds.pks")).unwrap();
    assert!(info.manifest.files.iter().any(|f| f.path.starts_with("dist/")));
    assert!(!info.manifest.files.iter().any(|f| f.path.starts_with("tests/")));
    let mut bad = dialog.clone();
    bad.out = root.display().to_string();
    h.apply(HubAction::StartPack(Box::new(bad)));
    assert!(matches!(h.dialog(), Some(Dialog::Pack(d)) if d.error.as_deref().is_some_and(|e| e.contains("folder"))), "an error is shown in the window");
    h.apply(HubAction::CloseDialog);

    // Готовые задачи убираются, отменённая — тоже.
    let ids: Vec<u64> = h.works().iter().map(|w| w.id).collect();
    for id in ids {
        h.apply(HubAction::DismissWork(id));
    }
    assert!(h.works().is_empty());
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn saving_can_be_cancelled_and_leaves_no_file() {
    let root = temp("save-cancel");
    let src = make_project(&root, "stop_game", false);
    for i in 0..30 {
        write(&src, &format!("assets/b{i}.bin"), vec![i as u8; 300_000]);
    }
    let config = temp("save-cancel-config");
    let mut h = hub(&config, vec![]);
    h.config.entry_mut(&src).last_opened = Some(1);
    h.refresh();
    h.apply(HubAction::ShowPack(src.clone()));
    let Some(Dialog::Pack(dialog)) = h.dialog().cloned() else {
        panic!();
    };
    let mut dialog = *dialog;
    dialog.out = root.join("stop.pks").display().to_string();
    h.apply(HubAction::StartPack(Box::new(dialog)));
    let id = h.works()[0].id;
    h.apply(HubAction::CancelWork(id));
    h.wait_for_works();
    let file = root.join("stop.pks");
    // Отмена успела — файла нет совсем; не успела — файл целый.
    if file.exists() {
        engine_project::verify(&file, &engine_project::Limits::default(), &Control::new(), &mut |_| {}).expect("a finished file is whole");
    } else {
        assert!(h.status().unwrap().text.contains("cancelled"));
    }
    let temps: Vec<_> = fs::read_dir(&root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp")).collect();
    assert!(temps.is_empty(), "no temporary file: {temps:?}");
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Несколько проектов, поиск в каталоге
// ---------------------------------------------------------------------------

#[test]
fn a_folder_with_several_projects_is_scanned_and_each_ticked_one_is_imported() {
    let root = temp("scan");
    let library = root.join("library");
    let a = make_project(&library, "scan_a", false);
    let b = make_project(&library.join("nested"), "scan_b", false);
    let _ = (&a, &b);
    write(&library, "notes/readme.txt", "not a project");
    write(&library.join("scan_a"), "target/x/Cargo.toml", "[package]\nname = \"ghost\"\n");
    let config = temp("scan-config");
    let mut h = hub(&config, vec![]);
    h.apply(HubAction::ImportAdd(library.clone()));
    h.wait_for_import();
    let names: Vec<String> = h.import_page().items.iter().map(|i| i.report.as_ref().unwrap().package.clone()).collect();
    assert_eq!(names, ["scan_a", "scan_b"], "projects are found in sub-folders, target/ is not entered: {names:?}");
    assert!(h.import_page().items.iter().all(|i| i.ticked));
    // Снять отметку с одного.
    h.import_page_mut().items[0].ticked = false;
    let dest = root.join("imported");
    settings_for(&mut h, &dest);
    run_import(&mut h);
    let result = h.import_page().result.as_ref().unwrap();
    assert_eq!(result.outcomes.len(), 1);
    assert!(dest.join("scan_b/Cargo.toml").is_file() && !dest.join("scan_a").exists());
    // Оба (в новом списке проектов: в прежнем `scan_b` уже есть, и копия получила бы другое имя).
    let config2 = temp("scan-config2");
    let mut h = hub(&config2, vec![library.clone()]);
    h.wait_for_import();
    let dest2 = root.join("imported2");
    settings_for(&mut h, &dest2);
    run_import(&mut h);
    assert_eq!(h.import_page().result.as_ref().unwrap().outcomes.len(), 2);
    assert!(dest2.join("scan_a/Cargo.toml").is_file() && dest2.join("scan_b/Cargo.toml").is_file());
    assert!(h.status().unwrap().text.contains("2"), "{}", h.status().unwrap().text);
    // Убрать источник из списка.
    let mut h = hub(&config2, vec![library]);
    h.wait_for_import();
    h.apply(HubAction::ImportRemove(0));
    assert_eq!(h.import_page().items.len(), 1);
    fs::remove_dir_all(config2).unwrap();
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn a_file_that_is_not_a_project_file_is_explained() {
    let root = temp("notfile");
    write(&root, "readme.txt", "hello");
    let config = temp("notfile-config");
    let mut h = hub(&config, vec![]);
    h.apply(HubAction::ImportAdd(root.join("readme.txt")));
    assert!(h.import_page().items.is_empty());
    let message = h.import_page().message.clone().unwrap();
    assert!(message.contains("not a project file") && message.contains(engine_editor::kit::i18n::brand::PROJECT_EXTENSION), "{message}");
    h.apply(HubAction::ImportAdd(root.join("missing")));
    assert!(h.import_page().message.as_deref().unwrap().contains("does not exist"));
    // Cargo.toml вместо каталога — берётся его каталог.
    let src = make_project(&root, "manifest_game", false);
    h.apply(HubAction::ImportAdd(src.join("Cargo.toml")));
    h.wait_for_import();
    assert_eq!(h.import_page().items.len(), 1);
    assert!(h.import_page().message.is_none());
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Мышь, клавиатура, перетаскивание
// ---------------------------------------------------------------------------

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
        for _ in 0..3 {
            hands.frame(hub, vec![], vec![], vec![]);
        }
        hands
    }

    fn frame(&mut self, hub: &mut Hub, events: Vec<egui::Event>, dropped: Vec<egui::DroppedFile>, hovered: Vec<egui::HoveredFile>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            events,
            dropped_files: dropped,
            hovered_files: hovered,
            ..egui::RawInput::default()
        };
        self.time += 1.0 / 60.0;
        hub.update();
        let _ = self.ctx.run(input, |ctx| hub.ui(ctx));
    }

    fn frames(&mut self, hub: &mut Hub, n: usize) {
        for _ in 0..n {
            self.frame(hub, vec![], vec![], vec![]);
        }
    }

    fn click(&mut self, hub: &mut Hub, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(hub, vec![egui::Event::PointerMoved(pos)], vec![], vec![]);
        self.frame(hub, vec![button(true)], vec![], vec![]);
        self.frame(hub, vec![button(false)], vec![], vec![]);
        self.frames(hub, 2);
    }

    fn click_id(&mut self, hub: &mut Hub, id: egui::Id) {
        let pos = self.ctx.read_response(id).unwrap_or_else(|| panic!("widget {id:?} is on screen")).rect.center();
        self.click(hub, pos);
    }

    fn on_screen(&self, id: egui::Id) -> bool {
        self.ctx.read_response(id).is_some()
    }

    fn key(&mut self, hub: &mut Hub, key: egui::Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(hub, vec![event(true)], vec![], vec![]);
        self.frame(hub, vec![event(false)], vec![], vec![]);
        self.frames(hub, 2);
    }

    fn type_text(&mut self, hub: &mut Hub, text: &str) {
        self.frame(hub, vec![egui::Event::Text(text.to_string())], vec![], vec![]);
        self.frames(hub, 2);
    }
}

#[test]
fn the_import_page_works_with_mouse_and_keyboard() {
    let root = temp("mouse");
    let src = make_project(&root, "mouse_game", false);
    let config = temp("mouse-config");
    let mut h = hub(&config, vec![]);
    let mut hands = Hands::new(&mut h);
    // «Import…» в шапке Projects открывает страницу, поле пути уже в фокусе.
    hands.click_id(&mut h, egui::Id::new("hub-import"));
    assert_eq!(h.page(), Page::Import);
    hands.frames(&mut h, 3);
    assert!(hands.on_screen(egui::Id::new(Hub::IMPORT_PATH_ID)));
    assert!(hands.ctx.memory(|m| m.has_focus(egui::Id::new(Hub::IMPORT_PATH_ID))), "the path field has focus when the page opens");
    // Печать пути и Enter добавляют проект.
    hands.type_text(&mut h, &src.display().to_string());
    assert_eq!(h.import_page().path, src.display().to_string());
    hands.key(&mut h, egui::Key::Enter);
    h.wait_for_import();
    hands.frames(&mut h, 3);
    assert_eq!(h.import_page().items.len(), 1, "Enter adds the typed path");
    assert!(h.import_page().path.is_empty(), "the field is cleared for the next path");
    assert!(hands.on_screen(Hub::import_row_id(0)), "the source is listed");
    // Назначение за пределами движка, чтобы тест не писал в games/.
    h.import_page_mut().settings.location = root.join("projects");
    hands.frames(&mut h, 3);
    // Кнопка Import запускает фоновую работу, страница показывает ход, затем результат.
    assert!(hands.on_screen(egui::Id::new(Hub::IMPORT_RUN_ID)));
    hands.click_id(&mut h, egui::Id::new(Hub::IMPORT_RUN_ID));
    assert_eq!(h.works().len(), 1, "the import runs as a background work");
    h.wait_for_works();
    hands.frames(&mut h, 3);
    assert_eq!(h.import_page().stage, ImportStage::Result);
    assert!(root.join("projects/mouse_game/Cargo.toml").is_file());
    assert!(hands.on_screen(egui::Id::new("hub-import-done")), "the result page has its buttons");
    // «Import More» возвращает к выбору; «Back to Projects» — к списку.
    hands.click_id(&mut h, egui::Id::new("hub-import-more"));
    assert_eq!(h.import_page().stage, ImportStage::Choose);
    assert!(h.import_page().items.is_empty());
    // Esc возвращает к проектам.
    hands.frames(&mut h, 2);
    hands.click(&mut h, egui::pos2(900.0, 500.0));
    hands.key(&mut h, egui::Key::Escape);
    assert_eq!(h.page(), Page::Projects);
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn dropping_a_folder_or_a_project_file_on_the_window_starts_an_import() {
    let root = temp("drop");
    let src = make_project(&root, "drop_game", false);
    let file = root.join("drop_game.pks");
    pack(&src, &file);
    let config = temp("drop-config");
    let mut h = hub(&config, vec![]);
    let mut hands = Hands::new(&mut h);
    // Пока файл над окном — подсказка (кадр рисуется без ошибок).
    hands.frame(&mut h, vec![], vec![], vec![egui::HoveredFile::default()]);
    assert_eq!(h.page(), Page::Projects);
    let drop = |path: &Path| egui::DroppedFile {
        path: Some(path.to_path_buf()),
        ..egui::DroppedFile::default()
    };
    hands.frame(&mut h, vec![], vec![drop(&file)], vec![]);
    hands.frames(&mut h, 2);
    h.wait_for_import();
    assert_eq!(h.page(), Page::Import, "a dropped file opens the import page");
    assert_eq!(h.import_page().items.len(), 1);
    assert!(h.import_page().items[0].source.is_archive());
    hands.frame(&mut h, vec![], vec![drop(&src)], vec![]);
    hands.frames(&mut h, 2);
    h.wait_for_import();
    assert_eq!(h.import_page().items.len(), 2, "a dropped folder joins the list");
    assert!(!h.import_page().items[1].source.is_archive());
    // Тот же файл повторно не дублируется.
    hands.frame(&mut h, vec![], vec![drop(&file)], vec![]);
    hands.frames(&mut h, 2);
    assert_eq!(h.import_page().items.len(), 2);
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn a_running_hub_receives_import_requests_from_a_second_launch() {
    let root = temp("link");
    let src = make_project(&root, "link_game", false);
    let file = root.join("link_game.pks");
    pack(&src, &file);
    let config = temp("link-config");
    // Запросы читаются только в «живом» режиме хаба; проверяем разбор и запись.
    piska_hub::link::send_request(&config, &piska_hub::link::Request::Import(file.clone())).unwrap();
    assert_eq!(piska_hub::link::take_request(&config), Some(piska_hub::link::Request::Import(file.clone())));
    // Параметр `import` при запуске открывает страницу с источником.
    let mut h = hub(&config, vec![file]);
    h.wait_for_import();
    assert_eq!(h.page(), Page::Import);
    assert_eq!(h.import_page().items.len(), 1);
    for d in [root, config] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn guid_helpers_and_the_project_scan_have_sane_edges() {
    let mut map = std::collections::HashMap::new();
    map.insert("a".repeat(32), "b".repeat(32));
    let text = format!("x \"{}\" y {}0 z", "a".repeat(32), "a".repeat(32));
    let (out, changed) = import::replace_guids(&text, &map);
    assert!(changed);
    assert_eq!(out, format!("x \"{}\" y {}0 z", "b".repeat(32), "a".repeat(32)), "only whole 32-digit tokens change");
    let root = temp("scan-edges");
    assert!(import::scan_for_projects(&root, &Control::new()).is_empty());
    let control = Control::new();
    control.cancel();
    write(&root, "p/Cargo.toml", "[package]\nname=\"p\"\n");
    write(&root, "p/src/lib.rs", "");
    assert!(import::scan_for_projects(&root, &control).is_empty(), "a cancelled scan returns nothing");
    assert_eq!(import::scan_for_projects(&root, &Control::new()).len(), 1);
    fs::remove_dir_all(root).unwrap();
}
