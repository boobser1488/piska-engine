//! `project-pack`, `project-unpack`, `project-info` — файл с исходниками проекта
//! (`docs/api/project-file.md`). Расширение и названия — из `engine_i18n::brand`.

use std::path::{Path, PathBuf};

use engine_i18n::brand;
use engine_project::{CollectOptions, Conflict, Control, Limits, PackOptions, Phase, Progress, UnpackOptions};

use crate::{human_size, Error, Result};

/// Справка по трём командам (подключается к общей справке).
pub fn usage() -> String {
    let ext = brand::PROJECT_EXTENSION;
    format!(
        "  piska-export project-pack   <project dir> [-o <file.{ext}>] [--no-tests] [--no-cpp] [--builds] [--ide]\n\
         \x20                             [--no-gitignore] [--reproducible]\n\
         \x20 piska-export project-unpack <file.{ext}> <dir> [--conflict fail|skip|overwrite|keep-both]\n\
         \x20 piska-export project-info   <file.{ext}> [--json] [--files] [--verify]\n\
         \n\
         project-pack saves the project's source ({description}) as one portable file: no target/, dist/,\n\
         .git/ or caches; the same project gives the same bytes with --reproducible.",
        description = brand::PROJECT_FILE_DESCRIPTION
    )
}

pub fn is_command(name: &str) -> bool {
    matches!(name, "project-pack" | "project-unpack" | "project-info")
}

struct Flags {
    positional: Vec<String>,
    out: Option<PathBuf>,
    tests: bool,
    cpp: bool,
    builds: bool,
    ide: bool,
    gitignore: bool,
    reproducible: bool,
    conflict: Conflict,
    json: bool,
    files: bool,
    verify: bool,
}

fn parse(args: &[String]) -> Result<Flags> {
    let mut flags = Flags {
        positional: Vec::new(),
        out: None,
        tests: true,
        cpp: true,
        builds: false,
        ide: false,
        gitignore: true,
        reproducible: false,
        conflict: Conflict::Fail,
        json: false,
        files: false,
        verify: false,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" | "--out" => {
                flags.out = Some(PathBuf::from(iter.next().ok_or_else(|| Error(format!("`{arg}` needs a value\n\n{}", usage())))?));
            }
            "--no-tests" => flags.tests = false,
            "--no-cpp" => flags.cpp = false,
            "--builds" => flags.builds = true,
            "--ide" => flags.ide = true,
            "--no-gitignore" => flags.gitignore = false,
            "--reproducible" => flags.reproducible = true,
            "--json" => flags.json = true,
            "--files" => flags.files = true,
            "--verify" => flags.verify = true,
            "--conflict" => {
                let value = iter.next().ok_or_else(|| Error(format!("`--conflict` needs a value\n\n{}", usage())))?;
                flags.conflict = match value.as_str() {
                    "fail" => Conflict::Fail,
                    "skip" => Conflict::Skip,
                    "overwrite" => Conflict::Overwrite,
                    "keep-both" => Conflict::KeepBoth,
                    other => return Err(Error(format!("`{other}` is not a conflict policy: use fail, skip, overwrite or keep-both"))),
                };
            }
            flag if flag.starts_with('-') => return Err(Error(format!("unknown option `{flag}`\n\n{}", usage()))),
            other => flags.positional.push(other.to_string()),
        }
    }
    Ok(flags)
}

/// Имя проекта из `Cargo.toml` (для имени файла по умолчанию).
fn project_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table.get("package")?.get("name")?.as_str().map(str::to_string)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn progress_printer() -> impl FnMut(&Progress) {
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    move |p: &Progress| {
        if p.phase == Phase::Scanning || last.elapsed() < std::time::Duration::from_millis(400) {
            return;
        }
        last = std::time::Instant::now();
        eprint!(
            "\rpiska-export: {}/{} files, {} of {}  {:<40.40}",
            p.files_done,
            p.files_total,
            human_size(p.bytes_done),
            human_size(p.bytes_total),
            p.current
        );
    }
}

fn clear_line() {
    eprint!("\r{:<100}\r", "");
}

pub fn run(command: &str, args: &[String]) -> Result<()> {
    let flags = parse(args)?;
    match command {
        "project-pack" => pack(flags),
        "project-unpack" => unpack(flags),
        _ => info(flags),
    }
}

fn pack(flags: Flags) -> Result<()> {
    let dir = PathBuf::from(
        flags
            .positional
            .first()
            .ok_or_else(|| Error(format!("`project-pack` needs the project folder\n\n{}", usage())))?,
    );
    let name = project_name(&dir).or_else(|| dir.canonicalize().ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned())));
    let out = flags
        .out
        .unwrap_or_else(|| PathBuf::from(brand::project_file_name(name.as_deref().unwrap_or("project"))));
    let options = PackOptions {
        collect: CollectOptions {
            tests: flags.tests,
            cpp: flags.cpp,
            builds: flags.builds,
            ide: flags.ide,
            gitignore: flags.gitignore,
        },
        tool: "piska-export".to_string(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        created: (!flags.reproducible).then(now),
        ..PackOptions::default()
    };
    let summary = engine_project::pack(&dir, &out, &options, &Control::default(), &mut progress_printer())?;
    clear_line();
    println!(
        "piska-export: {} — {} files, {} of source in {} ({}% of the original)",
        out.display(),
        summary.files,
        human_size(summary.bytes),
        human_size(summary.archive_size),
        if summary.bytes == 0 { 100 } else { summary.archive_size * 100 / summary.bytes }
    );
    for skipped in summary.skipped.iter().filter(|s| s.dir).take(12) {
        println!("piska-export:   left out {}/ ({:?})", skipped.rel, skipped.reason);
    }
    for note in &summary.notes {
        println!("piska-export: note: {note}");
    }
    Ok(())
}

fn unpack(flags: Flags) -> Result<()> {
    let [file, dest] = flags.positional.as_slice() else {
        return Err(Error(format!("`project-unpack` needs a file and a folder\n\n{}", usage())));
    };
    let options = UnpackOptions {
        conflict: flags.conflict,
        limits: Limits::default(),
        include: None,
    };
    let summary = engine_project::unpack(Path::new(file), Path::new(dest), &options, &Control::default(), &mut progress_printer())?;
    clear_line();
    println!("piska-export: unpacked {} files ({}) into {}", summary.written.len(), human_size(summary.bytes), dest);
    for (path, reason) in summary.skipped.iter().take(20) {
        println!("piska-export:   skipped {path}: {reason}");
    }
    for (from, to) in &summary.renamed {
        println!("piska-export:   {from} was kept as {to}");
    }
    Ok(())
}

fn date(unix: u64) -> String {
    // Гражданская дата из секунд Unix (алгоритм Говарда Хиннанта), UTC.
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

fn info(flags: Flags) -> Result<()> {
    let file = flags
        .positional
        .first()
        .ok_or_else(|| Error(format!("`project-info` needs the file\n\n{}", usage())))?;
    let path = Path::new(file);
    let info = if flags.verify {
        let info = engine_project::verify(path, &Limits::default(), &Control::default(), &mut progress_printer())?;
        clear_line();
        info
    } else {
        engine_project::read_info(path)?
    };
    let m = &info.manifest;
    if flags.json {
        let mut value = serde_json::to_value(m)?;
        if !flags.files {
            if let Some(object) = value.as_object_mut() {
                object.remove("files");
            }
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("project:   {} ({})", m.project.title, m.project.name);
    println!("format:    version {}  ·  {} on disk", info.version, human_size(info.file_size));
    println!("written:   {} {}", m.writer.tool, m.writer.engine_version);
    println!("created:   {}", if m.created == 0 { "not recorded (reproducible file)".to_string() } else { date(m.created) });
    println!("contains:  {}", m.parts.join(", "));
    println!("files:     {}  ·  {} unpacked  ·  {} packed", m.totals.files, human_size(m.totals.bytes), human_size(m.totals.packed));
    if flags.verify {
        println!("integrity: every entry checked, all checksums match");
    }
    if flags.files {
        for f in &m.files {
            println!("  {:>10}  {:08x}  {}", f.size, f.crc32, f.path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(date(0), "1970-01-01 00:00 UTC");
        assert_eq!(date(1_700_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn options_parse() {
        let flags = parse(&["dir".into(), "-o".into(), "x.pks".into(), "--no-tests".into(), "--builds".into(), "--conflict".into(), "keep-both".into()]).unwrap();
        assert!(!flags.tests && flags.builds && flags.cpp);
        assert_eq!(flags.conflict, Conflict::KeepBoth);
        assert!(parse(&["--nope".into()]).is_err());
        assert!(parse(&["--conflict".into(), "maybe".into()]).is_err());
        assert!(usage().contains(brand::PROJECT_EXTENSION));
    }

    #[test]
    fn pack_info_unpack_through_the_command_line_code() {
        let root = std::env::temp_dir().join(format!("piska-export-project-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("p");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("target")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[package]
name = \"cli_game\"
").unwrap();
        std::fs::write(project.join("src/lib.rs"), "pub fn build() {}
").unwrap();
        std::fs::write(project.join("target/junk"), "x").unwrap();
        let file = root.join(brand::project_file_name("cli_game"));
        let s = |x: &str| x.to_string();
        run("project-pack", &[project.display().to_string(), s("-o"), file.display().to_string(), s("--reproducible")]).unwrap();
        run("project-info", &[file.display().to_string(), s("--verify"), s("--files")]).unwrap();
        run("project-info", &[file.display().to_string(), s("--json")]).unwrap();
        let restored = root.join("restored");
        run("project-unpack", &[file.display().to_string(), restored.display().to_string()]).unwrap();
        assert!(restored.join("src/lib.rs").is_file() && restored.join("Cargo.toml").is_file());
        assert!(!restored.join("target").exists());
        // Второй раз в то же место: по умолчанию ошибка, с политикой — работает.
        assert!(run("project-unpack", &[file.display().to_string(), restored.display().to_string()]).is_err());
        run("project-unpack", &[file.display().to_string(), restored.display().to_string(), s("--conflict"), s("skip")]).unwrap();
        // Не проект и не файл проекта.
        assert!(run("project-pack", &[root.display().to_string()]).is_err());
        assert!(run("project-info", &[project.join("Cargo.toml").display().to_string()]).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
