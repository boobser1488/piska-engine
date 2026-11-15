//! Несохранённый код не пропадает молча (как в Visual Studio / VS Code): закрытие вкладки
//! с правками (крестик, средняя кнопка, Ctrl+W, меню вкладки «Close / Close Others / Close
//! to the Right / Close All») открывает вопрос «Save changes to …?» с кнопками Save /
//! Don't Save / Cancel (Enter — Save, Esc — Cancel); для нескольких вкладок — один вопрос
//! со списком изменённых файлов и «Save All». Выход из редактора (Quit, закрытие окна,
//! другой проект, Project Hub) спрашивает о том же; при несохранённой сцене вопрос один —
//! общий (см. `Dialog::Unsaved`).
//!
//! «Don't Save» убирает и копию восстановления файла (`code_recovery.rs`), иначе она
//! предлагалась бы при следующем запуске; Cancel ничего не меняет. Переключение вкладок
//! редактора (Scene, Game), режим Play и потеря фокуса ничего не спрашивают.

use std::path::PathBuf;

use crate::code::CodeState;
use crate::i18n::{tr_s, trf};
use crate::icons::Icon;
use crate::state::{Action, Editor};
use crate::theme;

/// Открытый вопрос о несохранённых вкладках.
pub(crate) struct CloseAsk {
    /// Вкладки, которые закроются после ответа (изменённые и нет).
    pub targets: Vec<PathBuf>,
    /// Действие, которое продолжится после ответа (Quit при выходе из редактора).
    pub then: Option<Box<Action>>,
    /// Ошибка последней попытки сохранить: вкладки остаются, окно её показывает.
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Save,
    Discard,
    Cancel,
}

/// Действия, после которых этого редактора не будет (процесс завершается).
pub(crate) fn leaves_editor(action: &Action) -> bool {
    matches!(action, Action::Quit | Action::ProjectHub | Action::OpenProject(_))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

impl CodeState {
    /// Закрыть вкладки `indices`; если среди них есть изменённые — сначала спросить.
    pub(crate) fn request_close(&mut self, indices: &[usize]) {
        if self.close_ask.is_some() {
            return;
        }
        let targets: Vec<PathBuf> = indices.iter().filter_map(|&i| self.documents.get(i)).map(|d| d.path.clone()).collect();
        if targets.is_empty() {
            return;
        }
        if self.dirty_names(&targets).is_empty() {
            self.close_paths(&targets);
        } else {
            self.close_ask = Some(CloseAsk { targets, then: None, error: None });
        }
    }

    /// Выход из редактора (`then`) с несохранёнными вкладками: спросить, потом продолжить.
    pub(crate) fn ask_before_leaving(&mut self, then: Action) {
        let targets: Vec<PathBuf> = self.documents.iter().filter(|d| d.dirty()).map(|d| d.path.clone()).collect();
        self.close_ask = Some(CloseAsk { targets, then: Some(Box::new(then)), error: None });
    }

    /// Имена изменённых файлов среди `targets`.
    fn dirty_names(&self, targets: &[PathBuf]) -> Vec<String> {
        self.documents.iter().filter(|d| d.dirty() && targets.contains(&d.path)).map(|d| file_name(&d.path)).collect()
    }

    /// Имена всех изменённых файлов.
    pub(crate) fn unsaved_names(&self) -> Vec<String> {
        self.documents.iter().filter(|d| d.dirty()).map(|d| file_name(&d.path)).collect()
    }

    fn close_paths(&mut self, targets: &[PathBuf]) {
        for index in (0..self.documents.len()).rev() {
            if targets.contains(&self.documents[index].path) {
                self.close(index);
            }
        }
    }

    /// «Don't Save»: правки отброшены вместе с копиями восстановления, вкладки закрыты.
    pub(crate) fn discard_unsaved(&mut self) {
        let dirty: Vec<PathBuf> = self.documents.iter().filter(|d| d.dirty()).map(|d| d.path.clone()).collect();
        self.discard_paths(&dirty);
    }

    fn discard_paths(&mut self, targets: &[PathBuf]) {
        if let Some(dir) = &self.recovery.dir {
            for document in self.documents.iter().filter(|d| d.dirty() && targets.contains(&d.path)) {
                crate::code_recovery::remove(dir, &document.path);
            }
        }
        self.close_paths(targets);
    }
}

impl Editor {
    /// Записать изменённые файлы из `paths` (с теми же последствиями, что у Ctrl+S).
    pub(crate) fn save_code_documents(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        for index in 0..self.code.documents.len() {
            let document = &self.code.documents[index];
            if !document.dirty() || !paths.contains(&document.path) {
                continue;
            }
            if let Some(path) = self.code.save_index(index)? {
                log::info!("saved `{}`", path.display());
                self.hint_rust_saved(&path);
                crate::code_checks::saved(self, &path);
            }
        }
        Ok(())
    }

    /// Записать все изменённые файлы (перед выходом).
    pub(crate) fn save_all_code(&mut self) -> Result<(), String> {
        let all: Vec<PathBuf> = self.code.documents.iter().map(|d| d.path.clone()).collect();
        self.save_code_documents(&all)
    }

    /// Ответ на вопрос о вкладках.
    pub(crate) fn answer_close_ask(&mut self, answer: Answer) {
        let Some(mut ask) = self.code.close_ask.take() else { return };
        match answer {
            Answer::Cancel => return,
            Answer::Save => {
                if let Err(why) = self.save_code_documents(&ask.targets) {
                    // Не записалось — вкладки остаются, ошибка видна в окне.
                    log::error!("{why}");
                    ask.error = Some(why);
                    self.code.close_ask = Some(ask);
                    return;
                }
                self.code.close_paths(&ask.targets);
            }
            Answer::Discard => self.code.discard_paths(&ask.targets),
        }
        if let Some(then) = ask.then {
            self.apply(*then);
        }
    }
}

/// Окно вопроса (рисуется поверх любой вкладки редактора).
pub(crate) fn dialog(editor: &mut Editor, ctx: &egui::Context) {
    let Some(ask) = editor.code.close_ask.as_ref() else { return };
    let names = editor.code.dirty_names(&ask.targets);
    if names.is_empty() {
        // Всё уже сохранено — спрашивать не о чем.
        editor.answer_close_ask(Answer::Save);
        return;
    }
    let error = ask.error.clone();
    let mut answer = None;
    let single = names.len() == 1;
    let response = crate::window_kit::dialog(egui::Id::new("code_close_ask"), "").width(0.0).show(ctx, |ui| {
        ui.set_width(420.0);
        let title = if single { trf!("Save changes to “{name}”?", name = names[0]) } else { trf!("Save changes to {n} files?", n = names.len()) };
        theme::dialog_title(ui, Icon::Warn, theme::WARN, &title);
        ui.add_space(theme::SP_M);
        if !single {
            for name in names.iter().take(8) {
                ui.label(egui::RichText::new(name).color(theme::TEXT_STRONG));
            }
            if names.len() > 8 {
                ui.label(egui::RichText::new(trf!("… and {n} more", n = names.len() - 8)).color(theme::TEXT_MUTED));
            }
            ui.add_space(theme::SP_S);
        }
        ui.label(egui::RichText::new(tr_s("Your changes are lost if you don't save them.")).color(theme::TEXT_MUTED));
        if let Some(error) = &error {
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new(error).color(theme::ERROR));
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        crate::window_kit::dialog_actions(ui, |ui| {
            let save = crate::window_kit::primary_action(ui, if single { tr_s("Save") } else { tr_s("Save All") });
            if save.clicked() || enter {
                answer = Some(Answer::Save);
            }
            if crate::window_kit::action_button(ui, tr_s("Cancel")).clicked() {
                answer = Some(Answer::Cancel);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if crate::window_kit::action_button(ui, tr_s("Don't Save")).clicked() {
                    answer = Some(Answer::Discard);
                }
            });
        });
    });
    if answer.is_none() && response.should_close() {
        answer = Some(Answer::Cancel);
    }
    if let Some(answer) = answer {
        editor.answer_close_ask(answer);
    }
}
