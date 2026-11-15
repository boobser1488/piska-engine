//! Подсказки при наборе (Ctrl+Space и само при наборе) — честно: это индекс, а не вывод
//! типов. Источники: ключевые слова и заготовки языка, символы файла и проекта, API
//! движка (типы прелюдии, методы `Ctx`, хуки `Component` с точными сигнатурами), слова
//! файла. После `ctx.` — методы `Ctx`; после `self.` — поля и методы, объявленные в этом
//! файле; после `Тип::` — члены этого типа из индекса. Тип прочих выражений не выводится.
//! Ctrl+Shift+Space — подсказка параметров вызова под курсором.

use std::collections::BTreeSet;
use std::path::Path;

use egui::{pos2, vec2, Align2, Key, Modifiers, Pos2, Rect, Sense};

use crate::code::Language;
use crate::code_symbols::{self, Kind, Symbol};
use crate::theme;

/// С какого размера текст считается большим (подсказки — из окна вокруг курсора).
const LARGE_TEXT: usize = 32_000;

/// Пункт списка.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub label: String,
    /// Что вставить; `$0` — где окажется курсор.
    pub insert: String,
    pub kind: &'static str,
    pub detail: String,
    pub doc: String,
}

/// Открытый список подсказок.
#[derive(Clone, Debug)]
pub(crate) struct Popup {
    pub path: std::path::PathBuf,
    /// Начало набираемого слова (байт): его заменит выбранный пункт.
    pub anchor: usize,
    pub items: Vec<Item>,
    pub selected: usize,
    pub at: Pos2,
}

fn snippets(language: Language) -> Vec<Item> {
    let s = |label: &str, insert: &str, detail: &str| Item { label: label.into(), insert: insert.into(), kind: "snippet", detail: detail.into(), doc: String::new() };
    match language {
        Language::Rust => {
            let mut out = vec![
                s("component!", "component! {\n    /// $0Description.\n    pub struct Name {\n        pub speed: f32 = 1.0,\n    }\n}\n", "component! { pub struct … }"), // i18n-skip: код
                s("fn", "fn $0() {\n}", "fn name() { }"),
                s("for", "for $0 in  {\n}", "for … in … { }"), // i18n-skip: код
                s("if let", "if let Some($0) =  {\n}", "if let Some(…) = … { }"),
                s("match", "match $0 {\n    _ => {}\n}", "match … { }"), // i18n-skip: код
            ];
            // `impl Component` с хуками — сигнатуры прямо из исходника движка.
            let hooks: Vec<&Symbol> = code_symbols::engine_index().iter().filter(|s| s.container.as_deref() == Some("Component") && s.name.starts_with("on_")).collect();
            if !hooks.is_empty() {
                let body: String = hooks.iter().filter(|h| matches!(h.name.as_str(), "on_start" | "on_update")).map(|h| format!("    {} {{\n    }}\n", h.signature.trim_start_matches("pub "))).collect();
                out.push(s("impl Component", &format!("impl Component for $0Name {{\n{body}}}"), "impl Component for … { on_start, on_update }")); // i18n-skip: код
                for h in hooks {
                    out.push(Item { label: h.name.clone(), insert: format!("{} {{\n    $0\n}}", h.signature), kind: "hook", detail: h.signature.clone(), doc: h.doc.clone() });
                }
            }
            out
        }
        Language::Scenario => vec![
            s("function", "function $0()\nend", "function name() … end"), // i18n-skip: код
            s("for", "for i = 1, $0 do\nend", "for i = 1, n do … end"), // i18n-skip: код
            s("wait", "wait($0)", "wait(seconds)"),
            s("if", "if $0 then\nend", "if … then … end"), // i18n-skip: код
            s("while", "while $0 do\nend", "while … do … end"), // i18n-skip: код
        ],
        Language::Cpp => vec![
            s("struct component", "struct $0Name : engine::Component {\n    float speed = 1.0f;\n    void update(float dt) override {\n    }\n};\n", "struct … : engine::Component"),
            s("for", "for (int i = 0; i < $0; ++i) {\n}", "for (…) { }"), // i18n-skip: код
        ],
        _ => Vec::new(),
    }
}

fn keywords(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &["as", "break", "const", "continue", "crate", "else", "enum", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "use", "where", "while"],
        Language::Scenario => engine_scenario::KEYWORDS,
        Language::Cpp => &["auto", "bool", "break", "case", "class", "const", "constexpr", "continue", "default", "double", "else", "enum", "false", "float", "for", "if", "int", "namespace", "nullptr", "override", "private", "public", "return", "static", "struct", "switch", "this", "true", "virtual", "void", "while"],
        Language::Wgsl => &["fn", "let", "var", "const", "struct", "return", "if", "else", "for", "loop", "vec2", "vec3", "vec4", "f32", "u32", "i32", "mat4x4"],
        _ => &[],
    }
}

fn symbol_item(s: &Symbol) -> Item {
    let insert = match s.kind {
        Kind::Function | Kind::Method => format!("{}($0)", s.name),
        _ => s.name.clone(),
    };
    Item { label: s.name.clone(), insert, kind: s.kind.label(), detail: s.signature.clone(), doc: s.doc.clone() }
}

/// Слово перед байтом `at`: (начало, текст).
fn word_before(text: &str, at: usize) -> (usize, &str) {
    let mut start = at;
    while let Some(c) = text[..start].chars().next_back().filter(|c| c.is_alphanumeric() || *c == '_') {
        start -= c.len_utf8();
    }
    (start, &text[start..at])
}

/// Подсказки у курсора: начало слова и пункты. Пустой список — подсказывать нечего.
pub(crate) fn complete(path: &Path, text: &str, caret: usize, language: Language, project: &[Symbol], explicit: bool) -> (usize, Vec<Item>) {
    let caret = crate::code_diagnostics::floor_char(text, caret);
    let (anchor, prefix) = word_before(text, caret);
    let before = &text[..anchor];
    // Большой файл: символы и слова — только из окна вокруг курсора, чтобы подсказки не
    // стоили нажатию клавиши сотен миллисекунд.
    let window_text = if text.len() > LARGE_TEXT {
        let from = crate::code_diagnostics::floor_char(text, caret.saturating_sub(LARGE_TEXT / 2));
        let to = crate::code_diagnostics::floor_char(text, (caret + LARGE_TEXT / 2).min(text.len()));
        &text[from..to]
    } else {
        text
    };
    let file = code_symbols::symbols(path, window_text);
    let engine = code_symbols::engine_index();
    let mut items: Vec<Item> = Vec::new();
    let member = before.ends_with('.') && !before.ends_with("..");
    let path_sep = before.ends_with("::");
    if member || path_sep {
        let receiver_end = before.len() - if member { 1 } else { 2 };
        let (_, receiver) = word_before(text, receiver_end);
        let members: Vec<&Symbol> = if member && receiver == "ctx" && language == Language::Rust {
            engine.iter().filter(|s| s.container.as_deref() == Some("Ctx") && s.kind == Kind::Method).collect() // i18n-skip: код
        } else if member && receiver == "self" {
            file.iter().filter(|s| matches!(s.kind, Kind::Field | Kind::Method) && s.container.is_some()).collect()
        } else if path_sep {
            engine.iter().chain(project.iter()).chain(file.iter()).filter(|s| s.container.as_deref() == Some(receiver)).collect()
        } else {
            Vec::new()
        };
        for s in members {
            let mut item = symbol_item(s);
            if s.kind == Kind::Field {
                item.insert = s.name.clone();
            }
            items.push(item);
        }
        if items.is_empty() && !explicit {
            return (anchor, Vec::new());
        }
    } else {
        if prefix.chars().count() < 2 && !explicit {
            return (anchor, Vec::new());
        }
        for k in keywords(language) {
            items.push(Item { label: k.to_string(), insert: k.to_string(), kind: "keyword", detail: String::new(), doc: String::new() });
        }
        items.extend(snippets(language));
        // Большой файл: его символы — только из окна у курсора (индекс проекта знает файл
        // целиком, и десятки тысяч его символов стоили бы нажатию миллисекунды).
        let large = text.len() > LARGE_TEXT;
        let defs = file.iter().chain(project.iter().filter(|s| !large || s.path != path)).filter(|s| s.kind != Kind::Impl && s.kind != Kind::Field);
        items.extend(defs.map(symbol_item));
        if language == Language::Rust {
            items.extend(engine.iter().filter(|s| s.container.is_none() && matches!(s.kind, Kind::Struct | Kind::Enum | Kind::Trait | Kind::Function | Kind::Macro)).map(symbol_item));
        }
        // Слова файла.
        let mut words = BTreeSet::new();
        for w in window_text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if w.len() >= 3 && !w.starts_with(|c: char| c.is_ascii_digit()) && w != prefix {
                words.insert(w);
            }
        }
        items.extend(words.into_iter().map(|w| Item { label: w.to_string(), insert: w.to_string(), kind: "word", detail: String::new(), doc: String::new() }));
    }
    // Отбор по набранному: сначала начало слова с учётом регистра, потом без, потом
    // нечёткое; повторы — по имени (первый — самый осмысленный источник).
    let lower = prefix.to_lowercase();
    let mut scored: Vec<(i32, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let score = if prefix.is_empty() {
                0
            } else if item.label.starts_with(prefix) {
                3_000_000
            } else if item.label.to_lowercase().starts_with(&lower) {
                2_000_000
            } else {
                crate::code_nav::fuzzy(prefix, &item.label)? + 1_000_000
            };
            (item.label != prefix || item.kind == "snippet").then_some((score, i, item))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut seen = BTreeSet::new();
    let items: Vec<Item> = scored.into_iter().map(|(_, _, i)| i).filter(|i| seen.insert((i.label.clone(), i.kind == "snippet"))).take(60).collect();
    (anchor, items)
}

/// Подсказка параметров: сигнатура функции, в скобках вызова которой стоит курсор.
pub(crate) fn parameter_hint(path: &Path, text: &str, caret: usize, project: &[Symbol]) -> Option<String> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = caret.min(text.len());
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' if depth == 0 => {
                let (_, name) = word_before(text, i);
                if name.is_empty() {
                    return None;
                }
                let file = code_symbols::symbols(path, text);
                let found = code_symbols::definitions(name, &file, project);
                let signature = found.first().map(|s| s.signature.clone()).or_else(|| code_symbols::engine_definitions(name).first().map(|s| s.signature.clone()))?;
                return Some(signature);
            }
            b'(' => depth -= 1,
            b';' | b'{' | b'}' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// Нарисовать список; клавиши ↑ ↓ Enter Tab Esc. `Ok(Some)` — выбран пункт, `Err` — закрыть.
pub(crate) fn show(ctx: &egui::Context, popup: &mut Popup) -> Result<Option<Item>, ()> {
    let (up, down, accept, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let n = popup.items.len().max(1);
    if up {
        popup.selected = (popup.selected + n - 1) % n;
    }
    if down {
        popup.selected = (popup.selected + 1) % n;
    }
    if escape {
        return Err(());
    }
    let mut chosen = accept.then(|| popup.items.get(popup.selected).cloned()).flatten();
    let rows = popup.items.len().min(10);
    egui::Area::new(egui::Id::new("code_completion")).fade_in(false).order(egui::Order::Foreground).fixed_pos(popup.at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(theme::BG_RAISED).stroke(egui::Stroke::new(1.0_f32, theme::LINE_STRONG)).inner_margin(egui::Margin::same(theme::SP_1)).show(ui, |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("code_completion_list").max_height(rows as f32 * theme::ROW).show_rows(ui, theme::ROW, popup.items.len(), |ui, range| {
                for k in range {
                    let item = &popup.items[k];
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
                    theme::list_row(ui.painter(), rect, k == popup.selected, true, response.hovered());
                    let painter = ui.painter_at(rect);
                    painter.text(pos2(rect.left() + theme::SP_3, rect.center().y), Align2::LEFT_CENTER, crate::code_symbols::kind_text(item.kind), theme::caption(), theme::INFO);
                    let name = painter.text(pos2(rect.left() + 76.0, rect.center().y), Align2::LEFT_CENTER, &item.label, theme::mono(), theme::TEXT_STRONG);
                    painter
                        .with_clip_rect(Rect::from_min_max(pos2(name.right() + theme::SP_6, rect.top()), rect.max))
                        .text(pos2(name.right() + theme::SP_6, rect.center().y), Align2::LEFT_CENTER, &item.detail, theme::caption(), theme::TEXT_MUTED);
                    if k == popup.selected && (up || down) {
                        ui.scroll_to_rect(rect, None);
                    }
                    if response.clicked() {
                        chosen = Some(item.clone());
                    }
                }
            });
            if let Some(item) = popup.items.get(popup.selected) {
                if !item.doc.is_empty() || !item.detail.is_empty() {
                    let y = ui.cursor().top() + theme::SP_1;
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0_f32, theme::LINE_SOFT));
                    ui.add_space(theme::SP_3);
                    let text = if item.doc.is_empty() { item.detail.clone() } else { format!("{}\n{}", item.detail, item.doc) };
                    ui.label(egui::RichText::new(text).size(theme::FONT_CAPTION).color(theme::TEXT));
                }
            }
        });
    });
    Ok(chosen)
}

/// Вставить выбранный пункт вместо `anchor..caret`: (новый текст, курсор).
pub(crate) fn accept(text: &str, anchor: usize, caret: usize, item: &Item) -> (String, usize) {
    // Отступ строки — для многострочных заготовок.
    let line_start = text[..anchor].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[line_start..anchor].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let newline = format!("\n{indent}");
    let marker = item.insert.find("$0").unwrap_or(item.insert.len());
    let insert = item.insert.replacen("$0", "", 1).replace('\n', &newline);
    let at = anchor + item.insert[..marker].replace('\n', &newline).len();
    (format!("{}{insert}{}", &text[..anchor], &text[caret..]), at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_completion_after_ctx_and_self() {
        let text = "component! { pub struct Mover { pub speed: f32 = 1.0 } }\nimpl Component for Mover {\n    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {\n        ctx.\n        self.sp\n    }\n}\n";
        let caret = text.find("ctx.\n").unwrap() + 4;
        let (anchor, items) = complete(Path::new("m.rs"), text, caret, Language::Rust, &[], false);
        assert_eq!(anchor, caret);
        assert!(items.len() > 5, "Ctx methods: {}", items.len());
        assert!(items.iter().all(|i| i.kind == "method"));
        let caret = text.find("self.sp").unwrap() + 7;
        let (_, items) = complete(Path::new("m.rs"), text, caret, Language::Rust, &[], false);
        assert_eq!(items[0].label, "speed");
        assert_eq!(items[0].detail, "speed: f32");
    }

    #[test]
    fn words_keywords_snippets_and_hints() {
        let text = "let velocity = 1;\nlet v = vel";
        let (anchor, items) = complete(Path::new("a.rs"), text, text.len(), Language::Rust, &[], false);
        assert_eq!(&text[anchor..], "vel");
        assert_eq!(items[0].label, "velocity");
        let (_, items) = complete(Path::new("a.rs"), "comp", 4, Language::Rust, &[], false);
        assert!(items.iter().any(|i| i.label == "component!" && i.kind == "snippet"));
        let (_, items) = complete(Path::new("a.scenario"), "fun", 3, Language::Scenario, &[], false);
        assert!(items.iter().any(|i| i.label == "function"));
        let (_, items) = complete(Path::new("a.rs"), "impl Comp", 9, Language::Rust, &[], false);
        let imp = items.iter().find(|i| i.label == "impl Component").expect("the impl snippet");
        assert!(imp.insert.contains("fn on_update(&mut self, ctx: &mut Ctx, dt: f32)"), "{}", imp.insert);
        let text = "fn add(a: i32, b: i32) -> i32 { a + b }\nfn x() { add(1, ";
        assert_eq!(parameter_hint(Path::new("a.rs"), text, text.len(), &[]).as_deref(), Some("fn add(a: i32, b: i32) -> i32"));
        let (t, at) = accept("    fo", 4, 6, &Item { label: "for".into(), insert: "for $0 in  {\n}".into(), kind: "snippet", detail: String::new(), doc: String::new() });
        assert_eq!(t, "    for  in  {\n    }");
        assert_eq!(at, 8);
    }
}
