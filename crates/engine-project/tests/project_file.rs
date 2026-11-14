//! Файл проекта: круг «упаковать → прочитать → распаковать», что входит и что нет,
//! детерминизм, отказ на опасных и повреждённых файлах, отмена, политика конфликтов.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use engine_project::*;
use flate2::Crc;

fn temp(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("engine-project-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, rel: &str, body: impl AsRef<[u8]>) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Небольшой проект со всем, что должно и не должно попасть в файл.
fn sample_project(root: &Path) {
    write(root, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n[package.metadata.piska]\ntitle = \"Demo Game\"\n");
    write(root, "Cargo.lock", "# lock\n");
    write(root, "build.rs", "fn main() {}\n");
    write(root, "README.md", "# demo\n");
    write(root, "LICENSE-MIT", "MIT\n");
    write(root, "src/lib.rs", "pub fn build() {}\n".repeat(200));
    write(root, "src/scripts/mod.rs", "// mod\n");
    write(root, "cpp/Wobble.cpp", "// cpp\n");
    write(root, "tests/smoke.rs", "#[test] fn t() {}\n");
    write(root, "tools/gen.py", "print(1)\n");
    write(root, "examples/a/main.rs", "fn main() {}\n");
    write(root, "assets/project.settings", "(window: ())\n");
    write(root, "assets/icon.png", [137u8, 80, 78, 71, 1, 2, 3]);
    write(root, "assets/icon.png.meta", "(guid: \"aaaa\", importer: \"Texture\")\n");
    write(root, "assets/scenes/main.scene", "(objects: [])\n");
    write(root, "assets/scenes/main.scene.meta", "(guid: \"bbbb\", importer: \"Scene\")\n");
    write(root, "assets/кириллица/файл с пробелом.txt", "привет\n");
    // Производное и мусор.
    write(root, "target/debug/demo.exe", "binary");
    write(root, "dist/demo/web/index.html", "<html>");
    write(root, ".git/HEAD", "ref: refs/heads/main\n");
    write(root, "cpp/build/CMakeCache.txt", "cache");
    write(root, "plugins/p/build/x.o", "obj");
    write(root, "assets/Thumbs.db", "junk");
    write(root, "src/lib.rs.swp", "junk");
    write(root, "assets/old.tmp", "junk");
    write(root, ".vscode/settings.json", "{}");
    write(root, ".gitignore", "/secret.txt\n*.log\n/assets/ignored/\n");
    write(root, "secret.txt", "do not pack");
    write(root, "run.log", "log");
    write(root, "assets/ignored/a.png", "x");
}

fn tree(root: &Path) -> BTreeMap<String, (u64, u32)> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, (u64, u32)>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = format!("{prefix}{name}");
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{rel}/"), out);
            } else {
                let bytes = fs::read(entry.path()).unwrap();
                let mut crc = Crc::new();
                crc.update(&bytes);
                out.insert(rel, (bytes.len() as u64, crc.sum()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, "", &mut out);
    out
}

fn quiet(_: &Progress) {}

fn pack_default(project: &Path, out: &Path) -> PackSummary {
    pack(project, out, &PackOptions::default(), &Control::default(), &mut quiet).unwrap()
}

#[test]
fn pack_unpack_round_trip_keeps_exactly_the_source_files() {
    let project = temp("rt-src");
    sample_project(&project);
    let out_dir = temp("rt-out");
    let file = out_dir.join("demo.pks");
    let summary = pack_default(&project, &file);

    let info = read_info(&file).unwrap();
    assert_eq!(info.version, FORMAT_VERSION);
    assert_eq!(info.manifest.project.name, "demo");
    assert_eq!(info.manifest.project.title, "Demo Game");
    assert_eq!(info.manifest.totals.files, summary.files);
    assert_eq!(info.file_size, summary.archive_size);
    let listed: Vec<&str> = info.manifest.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(listed, sorted, "entries are sorted");
    for expected in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "README.md",
        "LICENSE-MIT",
        "src/lib.rs",
        "src/scripts/mod.rs",
        "cpp/Wobble.cpp",
        "tests/smoke.rs",
        "tools/gen.py",
        "examples/a/main.rs",
        "assets/project.settings",
        "assets/icon.png",
        "assets/icon.png.meta",
        "assets/scenes/main.scene.meta",
        "assets/кириллица/файл с пробелом.txt",
        ".gitignore",
    ] {
        assert!(listed.contains(&expected), "{expected} is packed; got {listed:?}");
    }
    for unwanted in [
        "target/debug/demo.exe",
        "dist/demo/web/index.html",
        ".git/HEAD",
        "cpp/build/CMakeCache.txt",
        "plugins/p/build/x.o",
        "assets/Thumbs.db",
        "src/lib.rs.swp",
        "assets/old.tmp",
        ".vscode/settings.json",
        "secret.txt",
        "run.log",
        "assets/ignored/a.png",
    ] {
        assert!(!listed.contains(&unwanted), "{unwanted} must not be packed");
    }
    for part in ["assets", "tests", "cpp", "tools", "examples", "readme", "license", "icon", "settings"] {
        assert!(info.manifest.parts.iter().any(|p| p == part), "part {part}: {:?}", info.manifest.parts);
    }
    assert!(!info.manifest.parts.iter().any(|p| p == "builds"));
    assert!(summary.skipped.iter().any(|s| s.rel == "target" && s.reason == SkipReason::Derived));
    assert!(summary.skipped.iter().any(|s| s.rel == "secret.txt" && s.reason == SkipReason::Gitignore));
    assert!(summary.skipped.iter().any(|s| s.rel == "dist" && s.reason == SkipReason::Option("builds")));

    // Проверка целиком и распаковка дают то же дерево, что исходник (без пропущенного).
    verify(&file, &Limits::default(), &Control::default(), &mut quiet).unwrap();
    let restored = temp("rt-dst");
    let unpacked = unpack(&file, &restored, &UnpackOptions::default(), &Control::default(), &mut quiet).unwrap();
    assert_eq!(unpacked.written.len() as u64, summary.files);
    let want: BTreeMap<_, _> = tree(&project).into_iter().filter(|(k, _)| listed.contains(&k.as_str())).collect();
    assert_eq!(tree(&restored), want, "file tree, sizes and checksums survive the round trip");
    assert_eq!(fs::read_to_string(restored.join("assets/кириллица/файл с пробелом.txt")).unwrap(), "привет\n");
    for (record, (size, crc)) in info.manifest.files.iter().zip(want.values()) {
        assert_eq!((record.size, record.crc32), (*size, *crc), "{}", record.path);
    }

    // Нет временных файлов рядом.
    let leftovers: Vec<_> = fs::read_dir(&out_dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    for d in [project, out_dir, restored] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn options_turn_optional_parts_on_and_off() {
    let project = temp("opt-src");
    sample_project(&project);
    let out = temp("opt-out");
    let run = |collect: CollectOptions| -> Vec<String> {
        let options = PackOptions {
            collect,
            ..PackOptions::default()
        };
        let file = out.join("x.pks");
        pack(&project, &file, &options, &Control::default(), &mut quiet).unwrap();
        read_info(&file).unwrap().manifest.files.into_iter().map(|f| f.path).collect()
    };
    let no_tests = run(CollectOptions {
        tests: false,
        ..CollectOptions::default()
    });
    assert!(!no_tests.iter().any(|p| p.starts_with("tests/")));
    assert!(no_tests.iter().any(|p| p.starts_with("cpp/")));
    let no_cpp = run(CollectOptions {
        cpp: false,
        ..CollectOptions::default()
    });
    assert!(!no_cpp.iter().any(|p| p.starts_with("cpp/")));
    let with_builds = run(CollectOptions {
        builds: true,
        ..CollectOptions::default()
    });
    assert!(with_builds.iter().any(|p| p == "dist/demo/web/index.html"));
    assert!(!with_builds.iter().any(|p| p.starts_with("target/")), "builds never bring target/");
    let with_ide = run(CollectOptions {
        ide: true,
        ..CollectOptions::default()
    });
    assert!(with_ide.iter().any(|p| p == ".vscode/settings.json"));
    let no_ignore = run(CollectOptions {
        gitignore: false,
        ..CollectOptions::default()
    });
    assert!(no_ignore.iter().any(|p| p == "secret.txt"), "without .gitignore the files come");
    assert!(!no_ignore.iter().any(|p| p == ".git/HEAD"), "the built-in deny list stays");
    // C++ выключен, а build.rs его собирает — замечание.
    write(&project, "build.rs", "fn main() { engine_cpp_build::compile(\"cpp\"); }\n");
    let options = PackOptions {
        collect: CollectOptions {
            cpp: false,
            ..CollectOptions::default()
        },
        ..PackOptions::default()
    };
    let summary = pack(&project, &out.join("y.pks"), &options, &Control::default(), &mut quiet).unwrap();
    assert!(summary.notes.iter().any(|n| n.contains("build.rs")), "{:?}", summary.notes);
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn the_archive_is_deterministic_and_does_not_pack_itself() {
    let project = temp("det-src");
    sample_project(&project);
    let out = temp("det-out");
    let (a, b) = (out.join("a.pks"), out.join("b.pks"));
    pack_default(&project, &a);
    // Другой порядок создания файлов и время не влияют.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(&project, "assets/scenes/main.scene", "(objects: [])\n");
    pack_default(&project, &b);
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap(), "same project, same bytes");
    // Время создания записывается только по просьбе.
    let stamped = PackOptions {
        created: Some(1_700_000_000),
        ..PackOptions::default()
    };
    pack(&project, &out.join("c.pks"), &stamped, &Control::default(), &mut quiet).unwrap();
    assert_eq!(read_info(&out.join("c.pks")).unwrap().manifest.created, 1_700_000_000);
    assert_eq!(read_info(&a).unwrap().manifest.created, 0);
    // Файл проекта внутри самого проекта не упаковывает себя.
    let inside = project.join("self.pks");
    pack_default(&project, &inside);
    assert!(!read_info(&inside).unwrap().manifest.files.iter().any(|f| f.path == "self.pks"));
    pack_default(&project, &inside);
    assert!(!read_info(&inside).unwrap().manifest.files.iter().any(|f| f.path == "self.pks"));
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn symlinks_are_not_followed_out_of_the_project() {
    let project = temp("link-src");
    let outside = temp("link-outside");
    write(&project, "Cargo.toml", "[package]\nname = \"l\"\n");
    write(&outside, "secret.txt", "top secret");
    let dir_link = project.join("assets_link");
    let file_link = project.join("link.txt");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&outside, &dir_link).is_ok() && std::os::windows::fs::symlink_file(outside.join("secret.txt"), &file_link).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &dir_link).is_ok() && std::os::unix::fs::symlink(outside.join("secret.txt"), &file_link).is_ok();
    if !made {
        eprintln!("symlinks cannot be created here (privileges); skipping the link part of the test");
    } else {
        let collected = collect(&project, &CollectOptions::default(), &Control::default()).unwrap();
        let names: Vec<&str> = collected.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, ["Cargo.toml"]);
        assert!(collected.skipped.iter().any(|s| s.reason == SkipReason::Link && s.rel == "assets_link"));
        assert!(collected.skipped.iter().any(|s| s.reason == SkipReason::Link && s.rel == "link.txt"));
    }
    // Своё дерево вне проекта тоже не тронуто.
    assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "top secret");
    for d in [project, outside] {
        let _ = fs::remove_dir_all(d);
    }
}

#[test]
fn a_folder_without_cargo_toml_is_refused() {
    let dir = temp("noproject");
    let err = pack(&dir, &dir.join("x.pks"), &PackOptions::default(), &Control::default(), &mut quiet).unwrap_err();
    assert!(matches!(err, Error::NoProject(_)), "{err}");
    assert!(!dir.join("x.pks").exists());
    fs::remove_dir_all(dir).unwrap();
}

// ---------------------------------------------------------------------------
// Свой писатель: файлы, которые наш `pack` не создал бы (опасные, «бомбы»).
// ---------------------------------------------------------------------------

struct Crafted {
    name: String,
    /// Данные записи как они лежат в файле (после сжатия).
    stored: Vec<u8>,
    method: u8,
    /// Что записать в трейлер записи и в список.
    size: u64,
    crc: u32,
}

fn crafted_plain(name: &str, body: &[u8]) -> Crafted {
    let mut crc = Crc::new();
    crc.update(body);
    Crafted {
        name: name.to_string(),
        stored: body.to_vec(),
        method: 0,
        size: body.len() as u64,
        crc: crc.sum(),
    }
}

fn craft(entries: &[Crafted]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut files = Vec::new();
    for e in entries {
        out.extend_from_slice(&[1, e.method]);
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(e.name.as_bytes());
        for chunk in e.stored.chunks(1000) {
            out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&e.size.to_le_bytes());
        out.extend_from_slice(&e.crc.to_le_bytes());
        files.push(FileRecord {
            path: e.name.clone(),
            size: e.size,
            crc32: e.crc,
            packed: e.stored.len() as u64,
            method: e.method,
        });
    }
    let manifest = Manifest {
        format: FORMAT_VERSION,
        files,
        ..Manifest::default()
    };
    let json = serde_json_bytes(&manifest);
    let mut crc = Crc::new();
    crc.update(&json);
    let marker = out.len() as u64;
    out.push(0);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&marker.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&[0x89, b'P', b'R', b'J', b'E', 0x0D, 0x0A, 0x1A]);
    out
}

fn serde_json_bytes(manifest: &Manifest) -> Vec<u8> {
    // Тот же JSON, что пишет `pack`, — без зависимости теста от serde_json: вручную.
    let files: Vec<String> = manifest
        .files
        .iter()
        .map(|f| {
            format!(
                "{{\"path\":{:?},\"size\":{},\"crc32\":{},\"packed\":{},\"method\":{}}}",
                f.path, f.size, f.crc32, f.packed, f.method
            )
        })
        .collect();
    format!("{{\"format\":{},\"files\":[{}]}}", FORMAT_VERSION, files.join(",")).into_bytes()
}

fn unpack_bytes(bytes: &[u8], limits: Limits) -> (Result<UnpackSummary>, PathBuf) {
    let dir = temp("craft");
    let file = dir.join("x.pks");
    fs::write(&file, bytes).unwrap();
    let dest = dir.join("out");
    let options = UnpackOptions {
        limits,
        ..UnpackOptions::default()
    };
    (unpack(&file, &dest, &options, &Control::default(), &mut quiet), dir)
}

#[test]
fn a_crafted_valid_file_unpacks_so_the_helper_is_trustworthy() {
    let bytes = craft(&[crafted_plain("a/b.txt", b"hello"), crafted_plain("c.txt", b"world")]);
    let (result, dir) = unpack_bytes(&bytes, Limits::default());
    let summary = result.unwrap();
    assert_eq!(summary.written, ["a/b.txt", "c.txt"]);
    assert_eq!(fs::read(dir.join("out/a/b.txt")).unwrap(), b"hello");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn path_traversal_and_absolute_paths_are_refused_before_anything_is_written() {
    for bad in ["../evil.txt", "a/../../evil.txt", "/etc/evil", "C:/evil.txt", "C:\\evil.txt", "a\\..\\evil", "./a", "a//b", "con.txt", "x/nul"] {
        let bytes = craft(&[crafted_plain("ok.txt", b"fine"), crafted_plain(bad, b"evil")]);
        let (result, dir) = unpack_bytes(&bytes, Limits::default());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::UnsafePath { .. }), "{bad}: {err}");
        assert!(err.to_string().contains(&format!("`{bad}`")), "{err}");
        assert!(!dir.join("out/ok.txt").exists(), "{bad}: the check happens before the first write");
        assert!(!dir.join("evil.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn case_collisions_and_file_folder_clashes_are_refused() {
    let bytes = craft(&[crafted_plain("Assets/A.png", b"1"), crafted_plain("assets/a.png", b"2")]);
    let (result, dir) = unpack_bytes(&bytes, Limits::default());
    assert!(matches!(result.unwrap_err(), Error::CaseCollision { .. }));
    fs::remove_dir_all(dir).unwrap();
    let bytes = craft(&[crafted_plain("a", b"1"), crafted_plain("a/b", b"2")]);
    let (result, dir) = unpack_bytes(&bytes, Limits::default());
    assert!(matches!(result.unwrap_err(), Error::Manifest(_)));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn limits_stop_too_many_files_and_too_much_data() {
    let entries: Vec<Crafted> = (0..10).map(|i| crafted_plain(&format!("f{i}.txt"), b"0123456789")).collect();
    let bytes = craft(&entries);
    let (result, dir) = unpack_bytes(
        &bytes,
        Limits {
            max_files: 5,
            ..Limits::default()
        },
    );
    let err = result.unwrap_err();
    assert!(matches!(err, Error::TooManyFiles { found: 10, limit: 5 }), "{err}");
    assert!(!dir.join("out").exists() || fs::read_dir(dir.join("out")).unwrap().next().is_none());
    fs::remove_dir_all(dir).unwrap();
    let (result, dir) = unpack_bytes(
        &bytes,
        Limits {
            max_total: 50,
            ..Limits::default()
        },
    );
    assert!(matches!(result.unwrap_err(), Error::TooLarge { limit: 50, .. }));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_decompression_bomb_is_stopped_at_the_declared_size() {
    // Запись объявляет 10 байт, а поток deflate распаковывается в 5 МБ нулей.
    use flate2::write::DeflateEncoder;
    use std::io::Write;
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&vec![0u8; 5 * 1024 * 1024]).unwrap();
    let stored = encoder.finish().unwrap();
    assert!(stored.len() < 20_000);
    let mut crc = Crc::new();
    crc.update(b"0123456789");
    let bomb = Crafted {
        name: "bomb.bin".to_string(),
        stored,
        method: 1,
        size: 10,
        crc: crc.sum(),
    };
    let (result, dir) = unpack_bytes(&craft(&[bomb]), Limits::default());
    let err = result.unwrap_err();
    assert!(matches!(err, Error::LongerThanDeclared { .. }), "{err}");
    assert!(err.to_string().contains("bomb.bin"));
    let leftovers = fs::read_dir(dir.join("out")).map(|d| d.flatten().count()).unwrap_or(0);
    assert_eq!(leftovers, 0, "the partial file is removed");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damage_is_reported_with_the_name_of_the_entry() {
    let project = temp("dmg-src");
    sample_project(&project);
    let out = temp("dmg-out");
    let file = out.join("demo.pks");
    pack_default(&project, &file);
    let original = fs::read(&file).unwrap();

    // Находим данные записи `assets/scenes/main.scene.meta` (сжатые, но короткие) и портим байт.
    let needle = b"assets/scenes/main.scene.meta";
    let at = original.windows(needle.len()).position(|w| w == needle).unwrap() + needle.len() + 4 + 3;
    let mut damaged = original.clone();
    damaged[at] ^= 0xFF;
    fs::write(&file, &damaged).unwrap();
    let err = verify(&file, &Limits::default(), &Control::default(), &mut quiet).unwrap_err();
    assert!(err.to_string().contains("assets/scenes/main.scene.meta"), "the damaged entry is named: {err}");
    let dest = out.join("dest");
    let err = unpack(&file, &dest, &UnpackOptions::default(), &Control::default(), &mut quiet).unwrap_err();
    assert!(err.to_string().contains("main.scene.meta"), "{err}");

    // Обрезанный файл.
    fs::write(&file, &original[..original.len() / 2]).unwrap();
    let err = read_info(&file).unwrap_err();
    assert!(matches!(err, Error::Truncated { .. }), "{err}");
    assert!(err.to_string().contains("interrupted"));
    // Не наш файл и текстовый режим.
    fs::write(&file, b"PK\x03\x04 not ours, at all, really not ours").unwrap();
    assert!(matches!(read_info(&file).unwrap_err(), Error::NotAProjectFile(_)));
    fs::write(&file, b"x").unwrap();
    assert!(matches!(read_info(&file).unwrap_err(), Error::NotAProjectFile(_)));
    // Более новая версия.
    let mut newer = original.clone();
    newer[8] = 9;
    fs::write(&file, &newer).unwrap();
    assert!(matches!(read_info(&file).unwrap_err(), Error::UnsupportedVersion { found: 9, .. }));
    // Испорченный список.
    let mut bad_list = original.clone();
    let n = bad_list.len();
    bad_list[n - 40] ^= 0x55;
    fs::write(&file, &bad_list).unwrap();
    let err = read_info(&file).unwrap_err();
    assert!(matches!(err, Error::Manifest(_)), "{err}");
    // Файла нет.
    assert!(matches!(read_info(&out.join("missing.pks")).unwrap_err(), Error::Io { .. }));
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn cancelling_leaves_no_file_behind() {
    let project = temp("cancel-src");
    sample_project(&project);
    write(&project, "assets/big.bin", vec![7u8; 3 * 1024 * 1024]);
    let out = temp("cancel-out");
    let file = out.join("demo.pks");
    let control = Control::new();
    let watcher = control.clone();
    let mut calls = 0;
    let err = pack(&project, &file, &PackOptions::default(), &control, &mut |p: &Progress| {
        calls += 1;
        if p.phase == Phase::Working && p.bytes_done > 0 {
            watcher.cancel();
        }
    })
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(calls > 1);
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0, "neither the file nor the temp file remains");

    // Прежний файл не затирается неудачной упаковкой.
    fs::write(&file, b"previous").unwrap();
    let control = Control::new();
    control.cancel();
    assert!(matches!(pack(&project, &file, &PackOptions::default(), &control, &mut quiet), Err(Error::Cancelled)));
    assert_eq!(fs::read(&file).unwrap(), b"previous");
    // Отмена распаковки.
    pack_default(&project, &out.join("ok.pks"));
    let control = Control::new();
    let watcher = control.clone();
    let err = unpack(&out.join("ok.pks"), &out.join("dst"), &UnpackOptions::default(), &control, &mut |p: &Progress| {
        if p.files_done >= 2 {
            watcher.cancel();
        }
    })
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    let leftovers: Vec<_> = walk_names(&out.join("dst")).into_iter().filter(|n| n.contains(".part")).collect();
    assert!(leftovers.is_empty(), "no half-written files: {leftovers:?}");
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

fn walk_names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                out.extend(walk_names(&e.path()));
            } else {
                out.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out
}

#[test]
fn conflict_policies_for_existing_files() {
    let project = temp("conf-src");
    sample_project(&project);
    let out = temp("conf-out");
    let file = out.join("demo.pks");
    pack_default(&project, &file);
    let run = |dest: &Path, conflict: Conflict| unpack(&file, dest, &UnpackOptions { conflict, ..UnpackOptions::default() }, &Control::default(), &mut quiet);

    let dest = out.join("d");
    run(&dest, Conflict::Fail).unwrap();
    fs::write(dest.join("README.md"), "mine").unwrap();
    fs::write(dest.join("assets/icon.png"), "mine-png").unwrap();
    // Fail: ошибка, ничего не затёрто.
    let err = run(&dest, Conflict::Fail).unwrap_err();
    assert!(matches!(err, Error::Exists(_)), "{err}");
    assert_eq!(fs::read_to_string(dest.join("README.md")).unwrap(), "mine");
    // Skip: чужое остаётся, остальное дописывается.
    let skipped = run(&dest, Conflict::Skip).unwrap();
    assert_eq!(fs::read_to_string(dest.join("README.md")).unwrap(), "mine");
    assert!(skipped.skipped.iter().any(|(p, _)| p == "README.md"));
    // KeepBoth: рядом появляется `.imported`, пара `.meta` идёт за ассетом.
    let kept = run(&dest, Conflict::KeepBoth).unwrap();
    assert_eq!(fs::read_to_string(dest.join("README.md")).unwrap(), "mine");
    assert_eq!(fs::read_to_string(dest.join("README.imported.md")).unwrap(), "# demo\n");
    assert!(kept.renamed.iter().any(|(a, b)| a == "README.md" && b == "README.imported.md"));
    assert!(dest.join("assets/icon.imported.png").exists());
    assert!(dest.join("assets/icon.imported.png.meta").exists() || dest.join("assets/icon.png.imported.meta").exists(), "the meta follows its asset");
    // Overwrite.
    run(&dest, Conflict::Overwrite).unwrap();
    assert_eq!(fs::read_to_string(dest.join("README.md")).unwrap(), "# demo\n");
    // Файл не может занять место каталога.
    let blocked = out.join("blocked");
    fs::create_dir_all(blocked.join("Cargo.toml")).unwrap();
    assert!(matches!(run(&blocked, Conflict::Overwrite), Err(Error::Exists(_))));
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn merge_tree_moves_a_prepared_tree_by_policy() {
    let root = temp("merge");
    let staging = root.join("staging");
    let dest = root.join("dest");
    write(&staging, "a.txt", "new-a");
    write(&staging, "sub/b.txt", "new-b");
    write(&staging, "sub/b.txt.meta", "new-meta");
    write(&dest, "a.txt", "old-a");
    write(&dest, "sub/b.txt", "old-b");
    let summary = merge_tree(&staging, &dest, Conflict::KeepBoth, &Control::default()).unwrap();
    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "old-a");
    assert_eq!(fs::read_to_string(dest.join("a.imported.txt")).unwrap(), "new-a");
    assert_eq!(fs::read_to_string(dest.join("sub/b.imported.txt")).unwrap(), "new-b");
    assert_eq!(fs::read_to_string(dest.join("sub/b.imported.txt.meta")).unwrap(), "new-meta", "meta pair is kept together");
    assert_eq!(summary.renamed.len(), 3, "a.txt, sub/b.txt and its .meta");
    write(&staging, "a.txt", "newer-a");
    let summary = merge_tree(&staging, &dest, Conflict::Skip, &Control::default()).unwrap();
    assert_eq!(summary.skipped, ["a.txt"]);
    write(&staging, "a.txt", "newest-a");
    merge_tree(&staging, &dest, Conflict::Overwrite, &Control::default()).unwrap();
    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "newest-a");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unpack_never_writes_through_a_link_in_the_destination() {
    let root = temp("linkdst");
    let outside = temp("linkdst-outside");
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    let link = dest.join("a");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
    if made {
        let bytes = craft(&[crafted_plain("a/evil.txt", b"x")]);
        fs::write(root.join("x.pks"), bytes).unwrap();
        let err = unpack(&root.join("x.pks"), &dest, &UnpackOptions::default(), &Control::default(), &mut quiet).unwrap_err();
        assert!(matches!(err, Error::LinkInTheWay { .. }), "{err}");
        assert!(!outside.join("evil.txt").exists());
    } else {
        eprintln!("symlinks cannot be created here; skipping");
    }
    for d in [root, outside] {
        let _ = fs::remove_dir_all(d);
    }
}

#[test]
fn small_entries_can_be_read_without_unpacking_everything() {
    let project = temp("small-src");
    sample_project(&project);
    write(&project, "assets/big.bin", vec![1u8; 2 * 1024 * 1024]);
    let out = temp("small-out");
    let file = out.join("demo.pks");
    pack_default(&project, &file);
    let info = read_info(&file).unwrap();
    let found = read_manifest_entries(&file, &info, &|r| r.path.ends_with(".meta"), 1 << 20, &Control::default()).unwrap();
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["assets/icon.png.meta", "assets/scenes/main.scene.meta"]);
    assert!(String::from_utf8_lossy(&found[0].1).contains("aaaa"));
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn the_include_filter_leaves_entries_out() {
    let project = temp("inc-src");
    sample_project(&project);
    let out = temp("inc-out");
    let file = out.join("demo.pks");
    pack_default(&project, &file);
    let options = UnpackOptions {
        include: Some(std::sync::Arc::new(|p: &str| !p.starts_with("tests/") && !p.starts_with("cpp/"))),
        ..UnpackOptions::default()
    };
    let summary = unpack(&file, &out.join("d"), &options, &Control::default(), &mut quiet).unwrap();
    assert!(!out.join("d/tests").exists() && !out.join("d/cpp").exists());
    assert!(out.join("d/src/lib.rs").exists());
    assert!(summary.skipped.iter().any(|(p, _)| p == "tests/smoke.rs"));
    for d in [project, out] {
        fs::remove_dir_all(d).unwrap();
    }
}
