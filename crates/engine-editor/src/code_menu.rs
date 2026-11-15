//! Контекстные меню вкладки Code: текст (правка, переходы, исправления, свёртка, путь
//! файла) и вкладки документов (закрыть другие / справа / все, закрепить, путь).
//! Пункты показывают своё сочетание и выключены, когда не к месту.

use std::path::PathBuf;

use egui::Event;

use crate::i18n::tr_s;
use crate::icons::Icon;
use crate::state::Editor;
use crate::theme::{self, item as menu_item};

/// Что выбрали в меню текста.
#[derive(Clone, Debug)]
enum TextAction {
    Input(Event),
    SelectAll,
    /// Команда вкладки Code (как от сочетания клавиш).
    Command(&'static str),
    GoToDefinition,
    PeekDefinition,
    FindReferences,
    Rename,
    CopyPath,
    Reveal,
    External,
}

/// Текст системного буфера обмена для «Paste» из меню (Ctrl+V приносит его сам).
#[cfg(not(test))]
fn clipboard_text() -> Option<String> {
    egui_winit::clipboard::Clipboard::new(None).get().filter(|t| !t.is_empty())
}

#[cfg(test)]
thread_local!(pub(crate) static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) });

#[cfg(test)]
fn clipboard_text() -> Option<String> {
    TEST_CLIPBOARD.with(|c| c.borrow().clone())
}

/// Меню правого щелчка по тексту документа (`response` — отклик области текста).
pub(crate) fn text_menu(editor: &mut Editor, response: &egui::Response) {
    let Some(doc) = editor.code.documents.get(editor.code.active) else { return };
    let id = doc.text_id();
    let path = doc.path.clone();
    let ctx = response.ctx.clone();
    if !response.context_menu_opened() && !response.secondary_clicked() {
        return;
    }
    // Что под главным курсором.
    let (_, caret) = crate::code_keys::selection(&ctx, id, &doc.text).unwrap_or((0, 0));
    let ident = crate::code_symbols::ident_at(&doc.text, caret).or_else(|| crate::code_symbols::ident_at(&doc.text, caret.saturating_sub(1))).is_some();
    let fixes = crate::code_checks::of_document(doc, &editor.code.checks).iter().any(|d| !d.fixes.is_empty() && d.range.start <= caret && caret <= d.range.end);
    let line = doc.text[..caret.min(doc.text.len())].matches('\n').count();
    let foldable = crate::code_widget::with_state(&ctx, id, |st| {
        let regions = st.fold_regions(&doc.text, doc.language, f64::INFINITY, false);
        regions.iter().any(|r| r.start_line <= line && line <= r.end_line)
    });
    let comment = crate::code_edit::line_comment(doc.language).is_some();
    let paste = clipboard_text();
    let external = !editor.prefs.external_editor.trim().is_empty();
    let keys = |id: &str| editor.prefs.shortcuts.text(id);
    let k = Keys {
        go: keys("go-to-definition"),
        peek: keys("peek-definition"),
        refs: keys("find-references"),
        rename: keys("rename-symbol"),
        fix: keys("quick-fix"),
        comment: keys("toggle-comment"),
        format: keys("format-document"),
        fold: keys("toggle-outlining"),
        bookmark: keys("toggle-bookmark"),
    };
    let mut chosen: Option<TextAction> = None;
    response.context_menu(|ui| {
        theme::menu_body(ui);
        let mut pick = |a: TextAction| chosen = Some(a);
        if menu_item(tr_s("Cut")).icon(Icon::Copy).shortcut("Ctrl+X").show(ui) {
            pick(TextAction::Input(Event::Cut));
        }
        if menu_item(tr_s("Copy")).icon(Icon::Copy).shortcut("Ctrl+C").show(ui) {
            pick(TextAction::Input(Event::Copy));
        }
        if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(paste.is_some(), tr_s("The clipboard has no text")).show(ui) {
            pick(TextAction::Input(Event::Paste(paste.clone().unwrap_or_default())));
        }
        if menu_item(tr_s("Select All")).shortcut("Ctrl+A").show(ui) {
            pick(TextAction::SelectAll);
        }
        theme::menu_separator(ui);
        let no_word = tr_s("Put the caret on a name first");
        if menu_item(tr_s("Go to Definition")).icon(Icon::ArrowUp).shortcut(&k.go).enabled_if(ident, no_word).show(ui) {
            pick(TextAction::GoToDefinition);
        }
        if menu_item(tr_s("Peek Definition")).shortcut(&k.peek).enabled_if(ident, no_word).show(ui) {
            pick(TextAction::PeekDefinition);
        }
        if menu_item(tr_s("Find All References")).icon(Icon::Search).shortcut(&k.refs).enabled_if(ident, no_word).show(ui) {
            pick(TextAction::FindReferences);
        }
        if menu_item(tr_s("Rename…")).icon(Icon::Text).shortcut(&k.rename).enabled_if(ident, no_word).show(ui) {
            pick(TextAction::Rename);
        }
        theme::menu_separator(ui);
        if menu_item(tr_s("Quick Fix…")).icon(Icon::Bulb).shortcut(&k.fix).enabled_if(fixes, tr_s("No fix for the code at the caret")).show(ui) {
            pick(TextAction::Command("quick-fix"));
        }
        theme::menu_separator(ui);
        if menu_item(tr_s("Toggle Line Comment")).shortcut(&k.comment).enabled_if(comment, tr_s("This file type has no line comments")).show(ui) {
            pick(TextAction::Command("toggle-comment"));
        }
        if menu_item(tr_s("Format Document")).shortcut(&k.format).show(ui) {
            pick(TextAction::Command("format-document"));
        }
        if menu_item(tr_s("Fold / Unfold")).icon(Icon::Collapse).shortcut(&k.fold).enabled_if(foldable, tr_s("No block to fold at the caret")).show(ui) {
            pick(TextAction::Command("toggle-outlining"));
        }
        if menu_item(tr_s("Toggle Bookmark")).icon(Icon::Star).shortcut(&k.bookmark).show(ui) {
            pick(TextAction::Command("toggle-bookmark"));
        }
        theme::menu_separator(ui);
        if menu_item(tr_s("Copy Path")).icon(Icon::Copy).show(ui) {
            pick(TextAction::CopyPath);
        }
        if menu_item(tr_s("Show in File Manager")).icon(Icon::FolderOpen).show(ui) {
            pick(TextAction::Reveal);
        }
        if menu_item(tr_s("Open in External Editor")).icon(Icon::External).enabled_if(external, tr_s("Set the external code editor in Preferences → General")).show(ui) {
            pick(TextAction::External);
        }
    });
    let Some(action) = chosen else { return };
    // Действие — в текст: фокус обратно в него.
    ctx.memory_mut(|m| m.request_focus(id));
    match action {
        TextAction::Input(event) => crate::code_widget::with_state(&ctx, id, |st| st.pending.push(event)),
        TextAction::SelectAll => {
            let len = editor.code.documents[editor.code.active].text.len();
            crate::code_widget::with_state(&ctx, id, |st| st.carets = crate::code_buffer::Carets::single(crate::code_buffer::Sel::new(0, len)));
        }
        TextAction::Command(command) => editor.code.commands.push(command),
        TextAction::GoToDefinition => editor.go_to_definition(false),
        TextAction::PeekDefinition => editor.go_to_definition(true),
        TextAction::FindReferences => editor.find_all_references(),
        TextAction::Rename => editor.begin_code_rename(),
        TextAction::CopyPath => ctx.copy_text(path.display().to_string()),
        TextAction::Reveal => crate::code_tree::run(editor, &ctx, crate::code_tree::TreeCmd::Reveal(path)),
        TextAction::External => crate::code_tree::run(editor, &ctx, crate::code_tree::TreeCmd::External(path)),
    }
}

struct Keys {
    go: String,
    peek: String,
    refs: String,
    rename: String,
    fix: String,
    comment: String,
    format: String,
    fold: String,
    bookmark: String,
}

/// Что выбрали в меню вкладки.
#[derive(Clone, Debug)]
pub(crate) enum TabAction {
    Close(usize),
    CloseOthers(usize),
    CloseRight(usize),
    CloseAll,
    Pin(usize, bool),
    CopyPath(PathBuf),
    Reveal(PathBuf),
}

/// Пункты меню вкладки `index`.
pub(crate) fn tab_menu(ui: &mut egui::Ui, index: usize, count: usize, pinned: bool, path: &std::path::Path, close_keys: &str, out: &mut Option<TabAction>) {
    theme::menu_body(ui);
    if menu_item(tr_s("Close")).icon(Icon::Close).shortcut(close_keys).show(ui) {
        *out = Some(TabAction::Close(index));
    }
    if menu_item(tr_s("Close Others")).enabled_if(count > 1, tr_s("This is the only tab")).show(ui) {
        *out = Some(TabAction::CloseOthers(index));
    }
    if menu_item(tr_s("Close to the Right")).enabled_if(index + 1 < count, tr_s("No tabs to the right")).show(ui) {
        *out = Some(TabAction::CloseRight(index));
    }
    if menu_item(tr_s("Close All")).show(ui) {
        *out = Some(TabAction::CloseAll);
    }
    theme::menu_separator(ui);
    let label = if pinned { tr_s("Unpin Tab") } else { tr_s("Pin Tab") };
    if menu_item(label).icon(Icon::Lock).show(ui) {
        *out = Some(TabAction::Pin(index, !pinned));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Copy Path")).icon(Icon::Copy).show(ui) {
        *out = Some(TabAction::CopyPath(path.to_path_buf()));
    }
    if menu_item(tr_s("Show in File Manager")).icon(Icon::FolderOpen).show(ui) {
        *out = Some(TabAction::Reveal(path.to_path_buf()));
    }
}

/// Выполнить пункт меню вкладки. Закреплённые вкладки «Close Others / All / справа» не
/// закрывают (как в Visual Studio).
pub(crate) fn run_tab(editor: &mut Editor, ctx: &egui::Context, action: TabAction) {
    let code = &mut editor.code;
    let keep = |code: &crate::code::CodeState, k: usize| code.documents.get(k).is_some_and(|d| d.pinned);
    match action {
        // Изменённые вкладки не закрываются молча: `request_close` спрашивает (`code_close.rs`).
        TabAction::Close(k) => code.request_close(&[k]),
        TabAction::CloseOthers(k) => {
            let list: Vec<usize> = (0..code.documents.len()).filter(|&i| i != k && !keep(code, i)).collect();
            code.request_close(&list);
        }
        TabAction::CloseRight(k) => {
            let list: Vec<usize> = (k + 1..code.documents.len()).filter(|&i| !keep(code, i)).collect();
            code.request_close(&list);
        }
        TabAction::CloseAll => {
            let list: Vec<usize> = (0..code.documents.len()).filter(|&i| !keep(code, i)).collect();
            code.request_close(&list);
        }
        TabAction::Pin(k, pinned) => code.set_pinned(k, pinned),
        TabAction::CopyPath(path) => ctx.copy_text(path.display().to_string()),
        TabAction::Reveal(path) => crate::code_tree::run(editor, ctx, crate::code_tree::TreeCmd::Reveal(path)),
    }
}
