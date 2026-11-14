//! Чтение файла проекта: сведения без распаковки, проверка целиком, распаковка.
//!
//! Всё потоковое: файл читается блоками, в память целиком попадает только список файлов.
//! Пути из файла проверяются до того, как на диске появится хоть один байт; число файлов и
//! суммарный объём ограничены; каждая запись читается не дальше размера, объявленного в
//! списке (защита от «бомб»); контрольная сумма каждой записи сверяется, ошибка называет запись.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::bufread::DeflateDecoder;
use flate2::Crc;

use crate::format::*;
use crate::merge::{ensure_parent, resolve_target, Conflict, Target};
use crate::path::validate_relative;
use crate::{Control, Error, Limits, Phase, Progress, Result};

/// Прочитанные сведения о файле проекта.
#[derive(Clone, Debug)]
pub struct Info {
    /// Версия формата.
    pub version: u16,
    pub manifest: Manifest,
    /// Размер файла проекта на диске.
    pub file_size: u64,
}

fn open(path: &Path) -> Result<(BufReader<File>, u64)> {
    let file = File::open(path).map_err(|e| Error::io(path, e))?;
    let len = file.metadata().map_err(|e| Error::io(path, e))?.len();
    Ok((BufReader::with_capacity(128 * 1024, file), len))
}

fn truncated(at: impl Into<String>) -> Error {
    Error::Truncated { at: at.into() }
}

fn read_exact_or(r: &mut impl Read, buf: &mut [u8], at: impl FnOnce() -> Error) -> Result<()> {
    r.read_exact(buf).map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof { at() } else { Error::io("project file", e) })
}

fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Заголовок и список файлов; сами записи не читаются.
pub fn read_info(path: &Path) -> Result<Info> {
    let (mut r, len) = open(path)?;
    let shown = path.display().to_string();
    let mut header = [0u8; HEADER_LEN as usize];
    r.read_exact(&mut header).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            Error::NotAProjectFile(shown.clone())
        } else {
            Error::io(path, e)
        }
    })?;
    if header[..8] != MAGIC {
        return Err(Error::NotAProjectFile(shown));
    }
    let version = u16_le(&header[8..10]);
    if version == 0 || version > FORMAT_VERSION {
        return Err(Error::UnsupportedVersion {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    if len < HEADER_LEN + TRAILER_LEN + 1 {
        return Err(truncated("before the file list"));
    }
    r.seek(SeekFrom::Start(len - TRAILER_LEN)).map_err(|e| Error::io(path, e))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    read_exact_or(&mut r, &mut trailer, || truncated("at the end"))?;
    if trailer[16..] != END_MAGIC {
        return Err(truncated("at the end (the end marker is missing)"));
    }
    let marker = u64_le(&trailer[0..8]);
    let list_len = u32_le(&trailer[8..12]);
    let stored_crc = u32_le(&trailer[12..16]);
    if list_len > MAX_MANIFEST {
        return Err(Error::Manifest(format!("the list claims {list_len} bytes")));
    }
    if marker < HEADER_LEN || marker + 1 + 4 + u64::from(list_len) + 4 + TRAILER_LEN != len {
        return Err(Error::Manifest("the end of the file does not match its list; the file was changed or cut".to_string()));
    }
    r.seek(SeekFrom::Start(marker)).map_err(|e| Error::io(path, e))?;
    let mut head = [0u8; 5];
    read_exact_or(&mut r, &mut head, || truncated("in the file list"))?;
    if head[0] != MARK_END || u32_le(&head[1..5]) != list_len {
        return Err(Error::Manifest("the list header does not match the end of the file".to_string()));
    }
    let mut json = vec![0u8; list_len as usize];
    read_exact_or(&mut r, &mut json, || truncated("in the file list"))?;
    let mut crc_bytes = [0u8; 4];
    read_exact_or(&mut r, &mut crc_bytes, || truncated("in the file list"))?;
    let mut crc = Crc::new();
    crc.update(&json);
    if crc.sum() != stored_crc || u32_le(&crc_bytes) != stored_crc {
        return Err(Error::Manifest(format!(
            "checksum mismatch (stored {stored_crc:08x}, computed {:08x}); the file is damaged",
            crc.sum()
        )));
    }
    let manifest: Manifest = serde_json::from_slice(&json).map_err(|e| Error::Manifest(e.to_string()))?;
    Ok(Info {
        version,
        manifest,
        file_size: len,
    })
}

impl Manifest {
    /// Пути безопасны, число файлов и объём в пределах, нет совпадений с точностью до
    /// регистра. Вызывается до того, как что-то пишется на диск.
    pub fn validate(&self, limits: &Limits) -> Result<()> {
        let count = self.files.len() as u64;
        if count > limits.max_files {
            return Err(Error::TooManyFiles {
                found: count,
                limit: limits.max_files,
            });
        }
        let mut total: u64 = 0;
        let mut seen: HashMap<String, &str> = HashMap::with_capacity(self.files.len());
        for file in &self.files {
            validate_relative(&file.path, limits.max_path).map_err(|why| Error::UnsafePath {
                path: file.path.clone(),
                why,
            })?;
            total = total.saturating_add(file.size);
            if total > limits.max_total {
                return Err(Error::TooLarge {
                    found: total,
                    limit: limits.max_total,
                });
            }
            if let Some(other) = seen.insert(file.path.to_lowercase(), &file.path) {
                return Err(if other == file.path {
                    Error::Manifest(format!("`{}` is listed twice", file.path))
                } else {
                    Error::CaseCollision {
                        a: other.to_string(),
                        b: file.path.clone(),
                    }
                });
            }
            if file.method > METHOD_DEFLATE {
                return Err(Error::Corrupt {
                    entry: file.path.clone(),
                    what: format!("unknown storage method {}", file.method),
                });
            }
        }
        // Файл не может быть и каталогом другого файла (`a` и `a/b`).
        let paths: HashSet<&str> = self.files.iter().map(|f| f.path.as_str()).collect();
        for file in &self.files {
            let mut at = 0;
            while let Some(i) = file.path[at..].find('/') {
                let parent = &file.path[..at + i];
                if paths.contains(parent) {
                    return Err(Error::Manifest(format!("`{parent}` is both a file and a folder")));
                }
                at += i + 1;
            }
        }
        Ok(())
    }
}

/// Блоки данных записи как поток байт: конец — блок длины 0.
struct FrameReader<'a> {
    r: &'a mut BufReader<File>,
    left: u32,
    done: bool,
    packed: u64,
}

impl<'a> FrameReader<'a> {
    fn new(r: &'a mut BufReader<File>) -> Self {
        Self {
            r,
            left: 0,
            done: false,
            packed: 0,
        }
    }

    /// Дочитать до конца записи; сколько байт данных в файле всего.
    fn finish(&mut self) -> io::Result<u64> {
        io::copy(self, &mut io::sink())?;
        Ok(self.packed)
    }
}

impl Read for FrameReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            if self.done {
                return Ok(0);
            }
            let mut len = [0u8; 4];
            self.r.read_exact(&mut len)?;
            let len = u32::from_le_bytes(len);
            if len == 0 {
                self.done = true;
                return Ok(0);
            }
            if len > MAX_FRAME {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a data block claims {len} bytes")));
            }
            self.left = len;
        }
        let want = buf.len().min(self.left as usize);
        let n = self.r.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n as u32;
        self.packed += n as u64;
        Ok(n)
    }
}

/// Запись, к данным которой обратился обход.
struct EntryData<'a> {
    r: &'a mut BufReader<File>,
    record: &'a FileRecord,
    consumed: bool,
}

fn map_read(entry: &str, e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::UnexpectedEof => truncated(format!("inside `{entry}`")),
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => Error::Corrupt {
            entry: entry.to_string(),
            what: format!("the data cannot be decompressed ({e})"),
        },
        _ => Error::io(entry, e),
    }
}

impl<'a> EntryData<'a> {
    /// Прочитать запись, проверить размер и контрольную сумму, отдать содержимое.
    fn copy_to(&mut self, sink: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let record = self.record;
        let name = record.path.as_str();
        let mut crc = Crc::new();
        let mut size: u64 = 0;
        let mut buffer = vec![0u8; 64 * 1024];
        let limit = record.size.saturating_add(1);
        let mut frames = FrameReader::new(self.r);
        let mut trailing = 0u64;
        {
            let mut pump = |source: &mut dyn Read| -> Result<()> {
                loop {
                    let n = source.read(&mut buffer).map_err(|e| map_read(name, e))?;
                    if n == 0 {
                        return Ok(());
                    }
                    crc.update(&buffer[..n]);
                    size += n as u64;
                    if size > record.size {
                        return Err(Error::LongerThanDeclared {
                            entry: name.to_string(),
                            declared: record.size,
                        });
                    }
                    sink(&buffer[..n])?;
                }
            };
            if record.method == METHOD_STORED {
                pump(&mut (&mut frames).take(limit))?;
            } else {
                let mut buffered = io::BufReader::new(&mut frames);
                {
                    let mut decoder = DeflateDecoder::new(&mut buffered).take(limit);
                    pump(&mut decoder)?;
                }
                trailing = io::copy(&mut buffered, &mut io::sink()).map_err(|e| map_read(name, e))?;
            }
        }
        let packed = frames.finish().map_err(|e| map_read(name, e))?;
        if trailing > 0 {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: "there is unexpected data after the end of the compressed stream".to_string(),
            });
        }
        let (stored_size, stored_crc) = self.read_trailer()?;
        let computed = crc.sum();
        if size != record.size {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: format!("the entry holds {size} bytes, the file list says {}", record.size),
            });
        }
        if computed != record.crc32 || stored_crc != record.crc32 {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: format!("checksum mismatch (expected {:08x}, computed {computed:08x}); the data is damaged", record.crc32),
            });
        }
        if stored_size != record.size || packed != record.packed {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: "the entry does not match the file list".to_string(),
            });
        }
        self.consumed = true;
        Ok(())
    }

    /// Пропустить запись, не читая данных (перемоткой). Размер и сумма — только сверка со списком.
    fn skip(&mut self) -> Result<()> {
        let name = self.record.path.as_str();
        loop {
            let mut len = [0u8; 4];
            self.r.read_exact(&mut len).map_err(|e| map_read(name, e))?;
            let len = u32::from_le_bytes(len);
            if len == 0 {
                break;
            }
            if len > MAX_FRAME {
                return Err(Error::Corrupt {
                    entry: name.to_string(),
                    what: format!("a data block claims {len} bytes"),
                });
            }
            self.r.seek_relative(i64::from(len)).map_err(|e| map_read(name, e))?;
        }
        let (size, crc) = self.read_trailer()?;
        if size != self.record.size || crc != self.record.crc32 {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: "the entry does not match the file list".to_string(),
            });
        }
        self.consumed = true;
        Ok(())
    }

    fn read_trailer(&mut self) -> Result<(u64, u32)> {
        let mut tail = [0u8; 12];
        let name = self.record.path.clone();
        read_exact_or(self.r, &mut tail, || truncated(format!("inside `{name}`")))?;
        Ok((u64_le(&tail[..8]), u32_le(&tail[8..12])))
    }
}

/// Пройти записи по порядку списка.
fn walk(path: &Path, info: &Info, control: &Control, mut visit: impl FnMut(usize, &FileRecord, &mut EntryData) -> Result<()>) -> Result<()> {
    let (mut r, _) = open(path)?;
    r.seek(SeekFrom::Start(HEADER_LEN)).map_err(|e| Error::io(path, e))?;
    for (index, record) in info.manifest.files.iter().enumerate() {
        control.check()?;
        let name = record.path.as_str();
        let mut head = [0u8; 4];
        read_exact_or(&mut r, &mut head, || truncated(format!("before `{name}`")))?;
        let name_len = usize::from(u16_le(&head[2..4]));
        let mut stored = vec![0u8; name_len];
        read_exact_or(&mut r, &mut stored, || truncated(format!("before `{name}`")))?;
        if head[0] != MARK_FILE || head[1] != record.method || stored != name.as_bytes() {
            return Err(Error::Corrupt {
                entry: name.to_string(),
                what: format!(
                    "the entry header does not match the file list (found `{}`)",
                    String::from_utf8_lossy(&stored)
                ),
            });
        }
        let mut data = EntryData {
            r: &mut r,
            record,
            consumed: false,
        };
        visit(index, record, &mut data)?;
        if !data.consumed {
            data.copy_to(&mut |_| Ok(()))?;
        }
    }
    let mut end = [0u8; 1];
    read_exact_or(&mut r, &mut end, || truncated("after the last entry"))?;
    if end[0] != MARK_END {
        return Err(Error::Manifest("the file holds entries that the file list does not mention".to_string()));
    }
    Ok(())
}

/// Проверить файл целиком: список, пути, пределы, размеры и контрольные суммы всех записей.
/// Ничего не пишет на диск. Ошибка называет повреждённую запись.
pub fn verify(path: &Path, limits: &Limits, control: &Control, progress: &mut dyn FnMut(&Progress)) -> Result<Info> {
    let info = read_info(path)?;
    info.manifest.validate(limits)?;
    let mut report = Progress {
        phase: Phase::Verifying,
        files_done: 0,
        files_total: info.manifest.files.len() as u64,
        bytes_done: 0,
        bytes_total: info.manifest.totals.bytes,
        current: String::new(),
    };
    progress(&report);
    walk(path, &info, control, |_, record, data| {
        report.current = record.path.clone();
        let mut done = 0u64;
        data.copy_to(&mut |chunk| {
            done += chunk.len() as u64;
            Ok(())
        })?;
        report.bytes_done += done;
        report.files_done += 1;
        progress(&report);
        Ok(())
    })?;
    Ok(info)
}

/// Прочитать в память записи, которые выбрал `wanted` (до `max_each` байт каждая); остальные
/// пропускаются перемоткой, без чтения. Для предварительной проверки (`.meta`, `Cargo.toml`).
pub fn read_manifest_entries(
    path: &Path,
    info: &Info,
    wanted: &dyn Fn(&FileRecord) -> bool,
    max_each: u64,
    control: &Control,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut found = Vec::new();
    walk(path, info, control, |_, record, data| {
        if wanted(record) && record.size <= max_each {
            let mut bytes = Vec::with_capacity(record.size as usize);
            data.copy_to(&mut |chunk| {
                bytes.extend_from_slice(chunk);
                Ok(())
            })?;
            found.push((record.path.clone(), bytes));
        } else {
            data.skip()?;
        }
        Ok(())
    })?;
    Ok(found)
}

/// Какие записи распаковывать: `true` — распаковать.
pub type Include = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Параметры распаковки.
#[derive(Clone, Default)]
pub struct UnpackOptions {
    pub limits: Limits,
    /// Что делать с файлами, которые уже есть в каталоге назначения. По умолчанию — ошибка.
    pub conflict: Conflict,
    /// Какие записи распаковывать (`true` — распаковать); `None` — все.
    pub include: Option<Include>,
}

/// Что распаковано.
#[derive(Clone, Debug, Default)]
pub struct UnpackSummary {
    /// Файлы, записанные на диск (пути от каталога назначения).
    pub written: Vec<String>,
    /// Не записаны: путь и причина.
    pub skipped: Vec<(String, String)>,
    /// Записаны под другим именем (политика «keep both»): было и стало.
    pub renamed: Vec<(String, String)>,
    pub bytes: u64,
}

/// Распаковать в `dest` (создаётся). Сначала проверяются список и пути, потом идёт запись:
/// файлы пишутся под временным именем и переименовываются; при ошибке или отмене
/// недописанный файл удаляется (уже записанные остаются — для чистого отката распаковывайте
/// в пустой временный каталог, как делает хаб).
pub fn unpack(archive: &Path, dest: &Path, options: &UnpackOptions, control: &Control, progress: &mut dyn FnMut(&Progress)) -> Result<UnpackSummary> {
    let info = read_info(archive)?;
    info.manifest.validate(&options.limits)?;
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let mut report = Progress {
        phase: Phase::Working,
        files_done: 0,
        files_total: info.manifest.files.len() as u64,
        bytes_done: 0,
        bytes_total: info.manifest.totals.bytes,
        current: String::new(),
    };
    progress(&report);
    let mut summary = UnpackSummary::default();
    let mut renamed: HashMap<String, PathBuf> = HashMap::new();
    let mut verified: HashSet<PathBuf> = HashSet::new();
    walk(archive, &info, control, |_, record, data| {
        report.current = record.path.clone();
        let rel = record.path.as_str();
        if options.include.as_ref().is_some_and(|f| !f(rel)) {
            data.skip()?;
            summary.skipped.push((rel.to_string(), "left out by the import settings".to_string()));
            report.files_done += 1;
            progress(&report);
            return Ok(());
        }
        let mut target = dest.join(rel);
        // `foo.png.meta` остаётся парой с переименованным `foo.png`.
        if let Some(base) = rel.strip_suffix(".meta") {
            if let Some(new_base) = renamed.get(base) {
                target = PathBuf::from(format!("{}.meta", new_base.display()));
            }
        }
        ensure_parent(dest, &target, &mut verified)?;
        match resolve_target(&target, options.conflict)? {
            Target::Skip => {
                data.skip()?;
                summary.skipped.push((rel.to_string(), "already exists".to_string()));
            }
            Target::Write(final_path) => {
                if final_path != dest.join(rel) {
                    let shown = final_path.strip_prefix(dest).unwrap_or(&final_path).to_string_lossy().replace('\\', "/");
                    summary.renamed.push((rel.to_string(), shown));
                    renamed.insert(rel.to_string(), final_path.clone());
                }
                let part = final_path.with_file_name(format!(
                    ".{}.part",
                    final_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ));
                let result = (|| -> Result<()> {
                    let file = File::options().write(true).create(true).truncate(true).open(&part).map_err(|e| Error::io(&part, e))?;
                    let mut out = io::BufWriter::with_capacity(128 * 1024, file);
                    let mut done = 0u64;
                    data.copy_to(&mut |chunk| {
                        out.write_all(chunk).map_err(|e| Error::io(&part, e))?;
                        done += chunk.len() as u64;
                        Ok(())
                    })?;
                    out.flush().map_err(|e| Error::io(&part, e))?;
                    drop(out);
                    summary.bytes += done;
                    std::fs::rename(&part, &final_path).map_err(|e| Error::io(&final_path, e))
                })();
                if let Err(err) = result {
                    let _ = std::fs::remove_file(&part);
                    return Err(err);
                }
                summary.written.push(rel.to_string());
            }
        }
        report.bytes_done += record.size;
        report.files_done += 1;
        progress(&report);
        Ok(())
    })?;
    Ok(summary)
}
