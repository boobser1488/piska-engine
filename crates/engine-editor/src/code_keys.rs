//! Команды правки вкладки Code из реестра горячих клавиш (дублировать строку,
//! переместить, закомментировать, регистр…) — у каждого курсора; команды нескольких
//! курсоров (следующее вхождение, все вхождения) и свёртки (Ctrl+M …). Набор, стрелки,
//! мышь — в самом виджете (`code_widget.rs`).

use std::ops::Range;

use crate::code::{Document, Language};
use crate::code_buffer::Sel;
use crate::code_edit::{self, CommentMode};
use crate::code_widget::{self, edit_each};

/// Команды правки текста (их сочетания — в `shortcuts.rs`, область «Code Editor»);
/// работают в раскладке Visual Studio.
pub(crate) const TEXT_COMMANDS: &[&str] = &[
    "duplicate-line",
    "delete-line",
    "cut-line",
    "move-line-up",
    "move-line-down",
    "copy-line-up",
    "copy-line-down",
    "join-lines",
    "sort-lines",
    "toggle-comment",
    "comment-lines",
    "uncomment-lines",
    "block-comment",
    "upper-case",
    "lower-case",
    "go-to-bracket",
    "expand-selection",
    "shrink-selection",
    "toggle-bookmark",
    "next-bookmark",
    "prev-bookmark",
];

/// Несколько курсоров и свёртка — в любой раскладке (это возможности самого виджета).
/// Первые шесть нуждаются в раскладке кадра и выполняются виджетом (`code.rs`).
pub(crate) const CARET_COMMANDS: &[&str] = &[
    "add-caret-above",
    "add-caret-below",
    "box-select-left",
    "box-select-right",
    "box-select-up",
    "box-select-down",
    "add-next-occurrence",
    "select-all-occurrences",
    "toggle-outlining",
    "collapse-to-definitions",
    "toggle-all-outlining",
    "stop-outlining",
];

/// Команды, которым нужна раскладка кадра (выполняет `code.rs` внутри виджета).
pub(crate) const VIEW_COMMANDS: &[&str] = &["add-caret-above", "add-caret-below", "box-select-left", "box-select-right", "box-select-up", "box-select-down"];

/// Выделение главного курсора в байтах: (якорь, курсор).
pub(crate) fn selection(ctx: &egui::Context, id: egui::Id, text: &str) -> Option<(usize, usize)> {
    let sel = code_widget::with_state(ctx, id, |st| st.carets.primary());
    let len = text.len();
    Some((sel.anchor.min(len), sel.caret.min(len)))
}

/// Поставить одно выделение (байты); прочие курсоры убираются.
#[cfg(test)]
pub(crate) fn set_selection(ctx: &egui::Context, id: egui::Id, text: &str, anchor: usize, caret: usize) {
    code_widget::with_state(ctx, id, |st| st.set_selection(text, anchor, caret));
}

/// Что вкладка Code делает с клавиатурой в этом кадре.
pub(crate) struct KeyContext<'a> {
    pub language: Language,
    /// Стек прежних выделений для «сузить выделение».
    pub selections: &'a mut Vec<Range<usize>>,
}

/// Выполнить команду правки над документом. `true` — команда что-то сделала.
pub(crate) fn run(ctx: &egui::Context, id: egui::Id, doc: &mut Document, command: &str, keys: &mut KeyContext<'_>) -> bool {
    let now = ctx.input(|i| i.time);
    let mut st = code_widget::take(ctx, id);
    st.observe(&doc.text, keys.language, now);
    let (done, clipboard) = run_on(&mut st, doc, command, keys, now);
    code_widget::put(ctx, id, st);
    if let Some(text) = clipboard {
        ctx.copy_text(text);
    }
    done
}

fn run_on(st: &mut code_widget::EditState, doc: &mut Document, command: &str, keys: &mut KeyContext<'_>, now: f64) -> (bool, Option<String>) {
    let language = keys.language;
    let text = doc.text.as_str();
    let mut clipboard = None;
    let edited = match command {
        "duplicate-line" => edit_each(text, &st.carets, |t, s| Some(code_edit::duplicate(t, s.anchor, s.caret))),
        "delete-line" => edit_each(text, &st.carets, |t, s| Some(code_edit::delete_lines(t, s.anchor, s.caret).0)),
        "cut-line" => {
            let removed: Vec<String> = st.carets.sels.iter().map(|s| code_edit::delete_lines(text, s.anchor, s.caret).1).collect();
            clipboard = Some(removed.concat());
            edit_each(text, &st.carets, |t, s| Some(code_edit::delete_lines(t, s.anchor, s.caret).0))
        }
        "move-line-up" => edit_each(text, &st.carets, |t, s| code_edit::move_lines(t, s.anchor, s.caret, true)),
        "move-line-down" => edit_each(text, &st.carets, |t, s| code_edit::move_lines(t, s.anchor, s.caret, false)),
        "copy-line-up" => edit_each(text, &st.carets, |t, s| Some(code_edit::copy_lines(t, s.anchor, s.caret, true))),
        "copy-line-down" => edit_each(text, &st.carets, |t, s| Some(code_edit::copy_lines(t, s.anchor, s.caret, false))),
        "join-lines" => edit_each(text, &st.carets, |t, s| Some(code_edit::join_lines(t, s.anchor, s.caret))),
        "sort-lines" => edit_each(text, &st.carets, |t, s| Some(code_edit::sort_lines(t, s.anchor, s.caret))),
        "toggle-comment" | "comment-lines" | "uncomment-lines" => {
            let mode = match command {
                "comment-lines" => CommentMode::Comment,
                "uncomment-lines" => CommentMode::Uncomment,
                _ => CommentMode::Toggle,
            };
            match code_edit::line_comment(language) {
                Some(prefix) => edit_each(text, &st.carets, |t, s| Some(code_edit::comment_lines(t, s.anchor, s.caret, prefix, mode))),
                None => None,
            }
        }
        "block-comment" => edit_each(text, &st.carets, |t, s| code_edit::toggle_block_comment(t, s.anchor, s.caret, language)),
        "upper-case" => edit_each(text, &st.carets, |t, s| Some(code_edit::change_case(t, s.anchor, s.caret, true))),
        "lower-case" => edit_each(text, &st.carets, |t, s| Some(code_edit::change_case(t, s.anchor, s.caret, false))),
        "go-to-bracket" => {
            let mut any = false;
            let mut carets = st.carets.clone();
            for sel in &mut carets.sels {
                if let Some((_, other)) = code_edit::matching_bracket(text, sel.caret) {
                    // Курсор — за парной скобкой, если шли вперёд; перед ней — если назад.
                    let at = if other > sel.caret { other + 1 } else { other };
                    *sel = Sel::one(at);
                    any = true;
                }
            }
            if any {
                carets.normalize();
                st.carets = carets;
                st.history.seal();
                st.scroll_to_caret.get_or_insert(false);
            }
            return (any, None);
        }
        "expand-selection" => {
            let sel = st.carets.primary();
            let range = code_edit::expand_selection(text, sel.anchor, sel.caret);
            keys.selections.push(sel.range());
            st.set_selection(text, range.start, range.end);
            return (true, None);
        }
        "shrink-selection" => {
            let Some(previous) = keys.selections.pop() else { return (false, None) };
            st.set_selection(text, previous.start, previous.end);
            return (true, None);
        }
        "toggle-bookmark" | "next-bookmark" | "prev-bookmark" => {
            let caret = st.carets.primary().caret;
            let line = code_edit::line_start(text, caret);
            match command {
                "toggle-bookmark" => {
                    if let Some(at) = doc.bookmarks.iter().position(|b| *b == line) {
                        doc.bookmarks.remove(at);
                    } else {
                        doc.bookmarks.push(line);
                        doc.bookmarks.sort_unstable();
                    }
                }
                _ => {
                    let marks = &doc.bookmarks;
                    let target = if command == "next-bookmark" { marks.iter().find(|b| **b > line).or(marks.first()) } else { marks.iter().rev().find(|b| **b < line).or(marks.last()) };
                    let Some(&target) = target else { return (false, None) };
                    st.reveal(target);
                    st.set_selection(text, target, target);
                }
            }
            return (true, None);
        }
        "add-next-occurrence" => return (code_widget::add_next_occurrence(st, text), None),
        "select-all-occurrences" => return (code_widget::select_all_occurrences(st, text), None),
        "toggle-outlining" => {
            let line = st.index.line_of(st.carets.primary().caret);
            return (st.toggle_fold_at(text, language, line, now), None);
        }
        "collapse-to-definitions" => {
            st.collapse_to_definitions(text, language, now);
            return (true, None);
        }
        "toggle-all-outlining" => {
            st.toggle_all_folds(text, language, now);
            return (true, None);
        }
        "stop-outlining" => {
            st.stop_outlining();
            return (true, None);
        }
        _ => None,
    };
    let Some((edits, after)) = edited else { return (false, clipboard) };
    st.commit(&mut doc.text, edits, after, false, now);
    // Каждая команда — свой шаг Undo.
    st.history.seal();
    (true, clipboard)
}

/// Курсоры документа (для тестов).
#[cfg(test)]
pub(crate) fn carets(ctx: &egui::Context, id: egui::Id) -> crate::code_buffer::Carets {
    code_widget::with_state(ctx, id, |st| st.carets.clone())
}
