//! Запись файла проекта: потоково, один проход, детерминированно.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};

use crate::collect::{collect, CollectOptions, SkipReason, Skipped};
use crate::format::*;
use crate::path::validate_relative;
use crate::{Control, Error, Limits, Phase, Progress, Result};

/// Параметры упаковки.
#[derive(Clone, Debug)]
pub struct PackOptions {
    pub collect: CollectOptions,
    pub limits: Limits,
    /// Имя проекта; `None` — `[package] name` из `Cargo.toml`.
    pub project_name: Option<String>,
    /// Заголовок; `None` — `[package.metadata.piska] title` или имя.
    pub project_title: Option<String>,
    /// Какая программа пишет (попадает в список).
    pub tool: String,
    /// Версия движка (попадает в список).
    pub engine_version: String,
    /// Время создания (секунды Unix). `None` — не записывать: файл воспроизводим, одинаковый
    /// проект даёт одинаковые байты.
    pub created: Option<u64>,
    /// Уровень сжатия deflate 0–9.
    pub level: u32,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            collect: CollectOptions::default(),
            limits: Limits::default(),
            project_name: None,
            project_title: None,
            tool: "engine-project".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            created: None,
            level: 6,
        }
    }
}

/// Что получилось.
#[derive(Clone, Debug, Default)]
pub struct PackSummary {
    pub files: u64,
    /// Размер содержимого без сжатия.
    pub bytes: u64,
    /// Размер данных после сжатия.
    pub packed: u64,
    /// Размер файла проекта.
    pub archive_size: u64,
    pub skipped: Vec<Skipped>,
    /// Замечания, на которые стоит обратить внимание (текст на английском).
    pub notes: Vec<String>,
}

/// Блоки данных записи: `[u32 длина][данные]…[u32 0]`.
struct Frames<'a, W: Write> {
    out: &'a mut W,
    buf: Vec<u8>,
    packed: u64,
}

impl<'a, W: Write> Frames<'a, W> {
    fn new(out: &'a mut W) -> Self {
        Self {
            out,
            buf: Vec::with_capacity(FRAME * 2),
            packed: 0,
        }
    }

    fn finish(mut self) -> std::io::Result<u64> {
        if !self.buf.is_empty() {
            self.out.write_all(&(self.buf.len() as u32).to_le_bytes())?;
            self.out.write_all(&self.buf)?;
            self.packed += self.buf.len() as u64;
        }
        self.out.write_all(&0u32.to_le_bytes())?;
        Ok(self.packed)
    }
}

impl<W: Write> Write for Frames<'_, W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        let mut offset = 0;
        while self.buf.len() - offset >= FRAME {
            self.out.write_all(&(FRAME as u32).to_le_bytes())?;
            self.out.write_all(&self.buf[offset..offset + FRAME])?;
            self.packed += FRAME as u64;
            offset += FRAME;
        }
        self.buf.drain(..offset);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Какие части есть в списке файлов.
pub(crate) fn detect_parts(paths: &[&str]) -> Vec<String> {
    let has = |f: &dyn Fn(&str) -> bool| paths.iter().any(|p| f(p));
    let root_file = |p: &str, prefixes: &[&str]| {
        !p.contains('/') && {
            let lower = p.to_ascii_lowercase();
            prefixes.iter().any(|x| lower.starts_with(x))
        }
    };
    let mut parts = Vec::new();
    let mut add = |name: &str, on: bool| {
        if on {
            parts.push(name.to_string());
        }
    };
    add("assets", has(&|p| p.starts_with("assets/")));
    add("tests", has(&|p| p.starts_with("tests/")));
    add("cpp", has(&|p| p.starts_with("cpp/")));
    add("tools", has(&|p| p.starts_with("tools/")));
    add("examples", has(&|p| p.starts_with("examples/")));
    add("builds", has(&|p| p.starts_with("dist/")));
    add("readme", has(&|p| root_file(p, &["readme"])));
    add("license", has(&|p| root_file(p, &["license", "licence", "copying"])));
    add("icon", has(&|p| p == "assets/icon.png" || p == "icon.png" || root_file(p, &["icon."])));
    add("settings", has(&|p| p == "assets/project.settings" || p == "project.settings"));
    parts
}

/// Имя и заголовок из `Cargo.toml`.
fn names_from_manifest(root: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return (None, None);
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return (None, None);
    };
    let package = table.get("package").and_then(|p| p.as_table());
    let name = package.and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_string);
    let title = package
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("piska"))
        .and_then(|m| m.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    (name, title)
}

/// Упаковать проект `root` в файл `out`. Пишется во временный файл рядом и переименовывается:
/// отмена и ошибка не оставляют половину файла и не затирают прежний.
pub fn pack(root: &Path, out: &Path, options: &PackOptions, control: &Control, progress: &mut dyn FnMut(&Progress)) -> Result<PackSummary> {
    let mut report = Progress {
        phase: Phase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current: String::new(),
    };
    progress(&report);
    let mut collected = collect(root, &options.collect, control)?;
    // Файл проекта, который пишется внутрь самого проекта, не должен упаковать себя.
    let out_abs = out
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(out.file_name().unwrap_or_default()));
    if let Some(out_abs) = &out_abs {
        collected.files.retain(|f| f.path.canonicalize().ok().as_ref() != Some(out_abs));
    }
    let files = collected.files;
    let total = files.len() as u64;
    let bytes: u64 = files.iter().map(|f| f.size).sum();
    if total > options.limits.max_files {
        return Err(Error::TooManyFiles {
            found: total,
            limit: options.limits.max_files,
        });
    }
    if bytes > options.limits.max_total {
        return Err(Error::TooLarge {
            found: bytes,
            limit: options.limits.max_total,
        });
    }
    for file in &files {
        validate_relative(&file.rel, options.limits.max_path).map_err(|why| Error::UnsafePath {
            path: file.rel.clone(),
            why,
        })?;
    }
    let mut lower: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for file in &files {
        if let Some(other) = lower.insert(file.rel.to_lowercase(), &file.rel) {
            return Err(Error::CaseCollision {
                a: other.to_string(),
                b: file.rel.clone(),
            });
        }
    }

    let mut notes = Vec::new();
    let cpp_off = collected
        .skipped
        .iter()
        .any(|s| s.reason == SkipReason::Option("cpp") && s.dir);
    if cpp_off
        && std::fs::read_to_string(root.join("build.rs")).is_ok_and(|text| text.contains("cpp"))
        && files.iter().any(|f| f.rel == "build.rs")
    {
        notes.push("C++ sources are left out, but build.rs still compiles `cpp/`; the unpacked project will not build until you restore the folder or edit build.rs".to_string());
    }

    let temp: PathBuf = out.with_file_name(format!(
        ".{}.{}.tmp",
        out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id()
    ));
    let result = write_all(root, &temp, &files, bytes, options, control, progress, &mut report);
    match result {
        Ok((packed, archive_size)) => {
            std::fs::rename(&temp, out).map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                Error::io(out, e)
            })?;
            Ok(PackSummary {
                files: total,
                bytes,
                packed,
                archive_size,
                skipped: collected.skipped,
                notes,
            })
        }
        Err(err) => {
            let _ = std::fs::remove_file(&temp);
            Err(err)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_all(
    root: &Path,
    temp: &Path,
    files: &[crate::collect::Candidate],
    bytes_total: u64,
    options: &PackOptions,
    control: &Control,
    progress: &mut dyn FnMut(&Progress),
    report: &mut Progress,
) -> Result<(u64, u64)> {
    let file = File::create(temp).map_err(|e| Error::io(temp, e))?;
    let mut out = BufWriter::with_capacity(256 * 1024, file);
    let werr = |e: std::io::Error| Error::io(temp, e);
    out.write_all(&MAGIC).map_err(werr)?;
    out.write_all(&FORMAT_VERSION.to_le_bytes()).map_err(werr)?;
    out.write_all(&[0u8; 2]).map_err(werr)?;
    out.write_all(&[0u8; 4]).map_err(werr)?;

    report.phase = Phase::Working;
    report.files_total = files.len() as u64;
    report.bytes_total = bytes_total;
    let mut records = Vec::with_capacity(files.len());
    let mut buffer = vec![0u8; 64 * 1024];
    let mut written_total: u64 = 0;
    for candidate in files {
        control.check()?;
        report.current = candidate.rel.clone();
        progress(report);
        let method = if already_compressed(&candidate.rel) { METHOD_STORED } else { METHOD_DEFLATE };
        let name = candidate.rel.as_bytes();
        out.write_all(&[MARK_FILE, method]).map_err(werr)?;
        out.write_all(&(name.len() as u16).to_le_bytes()).map_err(werr)?;
        out.write_all(name).map_err(werr)?;
        let mut source = File::open(&candidate.path).map_err(|e| Error::io(&candidate.path, e))?;
        let mut crc = Crc::new();
        let mut size: u64 = 0;
        let frames = Frames::new(&mut out);
        // Читаем блоками, чтобы отмена и ход работы срабатывали и на огромном файле.
        let packed = if method == METHOD_STORED {
            let mut frames = frames;
            loop {
                let n = source.read(&mut buffer).map_err(|e| Error::io(&candidate.path, e))?;
                if n == 0 {
                    break;
                }
                crc.update(&buffer[..n]);
                size += n as u64;
                frames.write_all(&buffer[..n]).map_err(werr)?;
                report.bytes_done += n as u64;
                written_total += n as u64;
                if written_total > options.limits.max_total {
                    return Err(Error::TooLarge {
                        found: written_total,
                        limit: options.limits.max_total,
                    });
                }
                control.check()?;
                progress(report);
            }
            frames.finish().map_err(werr)?
        } else {
            let mut encoder = DeflateEncoder::new(frames, Compression::new(options.level.min(9)));
            loop {
                let n = source.read(&mut buffer).map_err(|e| Error::io(&candidate.path, e))?;
                if n == 0 {
                    break;
                }
                crc.update(&buffer[..n]);
                size += n as u64;
                encoder.write_all(&buffer[..n]).map_err(werr)?;
                report.bytes_done += n as u64;
                written_total += n as u64;
                if written_total > options.limits.max_total {
                    return Err(Error::TooLarge {
                        found: written_total,
                        limit: options.limits.max_total,
                    });
                }
                control.check()?;
                progress(report);
            }
            encoder.finish().map_err(werr)?.finish().map_err(werr)?
        };
        out.write_all(&size.to_le_bytes()).map_err(werr)?;
        out.write_all(&crc.sum().to_le_bytes()).map_err(werr)?;
        records.push(FileRecord {
            path: candidate.rel.clone(),
            size,
            crc32: crc.sum(),
            packed,
            method,
        });
        report.files_done += 1;
    }
    finish_file(root, out, records, options, temp)
}

fn finish_file(
    root: &Path,
    mut out: BufWriter<File>,
    records: Vec<FileRecord>,
    options: &PackOptions,
    temp: &Path,
) -> Result<(u64, u64)> {
    let werr = |e: std::io::Error| Error::io(temp, e);
    let paths: Vec<&str> = records.iter().map(|r| r.path.as_str()).collect();
    let (cargo_name, cargo_title) = names_from_manifest(root);
    let name = options
        .project_name
        .clone()
        .or(cargo_name)
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let title = options.project_title.clone().or(cargo_title).unwrap_or_else(|| name.clone());
    let totals = Totals {
        files: records.len() as u64,
        bytes: records.iter().map(|r| r.size).sum(),
        packed: records.iter().map(|r| r.packed).sum(),
    };
    let manifest = Manifest {
        format: FORMAT_VERSION,
        project: ProjectMeta { name, title },
        writer: Writer {
            tool: options.tool.clone(),
            engine_version: options.engine_version.clone(),
        },
        created: options.created.unwrap_or(0),
        parts: detect_parts(&paths),
        totals: totals.clone(),
        files: records.clone(),
    };
    let json = serde_json::to_vec(&manifest).map_err(|e| Error::Manifest(e.to_string()))?;
    let mut crc = Crc::new();
    crc.update(&json);
    // Смещение маркера конца — по фактической позиции в файле.
    use std::io::Seek;
    let marker_at = out.stream_position().map_err(werr)?;
    out.write_all(&[MARK_END]).map_err(werr)?;
    out.write_all(&(json.len() as u32).to_le_bytes()).map_err(werr)?;
    out.write_all(&json).map_err(werr)?;
    out.write_all(&crc.sum().to_le_bytes()).map_err(werr)?;
    out.write_all(&marker_at.to_le_bytes()).map_err(werr)?;
    out.write_all(&(json.len() as u32).to_le_bytes()).map_err(werr)?;
    out.write_all(&crc.sum().to_le_bytes()).map_err(werr)?;
    out.write_all(&END_MAGIC).map_err(werr)?;
    out.flush().map_err(werr)?;
    let size = out.stream_position().map_err(werr)?;
    Ok((totals.packed, size))
}
