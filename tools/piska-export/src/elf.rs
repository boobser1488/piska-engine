//! Чтение 64-битных ELF (little-endian): что нужно, чтобы проверить собранную
//! библиотеку Android или программу Linux без внешних инструментов — интерпретатор,
//! список `NEEDED`, экспортируемые символы, требуемые версии glibc.

use crate::{Error, Result};

const SHT_SYMTAB_DYN: u32 = 11; // SHT_DYNSYM
const SHT_DYNAMIC: u32 = 6;
const SHT_GNU_VERNEED: u32 = 0x6fff_fffe;
const PT_INTERP: u32 = 3;
const DT_NEEDED: u64 = 1;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

/// Сводка по ELF-файлу.
#[derive(Debug, Default)]
pub struct ElfInfo {
    /// Архитектура: `"x86_64"`, `"aarch64"` или номер `e_machine`.
    pub machine: String,
    /// Динамический загрузчик (`PT_INTERP`), у библиотек — `None`.
    pub interpreter: Option<String>,
    /// Нужные библиотеки (`DT_NEEDED`) в порядке файла.
    pub needed: Vec<String>,
    /// Определённые в файле динамические символы (то, что видно `dlsym`).
    pub exported: Vec<String>,
    /// Требуемые версии символов по библиотекам: `("libc.so.6", "GLIBC_2.17")`.
    pub version_needs: Vec<(String, String)>,
}

impl ElfInfo {
    pub fn exports(&self, symbol: &str) -> bool {
        self.exported.iter().any(|s| s == symbol)
    }

    /// Наибольшая требуемая версия glibc (`GLIBC_2.34` → `(2, 34)`).
    pub fn max_glibc(&self) -> Option<(u32, u32)> {
        self.version_needs
            .iter()
            .filter_map(|(_, version)| parse_glibc(version))
            .max()
    }
}

/// `"GLIBC_2.31"` → `(2, 31)`; `GLIBC_PRIVATE` и прочее — `None`.
pub fn parse_glibc(version: &str) -> Option<(u32, u32)> {
    let rest = version.strip_prefix("GLIBC_")?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |m| m.parse().ok())?;
    Some((major, minor))
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn slice(&self, offset: u64, len: u64) -> Result<&[u8]> {
        let start = usize::try_from(offset).map_err(|_| bad())?;
        let len = usize::try_from(len).map_err(|_| bad())?;
        self.bytes.get(start..start.checked_add(len).ok_or_else(bad)?).ok_or_else(bad)
    }
    fn u16(&self, offset: u64) -> Result<u16> {
        Ok(u16::from_le_bytes(self.slice(offset, 2)?.try_into().map_err(|_| bad())?))
    }
    fn u32(&self, offset: u64) -> Result<u32> {
        Ok(u32::from_le_bytes(self.slice(offset, 4)?.try_into().map_err(|_| bad())?))
    }
    fn u64(&self, offset: u64) -> Result<u64> {
        Ok(u64::from_le_bytes(self.slice(offset, 8)?.try_into().map_err(|_| bad())?))
    }
    /// Строка с нулём в конце из таблицы строк по смещению.
    fn cstr(&self, table: &Section, offset: u64) -> Result<String> {
        let bytes = self.slice(table.offset, table.size)?;
        let start = usize::try_from(offset).map_err(|_| bad())?;
        let tail = bytes.get(start..).ok_or_else(bad)?;
        let end = tail.iter().position(|&b| b == 0).ok_or_else(bad)?;
        Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
    }
}

fn bad() -> Error {
    Error("the file is not a valid 64-bit ELF (truncated or corrupt)".to_string())
}

#[derive(Clone, Copy, Debug)]
struct Section {
    kind: u32,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    entsize: u64,
}

/// Разобрать ELF64 little-endian (aarch64, x86_64).
pub fn parse(bytes: &[u8]) -> Result<ElfInfo> {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" {
        return Err(Error("not an ELF file".to_string()));
    }
    if bytes[4] != 2 || bytes[5] != 1 {
        return Err(Error("only 64-bit little-endian ELF files are supported".to_string()));
    }
    let r = Reader { bytes };
    let machine = match r.u16(18)? {
        EM_X86_64 => "x86_64".to_string(),
        EM_AARCH64 => "aarch64".to_string(),
        other => other.to_string(),
    };
    let phoff = r.u64(32)?;
    let shoff = r.u64(40)?;
    let phentsize = u64::from(r.u16(54)?);
    let phnum = u64::from(r.u16(56)?);
    let shentsize = u64::from(r.u16(58)?);
    let shnum = u64::from(r.u16(60)?);

    let mut info = ElfInfo {
        machine,
        ..ElfInfo::default()
    };

    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if r.u32(ph)? == PT_INTERP {
            let offset = r.u64(ph + 8)?;
            let size = r.u64(ph + 32)?;
            let raw = r.slice(offset, size)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            info.interpreter = Some(String::from_utf8_lossy(&raw[..end]).into_owned());
        }
    }

    let mut sections = Vec::new();
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        sections.push(Section {
            kind: r.u32(sh + 4)?,
            offset: r.u64(sh + 24)?,
            size: r.u64(sh + 32)?,
            link: r.u32(sh + 40)?,
            info: r.u32(sh + 44)?,
            entsize: r.u64(sh + 56)?,
        });
    }
    let linked = |section: &Section| sections.get(section.link as usize).copied().ok_or_else(bad);

    for section in &sections {
        match section.kind {
            SHT_DYNAMIC => {
                let strings = linked(section)?;
                let count = section.size / 16;
                for i in 0..count {
                    let tag = r.u64(section.offset + i * 16)?;
                    if tag == 0 {
                        break;
                    }
                    if tag == DT_NEEDED {
                        let value = r.u64(section.offset + i * 16 + 8)?;
                        info.needed.push(r.cstr(&strings, value)?);
                    }
                }
            }
            SHT_SYMTAB_DYN => {
                let strings = linked(section)?;
                let entsize = if section.entsize == 0 { 24 } else { section.entsize };
                for i in 1..section.size / entsize {
                    let sym = section.offset + i * entsize;
                    let name = u64::from(r.u32(sym)?);
                    let shndx = r.u16(sym + 6)?;
                    // shndx 0 — символ не определён здесь (его ищут в других библиотеках).
                    if shndx != 0 && name != 0 {
                        info.exported.push(r.cstr(&strings, name)?);
                    }
                }
            }
            SHT_GNU_VERNEED => {
                let strings = linked(section)?;
                let mut entry = section.offset;
                for _ in 0..section.info {
                    let count = r.u16(entry + 2)?;
                    let file = r.cstr(&strings, u64::from(r.u32(entry + 4)?))?;
                    let mut aux = entry + u64::from(r.u32(entry + 8)?);
                    for _ in 0..count {
                        let name = r.cstr(&strings, u64::from(r.u32(aux + 8)?))?;
                        info.version_needs.push((file.clone(), name));
                        let next = r.u32(aux + 12)?;
                        if next == 0 {
                            break;
                        }
                        aux += u64::from(next);
                    }
                    let next = r.u32(entry + 12)?;
                    if next == 0 {
                        break;
                    }
                    entry += u64::from(next);
                }
            }
            _ => {}
        }
    }
    Ok(info)
}

pub fn parse_file(path: &std::path::Path) -> Result<ElfInfo> {
    let bytes = std::fs::read(path).map_err(|e| Error(format!("cannot read `{}`: {e}", path.display())))?;
    parse(&bytes).map_err(|e| Error(format!("`{}`: {e}", path.display())))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Минимальный ELF64 x86_64: PT_INTERP, .dynamic с двумя NEEDED, .dynsym с одним
    /// определённым и одним внешним символом, .gnu.version_r с двумя версиями glibc.
    pub(crate) fn sample_elf() -> Vec<u8> {
        let mut strtab = vec![0u8];
        let mut add = |s: &str| {
            let at = strtab.len() as u32;
            strtab.extend_from_slice(s.as_bytes());
            strtab.push(0);
            at
        };
        let libc = add("libc.so.6");
        let libm = add("libm.so.6");
        let main_sym = add("piska_entry");
        let ext_sym = add("printf");
        let v1 = add("GLIBC_2.17");
        let v2 = add("GLIBC_2.29");
        let interp = b"/lib64/ld-linux-x86-64.so.2\0".to_vec();

        let mut dynamic = Vec::new();
        for (tag, value) in [(1u64, libc as u64), (1, libm as u64), (0, 0)] {
            dynamic.extend_from_slice(&tag.to_le_bytes());
            dynamic.extend_from_slice(&value.to_le_bytes());
        }
        let mut dynsym = vec![0u8; 24];
        for (name, shndx) in [(main_sym, 9u16), (ext_sym, 0)] {
            let mut sym = vec![0u8; 24];
            sym[0..4].copy_from_slice(&name.to_le_bytes());
            sym[6..8].copy_from_slice(&shndx.to_le_bytes());
            dynsym.extend_from_slice(&sym);
        }
        // Verneed: одна запись (libc) с двумя Vernaux.
        let mut verneed = Vec::new();
        verneed.extend_from_slice(&1u16.to_le_bytes()); // vn_version
        verneed.extend_from_slice(&2u16.to_le_bytes()); // vn_cnt
        verneed.extend_from_slice(&libc.to_le_bytes()); // vn_file
        verneed.extend_from_slice(&16u32.to_le_bytes()); // vn_aux
        verneed.extend_from_slice(&0u32.to_le_bytes()); // vn_next
        for (i, v) in [v1, v2].into_iter().enumerate() {
            verneed.extend_from_slice(&0u32.to_le_bytes()); // vna_hash
            verneed.extend_from_slice(&0u16.to_le_bytes()); // vna_flags
            verneed.extend_from_slice(&0u16.to_le_bytes()); // vna_other
            verneed.extend_from_slice(&v.to_le_bytes()); // vna_name
            verneed.extend_from_slice(&(if i == 0 { 16u32 } else { 0 }).to_le_bytes());
        }

        // Раскладка: заголовок 64, один program header (56), затем данные, затем секции.
        let mut file = vec![0u8; 64 + 56];
        let place = |file: &mut Vec<u8>, data: &[u8]| {
            while !file.len().is_multiple_of(8) {
                file.push(0);
            }
            let at = file.len() as u64;
            file.extend_from_slice(data);
            at
        };
        let interp_at = place(&mut file, &interp);
        let strtab_at = place(&mut file, &strtab);
        let dynamic_at = place(&mut file, &dynamic);
        let dynsym_at = place(&mut file, &dynsym);
        let verneed_at = place(&mut file, &verneed);
        while !file.len().is_multiple_of(8) {
            file.push(0);
        }
        let shoff = file.len() as u64;
        // Секции: 0 — пустая, 1 — строки, 2 — dynamic, 3 — dynsym, 4 — verneed.
        let sections: [(u32, u64, u64, u32, u32, u64); 5] = [
            (0, 0, 0, 0, 0, 0),
            (3, strtab_at, strtab.len() as u64, 0, 0, 0),
            (SHT_DYNAMIC, dynamic_at, dynamic.len() as u64, 1, 0, 16),
            (SHT_SYMTAB_DYN, dynsym_at, dynsym.len() as u64, 1, 1, 24),
            (SHT_GNU_VERNEED, verneed_at, verneed.len() as u64, 1, 1, 0),
        ];
        for (kind, offset, size, link, info, entsize) in sections {
            let mut sh = vec![0u8; 64];
            sh[4..8].copy_from_slice(&kind.to_le_bytes());
            sh[24..32].copy_from_slice(&offset.to_le_bytes());
            sh[32..40].copy_from_slice(&size.to_le_bytes());
            sh[40..44].copy_from_slice(&link.to_le_bytes());
            sh[44..48].copy_from_slice(&info.to_le_bytes());
            sh[56..64].copy_from_slice(&entsize.to_le_bytes());
            file.extend_from_slice(&sh);
        }
        file[..4].copy_from_slice(b"\x7fELF");
        file[4] = 2;
        file[5] = 1;
        file[16..18].copy_from_slice(&3u16.to_le_bytes());
        file[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        file[32..40].copy_from_slice(&64u64.to_le_bytes());
        file[40..48].copy_from_slice(&shoff.to_le_bytes());
        file[54..56].copy_from_slice(&56u16.to_le_bytes());
        file[56..58].copy_from_slice(&1u16.to_le_bytes());
        file[58..60].copy_from_slice(&64u16.to_le_bytes());
        file[60..62].copy_from_slice(&5u16.to_le_bytes());
        // Program header: PT_INTERP.
        file[64..68].copy_from_slice(&PT_INTERP.to_le_bytes());
        file[64 + 8..64 + 16].copy_from_slice(&interp_at.to_le_bytes());
        file[64 + 32..64 + 40].copy_from_slice(&(interp.len() as u64).to_le_bytes());
        file
    }

    #[test]
    fn reads_interpreter_needed_exports_and_versions() {
        let info = parse(&sample_elf()).unwrap();
        assert_eq!(info.machine, "x86_64");
        assert_eq!(info.interpreter.as_deref(), Some("/lib64/ld-linux-x86-64.so.2"));
        assert_eq!(info.needed, ["libc.so.6", "libm.so.6"]);
        assert!(info.exports("piska_entry"));
        assert!(!info.exports("printf"), "undefined symbols are not exports");
        assert_eq!(
            info.version_needs,
            [
                ("libc.so.6".to_string(), "GLIBC_2.17".to_string()),
                ("libc.so.6".to_string(), "GLIBC_2.29".to_string())
            ]
        );
        assert_eq!(info.max_glibc(), Some((2, 29)));
    }

    #[test]
    fn rejects_non_elf_and_truncated_files() {
        assert!(parse(b"MZ\x90\x00 definitely not an elf file, long enough to pass the size check....").is_err());
        let mut elf = sample_elf();
        elf.truncate(200);
        assert!(parse(&elf).is_err());
    }

    #[test]
    fn glibc_versions_parse() {
        assert_eq!(parse_glibc("GLIBC_2.31"), Some((2, 31)));
        assert_eq!(parse_glibc("GLIBC_2.2.5"), Some((2, 2)));
        assert_eq!(parse_glibc("GLIBC_PRIVATE"), None);
        assert_eq!(parse_glibc("GCC_3.0"), None);
    }
}
