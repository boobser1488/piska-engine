//! Проверка исходников интерфейса (редактор, хаб, Modeler): контролы — только через набор
//! (`kit::controls`), промежутки — только токены шкалы. Сырые `ui.button(`, `Button::new(`,
//! `TextEdit::`, `ComboBox::`, `DragValue::new(` вне файлов набора допускаются не больше, чем
//! записано в `tools/ui_raw_allowlist.txt` (число только уменьшается), `add_space(<число>)` —
//! никогда.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ROOTS: [&str; 4] = ["crates/engine-editor/src", "tools/piska-hub/src", "tools/piska-installer/src", "modeler/crates/engine-modeler/src/ui"];
/// Файлы набора — в них сырые виджеты egui и есть реализация контролов.
const KIT: [&str; 8] = [
    "crates/engine-editor/src/controls.rs",
    "crates/engine-editor/src/theme.rs",
    "crates/engine-editor/src/window_kit.rs",
    "crates/engine-editor/src/metrics.rs",
    "modeler/crates/engine-modeler/src/ui/kit.rs",
    "tools/piska-hub/src/widgets.rs",
    "modeler/crates/engine-modeler/src/ui/controls.rs",
    "modeler/crates/engine-modeler/src/ui/metrics.rs",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn patterns() -> Vec<(&'static str, fn(&str) -> usize)> {
    fn count(line: &str, needle: &str) -> usize {
        line.matches(needle).count()
    }
    vec![
        ("ui.button(", |l| {
            l.match_indices("ui.button(").filter(|(i, _)| *i == 0 || !l[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '_')).count()
        }),
        ("Button::new(", |l| count(l, "Button::new(")),
        ("TextEdit::", |l| count(l, "TextEdit::")),
        ("ui.small_button(", |l| count(l, "ui.small_button(")),
        ("ComboBox::", |l| count(l, "ComboBox::")),
        ("DragValue::new(", |l| count(l, "DragValue::new(")),
        ("add_space(literal)", |l| {
            l.match_indices("add_space(").filter(|(i, m)| l[i + m.len()..].trim_start().starts_with(|c: char| c.is_ascii_digit())).count()
        }),
    ]
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn ui_code_uses_the_kit_and_the_spacing_scale() {
    let root = root();
    let allow_text = std::fs::read_to_string(root.join("tools/ui_raw_allowlist.txt")).expect("tools/ui_raw_allowlist.txt");
    let mut allowed: BTreeMap<(String, String), usize> = BTreeMap::new();
    for line in allow_text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let line = line.split('#').next().unwrap_or("").trim_end();
        let parts: Vec<&str> = line.split('\t').collect();
        if let [file, pattern, n] = parts[..] {
            allowed.insert((file.to_string(), pattern.to_string()), n.trim().parse().expect("a count"));
        }
    }
    let mut problems = Vec::new();
    let mut lower = Vec::new();
    for dir in ROOTS {
        let mut files = Vec::new();
        walk(&root.join(dir), &mut files);
        for file in files {
            let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let rel = rel.trim_start_matches("./").to_string();
            if KIT.contains(&rel.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
            for (name, counter) in patterns() {
                let n: usize = lines.iter().map(|l| counter(l)).sum();
                let max = if name == "add_space(literal)" { 0 } else { allowed.get(&(rel.clone(), name.to_string())).copied().unwrap_or(0) };
                if n > max {
                    problems.push(format!("{rel}: {n} × `{name}` (allowed {max}) — use kit::controls / theme spacing tokens"));
                } else if n < max {
                    lower.push(format!("{rel}\t{name}\t{n}"));
                }
            }
        }
    }
    if !lower.is_empty() {
        eprintln!("tools/ui_raw_allowlist.txt can be lowered:\n{}", lower.join("\n"));
    }
    assert!(problems.is_empty(), "raw egui widgets / literal spacing outside the kit:\n{}", problems.join("\n"));
}
