//! Восстановление несохранённых правок вкладки Code после сбоя: несохранённый текст
//! открытых файлов раз в несколько секунд пишется в файл восстановления
//! (`%APPDATA%/PiskaEngine/recovery/`, у Linux / macOS — `~/.config/PiskaEngine/recovery/`).
//! Сохранили файл или правки ушли — файл восстановления удаляется. Если редактор закрылся
//! с несохранёнными правками (сбой, снятый процесс), при следующем открытии файла вкладка
//! предлагает «Restore» / «Discard».

use std::path::{Path, PathBuf};

/// Как часто писать несохранённые правки, секунды.
const INTERVAL: f64 = 3.0;

/// Состояние автосохранения вкладки.
#[derive(Debug)]
pub(crate) struct Recovery {
    /// Каталог файлов восстановления; `None` — выключено (тесты без своего каталога).
    pub dir: Option<PathBuf>,
    last: f64,
}

impl Default for Recovery {
    fn default() -> Self {
        Self { dir: default_dir(), last: 0.0 }
    }
}

fn default_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    if let Some(dir) = std::env::var_os("PISKA_CODE_RECOVERY_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("PiskaEngine").join("recovery"))
}

/// Файл восстановления для файла `path` (имя — по хешу пути без учёта регистра).
pub(crate) fn file_for(dir: &Path, path: &Path) -> PathBuf {
    let key = path.to_string_lossy().replace('\\', "/").to_lowercase();
    // FNV-1a: имя файла не зависит от версии стандартной библиотеки.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("{hash:016x}.recovery"))
}

/// Записать несохранённый текст (первая строка — путь файла).
pub(crate) fn write(dir: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut body = path.to_string_lossy().into_owned();
    body.push('\n');
    body.push_str(text);
    std::fs::write(file_for(dir, path), body)
}

pub(crate) fn remove(dir: &Path, path: &Path) {
    let _ = std::fs::remove_file(file_for(dir, path));
}

/// Несохранённый текст прошлого сеанса для `path`, если он отличается от `disk`.
pub(crate) fn find(dir: &Path, path: &Path, disk: &str) -> Option<String> {
    let body = std::fs::read_to_string(file_for(dir, path)).ok()?;
    let (first, text) = body.split_once('\n')?;
    if !crate::code_diagnostics::same_path(Path::new(first), path) || text == disk {
        return None;
    }
    Some(text.to_string())
}

/// Кадр: раз в `INTERVAL` секунд — записать несохранённые правки и убрать файлы
/// восстановления сохранённых документов.
pub(crate) fn tick(code: &mut crate::code::CodeState, now: f64) {
    let Some(dir) = code.recovery.dir.clone() else { return };
    if now - code.recovery.last < INTERVAL && now >= code.recovery.last {
        return;
    }
    code.recovery.last = now;
    for doc in &mut code.documents {
        if doc.recovered.is_some() {
            // Предложение восстановить ещё не принято — файл не трогаем.
            continue;
        }
        if doc.dirty() {
            if doc.recovery_version != Some(doc.version) && write(&dir, &doc.path, &doc.text).is_ok() {
                doc.recovery_version = Some(doc.version);
            }
        } else if doc.recovery_version.take().is_some() {
            remove(&dir, &doc.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_round_trip() {
        let dir = std::env::temp_dir().join(format!("piska_recovery_{}", std::process::id()));
        let path = Path::new("C:/game/src/scripts/Mover.rs");
        write(&dir, path, "fn a() {}\nlet x = 1;\n").unwrap();
        assert_eq!(find(&dir, path, "fn a() {}\n").as_deref(), Some("fn a() {}\nlet x = 1;\n"));
        if cfg!(windows) {
            assert_eq!(find(&dir, Path::new("c:/GAME/src/scripts/mover.rs"), "x"), Some("fn a() {}\nlet x = 1;\n".to_string()), "the same file in another case");
        }
        assert!(find(&dir, path, "fn a() {}\nlet x = 1;\n").is_none(), "nothing to restore when the disk has it");
        remove(&dir, path);
        assert!(find(&dir, path, "").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
