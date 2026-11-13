//! Файловые операции окна Project без egui и без редактора: перенос, копирование,
//! переименование, удаление (в корзину ОС), импорт снаружи. Всё работает строго внутри
//! каталога ассетов: пути приходят относительными (`models/crystal.obj`), каждый проходит
//! [`safe_join`], а `.meta` всегда едет вместе с файлом — так GUID и ссылки на ассет живут.
//!
//! Копии получают новый GUID: два ассета с одним GUID база не принимает.

use std::path::{Path, PathBuf};

use engine_assets::Guid;

/// Итог операции над несколькими элементами: что сделано, что нет и почему.
#[derive(Default, Debug, Clone)]
pub(crate) struct Outcome {
    /// (было, стало) — пути относительно каталога ассетов.
    pub done: Vec<(String, String)>,
    /// (путь, причина).
    pub failed: Vec<(String, String)>,
}

impl Outcome {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }

    /// Одна строка для тоста и консоли: «Moved 3 of 5 items. Failed: …».
    pub fn summary(&self, past: &str, total: usize) -> String {
        let noun = |n: usize| if n == 1 { "item" } else { "items" };
        if self.failed.is_empty() {
            return format!("{past} {} {}", self.done.len(), noun(self.done.len()));
        }
        let mut text = format!("{past} {} of {total} {}.", self.done.len(), noun(total));
        for (path, why) in self.failed.iter().take(3) {
            text.push_str(&format!(" `{path}`: {why}."));
        }
        if self.failed.len() > 3 {
            text.push_str(&format!(" And {} more failed.", self.failed.len() - 3));
        }
        if self.done.is_empty() {
            text.push_str(" Nothing was changed.");
        } else {
            text.push_str(" The rest was done.");
        }
        text
    }
}

/// Путь `.meta` рядом с файлом.
pub(crate) fn meta_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".meta");
    PathBuf::from(name)
}

/// Склеить корень и относительный путь. Отказ — для `..`, абсолютных путей, дисков и пустого
/// пути: любая операция остаётся внутри каталога ассетов.
pub(crate) fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = relative.trim_matches('/');
    if relative.is_empty() {
        return Err("the path is empty".to_string());
    }
    let ok = Path::new(relative).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if !ok || relative.contains('\\') {
        return Err(format!("`{relative}` is not a path inside the assets directory"));
    }
    Ok(root.join(relative))
}

/// Как [`safe_join`], но пустая строка — сам каталог ассетов (корень как место назначения).
pub(crate) fn dir_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.trim_matches('/').is_empty() {
        Ok(root.to_path_buf())
    } else {
        safe_join(root, relative)
    }
}

pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Можно ли так назвать файл или каталог. Текст ошибки показывается под полем ввода.
pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("The name cannot be empty".to_string());
    }
    if name != name.trim() {
        return Err("The name cannot start or end with a space".to_string());
    }
    if name.ends_with('.') {
        return Err("The name cannot end with a dot".to_string());
    }
    if let Some(bad) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Err(if bad.is_control() {
            "The name contains a control character".to_string()
        } else {
            format!("The name cannot contain `{bad}`")
        });
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(format!("`{stem}` is a reserved name on Windows"));
    }
    if name.to_ascii_lowercase().ends_with(".meta") {
        return Err("`.meta` files belong to the asset database".to_string());
    }
    if name.chars().count() > 200 {
        return Err("The name is too long".to_string());
    }
    Ok(())
}

/// `New Folder (3)` → (`New Folder`, 3). Нет суффикса — `(name, 0)`.
fn strip_counter(stem: &str) -> &str {
    if let Some(open) = stem.rfind(" (") {
        let tail = &stem[open + 2..];
        if let Some(digits) = tail.strip_suffix(')') {
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                return &stem[..open];
            }
        }
    }
    stem
}

/// Имя без конфликта в каталоге: `a.png` → `a (1).png`, `Folder` → `Folder (1)`.
/// Занятым считается и имя, у которого уже лежит `.meta`.
pub(crate) fn unique_name(dir: &Path, name: &str, is_dir: bool) -> String {
    let taken = |candidate: &str| {
        let path = dir.join(candidate);
        path.exists() || meta_of(&path).exists()
    };
    if !taken(name) {
        return name.to_string();
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    let stem = strip_counter(stem);
    for n in 1.. {
        let candidate = format!("{stem} ({n}){extension}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("the loop above always returns")
}

fn io_text(action: &str, path: &str, err: &std::io::Error) -> String {
    let hint = match err.kind() {
        std::io::ErrorKind::PermissionDenied => " (read-only or in use by another program)",
        std::io::ErrorKind::NotFound => " (it no longer exists; press Refresh)",
        _ => "",
    };
    format!("cannot {action} `{path}`: {err}{hint}")
}

/// Создать каталог `parent/name` (имя уже проверено). Возвращает путь созданного.
pub(crate) fn create_folder(root: &Path, parent: &str, name: &str) -> Result<String, String> {
    validate_name(name)?;
    let dir = dir_join(root, parent)?;
    let target = dir.join(name);
    if target.exists() {
        return Err(format!("`{name}` already exists here"));
    }
    std::fs::create_dir(&target).map_err(|e| io_text("create", name, &e))?;
    Ok(join(parent, name))
}

/// Переименовать на месте: `from` → тот же каталог, имя `new_name`. `.meta` едет следом.
pub(crate) fn rename_item(root: &Path, from: &str, new_name: &str) -> Result<String, String> {
    validate_name(new_name)?;
    let source = safe_join(root, from)?;
    if !source.exists() {
        return Err(format!("`{from}` no longer exists"));
    }
    let to = join(parent_of(from), new_name);
    if to == from {
        return Ok(to);
    }
    let target = safe_join(root, &to)?;
    // Смена регистра (`a.png` → `A.png`) на Windows — тот же файл: это допустимо.
    let same_file = to.eq_ignore_ascii_case(from);
    if !same_file && (target.exists() || meta_of(&target).exists()) {
        return Err(format!("`{new_name}` already exists here"));
    }
    move_with_meta(&source, &target, from)?;
    Ok(to)
}

/// Переименовать файл и его `.meta`; если `.meta` не поехал — вернуть файл на место.
fn move_with_meta(source: &Path, target: &Path, label: &str) -> Result<(), String> {
    std::fs::rename(source, target).map_err(|e| io_text("move", label, &e))?;
    let (old_meta, new_meta) = (meta_of(source), meta_of(target));
    if old_meta.is_file() {
        if let Err(err) = std::fs::rename(&old_meta, &new_meta) {
            let _ = std::fs::rename(target, source);
            return Err(io_text("move the .meta of", label, &err));
        }
    }
    Ok(())
}

/// Перенести элемент в каталог `dest` (`""` — корень). Занятое имя получает `(1)`.
/// Возвращает новый путь; перенос в тот же каталог — это ничего не менять.
pub(crate) fn move_into(root: &Path, from: &str, dest: &str) -> Result<String, String> {
    let source = safe_join(root, from)?;
    let dest_dir = dir_join(root, dest)?;
    if !source.exists() {
        return Err(format!("`{from}` no longer exists"));
    }
    if !dest_dir.is_dir() {
        return Err(format!("the folder `{dest}` does not exist"));
    }
    let from = from.trim_matches('/');
    let dest = dest.trim_matches('/');
    if source.is_dir() && (dest == from || dest.starts_with(&format!("{from}/"))) {
        return Err("a folder cannot be moved into itself".to_string());
    }
    if parent_of(from) == dest {
        return Ok(from.to_string());
    }
    let name = unique_name(&dest_dir, file_name(from), source.is_dir());
    move_with_meta(&source, &dest_dir.join(&name), from)?;
    Ok(join(dest, &name))
}

/// Копировать элемент в каталог `dest`; копия получает свободное имя и новые GUID.
pub(crate) fn copy_into(root: &Path, from: &str, dest: &str) -> Result<String, String> {
    let source = safe_join(root, from)?;
    let dest_dir = dir_join(root, dest)?;
    if !source.exists() {
        return Err(format!("`{from}` no longer exists"));
    }
    if !dest_dir.is_dir() {
        return Err(format!("the folder `{dest}` does not exist"));
    }
    let from = from.trim_matches('/');
    let dest = dest.trim_matches('/');
    if source.is_dir() && (dest == from || dest.starts_with(&format!("{from}/"))) {
        return Err("a folder cannot be copied into itself".to_string());
    }
    let name = unique_name(&dest_dir, file_name(from), source.is_dir());
    let target = dest_dir.join(&name);
    if source.is_dir() {
        if let Err(err) = copy_tree(&source, &target, true, &mut |_| {}) {
            // Недокопированный каталог не оставляем.
            let _ = std::fs::remove_dir_all(&target);
            return Err(err);
        }
    } else {
        copy_file(&source, &target, true)?;
    }
    Ok(join(dest, &name))
}

/// Подменить GUID в тексте `.meta`. `None` — GUID не нашёлся.
pub(crate) fn meta_with_new_guid(text: &str) -> Option<String> {
    let at = text.find("guid:")?;
    let rest = &text[at + 5..];
    let open = rest.find('"')?;
    let close = rest[open + 1..].find('"')?;
    let old = &rest[open + 1..open + 1 + close];
    Some(text.replacen(old, &Guid::generate().to_string(), 1))
}

fn copy_file(source: &Path, target: &Path, with_meta: bool) -> Result<(), String> {
    let label = source.display().to_string();
    std::fs::copy(source, target).map_err(|e| io_text("copy", &label, &e))?;
    if with_meta {
        let meta = meta_of(source);
        if let Ok(text) = std::fs::read_to_string(&meta) {
            // Нет разобранного GUID — `.meta` не копируем: база заведёт новый.
            if let Some(text) = meta_with_new_guid(&text) {
                std::fs::write(meta_of(target), text).map_err(|e| io_text("write the .meta of", &label, &e))?;
            }
        }
    }
    Ok(())
}

/// Рекурсивная копия каталога. Символические ссылки пропускаются (не выходим за корень).
/// `tick` вызывается после каждого файла.
fn copy_tree(source: &Path, target: &Path, with_meta: bool, tick: &mut dyn FnMut(&Path)) -> Result<(), String> {
    std::fs::create_dir(target).map_err(|e| io_text("create", &target.display().to_string(), &e))?;
    let entries = std::fs::read_dir(source).map_err(|e| io_text("read", &source.display().to_string(), &e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() || path.extension().is_some_and(|e| e == "meta") {
            continue;
        }
        let destination = target.join(&name);
        if kind.is_dir() {
            copy_tree(&path, &destination, with_meta, tick)?;
        } else {
            copy_file(&path, &destination, with_meta)?;
            tick(&path);
        }
    }
    Ok(())
}

/// Сколько обычных файлов (без `.meta`) в дереве.
fn count_files(path: &Path) -> usize {
    let Ok(kind) = std::fs::symlink_metadata(path) else { return 0 };
    if kind.is_symlink() {
        return 0;
    }
    if kind.is_file() {
        return usize::from(path.extension().is_none_or(|e| e != "meta"));
    }
    std::fs::read_dir(path).map_or(0, |entries| entries.flatten().map(|e| count_files(&e.path())).sum())
}

/// Скопировать файлы и каталоги СНАРУЖИ в каталог `dest` ассетов. `.meta` источников
/// не берём: база создаст свои. Возвращает пути созданного верхнего уровня.
/// `progress(сделано, всего)` вызывается после каждого файла.
pub(crate) fn import_external(
    root: &Path,
    dest: &str,
    sources: &[PathBuf],
    progress: &mut dyn FnMut(usize, usize),
) -> Outcome {
    let mut outcome = Outcome::default();
    let dest_dir = match dir_join(root, dest) {
        Ok(dir) if dir.is_dir() => dir,
        _ => {
            for source in sources {
                outcome.failed.push((source.display().to_string(), format!("the folder `{dest}` does not exist")));
            }
            return outcome;
        }
    };
    let total: usize = sources.iter().map(|s| count_files(s)).sum();
    let mut done = 0usize;
    progress(0, total);
    for source in sources {
        let label = source.display().to_string();
        let Ok(kind) = std::fs::symlink_metadata(source) else {
            outcome.failed.push((label, "the file is gone".to_string()));
            continue;
        };
        if kind.is_symlink() {
            outcome.failed.push((label, "links are not imported".to_string()));
            continue;
        }
        let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            outcome.failed.push((label, "the path has no file name".to_string()));
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".meta") {
            continue;
        }
        let name = unique_name(&dest_dir, &name, kind.is_dir());
        let target = dest_dir.join(&name);
        let result = if kind.is_dir() {
            if root.join(dest).starts_with(source) {
                Err("a folder cannot be imported into itself".to_string())
            } else {
                let mut tick = |_: &Path| {
                    done += 1;
                    progress(done, total);
                };
                copy_tree(source, &target, false, &mut tick).inspect_err(|_| {
                    let _ = std::fs::remove_dir_all(&target);
                })
            }
        } else {
            copy_file(source, &target, false).map(|()| {
                done += 1;
                progress(done, total);
            })
        };
        match result {
            Ok(()) => outcome.done.push((label, join(dest, &name))),
            Err(why) => outcome.failed.push((label, why)),
        }
    }
    outcome
}

/// Удалить насовсем. Только файлы внутри каталога ассетов; `.meta` — вместе с файлом.
pub(crate) fn delete_permanently(root: &Path, relative: &[String]) -> Outcome {
    let mut outcome = Outcome::default();
    for rel in relative {
        let result = safe_join(root, rel)
            .and_then(|path| delete_one(&path).map_err(|e| io_text("delete", rel, &e)));
        match result {
            Ok(()) => outcome.done.push((rel.clone(), String::new())),
            Err(why) => outcome.failed.push((rel.clone(), why)),
        }
    }
    outcome
}

fn delete_one(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return remove_dir_all_writable(path);
    }
    remove_file_writable(path)?;
    let meta = meta_of(path);
    if meta.is_file() {
        remove_file_writable(&meta)?;
    }
    Ok(())
}

fn remove_file_writable(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            // Файл только для чтения: снять флаг и повторить.
            let mut permissions = std::fs::metadata(path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            std::fs::set_permissions(path, permissions)?;
            std::fs::remove_file(path)
        }
        other => other,
    }
}

fn remove_dir_all_writable(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            clear_readonly(path);
            std::fs::remove_dir_all(path)
        }
        other => other,
    }
}

fn clear_readonly(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            clear_readonly(&entry.path());
        } else if let Ok(meta) = entry.metadata() {
            let mut permissions = meta.permissions();
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(entry.path(), permissions);
            }
        }
    }
}

// ----------------------------------------------------------------------
// Корзина
// ----------------------------------------------------------------------

/// Умеет ли эта система отправлять файлы в корзину. Нет — удаление насовсем и с вопросом.
pub(crate) fn recycle_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        true
    }
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FOUND.get_or_init(|| {
            std::process::Command::new("gio")
                .arg("version")
                .output()
                .is_ok_and(|out| out.status.success())
        })
    }
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    {
        false
    }
}

/// Отправить файлы и каталоги в корзину ОС одним вызовом. После вызова проверяется, что
/// каждый путь действительно исчез; иначе — ошибка, и пользователь узнаёт, что осталось.
pub(crate) fn trash(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    run_trash(paths)?;
    let left: Vec<String> = paths.iter().filter(|p| p.exists()).map(|p| p.display().to_string()).collect();
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!("the Recycle Bin did not take: {}", left.join(", ")))
    }
}

#[cfg(target_os = "windows")]
fn run_trash(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // Без `unsafe` и без новых зависимостей: Windows PowerShell зовёт .NET
    // `FileSystem.DeleteFile(..., SendToRecycleBin)`. Пути идут через файл-список, не через
    // командную строку, — кавычки и юникод в именах не ломают вызов.
    const SCRIPT: &str = "$ErrorActionPreference='Stop'\r\n\
        Add-Type -AssemblyName Microsoft.VisualBasic\r\n\
        $list = Get-Content -LiteralPath $env:PISKA_TRASH_LIST -Encoding UTF8\r\n\
        foreach ($p in $list) {\r\n\
          if ([string]::IsNullOrWhiteSpace($p)) { continue }\r\n\
          if (Test-Path -LiteralPath $p -PathType Container) {\r\n\
            [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin')\r\n\
          } elseif (Test-Path -LiteralPath $p) {\r\n\
            [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin')\r\n\
          }\r\n\
        }\r\n";
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let list = std::env::temp_dir().join(format!(
        "piska_trash_{}_{}.txt",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
    std::fs::write(&list, text).map_err(|e| format!("cannot prepare the Recycle Bin request: {e}"))?;
    let encoded = base64_utf16le(SCRIPT);
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
        .env("PISKA_TRASH_LIST", &list)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
    let _ = std::fs::remove_file(&list);
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.lines().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_string();
            Err(format!("the Recycle Bin refused: {why}"))
        }
        Err(err) => Err(format!("cannot start PowerShell to use the Recycle Bin: {err}")),
    }
}

#[cfg(target_os = "macos")]
fn run_trash(paths: &[PathBuf]) -> Result<(), String> {
    let list: Vec<String> = paths
        .iter()
        .map(|p| format!("POSIX file \"{}\"", p.display().to_string().replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    let script = format!("tell application \"Finder\" to delete {{{}}}", list.join(", "));
    match std::process::Command::new("osascript").args(["-e", &script]).output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!("Finder refused: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(err) => Err(format!("cannot start osascript: {err}")),
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
fn run_trash(paths: &[PathBuf]) -> Result<(), String> {
    match std::process::Command::new("gio").arg("trash").arg("--").args(paths).output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!("gio trash refused: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(err) => Err(format!("cannot start `gio trash`: {err}")),
    }
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn run_trash(_paths: &[PathBuf]) -> Result<(), String> {
    Err("this platform has no Recycle Bin".to_string())
}

/// Base64 от UTF-16LE — формат `powershell -EncodedCommand`.
#[cfg(target_os = "windows")]
fn base64_utf16le(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "piska_fs_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn asset(root: &Path, rel: &str, guid: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rel).unwrap();
        std::fs::write(meta_of(&path), format!("(\n    guid: \"{guid}\",\n    importer: \"Raw\",\n)\n")).unwrap();
    }

    #[test]
    fn paths_must_stay_inside_the_root() {
        let root = Path::new("/assets");
        assert!(safe_join(root, "a/b.png").is_ok());
        assert!(safe_join(root, "../x").is_err());
        assert!(safe_join(root, "a/../../x").is_err());
        assert!(safe_join(root, "").is_err());
        assert!(safe_join(root, "C:/Windows").is_err());
        assert!(safe_join(root, "a\\b").is_err());
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("good name.png").is_ok());
        assert!(validate_name("имя файла.png").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name(" a").is_err());
        assert!(validate_name("a.").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("what?").is_err());
        assert!(validate_name("NUL.txt").is_err());
        assert!(validate_name("x.meta").is_err());
    }

    #[test]
    fn unique_names_count_up_and_respect_meta() {
        let root = temp("unique");
        asset(&root, "a.png", "11111111111111111111111111111111");
        assert_eq!(unique_name(&root, "b.png", false), "b.png");
        assert_eq!(unique_name(&root, "a.png", false), "a (1).png");
        asset(&root, "a (1).png", "22222222222222222222222222222222");
        assert_eq!(unique_name(&root, "a (1).png", false), "a (2).png", "the counter is replaced, not stacked");
        std::fs::create_dir(root.join("Folder.v2")).unwrap();
        assert_eq!(unique_name(&root, "Folder.v2", true), "Folder.v2 (1)", "folders have no extension");
        std::fs::write(meta_of(&root.join("orphan.txt")), "x").unwrap();
        assert_eq!(unique_name(&root, "orphan.txt", false), "orphan (1).txt", "an orphan .meta blocks the name");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn move_takes_the_meta_along_and_resolves_conflicts() {
        let root = temp("move");
        asset(&root, "a.png", "11111111111111111111111111111111");
        asset(&root, "sub/a.png", "22222222222222222222222222222222");
        std::fs::create_dir(root.join("dst")).unwrap();
        let moved = move_into(&root, "a.png", "dst").unwrap();
        assert_eq!(moved, "dst/a.png");
        assert!(!root.join("a.png").exists() && !root.join("a.png.meta").exists());
        assert!(root.join("dst/a.png.meta").is_file());
        // Конфликт: имя занято — второй файл получает `(1)`.
        let again = move_into(&root, "sub/a.png", "dst").unwrap();
        assert_eq!(again, "dst/a (1).png");
        assert!(root.join("dst/a (1).png.meta").is_file());
        // В тот же каталог — ничего не меняется.
        assert_eq!(move_into(&root, "dst/a.png", "dst").unwrap(), "dst/a.png");
        // Каталог в самого себя — отказ.
        assert!(move_into(&root, "dst", "dst").is_err());
        assert!(move_into(&root, "sub", "sub/deeper").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_gives_a_new_guid_and_copies_folders() {
        let root = temp("copy");
        asset(&root, "tex/a.png", "11111111111111111111111111111111");
        asset(&root, "tex/deep/b.png", "22222222222222222222222222222222");
        let copy = copy_into(&root, "tex", "").unwrap();
        assert_eq!(copy, "tex (1)");
        let meta = std::fs::read_to_string(root.join("tex (1)/a.png.meta")).unwrap();
        assert!(!meta.contains("11111111111111111111111111111111"), "the copy must not reuse the GUID");
        assert!(meta.contains("guid:"));
        assert!(root.join("tex (1)/deep/b.png.meta").is_file());
        assert!(copy_into(&root, "tex", "tex/deep").is_err(), "a folder cannot be copied into itself");
        // Дубликат файла рядом с оригиналом.
        assert_eq!(copy_into(&root, "tex/a.png", "tex").unwrap(), "tex/a (1).png");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_keeps_the_meta_and_validates() {
        let root = temp("rename");
        asset(&root, "a.png", "11111111111111111111111111111111");
        asset(&root, "b.png", "22222222222222222222222222222222");
        assert_eq!(rename_item(&root, "a.png", "c.png").unwrap(), "c.png");
        assert!(root.join("c.png.meta").is_file() && !root.join("a.png.meta").exists());
        assert!(rename_item(&root, "c.png", "b.png").unwrap_err().contains("already exists"));
        assert!(rename_item(&root, "c.png", "x/y.png").is_err());
        assert_eq!(rename_item(&root, "c.png", "C.png").unwrap(), "C.png", "changing only the case is allowed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_copies_files_without_their_meta_and_reports_progress() {
        let root = temp("import_root");
        let outside = temp("import_src");
        std::fs::write(outside.join("one.txt"), "1").unwrap();
        std::fs::write(outside.join("one.txt.meta"), "alien").unwrap();
        std::fs::create_dir_all(outside.join("pack/inner")).unwrap();
        std::fs::write(outside.join("pack/inner/two.txt"), "2").unwrap();
        std::fs::write(root.join("one.txt"), "mine").unwrap();
        let mut last = (0, 0);
        let outcome = import_external(
            &root,
            "",
            &[outside.join("one.txt"), outside.join("pack"), outside.join("gone.txt")],
            &mut |done, total| last = (done, total),
        );
        assert_eq!(last, (2, 2));
        assert_eq!(outcome.done.len(), 2);
        assert_eq!(outcome.failed.len(), 1, "the missing file is reported, the rest is imported");
        assert_eq!(std::fs::read_to_string(root.join("one.txt")).unwrap(), "mine", "an existing file is never overwritten");
        assert_eq!(std::fs::read_to_string(root.join("one (1).txt")).unwrap(), "1");
        assert!(!root.join("one (1).txt.meta").exists(), "the alien .meta is not imported");
        assert!(root.join("pack/inner/two.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn permanent_delete_handles_read_only_files_and_stays_inside() {
        let root = temp("delete");
        asset(&root, "ro.txt", "11111111111111111111111111111111");
        let mut permissions = std::fs::metadata(root.join("ro.txt")).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(root.join("ro.txt"), permissions).unwrap();
        let outside = temp("delete_outside");
        std::fs::write(outside.join("keep.txt"), "x").unwrap();
        let outcome = delete_permanently(&root, &["ro.txt".to_string(), "../keep.txt".to_string(), "missing.txt".to_string()]);
        assert!(!root.join("ro.txt").exists() && !root.join("ro.txt.meta").exists());
        assert_eq!(outcome.done.len(), 1);
        assert_eq!(outcome.failed.len(), 2);
        assert!(outside.join("keep.txt").exists(), "nothing outside the root is touched");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn summary_names_what_was_and_was_not_done() {
        let outcome = Outcome {
            done: vec![("a".into(), "b".into())],
            failed: vec![("c.png".into(), "cannot move".into())],
        };
        let text = outcome.summary("Moved", 2);
        assert!(text.contains("Moved 1 of 2 items") && text.contains("c.png") && text.contains("The rest was done"));
    }
}
