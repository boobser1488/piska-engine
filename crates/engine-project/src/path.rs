//! Проверка путей из файла проекта: относительный, без `..`, без «хитростей» Windows.

/// Имена устройств Windows: файл с таким именем (с любым расширением) открыл бы устройство.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2",
    "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Путь из файла безопасен? Путь относительный, с `/`, без `.`/`..`, дисков, потоков NTFS,
/// управляющих символов и имён устройств. `Err` — причина.
pub fn validate_relative(name: &str, max_len: usize) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("the path is empty");
    }
    if name.len() > max_len {
        return Err("the path is too long");
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("the path contains a control character");
    }
    if name.contains('\\') {
        return Err("the path contains a backslash (paths use `/`)");
    }
    if name.starts_with('/') {
        return Err("the path is absolute");
    }
    if name.contains(':') {
        return Err("the path contains `:` (a drive letter or an NTFS stream)");
    }
    for part in name.split('/') {
        match part {
            "" => return Err("the path has an empty component"),
            "." => return Err("the path contains `.`"),
            ".." => return Err("the path leaves the project folder (`..`)"),
            _ => {}
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err("a path component ends with a dot or a space (Windows would rename it)");
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
        if RESERVED.contains(&stem.as_str()) {
            return Err("the path uses a reserved Windows device name");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_and_bad_paths() {
        for ok in ["Cargo.toml", "assets/a b/c.png", "src/lib.rs", ".gitignore", "a/.hidden/x"] {
            assert_eq!(validate_relative(ok, 1024), Ok(()), "{ok}");
        }
        for bad in [
            "", "/etc/passwd", "C:/x", "a/../b", "../x", "a/./b", "a//b", "a/", "a\\b", "x\0y", "nul", "dir/COM1.txt", "a/b.", "a /b",
            "a:b", "..", ".",
        ] {
            assert!(validate_relative(bad, 1024).is_err(), "{bad:?} must be refused");
        }
        assert!(validate_relative(&"a".repeat(2000), 1024).is_err());
    }
}
