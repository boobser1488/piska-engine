//! Архивы без внешних программ: ZIP (APK — это ZIP) и `.tar.gz` (раздача для Linux).
//!
//! ZIP: чтение записей готового архива (результат `aapt2 link`) и запись нового —
//! записи переносятся как есть, без пересжатия. Без ZIP64 (APK меньше 4 ГиБ).
//! Время всех записей фиксированное (1980-01-01): одинаковый вход — одинаковый архив.

use std::io::Write;

use crate::{Error, Result};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
/// Дата DOS 1980-01-01 00:00.
const DOS_DATE: u16 = (1 << 5) | 1;

/// Способ хранения записи ZIP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Без сжатия: так Android может отображать `.so` и `assets.pak` прямо из APK.
    Stored,
    #[cfg_attr(not(test), allow(dead_code))] // APK пишет только без сжатия; сжатые записи переносит как есть
    Deflated,
}

impl Method {
    fn code(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflated => 8,
        }
    }
}

/// Запись ZIP в том виде, как лежит в архиве (данные — сжатые, если `method` = 8).
#[derive(Clone, Debug)]
pub struct ZipEntry {
    pub name: String,
    pub method: u16,
    pub crc: u32,
    pub uncompressed: u32,
    pub data: Vec<u8>,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| Error("the ZIP archive is truncated".to_string()))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| Error("the ZIP archive is truncated".to_string()))
}

/// Прочитать все записи ZIP (по центральному каталогу).
pub fn read_zip(bytes: &[u8]) -> Result<Vec<ZipEntry>> {
    // Конец центрального каталога — в последних 22 + 65535 байтах.
    let min = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (min..=bytes.len().saturating_sub(22))
        .rev()
        .find(|&at| u32_at(bytes, at).is_ok_and(|sig| sig == END_OF_CENTRAL))
        .ok_or_else(|| Error("not a ZIP archive (no end of central directory)".to_string()))?;
    let count = u16_at(bytes, eocd + 10)? as usize;
    let mut at = u32_at(bytes, eocd + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, at)? != CENTRAL_HEADER {
            return Err(Error("corrupt ZIP central directory".to_string()));
        }
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let compressed = u32_at(bytes, at + 20)? as usize;
        let uncompressed = u32_at(bytes, at + 24)?;
        let name_len = u16_at(bytes, at + 28)? as usize;
        let extra_len = u16_at(bytes, at + 30)? as usize;
        let comment_len = u16_at(bytes, at + 32)? as usize;
        let local = u32_at(bytes, at + 42)? as usize;
        let name = bytes
            .get(at + 46..at + 46 + name_len)
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .ok_or_else(|| Error("the ZIP archive is truncated".to_string()))?;
        if u32_at(bytes, local)? != LOCAL_HEADER {
            return Err(Error(format!("corrupt ZIP local header for `{name}`")));
        }
        let start = local + 30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize;
        let data = bytes
            .get(start..start + compressed)
            .ok_or_else(|| Error(format!("the data of `{name}` is truncated")))?
            .to_vec();
        entries.push(ZipEntry {
            name,
            method,
            crc,
            uncompressed,
            data,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

/// Распаковать данные записи (stored / deflate) — для проверок содержимого APK.
#[cfg_attr(not(test), allow(dead_code))]
pub fn entry_bytes(entry: &ZipEntry) -> Result<Vec<u8>> {
    match entry.method {
        0 => Ok(entry.data.clone()),
        8 => {
            let mut out = Vec::with_capacity(entry.uncompressed as usize);
            let mut decoder = flate2::write::DeflateDecoder::new(&mut out);
            decoder.write_all(&entry.data)?;
            decoder.finish()?;
            Ok(out)
        }
        other => Err(Error(format!("`{}` uses unsupported ZIP method {other}", entry.name))),
    }
}

/// Новый ZIP в памяти.
#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    names: Vec<String>,
}

impl ZipWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавить файл, сжав его (или нет).
    pub fn add(&mut self, name: &str, bytes: &[u8], method: Method) -> Result<()> {
        let crc = crc32(bytes);
        let data = match method {
            Method::Stored => bytes.to_vec(),
            Method::Deflated => {
                let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
                encoder.write_all(bytes)?;
                encoder.finish()?
            }
        };
        let size = u32::try_from(bytes.len()).map_err(|_| Error(format!("`{name}` is larger than 4 GiB")))?;
        self.add_raw(&ZipEntry {
            name: name.to_string(),
            method: method.code(),
            crc,
            uncompressed: size,
            data,
        })
    }

    /// Перенести запись другого архива без пересжатия.
    pub fn add_raw(&mut self, entry: &ZipEntry) -> Result<()> {
        if self.names.iter().any(|n| n == &entry.name) {
            return Err(Error(format!("duplicate ZIP entry `{}`", entry.name)));
        }
        let too_big = || Error("the archive is larger than 4 GiB (ZIP64 is not supported)".to_string());
        let offset = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let compressed = u32::try_from(entry.data.len()).map_err(|_| too_big())?;
        let name = entry.name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| Error(format!("entry name too long: {}", entry.name)))?;
        // Флаг 0x800 — имя в UTF-8.
        let flags: u16 = 0x0800;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes()); // нужная версия
        local.extend_from_slice(&flags.to_le_bytes());
        local.extend_from_slice(&entry.method.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // время
        local.extend_from_slice(&DOS_DATE.to_le_bytes());
        local.extend_from_slice(&entry.crc.to_le_bytes());
        local.extend_from_slice(&compressed.to_le_bytes());
        local.extend_from_slice(&entry.uncompressed.to_le_bytes());
        local.extend_from_slice(&name_len.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // extra
        local.extend_from_slice(name);
        self.out.extend_from_slice(&local);
        self.out.extend_from_slice(&entry.data);

        let c = &mut self.central;
        c.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes()); // сделано версией
        c.extend_from_slice(&20u16.to_le_bytes()); // нужная версия
        c.extend_from_slice(&flags.to_le_bytes());
        c.extend_from_slice(&entry.method.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&DOS_DATE.to_le_bytes());
        c.extend_from_slice(&entry.crc.to_le_bytes());
        c.extend_from_slice(&compressed.to_le_bytes());
        c.extend_from_slice(&entry.uncompressed.to_le_bytes());
        c.extend_from_slice(&name_len.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes()); // extra
        c.extend_from_slice(&0u16.to_le_bytes()); // комментарий
        c.extend_from_slice(&0u16.to_le_bytes()); // диск
        c.extend_from_slice(&0u16.to_le_bytes()); // внутренние атрибуты
        c.extend_from_slice(&0u32.to_le_bytes()); // внешние атрибуты
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| Error("more than 65535 files in one archive".to_string()))?;
        self.names.push(entry.name.clone());
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
        let too_big = || Error("the archive is larger than 4 GiB (ZIP64 is not supported)".to_string());
        let start = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let size = u32::try_from(self.central.len()).map_err(|_| too_big())?;
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&END_OF_CENTRAL.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(&start.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        Ok(self.out)
    }
}

/// CRC-32 (IEEE), как в ZIP и gzip.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

/// Файл для `.tar.gz`: путь внутри архива, содержимое, исполняемый ли.
pub struct TarFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// Собрать `.tar.gz` (формат ustar; длинные пути — через префикс ustar, до 255 символов).
/// Каталоги добавляются сами (`0755`), файлы — `0644` или `0755` для исполняемых.
pub fn tar_gz(files: &[TarFile]) -> Result<Vec<u8>> {
    let mut tar = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    for file in files {
        let parts: Vec<&str> = file.path.split('/').collect();
        for depth in 1..parts.len() {
            let dir = format!("{}/", parts[..depth].join("/"));
            if !dirs.contains(&dir) {
                tar_header(&mut tar, &dir, 0, 0o755, b'5')?;
                dirs.push(dir);
            }
        }
        let mode = if file.executable { 0o755 } else { 0o644 };
        tar_header(&mut tar, &file.path, file.bytes.len() as u64, mode, b'0')?;
        tar.extend_from_slice(&file.bytes);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    // Конец архива — два пустых блока.
    tar.resize(tar.len() + 1024, 0);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&tar)?;
    Ok(encoder.finish()?)
}

fn tar_header(tar: &mut Vec<u8>, path: &str, size: u64, mode: u32, kind: u8) -> Result<()> {
    let mut header = [0u8; 512];
    let (prefix, name) = if path.len() <= 100 {
        ("", path)
    } else {
        // ustar: путь = prefix (до 155) + "/" + name (до 100), разрез по "/".
        let cut = path
            .char_indices()
            .filter(|&(i, c)| c == '/' && i <= 155 && path.len() - i - 1 <= 100 && i + 1 < path.len())
            .map(|(i, _)| i)
            .next_back()
            .ok_or_else(|| Error(format!("path too long for a tar archive: {path}")))?;
        (&path[..cut], &path[cut + 1..])
    };
    header[..name.len()].copy_from_slice(name.as_bytes());
    let octal = |field: &mut [u8], value: u64| {
        let text = format!("{value:0width$o}", width = field.len() - 1);
        field[..text.len()].copy_from_slice(text.as_bytes());
    };
    octal(&mut header[100..108], u64::from(mode));
    octal(&mut header[108..116], 0); // uid
    octal(&mut header[116..124], 0); // gid
    if size >= 8u64.pow(11) {
        return Err(Error(format!("`{path}` is too large for a tar archive")));
    }
    octal(&mut header[124..136], size);
    octal(&mut header[136..148], 0); // mtime: фиксированное — воспроизводимый архив
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    // Контрольная сумма считается с полем, заполненным пробелами.
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    let text = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(text.as_bytes());
    tar.extend_from_slice(&header);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn zip_round_trip_keeps_methods_and_contents() {
        let mut zip = ZipWriter::new();
        let text = b"hello hello hello hello hello hello".repeat(20);
        zip.add("AndroidManifest.xml", &text, Method::Deflated).unwrap();
        zip.add("lib/arm64-v8a/libgame.so", b"\x7fELF....", Method::Stored).unwrap();
        let bytes = zip.finish().unwrap();

        let entries = read_zip(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["AndroidManifest.xml", "lib/arm64-v8a/libgame.so"]);
        assert_eq!(entries[0].method, 8);
        assert!(entries[0].data.len() < text.len(), "deflated");
        assert_eq!(entry_bytes(&entries[0]).unwrap(), text);
        assert_eq!(entries[1].method, 0);
        assert_eq!(entry_bytes(&entries[1]).unwrap(), b"\x7fELF....");
        assert_eq!(entries[1].crc, crc32(b"\x7fELF...."));

        // Перенос записей в новый архив не меняет их.
        let mut copy = ZipWriter::new();
        for entry in &entries {
            copy.add_raw(entry).unwrap();
        }
        assert_eq!(copy.finish().unwrap(), bytes);
    }

    #[test]
    fn zip_rejects_duplicates_and_garbage() {
        let mut zip = ZipWriter::new();
        zip.add("a.txt", b"1", Method::Stored).unwrap();
        assert!(zip.add("a.txt", b"2", Method::Stored).is_err());
        assert!(read_zip(b"this is not a zip archive at all, sorry").is_err());
    }

    #[test]
    fn crc_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    /// Разобрать tar обратно: (путь, режим, тип, содержимое).
    fn untar(gz: &[u8]) -> Vec<(String, u32, u8, Vec<u8>)> {
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(gz).read_to_end(&mut tar).unwrap();
        let mut out = Vec::new();
        let mut at = 0;
        while at + 512 <= tar.len() && tar[at] != 0 {
            let h = &tar[at..at + 512];
            let field = |r: std::ops::Range<usize>| {
                let raw = &h[r];
                String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())]).into_owned()
            };
            let mut sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
            sum -= h[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            sum += 8 * u32::from(b' ');
            assert_eq!(u32::from_str_radix(field(148..154).trim(), 8).unwrap(), sum, "checksum");
            assert_eq!(&h[257..262], b"ustar");
            let prefix = field(345..500);
            let name = field(0..100);
            let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let mode = u32::from_str_radix(field(100..107).trim(), 8).unwrap();
            let size = usize::from_str_radix(field(124..135).trim(), 8).unwrap();
            let data = tar[at + 512..at + 512 + size].to_vec();
            out.push((path, mode, h[156], data));
            at += 512 + size.div_ceil(512) * 512;
        }
        out
    }

    #[test]
    fn tar_gz_has_directories_modes_and_long_paths() {
        let long = format!("game/assets/{}/{}.png", "deep".repeat(20), "y".repeat(30));
        assert!(long.len() > 100, "needs the ustar prefix");
        let files = [
            TarFile { path: "game/game".into(), bytes: b"ELF".to_vec(), executable: true },
            TarFile { path: "game/assets/a.txt".into(), bytes: vec![7; 700], executable: false },
            TarFile { path: long.clone(), bytes: b"png".to_vec(), executable: false },
        ];
        let entries = untar(&tar_gz(&files).unwrap());
        let summary: Vec<(&str, u32, u8)> = entries.iter().map(|(p, m, k, _)| (p.as_str(), *m, *k)).collect();
        assert_eq!(summary[0], ("game/", 0o755, b'5'));
        assert_eq!(summary[1], ("game/game", 0o755, b'0'));
        assert_eq!(summary[2], ("game/assets/", 0o755, b'5'));
        assert_eq!(summary[3], ("game/assets/a.txt", 0o644, b'0'));
        assert_eq!(entries[3].3, vec![7; 700]);
        let last = entries.last().unwrap();
        assert_eq!(last.0, long);
        assert_eq!(last.3, b"png");
    }
}
