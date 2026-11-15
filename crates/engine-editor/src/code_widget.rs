//! Свой текстовый виджет вкладки Code вместо `egui::TextEdit`: раскладывает и красит
//! только видимые строки (кэш раскладки по строке), держит несколько курсоров и
//! выделение столбцом, сворачивает блоки, хранит историю Undo / Redo по документу.
//!
//! Состояние ([`EditState`]) лежит в памяти egui под `Document::text_id()` — как у
//! текстового поля: история и курсоры файла переживают переключение вкладок.
//!
//! Кадр (`code.rs`): [`EditState::observe`] (подхватить правки, сделанные мимо виджета) →
//! [`layout`] → [`interact`] (мышь и клавиатура) → снова `observe` / [`layout`], если текст
//! изменился → [`paint_under`] → [`paint_text`] → подсветки поверх (`code_view.rs`) →
//! [`paint_over`] (курсоры, свёртки, номера строк).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, Event, FontId, Galley, Key, Modifiers, Pos2, Rect, Sense, Stroke};

use crate::code::Language;
use crate::code_buffer::{self, Carets, Edit, FoldKind, FoldRegion, History, Sel};
use crate::code_diagnostics::{floor_char, text_change, LineIndex};
use crate::code_edit::{self, Edited};
use crate::code_files::Indent;
use crate::code_highlight::{self, Highlighter, Token};
use crate::code_prefs::{CodePrefs, Keymap};

use crate::icons::{self, Icon};
use crate::theme;

/// Отступ текста сверху и снизу (как у прежнего поля).
pub(crate) const TOP_PAD: f32 = 4.0;
/// Колонка значков диагностик в колонке номеров строк и место под стрелку свёртки.
pub(crate) const MARK_W: f32 = 14.0;
const FOLD_W: f32 = 14.0;

/// Выделение столбцом (Shift+Alt+мышь, Ctrl+Shift+Alt+стрелки): строки и x в пикселях
/// от начала текста строки.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxSel {
    pub anchor_line: usize,
    pub anchor_x: f32,
    pub head_line: usize,
    pub head_x: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Обычное выделение главного курсора.
    Text,
    /// Строки по колонке номеров: якорь — начало строки.
    Lines(usize),
    /// Столбец.
    Box,
}

/// Свёрнутые блоки файла и найденные области.
#[derive(Clone, Debug, Default)]
pub(crate) struct Folds {
    /// Области: (байт начала строки-заголовка, байт начала последней строки, вид).
    regions: Vec<(usize, usize, FoldKind)>,
    /// Свёрнутые: байты начал строк-заголовков.
    pub collapsed: Vec<usize>,
    /// «Stop Outlining» (Ctrl+M Ctrl+P): областей нет, пока их снова не попросят.
    pub disabled: bool,
    stale: bool,
}

/// Кэш раскладки строк: ключ — текст строки, состояние разбора, шрифт, ширина переноса.
/// Две «эпохи»: что не понадобилось за кадр, выбрасывается.
#[derive(Clone, Default)]
struct GalleyCache {
    now: HashMap<u64, Arc<Galley>>,
    prev: HashMap<u64, Arc<Galley>>,
}

impl GalleyCache {
    fn get(&mut self, key: u64) -> Option<Arc<Galley>> {
        if let Some(g) = self.now.get(&key) {
            return Some(g.clone());
        }
        let g = self.prev.remove(&key)?;
        self.now.insert(key, g.clone());
        Some(g)
    }

    fn end_frame(&mut self) {
        self.prev = std::mem::take(&mut self.now);
    }
}

/// Состояние виджета документа.
#[derive(Clone, Default)]
pub(crate) struct EditState {
    pub carets: Carets,
    pub history: History,
    /// Правка текста уже записана в историю (API вкладки) — `observe` не пишет её снова.
    pub recorded: bool,
    /// Текст, которому соответствуют индекс строк и подсветка.
    mirror: String,
    seen: bool,
    pub index: LineIndex,
    hl: Highlighter,
    pub folds: Folds,
    cache: GalleyCache,
    /// Число рядов строки при переносе (0 — ещё не раскладывали).
    rows: Vec<u32>,
    rows_width: f32,
    pub box_sel: Option<BoxSel>,
    drag: Option<Drag>,
    /// Набираемое через IME (ещё не вставлено).
    pub preedit: Option<String>,
    /// Прокрутить к главному курсору: `Some(true)` — в центр.
    pub scroll_to_caret: Option<bool>,
    /// Следующее вхождение (Shift+Alt+.) ищется словом целиком.
    occurrence_word: bool,
    max_chars: usize,
    changed_at: f64,
    language: Option<Language>,
    /// События из меню (Cut / Copy / Paste) — обрабатываются в следующем кадре как ввод.
    pub pending: Vec<Event>,
}

/// Загрузить состояние документа на время кадра (вернуть — [`put`]).
pub(crate) fn take(ctx: &egui::Context, id: egui::Id) -> EditState {
    ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<EditState>(id)))
}

pub(crate) fn put(ctx: &egui::Context, id: egui::Id, st: EditState) {
    ctx.data_mut(|d| d.insert_temp(id, st));
}

/// Сделать что-то с состоянием документа вне кадра виджета (команды, тесты).
pub(crate) fn with_state<R>(ctx: &egui::Context, id: egui::Id, f: impl FnOnce(&mut EditState) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<EditState>(id)))
}

/// Внешний вид текста кадра.
#[derive(Clone, Debug)]
pub(crate) struct Look {
    pub font: FontId,
    pub language: Language,
    /// Ширина переноса (`INFINITY` — без переноса).
    pub wrap_width: f32,
    pub line_h: f32,
    /// Ширина символа моноширинного шрифта.
    pub cw: f32,
    pub ppp: f32,
}

impl Look {
    pub fn new(ctx: &egui::Context, font: FontId, language: Language) -> Self {
        // Ширина символа — по раскладке строки (позиции глифов округляются к пикселям, и
        // она может отличаться от ширины одного глифа).
        let (line_h, cw) = ctx.fonts(|f| {
            let galley = f.layout_no_wrap("0".repeat(64), font.clone(), Color32::WHITE);
            (f.row_height(&font), galley.size().x / 64.0)
        });
        Self { font, language, wrap_width: f32::INFINITY, line_h, cw, ppp: ctx.pixels_per_point() }
    }

    fn wraps(&self) -> bool {
        self.wrap_width.is_finite()
    }
}

/// Номер символа (с нуля) байта `b` строки.
fn col_of(line: &str, b: usize) -> usize {
    line[..floor_char(line, b.min(line.len()))].chars().count()
}

/// Байт символа `col` строки (за концом — конец).
fn byte_of(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
}

impl EditState {
    /// Подхватить изменения текста, сделанные мимо виджета, и поправить индекс строк,
    /// подсветку, свёртки и курсоры. Неписанное в историю (внешняя правка) — отдельный
    /// шаг Undo, как у прежнего поля.
    pub fn observe(&mut self, text: &str, language: Language, now: f64) {
        if !self.seen || self.language != Some(language) {
            self.seen = true;
            self.language = Some(language);
            self.mirror = text.to_string();
            self.index = LineIndex::new(text);
            self.hl = Highlighter::default();
            self.rows.clear();
            self.folds.stale = true;
            self.max_chars = max_chars(text, &self.index);
            self.carets.clamp(text);
            self.recorded = false;
            return;
        }
        if self.mirror.len() == text.len() && self.mirror == text {
            self.recorded = false;
            return;
        }
        let Some(change) = text_change(&self.mirror, text) else { return };
        let (start, old_end, new_end) = change;
        let edit = Edit::new(start..old_end, &text[start..new_end]);
        if !self.recorded {
            let before = self.carets.clone();
            self.carets.map_through(std::slice::from_ref(&edit));
            self.carets.clamp(text);
            let back = vec![Edit::new(start..new_end, &self.mirror[start..old_end])];
            self.history.record(vec![edit], back, before, self.carets.clone(), false, now);
        }
        self.recorded = false;
        self.apply_change(text, change, now);
        self.mirror.clear();
        self.mirror.push_str(text);
        self.carets.clamp(text);
    }

    /// После своей правки: индекс строк и подсветка — сразу (следующее событие кадра
    /// уже видит новый текст).
    fn resync(&mut self, text: &str, now: f64) {
        if self.seen {
            let language = self.language.unwrap_or(Language::Plain);
            self.observe(text, language, now);
        }
    }

    fn apply_change(&mut self, text: &str, change: (usize, usize, usize), now: f64) {
        let (start, old_end, new_end) = change;
        // Свёртки: правка внутри скрытой части свёрнутого — развернуть его (как в VS);
        // остальные сдвигаются вместе с текстом. Индекс здесь ещё старый, `mirror` — старый текст.
        let folds = &mut self.folds;
        let regions = folds.regions.clone();
        let (old_index, old_text) = (&self.index, &self.mirror);
        folds.collapsed.retain(|header| {
            let Some(&(h, last, _)) = regions.iter().find(|(r, _, _)| r == header) else { return true };
            if h > old_text.len() || last > old_text.len() {
                return false;
            }
            // Скрытая часть — от конца строки-заголовка до конца последней строки области.
            let hidden_start = old_index.line_end(old_text, old_index.line_of(h));
            let hidden_end = old_index.line_end(old_text, old_index.line_of(last));
            !(start < hidden_end && old_end > hidden_start)
        });
        let first = self.index.line_of(start);
        let old_last = self.index.line_of(old_end);
        self.index.apply_change(text, change);
        let new_last = self.index.line_of(new_end);
        self.hl.edited(first, old_last, new_last);
        // Ряды переноса правленых строк — заново.
        if !self.rows.is_empty() {
            let added = new_last as isize - old_last as isize;
            let at = (old_last + 1).min(self.rows.len());
            if added > 0 {
                self.rows.splice(at..at, std::iter::repeat_n(0, added as usize));
            } else if added < 0 {
                let end = (first + 1 + (-added) as usize).min(self.rows.len());
                self.rows.drain((first + 1).min(end)..end);
            }
            for r in self.rows.iter_mut().skip(first).take(new_last - first + 1) {
                *r = 0;
            }
            if self.rows.len() != self.index.lines() {
                self.rows.clear();
            }
        }
        let folds = &mut self.folds;
        let shift = |p: &mut usize| {
            let mut r = *p..*p;
            crate::code_diagnostics::shift_range(&mut r, change);
            *p = r.start;
        };
        for header in &mut folds.collapsed {
            shift(header);
        }
        for (h, last, _) in &mut folds.regions {
            shift(h);
            shift(last);
        }
        folds.stale = true;
        self.changed_at = now;
        // Самая длинная строка (для горизонтальной прокрутки): правленые строки.
        for line in first..=new_last.min(self.index.lines().saturating_sub(1)) {
            let s = self.index.line_start(line);
            let e = self.index.line_end(text, line);
            self.max_chars = self.max_chars.max(text[s..e].chars().count());
        }
    }

    /// Правка курсорами: применить, записать в историю, поставить курсоры.
    pub fn commit(&mut self, text: &mut String, edits: Vec<Edit>, after: Carets, typing: bool, now: f64) {
        if edits.is_empty() {
            self.carets = after;
            self.carets.clamp(text);
            return;
        }
        let before = self.carets.clone();
        let mut sorted = edits;
        sorted.sort_by_key(|e| (e.range.start, e.range.end));
        let back = code_buffer::apply(text, &sorted);
        self.history.record(sorted, back, before, after.clone(), typing, now);
        self.carets = after;
        self.carets.clamp(text);
        self.recorded = true;
        self.box_sel = None;
        self.scroll_to_caret.get_or_insert(false);
        self.resync(text, now);
    }

    /// Заменить текст целиком одним шагом Undo и поставить выделение `anchor..caret`.
    pub fn replace_all(&mut self, text: &mut String, new_text: String, anchor: usize, caret: usize, now: f64) {
        let after = Carets::single(Sel::new(floor_char(&new_text, anchor), floor_char(&new_text, caret)));
        match code_buffer::diff(text, &new_text) {
            Some(edit) => {
                let before = self.carets.clone();
                let back = vec![Edit::new(edit.range.start..edit.range.start + edit.text.len(), &text[edit.range.clone()])];
                *text = new_text;
                self.history.record(vec![edit], back, before, after.clone(), false, now);
                self.recorded = true;
            }
            None => self.history.seal(),
        }
        self.carets = after;
        self.carets.clamp(text);
        self.box_sel = None;
        self.scroll_to_caret.get_or_insert(false);
        self.resync(text, now);
    }

    /// Поставить одно выделение (байты).
    pub fn set_selection(&mut self, text: &str, anchor: usize, caret: usize) {
        self.carets = Carets::single(Sel::new(floor_char(text, anchor), floor_char(text, caret)));
        self.box_sel = None;
        self.history.seal();
        self.scroll_to_caret.get_or_insert(false);
    }

    pub fn undo(&mut self, text: &mut String) -> bool {
        match self.history.undo(text) {
            Some((carets, _)) => {
                self.carets = carets;
                self.carets.clamp(text);
                self.recorded = true;
                self.box_sel = None;
                self.scroll_to_caret.get_or_insert(false);
                self.resync(text, self.changed_at);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, text: &mut String) -> bool {
        match self.history.redo(text) {
            Some((carets, _)) => {
                self.carets = carets;
                self.carets.clamp(text);
                self.recorded = true;
                self.box_sel = None;
                self.scroll_to_caret.get_or_insert(false);
                self.resync(text, self.changed_at);
                true
            }
            None => false,
        }
    }

    // --- Свёртки ---

    /// Области свёртки в строках (пересчитываются, когда текст постоял).
    pub fn fold_regions(&mut self, text: &str, language: Language, now: f64, force: bool) -> Vec<FoldRegion> {
        if self.folds.disabled {
            return Vec::new();
        }
        let quiet = force || text.len() < 200_000 || now - self.changed_at > 0.4;
        if self.folds.stale && quiet {
            let found = code_buffer::fold_regions(text, language);
            self.folds.regions = found.iter().map(|r| (self.index.line_start(r.start_line), self.index.line_start(r.end_line), r.kind)).collect();
            let regions = &self.folds.regions;
            self.folds.collapsed.retain(|h| regions.iter().any(|(r, _, _)| r == h));
            self.folds.stale = false;
        }
        self.folds
            .regions
            .iter()
            .map(|&(h, last, kind)| FoldRegion { start_line: self.index.line_of(h), end_line: self.index.line_of(last), kind })
            .filter(|r| r.end_line > r.start_line)
            .collect()
    }

    /// Скрытые строки (по порядку, не пересекаются).
    pub fn hidden(&self) -> Vec<Range<usize>> {
        if self.folds.disabled || self.folds.collapsed.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<Range<usize>> = self
            .folds
            .regions
            .iter()
            .filter(|(h, _, _)| self.folds.collapsed.contains(h))
            .map(|&(h, last, _)| self.index.line_of(h) + 1..self.index.line_of(last) + 1)
            .filter(|r| !r.is_empty())
            .collect();
        out.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(out.len());
        for r in out {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        merged
    }

    /// Развернуть свёртки, в которых спрятан байт `offset` (поиск, переход к строке, ошибка).
    pub fn reveal(&mut self, offset: usize) {
        let line = self.index.line_of(offset);
        let index = &self.index;
        let regions = &self.folds.regions;
        self.folds.collapsed.retain(|h| {
            let Some(&(_, last, _)) = regions.iter().find(|(r, _, _)| r == h) else { return true };
            let (a, b) = (index.line_of(*h), index.line_of(last));
            !(line > a && line <= b)
        });
    }

    /// Ctrl+M Ctrl+M: свернуть / развернуть блок у курсора (самый внутренний).
    pub fn toggle_fold_at(&mut self, text: &str, language: Language, line: usize, now: f64) -> bool {
        self.folds.disabled = false;
        let regions = self.fold_regions(text, language, now, true);
        // Сначала — свёрнутый, в котором строка (развернуть), затем начинающийся на ней,
        // затем самый внутренний вокруг неё.
        let pick = regions
            .iter()
            .filter(|r| r.start_line <= line && line <= r.end_line && self.folds.collapsed.contains(&self.index.line_start(r.start_line)))
            .min_by_key(|r| r.end_line - r.start_line)
            .or_else(|| regions.iter().filter(|r| r.start_line == line).max_by_key(|r| r.end_line))
            .or_else(|| regions.iter().filter(|r| r.start_line <= line && line <= r.end_line).min_by_key(|r| r.end_line - r.start_line))
            .copied();
        let Some(region) = pick else { return false };
        let header = self.index.line_start(region.start_line);
        if let Some(at) = self.folds.collapsed.iter().position(|h| *h == header) {
            self.folds.collapsed.remove(at);
        } else {
            self.folds.collapsed.push(header);
            self.move_out_of_folds(text);
        }
        true
    }

    /// Ctrl+M Ctrl+O: свернуть тела определений (функции, методы).
    pub fn collapse_to_definitions(&mut self, text: &str, language: Language, now: f64) {
        self.folds.disabled = false;
        let regions = self.fold_regions(text, language, now, true);
        for r in regions {
            let s = self.index.line_start(r.start_line);
            let e = self.index.line_end(text, r.start_line);
            if is_definition(&text[s..e], language) && !self.folds.collapsed.contains(&s) {
                self.folds.collapsed.push(s);
            }
        }
        self.move_out_of_folds(text);
    }

    /// Ctrl+M Ctrl+L: свёрнуто хоть что-то — развернуть всё, иначе свернуть всё.
    pub fn toggle_all_folds(&mut self, text: &str, language: Language, now: f64) {
        self.folds.disabled = false;
        let regions = self.fold_regions(text, language, now, true);
        if self.folds.collapsed.is_empty() {
            self.folds.collapsed = regions.iter().map(|r| self.index.line_start(r.start_line)).collect();
            self.move_out_of_folds(text);
        } else {
            self.folds.collapsed.clear();
        }
    }

    /// Ctrl+M Ctrl+P: развернуть всё и не показывать области.
    pub fn stop_outlining(&mut self) {
        self.folds.collapsed.clear();
        self.folds.disabled = true;
        self.folds.stale = true;
    }

    /// Курсоры, оказавшиеся в свёрнутом, — на строку-заголовок (в её конец).
    fn move_out_of_folds(&mut self, text: &str) {
        let hidden = self.hidden();
        for sel in &mut self.carets.sels {
            for at in [&mut sel.anchor, &mut sel.caret] {
                let line = self.index.line_of(*at);
                if let Some(r) = hidden.iter().find(|r| r.contains(&line)) {
                    *at = self.index.line_end(text, r.start - 1);
                }
            }
        }
        self.carets.normalize();
    }

    // --- Раскладка ---

    /// Конец кадра: раскладки, не понадобившиеся в нём, выбрасываются.
    pub fn end_frame(&mut self) {
        self.cache.end_frame();
    }

    fn galley(&mut self, ctx: &egui::Context, look: &Look, text: &str, line: usize) -> Arc<Galley> {
        self.hl.ensure(text, &self.index, look.language, line);
        let s = self.index.line_start(line);
        let e = self.index.line_end(text, line);
        let line_text = &text[s..e];
        let state = self.hl.state(line);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(line_text, &mut hasher);
        std::hash::Hash::hash(&state, &mut hasher);
        std::hash::Hash::hash(&(look.font.size.to_bits(), look.wrap_width.to_bits(), look.ppp.to_bits(), look.language as u8), &mut hasher);
        let key = std::hash::Hasher::finish(&hasher);
        if let Some(g) = self.cache.get(key) {
            return g;
        }
        let mut tokens = Vec::new();
        code_highlight::tokenize_line(line_text, look.language, state, &mut tokens);
        let mut job = LayoutJob::default();
        job.wrap.max_width = look.wrap_width;
        let mut last = 0;
        for (token, range) in tokens {
            if range.start > last {
                job.append(&line_text[last..range.start], 0.0, format(&look.font, Token::Plain));
            }
            job.append(&line_text[range.clone()], 0.0, format(&look.font, token));
            last = range.end;
        }
        if last < line_text.len() || line_text.is_empty() {
            job.append(&line_text[last..], 0.0, format(&look.font, Token::Plain));
        }
        let galley = ctx.fonts(|f| f.layout_job(job));
        self.cache.now.insert(key, galley.clone());
        galley
    }
}

fn format(font: &FontId, token: Token) -> TextFormat {
    TextFormat { font_id: font.clone(), color: code_highlight::token_color(token), ..TextFormat::default() }
}


/// Строка — заголовок определения (функция, метод)?
fn is_definition(line: &str, language: Language) -> bool {
    let t = line.trim_start();
    match language {
        Language::Rust => {
            let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).collect();
            words.iter().take(6).any(|w| *w == "fn")
        }
        Language::Scenario => t.starts_with("function") || t.starts_with("local function") || t.contains("= function"),
        Language::Wgsl => t.starts_with("fn ") || t.contains(" fn "),
        Language::Cpp => {
            let head = t.split('{').next().unwrap_or("");
            let control = ["if", "for", "while", "switch", "else", "do", "catch", "return"].iter().any(|k| head.starts_with(k) && head[k.len()..].starts_with([' ', '(']));
            head.contains('(') && head.trim_end().ends_with([')', 't', 'e']) && !control
                || head.trim_end().ends_with("const")
        }
        _ => false,
    }
}

fn max_chars(text: &str, index: &LineIndex) -> usize {
    (0..index.lines()).map(|l| index.line_end(text, l) - index.line_start(l)).max().unwrap_or(0)
}

/// Геометрия кадра: где строки на экране, их раскладки (только видимые), что скрыто.
#[derive(Clone)]
pub(crate) struct View {
    /// Экранная точка (0, 0) содержимого прокрутки.
    pub origin: Pos2,
    /// Экранный x столбца 0.
    pub text_left: f32,
    pub line_h: f32,
    pub cw: f32,
    /// Верх строки в содержимом (без `TOP_PAD`); скрытые строки — нулевой высоты. Длина —
    /// строк + 1 (последний — общая высота).
    pub tops: Vec<f32>,
    pub wrap: bool,
    /// Строки (с нуля), попавшие в видимую часть.
    pub visible: Range<usize>,
    /// Разложенные строки (видимые и чуть вокруг) по порядку.
    pub laid: Vec<(usize, Arc<Galley>)>,
    pub hidden: Vec<Range<usize>>,
    /// Видимая часть: вся, колонка номеров, текст.
    pub clip: Rect,
    pub gutter: Rect,
    pub text_clip: Rect,
    /// Области свёртки (для стрелок в колонке номеров).
    pub regions: Vec<FoldRegion>,
    pub content: egui::Vec2,
}

impl View {
    pub fn is_hidden(&self, line: usize) -> bool {
        let at = self.hidden.partition_point(|r| r.end <= line);
        self.hidden.get(at).is_some_and(|r| r.contains(&line))
    }

    /// Экранный верх строки.
    pub fn line_top(&self, line: usize) -> f32 {
        let y = self.origin.y + TOP_PAD + self.tops.get(line).copied().unwrap_or_else(|| *self.tops.last().unwrap_or(&0.0));
        y
    }

    pub fn galley(&self, line: usize) -> Option<&Arc<Galley>> {
        self.laid.binary_search_by_key(&line, |(l, _)| *l).ok().map(|k| &self.laid[k].1)
    }

    /// Строка под экранным y (скрытая — её заголовок).
    pub fn line_at_y(&self, y: f32) -> usize {
        let local = y - self.origin.y - TOP_PAD;
        let lines = self.tops.len().saturating_sub(1).max(1);
        let mut line = self.tops.partition_point(|t| *t <= local).saturating_sub(1).min(lines - 1);
        while line > 0 && self.is_hidden(line) {
            line -= 1;
        }
        line
    }

    /// Высота строки на экране (ряды переноса × высота ряда).
    pub fn line_height(&self, line: usize) -> f32 {
        match (self.tops.get(line), self.tops.get(line + 1)) {
            (Some(a), Some(b)) => (b - a).max(0.0),
            _ => self.line_h,
        }
    }
}

/// Верхи строк в содержимом: скрытые — нулевой высоты, при переносе — по рядам
/// (ещё не разложенные строки — оценка по длине).
fn tops_of(st: &EditState, text: &str, look: &Look, hidden: &[Range<usize>]) -> Vec<f32> {
    let lines = st.index.lines();
    let per_row = (look.wrap_width / look.cw.max(1.0)).max(1.0);
    let mut tops = Vec::with_capacity(lines + 1);
    let mut y = 0.0_f32;
    let mut h_at = 0;
    for line in 0..lines {
        tops.push(y);
        while h_at < hidden.len() && hidden[h_at].end <= line {
            h_at += 1;
        }
        if hidden.get(h_at).is_some_and(|r| r.contains(&line)) {
            continue;
        }
        let rows = match st.rows.get(line) {
            Some(0) => {
                let len = st.index.line_end(text, line) - st.index.line_start(line);
                ((len as f32 / per_row).ceil() as u32).max(1)
            }
            Some(r) if look.wraps() => *r,
            _ => 1,
        };
        y += rows as f32 * look.line_h;
    }
    tops.push(y);
    tops
}

/// Разложить видимую часть: высоты строк, скрытые строки, раскладки видимых строк.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout(ctx: &egui::Context, st: &mut EditState, text: &str, look: &Look, viewport: Rect, origin: Pos2, clip: Rect, gutter_w: f32, now: f64) -> View {
    let lines = st.index.lines();
    let regions = st.fold_regions(text, look.language, now, false);
    let hidden = st.hidden();
    // Ряды переноса: ширина сменилась — всё заново.
    if look.wraps() {
        if st.rows.len() != lines || (st.rows_width - look.wrap_width).abs() > 0.5 {
            st.rows = vec![0; lines];
            st.rows_width = look.wrap_width;
        }
    } else if !st.rows.is_empty() {
        st.rows.clear();
    }
    let mut tops = tops_of(st, text, look, &hidden);
    // Видимые строки (с запасом в две строки).
    let local_top = viewport.top() - TOP_PAD;
    let local_bottom = viewport.bottom() - TOP_PAD;
    let first = tops.partition_point(|t| *t <= local_top).saturating_sub(1).min(lines.saturating_sub(1));
    let last = tops.partition_point(|t| *t < local_bottom).min(lines);
    let laid_range = first.saturating_sub(2)..(last + 2).min(lines);
    let mut laid = Vec::with_capacity(laid_range.len());
    let mut remeasured = false;
    for line in laid_range {
        let at = hidden.partition_point(|r| r.end <= line);
        if hidden.get(at).is_some_and(|r| r.contains(&line)) {
            continue;
        }
        let g = st.galley(ctx, look, text, line);
        if look.wraps() {
            let rows = g.rows.len().max(1) as u32;
            if st.rows[line] != rows {
                st.rows[line] = rows;
                remeasured = true;
            }
        }
        laid.push((line, g));
    }
    if remeasured {
        // Высоты уточнились — пересчитать верхи (дёшево, без раскладки).
        tops = tops_of(st, text, look, &hidden);
    }
    let total = tops[lines];
    let width = if look.wraps() { clip.width() } else { gutter_w + theme::SP_6 + st.max_chars as f32 * look.cw + look.cw * 8.0 };
    let content = vec2(width, total + TOP_PAD * 2.0);
    let gutter = Rect::from_min_max(clip.min, pos2(clip.left() + gutter_w, clip.bottom()));
    let text_clip = Rect::from_min_max(pos2(gutter.right(), clip.top()), clip.max);
    View {
        origin,
        text_left: origin.x + gutter_w + theme::SP_6,
        line_h: look.line_h,
        cw: look.cw,
        tops,
        wrap: look.wraps(),
        visible: first..last.max(first + 1).min(lines.max(1)),
        laid,
        hidden,
        clip,
        gutter,
        text_clip,
        regions,
        content,
    }
}

/// Перевод между байтами и экраном по раскладке кадра (и раскладке по требованию для
/// строк вне видимой части).
pub(crate) struct Mapper<'a> {
    pub ctx: &'a egui::Context,
    pub look: &'a Look,
    pub view: &'a View,
}

impl Mapper<'_> {
    fn galley(&self, st: &mut EditState, text: &str, line: usize) -> Arc<Galley> {
        let s = st.index.line_start(line);
        let e = st.index.line_end(text, line);
        match self.view.galley(line) {
            // Раскладка кадра годится, пока строку не правили в этом же кадре.
            Some(g) if g.text() == &text[s..e] => g.clone(),
            _ => st.galley(self.ctx, self.look, text, line),
        }
    }

    /// Экранный прямоугольник (нулевой ширины) курсора в байте `at`.
    pub fn caret_rect(&self, st: &mut EditState, text: &str, at: usize) -> Rect {
        let line = st.index.line_of(at);
        let s = st.index.line_start(line);
        let e = st.index.line_end(text, line);
        let col = col_of(&text[s..e], at - s.min(at));
        let g = self.galley(st, text, line);
        let r = g.pos_from_ccursor(CCursor::new(col));
        let top = self.view.line_top(line);
        Rect::from_min_max(pos2(self.view.text_left + r.min.x, top + r.min.y), pos2(self.view.text_left + r.min.x, top + r.min.y + self.look.line_h))
    }

    /// Байт строки `line` у экранного x (`row` — ряд переноса).
    fn offset_in_line(&self, st: &mut EditState, text: &str, line: usize, x: f32, row: usize) -> usize {
        let g = self.galley(st, text, line);
        let row_y = g.rows.get(row).map_or(0.0, |r| r.rect.center().y);
        let cursor = g.cursor_from_pos(vec2(x - self.view.text_left, row_y));
        let s = st.index.line_start(line);
        let e = st.index.line_end(text, line);
        s + byte_of(&text[s..e], cursor.ccursor.index)
    }

    /// Байт под экранной точкой.
    pub fn offset_at(&self, st: &mut EditState, text: &str, pos: Pos2) -> usize {
        let line = self.view.line_at_y(pos.y);
        let top = self.view.line_top(line);
        let g = self.galley(st, text, line);
        let cursor = g.cursor_from_pos(vec2(pos.x - self.view.text_left, (pos.y - top).max(0.0)));
        let s = st.index.line_start(line);
        let e = st.index.line_end(text, line);
        s + byte_of(&text[s..e], cursor.ccursor.index)
    }

    /// x курсора (от начала текста) и номер ряда переноса.
    fn caret_x_row(&self, st: &mut EditState, text: &str, at: usize) -> (f32, usize) {
        let line = st.index.line_of(at);
        let s = st.index.line_start(line);
        let e = st.index.line_end(text, line);
        let col = col_of(&text[s..e], at - s.min(at));
        let g = self.galley(st, text, line);
        let cursor = g.from_ccursor(CCursor::new(col));
        let r = g.pos_from_cursor(&cursor);
        (r.min.x + self.view.text_left, cursor.rcursor.row)
    }

    /// Курсор на ряд выше / ниже (через скрытые строки), x — желаемый.
    fn vertical(&self, st: &mut EditState, text: &str, at: usize, x: f32, down: bool) -> usize {
        let line = st.index.line_of(at);
        let (_, row) = self.caret_x_row(st, text, at);
        let g = self.galley(st, text, line);
        if down && row + 1 < g.rows.len() {
            return self.offset_in_line(st, text, line, x, row + 1);
        }
        if !down && row > 0 {
            return self.offset_in_line(st, text, line, x, row - 1);
        }
        let Some(target) = self.step_line(line, down, st.index.lines()) else {
            return if down { text.len() } else { 0 };
        };
        let rows = self.galley(st, text, target).rows.len().max(1);
        self.offset_in_line(st, text, target, x, if down { 0 } else { rows - 1 })
    }

    /// Следующая видимая строка вверх / вниз.
    fn step_line(&self, line: usize, down: bool, lines: usize) -> Option<usize> {
        let mut l = line;
        loop {
            if down {
                l += 1;
                if l >= lines {
                    return None;
                }
            } else {
                l = l.checked_sub(1)?;
            }
            if !self.view.is_hidden(l) {
                return Some(l);
            }
        }
    }
}

/// Что сделал ввод кадра.
#[derive(Default)]
pub(crate) struct InputResult {
    pub changed: bool,
    pub moved: bool,
}

/// Параметры набора.
pub(crate) struct TypingEnv<'a> {
    pub prefs: &'a CodePrefs,
    pub indent: Indent,
    pub now: f64,
    pub focused: bool,
}

fn unit(indent: Indent) -> String {
    if indent.tabs {
        "\t".to_string()
    } else {
        " ".repeat(indent.size.max(1))
    }
}

/// Правка каждым курсором. `f` по тексту и выделению курсора даёт новый текст и
/// выделение (`None` — курсор ничего не меняет). Возвращает правки в координатах
/// исходного текста и новые курсоры. Правки разных курсоров не должны пересекаться;
/// если пересеклись (две каретки на одной строке и строковая команда) — курсоры
/// применяются по очереди, и правка записывается одним куском.
pub(crate) fn edit_each(text: &str, carets: &Carets, mut f: impl FnMut(&str, Sel) -> Option<Edited>) -> Option<(Vec<Edit>, Carets)> {
    // Правка курсора и его новое (якорь, курсор).
    type Result1 = (Option<Edit>, Option<(usize, usize)>);
    let mut results: Vec<Result1> = Vec::with_capacity(carets.sels.len());
    let mut any = false;
    for sel in &carets.sels {
        match f(text, *sel) {
            Some(edited) => {
                let edit = code_buffer::diff(text, &edited.text);
                any = true;
                results.push((edit, Some((edited.anchor, edited.caret))));
            }
            None => results.push((None, None)),
        }
    }
    if !any {
        return None;
    }
    // Правки по порядку и без пересечений?
    let mut last_end = 0;
    let mut ok = true;
    for (edit, _) in &results {
        if let Some(e) = edit {
            if e.range.start < last_end {
                ok = false;
                break;
            }
            last_end = e.range.end.max(e.range.start + 1);
        }
    }
    if ok {
        let edits: Vec<Edit> = results.iter().filter_map(|(e, _)| e.clone()).collect();
        let mut out = carets.clone();
        let mut delta: isize = 0;
        for (k, (edit, pos)) in results.iter().enumerate() {
            match (edit, pos) {
                (_, Some((a, c))) => {
                    // Позиции — в тексте только с этой правкой; правки выше сдвигают их.
                    let shift = |p: usize| (p as isize + delta).max(0) as usize;
                    out.sels[k] = Sel::new(shift(*a), shift(*c));
                }
                (None, None) => {
                    let s = out.sels[k];
                    out.sels[k] = Sel::new(code_buffer::map_pos(s.anchor, &edits), code_buffer::map_pos(s.caret, &edits));
                }
                _ => {}
            }
            if let Some(e) = edit {
                delta += e.text.len() as isize - e.range.len() as isize;
            }
        }
        out.normalize();
        return Some((edits, out));
    }
    // По очереди, с последнего курсора.
    let mut cur = text.to_string();
    let mut out = carets.clone();
    for k in (0..carets.sels.len()).rev() {
        let sel = out.sels[k];
        let Some(edited) = f(&cur, sel) else { continue };
        let Some(change) = text_change(&cur, &edited.text) else {
            out.sels[k] = Sel::new(edited.anchor, edited.caret);
            continue;
        };
        for later in out.sels.iter_mut().skip(k + 1) {
            for p in [&mut later.anchor, &mut later.caret] {
                let mut r = *p..*p;
                crate::code_diagnostics::shift_range(&mut r, change);
                *p = r.start;
            }
        }
        out.sels[k] = Sel::new(edited.anchor, edited.caret);
        cur = edited.text;
    }
    out.normalize();
    let edit = code_buffer::diff(text, &cur)?;
    Some((vec![edit], out))
}

/// Вставить `insert` вместо выделения каждого курсора.
fn insert_each(text: &str, carets: &Carets, insert: impl Fn(usize) -> String) -> (Vec<Edit>, Carets) {
    let mut edits = Vec::with_capacity(carets.sels.len());
    let mut out = carets.clone();
    let mut delta: isize = 0;
    for (k, sel) in carets.sels.iter().enumerate() {
        let r = sel.range();
        let piece = insert(k);
        let at = (r.start as isize + delta) as usize + piece.len();
        delta += piece.len() as isize - r.len() as isize;
        out.sels[k] = Sel::one(at);
        if !(r.is_empty() && piece.is_empty()) {
            edits.push(Edit::new(r, piece));
        }
    }
    let _ = text;
    out.normalize();
    (edits, out)
}

/// Удалить у каждого курсора выделение или кусок `range_of(caret)`.
fn delete_each(text: &str, carets: &Carets, range_of: impl Fn(&str, usize) -> Range<usize>) -> (Vec<Edit>, Carets) {
    let mut ranges: Vec<Range<usize>> = carets.sels.iter().map(|s| if s.is_empty() { range_of(text, s.caret) } else { s.range() }).collect();
    // Слить пересекающиеся.
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    ranges.sort_by_key(|r| r.start);
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    let edits: Vec<Edit> = merged.iter().filter(|r| !r.is_empty()).map(|r| Edit::new(r.clone(), "")).collect();
    let mut out = carets.clone();
    for sel in &mut out.sels {
        let r = if sel.is_empty() { range_of(text, sel.caret) } else { sel.range() };
        let at = code_buffer::map_pos(r.start, &edits);
        *sel = Sel::one(at);
    }
    out.normalize();
    (edits, out)
}

fn prev_char(text: &str, at: usize) -> usize {
    text[..at].chars().next_back().map_or(at, |c| at - c.len_utf8())
}

fn next_char(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

/// Мышь и клавиатура кадра. `response` — отклик области текста (с колонкой номеров).
#[allow(clippy::too_many_arguments)]
pub(crate) fn interact(ui: &mut egui::Ui, st: &mut EditState, text: &mut String, map: &Mapper<'_>, response: &egui::Response, below: &egui::Response, env: &TypingEnv<'_>) -> InputResult {
    let mut result = InputResult::default();
    let view = map.view;
    let now = env.now;
    // Модификаторы щелчка — из самого события нажатия (как их видела система в тот миг).
    let modifiers = ui.input(|i| {
        i.events
            .iter()
            .find_map(|e| match e {
                Event::PointerButton { pressed: true, button: egui::PointerButton::Primary, modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or(i.modifiers)
    });
    let lines = st.index.lines();

    // --- Мышь ---
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if (response.hovered() || below.hovered()) && pressed {
        if let Some(pos) = pointer {
            st.history.seal();
            st.preedit = None;
            let in_gutter = pos.x < view.text_clip.left();
            let at = if below.hovered() { text.len() } else { map.offset_at(st, text, pos) };
            let line = st.index.line_of(at);
            let fold_hit = in_gutter && pos.x >= view.gutter.right() - FOLD_W && view.regions.iter().any(|r| r.start_line == line);
            if fold_hit {
                st.toggle_fold_at(text, map.look.language, line, now);
            } else if in_gutter && !below.hovered() {
                // Колонка номеров: выделить строку (Shift — до неё).
                let start = st.index.line_start(line);
                let end = if line + 1 < lines { st.index.line_start(line + 1) } else { text.len() };
                if modifiers.shift {
                    let anchor = st.carets.primary().anchor;
                    st.carets = Carets::single(Sel::new(anchor, if start < anchor { start } else { end }));
                } else {
                    st.carets = Carets::single(Sel::new(start, end));
                }
                st.drag = Some(Drag::Lines(line));
            } else if modifiers.alt && modifiers.shift {
                // Выделение столбцом от главного курсора.
                let primary = st.carets.primary();
                let anchor = st.box_sel.map(|b| (b.anchor_line, b.anchor_x)).unwrap_or_else(|| {
                    let (x, _) = map.caret_x_row(st, text, primary.caret);
                    (st.index.line_of(primary.caret), x - view.text_left)
                });
                let b = BoxSel { anchor_line: anchor.0, anchor_x: anchor.1, head_line: line, head_x: pos.x - view.text_left };
                apply_box(st, text, map, b);
                st.drag = Some(Drag::Box);
            } else if modifiers.alt {
                // Alt+щелчок (и Ctrl+Alt+щелчок, как в Visual Studio) — ещё один курсор.
                st.box_sel = None;
                st.carets.push(Sel::one(at));
                st.drag = Some(Drag::Text);
            } else if modifiers.shift {
                let anchor = st.carets.primary().anchor;
                st.carets = Carets::single(Sel::new(anchor, at));
                st.box_sel = None;
                st.drag = Some(Drag::Text);
            } else {
                st.carets = Carets::one(at);
                st.box_sel = None;
                st.drag = Some(Drag::Text);
            }
            ui.memory_mut(|m| m.request_focus(response.id));
            result.moved = true;
        }
    }
    let down = response.is_pointer_button_down_on() || below.is_pointer_button_down_on();
    if !down {
        st.drag = None;
    } else if let (Some(drag), Some(pos)) = (st.drag, ui.input(|i| i.pointer.latest_pos())) {
        if !pressed {
            let at = map.offset_at(st, text, pos);
            match drag {
                Drag::Text => {
                    let sel = st.carets.primary_mut();
                    if sel.caret != at {
                        sel.caret = at;
                        st.carets.normalize();
                        result.moved = true;
                    }
                }
                Drag::Lines(from) => {
                    let line = st.index.line_of(at);
                    let (a, b) = (from.min(line), from.max(line));
                    let start = st.index.line_start(a);
                    let end = if b + 1 < lines { st.index.line_start(b + 1) } else { text.len() };
                    let sel = if line < from { Sel::new(end, start) } else { Sel::new(start, end) };
                    if st.carets.primary() != sel {
                        st.carets = Carets::single(sel);
                        result.moved = true;
                    }
                }
                Drag::Box => {
                    if let Some(mut b) = st.box_sel {
                        let line = view.line_at_y(pos.y);
                        let x = pos.x - view.text_left;
                        if b.head_line != line || (b.head_x - x).abs() > 0.5 {
                            b.head_line = line;
                            b.head_x = x;
                            apply_box(st, text, map, b);
                            result.moved = true;
                        }
                    }
                }
            }
            // Тянут за край — прокрутка к курсору.
            if !view.text_clip.contains(pos) {
                st.scroll_to_caret = Some(false);
                ui.ctx().request_repaint();
            }
        }
    }
    let plain_click = !modifiers.alt && !modifiers.shift;
    if plain_click && response.double_clicked() && !response.triple_clicked() {
        if let Some(pos) = pointer.or_else(|| response.interact_pointer_pos()) {
            if pos.x >= view.text_clip.left() {
                let at = map.offset_at(st, text, pos);
                let w = code_buffer::word_range(text, at);
                st.carets = Carets::single(Sel::new(w.start, w.end));
                st.box_sel = None;
                result.moved = true;
            }
        }
    }
    if plain_click && response.triple_clicked() {
        if let Some(pos) = pointer.or_else(|| response.interact_pointer_pos()) {
            let at = map.offset_at(st, text, pos);
            let line = st.index.line_of(at);
            let start = st.index.line_start(line);
            let end = if line + 1 < lines { st.index.line_start(line + 1) } else { text.len() };
            st.carets = Carets::single(Sel::new(start, end));
            st.box_sel = None;
            result.moved = true;
        }
    }

    // Правый щелчок: внутри выделения — оставить его, иначе курсор туда (несколько
    // курсоров — один); меню рисует вкладка.
    let secondary = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    if secondary && response.hovered() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()).filter(|p| p.x >= view.text_clip.left()) {
            let at = map.offset_at(st, text, pos);
            let inside = st.carets.sels.iter().any(|s| !s.is_empty() && s.range().start <= at && at <= s.range().end);
            if !inside {
                st.carets = Carets::one(at);
                st.box_sel = None;
                st.history.seal();
                result.moved = true;
            }
            ui.memory_mut(|m| m.request_focus(response.id));
        }
    }

    // --- Клавиатура ---
    if !env.focused {
        return result;
    }
    let mut events: Vec<Event> = std::mem::take(&mut st.pending);
    events.extend(ui.input(|i| i.events.clone()));
    let vs = env.prefs.keymap == Keymap::VisualStudio;
    for event in events {
        let before_text_len = text.len();
        match event {
            Event::Text(t) if !t.is_empty() && t != "\n" && t != "\r" => {
                if t == "\t" {
                    continue;
                }
                type_text(st, text, &t, env, map.look.language, vs);
                result.changed = true;
            }
            Event::Ime(egui::ImeEvent::Preedit(p)) => {
                st.preedit = (!p.is_empty() && p != "\n" && p != "\r").then_some(p);
            }
            Event::Ime(egui::ImeEvent::Commit(c)) => {
                st.preedit = None;
                if !c.is_empty() && c != "\n" && c != "\r" {
                    type_text(st, text, &c, env, map.look.language, false);
                    result.changed = true;
                }
            }
            Event::Ime(egui::ImeEvent::Disabled) => st.preedit = None,
            Event::Copy | Event::Cut => {
                let cut = matches!(event, Event::Cut);
                let mut pieces = Vec::new();
                let whole_lines = st.carets.sels.iter().all(Sel::is_empty);
                for sel in &st.carets.sels {
                    if whole_lines {
                        // Visual Studio: без выделения — строка целиком.
                        let line = st.index.line_of(sel.caret);
                        let start = st.index.line_start(line);
                        let end = if line + 1 < lines { st.index.line_start(line + 1) } else { text.len() };
                        let mut piece = text[start..end].to_string();
                        if !piece.ends_with('\n') {
                            piece.push('\n');
                        }
                        pieces.push(piece);
                    } else {
                        pieces.push(text[sel.range()].to_string());
                    }
                }
                let joined = if whole_lines { pieces.concat() } else { pieces.join("\n") };
                if !joined.is_empty() {
                    ui.ctx().copy_text(joined);
                }
                if cut {
                    let (edits, after) = if whole_lines {
                        delete_each(text, &st.carets, |t, at| {
                            let s = code_edit::line_start(t, at);
                            let e = code_edit::line_end(t, at);
                            let e = if e < t.len() { e + 1 } else { e };
                            s..e
                        })
                    } else {
                        delete_each(text, &st.carets, |_, at| at..at)
                    };
                    st.commit(text, edits, after, false, now);
                    result.changed = true;
                }
            }
            Event::Paste(clip) => {
                let clip = clip.replace("\r\n", "\n");
                let parts: Vec<&str> = clip.trim_end_matches('\n').split('\n').collect();
                let per_caret = st.carets.is_multi() && parts.len() == st.carets.sels.len();
                let (edits, after) = insert_each(text, &st.carets, |k| if per_caret { parts[k].to_string() } else { clip.clone() });
                st.commit(text, edits, after, false, now);
                result.changed = true;
            }
            Event::Key { key, pressed: true, modifiers: m, .. } if key_press(ui, st, text, map, env, key, m, vs, &mut result) => {
                ui.input_mut(|i| {
                    if let Some(at) = i.events.iter().position(|e| matches!(e, Event::Key { key: k, pressed: true, modifiers: mm, .. } if *k == key && *mm == m)) {
                        i.events.remove(at);
                    }
                });
            }
            _ => {}
        }
        if text.len() != before_text_len {
            result.changed = true;
        }
    }
    result
}

/// Набор текста каждым курсором (с парными скобками и перешагиванием в раскладке VS).
fn type_text(st: &mut EditState, text: &mut String, t: &str, env: &TypingEnv<'_>, language: Language, smart: bool) {
    let mut chars = t.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    let smart = smart && env.prefs.auto_close;
    let edited = match single {
        // Скобка или кавычка: пара / перешагивание у каждого курсора; где правило не
        // сработало — обычная вставка.
        Some(c) if smart && matches!(c, '(' | '[' | '{' | ')' | ']' | '}' | '"' | '\'' | '`') => edit_each(text, &st.carets, |text, sel| {
            code_edit::type_char(text, sel.anchor, sel.caret, c, language).or_else(|| {
                let r = sel.range();
                let at = r.start + c.len_utf8();
                Some(Edited { text: format!("{}{c}{}", &text[..r.start], &text[r.end..]), anchor: at, caret: at })
            })
        }),
        _ => None,
    };
    let (edits, after) = match edited {
        Some(done) => done,
        None => insert_each(text, &st.carets, |_| t.to_string()),
    };
    st.commit(text, edits, after, true, env.now);
}

/// Выделение столбцом: курсор на каждой видимой строке от якоря до головы.
fn apply_box(st: &mut EditState, text: &str, map: &Mapper<'_>, b: BoxSel) {
    let (a, z) = (b.anchor_line.min(b.head_line), b.anchor_line.max(b.head_line));
    let mut sels = Vec::new();
    for line in a..=z.min(st.index.lines().saturating_sub(1)) {
        if map.view.is_hidden(line) {
            continue;
        }
        let left = map.view.text_left;
        let anchor = map.offset_in_line(st, text, line, left + b.anchor_x, 0);
        let head = map.offset_in_line(st, text, line, left + b.head_x, 0);
        sels.push(Sel::new(anchor, head));
    }
    if sels.is_empty() {
        return;
    }
    let primary = if b.head_line >= b.anchor_line { sels.len() - 1 } else { 0 };
    st.carets = Carets { sels, primary };
    st.carets.normalize();
    st.box_sel = Some(b);
    st.history.seal();
}

/// Клавиша. `true` — обработана (убрать из ввода).
#[allow(clippy::too_many_arguments)]
fn key_press(ui: &mut egui::Ui, st: &mut EditState, text: &mut String, map: &Mapper<'_>, env: &TypingEnv<'_>, key: Key, m: Modifiers, vs: bool, result: &mut InputResult) -> bool {
    let now = env.now;
    let lines = st.index.lines();
    let shift = m.shift;
    let word = m.command || m.ctrl;
    let language = map.look.language;
    // Движение курсоров: новая позиция по старой; Shift — тянуть выделение.
    type Step<'a> = &'a mut dyn FnMut(&mut EditState, &str, Sel) -> (usize, Option<f32>);
    let mut motion = |st: &mut EditState, text: &str, f: Step<'_>| {
        let sels = st.carets.sels.clone();
        let mut out = st.carets.clone();
        for (k, sel) in sels.iter().enumerate() {
            let (at, goal) = f(st, text, *sel);
            out.sels[k] = if shift { Sel { anchor: sel.anchor, caret: at, goal_x: goal } } else { Sel { anchor: at, caret: at, goal_x: goal } };
        }
        out.normalize();
        st.carets = out;
        st.box_sel = None;
        st.history.seal();
        st.scroll_to_caret.get_or_insert(false);
        result.moved = true;
    };
    match key {
        Key::ArrowLeft | Key::ArrowRight if !m.alt => {
            let right = key == Key::ArrowRight;
            motion(st, text, &mut |st, text, sel| {
                if !shift && !sel.is_empty() && !word {
                    return (if right { sel.range().end } else { sel.range().start }, None);
                }
                let at = if word {
                    if right { code_buffer::next_word(text, sel.caret) } else { code_buffer::prev_word(text, sel.caret) }
                } else if right {
                    next_char(text, sel.caret)
                } else {
                    prev_char(text, sel.caret)
                };
                // Через свёрнутое — на следующую видимую строку.
                let line = st.index.line_of(at);
                let at = if map.view.is_hidden(line) {
                    let r = map.view.hidden.iter().find(|r| r.contains(&line)).cloned().unwrap_or(line..line + 1);
                    if right { if r.end < lines { st.index.line_start(r.end) } else { text.len() } } else { st.index.line_end(text, r.start - 1) }
                } else {
                    at
                };
                (at, None)
            });
            true
        }
        Key::ArrowUp | Key::ArrowDown if !m.alt && !word => {
            let down = key == Key::ArrowDown;
            motion(st, text, &mut |st, text, sel| {
                let from = if !shift && !sel.is_empty() { if down { sel.range().end } else { sel.range().start } } else { sel.caret };
                let x = sel.goal_x.unwrap_or_else(|| map.caret_x_row(st, text, from).0);
                (map.vertical(st, text, from, x, down), Some(x))
            });
            true
        }
        Key::ArrowUp | Key::ArrowDown if word && !m.alt => {
            // Ctrl+↑ / ↓ — прокрутить на строку, курсор на месте.
            let dy = if key == Key::ArrowUp { map.look.line_h } else { -map.look.line_h };
            ui.scroll_with_delta(vec2(0.0, dy));
            true
        }
        Key::Home | Key::End if !m.alt => {
            let home = key == Key::Home;
            motion(st, text, &mut |st, text, sel| {
                if word {
                    return (if home { 0 } else { text.len() }, None);
                }
                let line = st.index.line_of(sel.caret);
                if home {
                    if vs {
                        (code_edit::smart_home(text, sel.caret), None)
                    } else {
                        (st.index.line_start(line), None)
                    }
                } else {
                    (st.index.line_end(text, line), None)
                }
            });
            true
        }
        Key::PageUp | Key::PageDown => {
            let down = key == Key::PageDown;
            let page = ((map.view.text_clip.height() / map.look.line_h).floor() as usize).saturating_sub(1).max(1);
            let before = map.caret_rect(st, text, st.carets.primary().caret).top();
            motion(st, text, &mut |st, text, sel| {
                let x = sel.goal_x.unwrap_or_else(|| map.caret_x_row(st, text, sel.caret).0);
                let mut at = sel.caret;
                for _ in 0..page {
                    let next = map.vertical(st, text, at, x, down);
                    if next == at {
                        break;
                    }
                    at = next;
                }
                (at, Some(x))
            });
            let after = map.caret_rect(st, text, st.carets.primary().caret).top();
            ui.scroll_with_delta(vec2(0.0, before - after));
            true
        }
        Key::A if word && !m.alt && !shift => {
            st.carets = Carets::single(Sel::new(0, text.len()));
            st.box_sel = None;
            result.moved = true;
            true
        }
        Key::Z if word && !m.alt => {
            if shift {
                st.redo(text);
            } else {
                st.undo(text);
            }
            result.changed = true;
            true
        }
        Key::Y if word && !m.alt && !shift => {
            st.redo(text);
            result.changed = true;
            true
        }
        Key::Escape if m.is_none() => {
            if st.carets.is_multi() || st.box_sel.is_some() {
                st.carets.collapse();
                st.box_sel = None;
                result.moved = true;
                return true;
            }
            false
        }
        Key::Backspace if !m.alt => {
            st.preedit = None;
            let smart = vs && env.prefs.auto_close && !word;
            let pairs = if smart { edit_each(text, &st.carets, |t, sel| code_edit::backspace_pair(t, sel.anchor, sel.caret)) } else { None };
            let (edits, after) = match pairs {
                Some(done) if !st.carets.is_multi() => done,
                _ => delete_each(text, &st.carets, |t, at| if word { code_buffer::prev_word(t, at)..at } else { prev_char(t, at)..at }),
            };
            st.commit(text, edits, after, true, now);
            result.changed = true;
            true
        }
        Key::Delete if !m.alt && !shift => {
            let (edits, after) = delete_each(text, &st.carets, |t, at| if word { at..code_buffer::next_word(t, at) } else { at..next_char(t, at) });
            st.commit(text, edits, after, true, now);
            result.changed = true;
            true
        }
        Key::Enter if !m.alt && !word => {
            let unit = unit(env.indent);
            let done = if vs {
                edit_each(text, &st.carets, |t, sel| Some(code_edit::newline(t, sel.anchor, sel.caret, &unit, language)))
            } else {
                Some(insert_each(text, &st.carets, |_| "\n".to_string()))
            };
            if let Some((edits, after)) = done {
                st.commit(text, edits, after, true, now);
                result.changed = true;
            }
            true
        }
        Key::Tab if !word && !m.alt => {
            let unit = unit(env.indent);
            let indent = env.indent;
            let done = edit_each(text, &st.carets, |t, sel| {
                let r = sel.range();
                let multi = t[r.clone()].contains('\n');
                if !vs && !shift && !multi {
                    let out = format!("{}\t{}", &t[..r.start], &t[r.end..]);
                    return Some(Edited { text: out, anchor: r.start + 1, caret: r.start + 1 });
                }
                if multi || shift {
                    return Some(code_edit::indent_lines(t, sel.anchor, sel.caret, &unit, shift));
                }
                if indent.tabs {
                    let out = format!("{}\t{}", &t[..r.start], &t[r.end..]);
                    return Some(Edited { text: out, anchor: r.start + 1, caret: r.start + 1 });
                }
                // Пробелы до следующей позиции табуляции.
                let start = code_edit::line_start(t, r.start);
                let column = t[start..r.start].chars().count();
                let size = indent.size.max(1);
                let spaces = " ".repeat(size - column % size);
                let out = format!("{}{spaces}{}", &t[..r.start], &t[r.end..]);
                let at = r.start + spaces.len();
                Some(Edited { text: out, anchor: at, caret: at })
            });
            if let Some((edits, after)) = done {
                let typing = !shift && !st.carets.sels.iter().any(|s| text[s.range()].contains('\n'));
                st.commit(text, edits, after, typing, now);
                result.changed = true;
            }
            true
        }
        _ => false,
    }
}

// --- Команды нескольких курсоров (из реестра горячих клавиш) ---

/// Ctrl+Alt+↑ / ↓: курсор на строку выше / ниже крайнего.
pub(crate) fn add_caret_vertical(st: &mut EditState, text: &str, map: &Mapper<'_>, down: bool) {
    let sel = st.carets.primary();
    let x = sel.goal_x.unwrap_or_else(|| map.caret_x_row(st, text, sel.caret).0);
    let at = map.vertical(st, text, sel.caret, x, down);
    if at == sel.caret {
        return;
    }
    st.carets.push(Sel { anchor: at, caret: at, goal_x: Some(x) });
    st.box_sel = None;
    st.scroll_to_caret.get_or_insert(false);
}

/// Ctrl+Shift+Alt+стрелки: растянуть выделение столбцом.
pub(crate) fn box_select_step(st: &mut EditState, text: &str, map: &Mapper<'_>, key: Key) {
    let mut b = st.box_sel.unwrap_or_else(|| {
        let caret = st.carets.primary().caret;
        let (x, _) = map.caret_x_row(st, text, caret);
        let line = st.index.line_of(caret);
        let x = x - map.view.text_left;
        BoxSel { anchor_line: line, anchor_x: x, head_line: line, head_x: x }
    });
    match key {
        Key::ArrowLeft => b.head_x = (b.head_x - map.look.cw).max(0.0),
        Key::ArrowRight => b.head_x += map.look.cw,
        Key::ArrowUp => b.head_line = map.step_line(b.head_line, false, st.index.lines()).unwrap_or(b.head_line),
        Key::ArrowDown => b.head_line = map.step_line(b.head_line, true, st.index.lines()).unwrap_or(b.head_line),
        _ => return,
    }
    // Округлить к границе символа.
    b.head_x = (b.head_x / map.look.cw).round() * map.look.cw;
    apply_box(st, text, map, b);
    st.scroll_to_caret.get_or_insert(false);
}

/// Shift+Alt+.: следующее вхождение выделения — ещё одно выделение (пустой курсор —
/// сначала выделить слово).
pub(crate) fn add_next_occurrence(st: &mut EditState, text: &str) -> bool {
    let primary = st.carets.primary();
    if primary.is_empty() {
        let w = code_buffer::word_range(text, primary.caret);
        if w.is_empty() || !text[w.clone()].chars().all(code_buffer::is_word) {
            return false;
        }
        *st.carets.primary_mut() = Sel::new(w.start, w.end);
        st.occurrence_word = true;
        st.scroll_to_caret.get_or_insert(false);
        return true;
    }
    let needle = text[primary.range()].to_string();
    if !st.carets.is_multi() {
        let r = primary.range();
        st.occurrence_word = text[r.clone()].chars().all(code_buffer::is_word) && code_buffer::word_range(text, r.start) == r;
    }
    let found = code_buffer::occurrences(text, &needle, st.occurrence_word);
    let taken = |r: &Range<usize>| st.carets.sels.iter().any(|s| s.range() == *r);
    // После последнего добавленного (главного), по кругу.
    let after = primary.range().end;
    let next = found.iter().find(|r| r.start >= after && !taken(r)).or_else(|| found.iter().find(|r| !taken(r))).cloned();
    let Some(r) = next else { return false };
    st.carets.push(Sel::new(r.start, r.end));
    st.box_sel = None;
    st.scroll_to_caret.get_or_insert(false);
    true
}

/// Shift+Alt+; (Ctrl+Shift+L): все вхождения выделения (или слова под курсором).
pub(crate) fn select_all_occurrences(st: &mut EditState, text: &str) -> bool {
    let primary = st.carets.primary();
    let (range, whole) = if primary.is_empty() {
        let w = code_buffer::word_range(text, primary.caret);
        (w.clone(), true)
    } else {
        let r = primary.range();
        let whole = text[r.clone()].chars().all(code_buffer::is_word) && code_buffer::word_range(text, r.start) == r;
        (r, whole)
    };
    if range.is_empty() {
        return false;
    }
    let needle = text[range.clone()].to_string();
    let found = code_buffer::occurrences(text, &needle, whole);
    if found.is_empty() {
        return false;
    }
    let primary_at = found.iter().position(|r| *r == range).unwrap_or(0);
    st.carets = Carets { sels: found.into_iter().map(|r| Sel::new(r.start, r.end)).collect(), primary: primary_at };
    st.carets.normalize();
    st.box_sel = None;
    st.occurrence_word = whole;
    true
}

// --- Рисование ---

fn snap(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

/// Прямоугольники куска `range` строки `line` (по рядам переноса), экран.
fn range_rects(view: &View, galley: &Galley, line_start: usize, line_text: &str, line: usize, range: Range<usize>, newline_tail: bool) -> Vec<Rect> {
    let mut out = Vec::new();
    let a = col_of(line_text, range.start.saturating_sub(line_start));
    let b = col_of(line_text, range.end.saturating_sub(line_start).min(line_text.len()));
    let top = view.line_top(line);
    let mut row_start = 0usize;
    let rows = galley.rows.len();
    for (k, row) in galley.rows.iter().enumerate() {
        let n = row.char_count_excluding_newline();
        let row_end = row_start + n;
        let from = a.max(row_start);
        let to = b.min(row_end);
        let last_row = k + 1 == rows;
        // Выделение захватывает перевод строки — хвостик после текста.
        let tail = last_row && newline_tail;
        if from <= to && (from < to || tail) {
            let x0 = row.x_offset(from - row_start);
            let mut x1 = row.x_offset(to - row_start);
            if tail {
                x1 += view.cw * 0.6;
            }
            if x1 > x0 {
                out.push(Rect::from_min_max(pos2(view.text_left + x0, top + row.min_y()), pos2(view.text_left + x1, top + row.max_y())));
            }
        }
        row_start += row.char_count_including_newline();
    }
    out
}

/// Фон: строка курсора, выделения, совпадения поиска.
pub(crate) fn paint_under(painter: &egui::Painter, st: &EditState, text: &str, view: &View, matches: &[Range<usize>], current_match: usize, focused: bool) {
    let painter = painter.with_clip_rect(view.text_clip);
    if focused {
        let line = st.index.line_of(st.carets.primary().caret);
        if !view.is_hidden(line) {
            let top = view.line_top(line);
            let band = Rect::from_min_max(pos2(view.gutter.right(), top), pos2(view.clip.right(), top + view.line_height(line).max(view.line_h)));
            painter.rect_filled(band, 0.0, theme::BG_HEADER.gamma_multiply(0.9));
        }
    }
    let visible_start = st.index.line_start(view.visible.start.saturating_sub(2));
    let visible_end = st.index.line_end(text, (view.visible.end + 2).min(st.index.lines()).saturating_sub(1));
    let paint_range = |range: &Range<usize>, color: Color32| {
        if range.end < visible_start || range.start > visible_end {
            return;
        }
        let first = st.index.line_of(range.start);
        let last = st.index.line_of(range.end);
        for line in first.max(view.visible.start.saturating_sub(2))..=last.min(view.visible.end + 1) {
            let Some(g) = view.galley(line) else { continue };
            let s = st.index.line_start(line);
            let e = st.index.line_end(text, line);
            let r = range.start.max(s)..range.end.min(e + 1);
            for rect in range_rects(view, g, s, &text[s..e], line, r, range.end > e) {
                painter.rect_filled(rect, 0.0, color);
            }
        }
    };
    let first_match = matches.partition_point(|m| m.end < visible_start);
    for (k, m) in matches.iter().enumerate().skip(first_match) {
        if m.start > visible_end {
            break;
        }
        paint_range(m, if k == current_match { theme::FIND_CURRENT } else { theme::FIND_MATCH });
    }
    let fill = if focused { theme::TEXT_SELECTION } else { theme::TEXT_SELECTION.gamma_multiply(0.55) };
    for sel in &st.carets.sels {
        if !sel.is_empty() {
            paint_range(&sel.range(), fill);
        }
    }
}

/// Текст видимых строк.
pub(crate) fn paint_text(painter: &egui::Painter, view: &View, ppp: f32) {
    let painter = painter.with_clip_rect(view.text_clip);
    for (line, galley) in &view.laid {
        let pos = pos2(snap(view.text_left, ppp), snap(view.line_top(*line), ppp));
        painter.galley(pos, galley.clone(), theme::TEXT);
    }
}

/// Поверх: свёрнутые блоки («…»), курсоры, набор IME, колонка номеров со стрелками свёртки.
/// Возвращает строку, по «…» которой щёлкнули или над которой держат указатель.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_over(ui: &mut egui::Ui, st: &EditState, text: &str, view: &View, look: &Look, focused: bool, current_line: usize, id: egui::Id) -> Option<usize> {
    let ppp = look.ppp;
    let painter = ui.painter().with_clip_rect(view.text_clip);
    let mut clicked_fold = None;
    // Свёрнутые: «… }» после строки-заголовка; наведение — что внутри.
    for r in &view.hidden {
        let header = r.start - 1;
        if header < view.visible.start.saturating_sub(1) || header > view.visible.end {
            continue;
        }
        let Some(g) = view.galley(header) else { continue };
        let top = view.line_top(header);
        let region = view.regions.iter().find(|x| x.start_line == header);
        let tail = match region.map(|x| x.kind) {
            Some(FoldKind::Braces) => {
                let s = st.index.line_start(r.end - 1);
                let e = st.index.line_end(text, r.end - 1);
                let last = text[s..e].trim();
                if last.starts_with(['}', ']', ')']) { format!(" {last}") } else { String::new() }
            }
            Some(FoldKind::Block) => {
                let s = st.index.line_start(r.end - 1);
                let e = st.index.line_end(text, r.end - 1);
                format!(" {}", text[s..e].trim())
            }
            _ => String::new(),
        };
        let label = format!("…{tail}"); // i18n-skip: знак свёрнутого блока
        let chip_font = look.font.clone();
        let chip_galley = ui.fonts(|f| f.layout_no_wrap(label, chip_font, theme::TEXT_MUTED));
        let last_row = g.rows.last().map_or(Rect::NOTHING, |row| row.rect);
        let x = view.text_left + last_row.right() + look.cw;
        let chip = Rect::from_min_size(pos2(snap(x, ppp), snap(top + last_row.top() + 1.0, ppp)), vec2(chip_galley.size().x + theme::SP_3 * 2.0, look.line_h - 2.0));
        let response = ui.interact(chip.intersect(view.text_clip), id.with(("fold_chip", header)), Sense::click());
        let hovered = response.hovered();
        painter.rect(chip, theme::R_S, if hovered { theme::BG_HOVER } else { theme::BG_PANEL }, Stroke::new(1.0_f32, theme::LINE_HOVER));
        painter.galley(pos2(chip.left() + theme::SP_3, snap(chip.center().y - chip_galley.size().y * 0.5, ppp)), chip_galley, theme::TEXT_MUTED);
        if response.clicked() {
            clicked_fold = Some(header);
        }
        if hovered {
            // Подсказка: скрытые строки (первые 24).
            let s = st.index.line_start(r.start);
            let last = (r.end - 1).min(r.start + 23);
            let e = st.index.line_end(text, last);
            let mut preview = text[s..e].to_string();
            if r.end - 1 > last {
                preview.push_str("\n…"); // i18n-skip: многоточие
            }
            let font = look.font.clone();
            response.on_hover_ui(|ui| {
                ui.label(egui::RichText::new(preview).font(font).color(theme::TEXT));
            });
        }
    }
    // Курсоры.
    if focused {
        let w = snap((1.5 * ppp).round().max(1.0) / ppp, ppp).max(1.0 / ppp);
        for sel in &st.carets.sels {
            let line = st.index.line_of(sel.caret);
            if view.is_hidden(line) {
                continue;
            }
            let Some(g) = view.galley(line) else { continue };
            let s = st.index.line_start(line);
            let e = st.index.line_end(text, line);
            let col = col_of(&text[s..e], sel.caret - s);
            let r = g.pos_from_ccursor(CCursor::new(col));
            let top = view.line_top(line);
            let x = snap(view.text_left + r.min.x, ppp);
            let caret = Rect::from_min_max(pos2(x, snap(top + r.min.y, ppp)), pos2(x + w, snap(top + r.min.y + look.line_h, ppp)));
            painter.rect_filled(caret, 0.0, theme::CARET);
        }
        // Набор IME: подчёркнутый текст у главного курсора.
        if let Some(pre) = &st.preedit {
            let caret = st.carets.primary().caret;
            let line = st.index.line_of(caret);
            if let Some(g) = view.galley(line) {
                let s = st.index.line_start(line);
                let e = st.index.line_end(text, line);
                let r = g.pos_from_ccursor(CCursor::new(col_of(&text[s..e], caret - s)));
                let pos = pos2(view.text_left + r.min.x, view.line_top(line) + r.min.y);
                let pg = ui.fonts(|f| f.layout_no_wrap(pre.clone(), look.font.clone(), theme::TEXT_STRONG));
                let rect = Rect::from_min_size(pos, pg.size());
                painter.rect_filled(rect, 0.0, theme::BG_INSET);
                painter.galley(pos, pg, theme::TEXT_STRONG);
                painter.hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(1.0_f32, theme::TEXT_STRONG));
            }
        }
    }
    // Колонка номеров строк (не прокручивается вбок).
    let gutter_painter = ui.painter().with_clip_rect(view.gutter.intersect(view.clip));
    gutter_painter.rect_filled(view.gutter, 0.0, theme::BG_HEADER);
    let hover_gutter = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| view.gutter.contains(p));
    let number_right = view.gutter.right() - FOLD_W;
    for (line, _) in &view.laid {
        let top = snap(view.line_top(*line), ppp);
        let current = *line + 1 == current_line;
        gutter_painter.text(pos2(number_right, top), Align2::RIGHT_TOP, (line + 1).to_string(), look.font.clone(), if current { theme::TEXT } else { theme::TEXT_DISABLED });
        if let Some(region) = view.regions.iter().find(|r| r.start_line == *line) {
            let collapsed = view.hidden.iter().any(|h| h.start == region.start_line + 1);
            if collapsed || hover_gutter {
                let center = pos2(view.gutter.right() - FOLD_W * 0.5, top + look.line_h * 0.5);
                let icon = if collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
                icons::paint(&gutter_painter, Rect::from_center_size(center, vec2(9.0, 9.0)), icon, if collapsed { theme::TEXT } else { theme::TEXT_MUTED });
            }
        }
    }

    clicked_fold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carets(list: &[(usize, usize)]) -> Carets {
        Carets { sels: list.iter().map(|(a, c)| Sel::new(*a, *c)).collect(), primary: list.len() - 1 }
    }

    #[test]
    fn typing_at_three_carets_gives_exact_text_and_carets() {
        let text = "a\nb\nc\n".to_string();
        let cs = carets(&[(1, 1), (3, 3), (5, 5)]);
        let (edits, after) = insert_each(&text, &cs, |_| "xy".into());
        let mut t = text.clone();
        code_buffer::apply(&mut t, &edits);
        assert_eq!(t, "axy\nbxy\ncxy\n");
        assert_eq!(after.sels.iter().map(|s| s.caret).collect::<Vec<_>>(), vec![3, 7, 11]);
    }

    #[test]
    fn line_commands_and_auto_close_at_every_caret() {
        // Ctrl+/ двумя курсорами на разных строках.
        let text = "one();\ntwo();\nthree();\n";
        let cs = carets(&[(0, 0), (14, 14)]);
        let (edits, after) = edit_each(text, &cs, |t, s| Some(code_edit::comment_lines(t, s.anchor, s.caret, "//", code_edit::CommentMode::Toggle))).unwrap();
        let mut t = text.to_string();
        code_buffer::apply(&mut t, &edits);
        assert_eq!(t, "// one();\ntwo();\n// three();\n");
        assert_eq!(after.sels.len(), 2);
        // Два курсора на одной строке и строковая команда — по очереди, без порчи текста.
        let cs = carets(&[(1, 1), (3, 3)]);
        let (edits, _) = edit_each(text, &cs, |t, s| Some(code_edit::duplicate(t, s.anchor, s.caret))).unwrap();
        let mut t = text.to_string();
        code_buffer::apply(&mut t, &edits);
        assert_eq!(t, "one();\none();\none();\ntwo();\nthree();\n");
        // Скобка с парой у каждого курсора.
        let text = "f\ng\n";
        let cs = carets(&[(1, 1), (3, 3)]);
        let (edits, after) = edit_each(text, &cs, |t, s| code_edit::type_char(t, s.anchor, s.caret, '(', Language::Rust)).unwrap();
        let mut t = text.to_string();
        code_buffer::apply(&mut t, &edits);
        assert_eq!(t, "f()\ng()\n");
        assert_eq!(after.sels.iter().map(|s| s.caret).collect::<Vec<_>>(), vec![2, 6]);
    }

    #[test]
    fn deleting_with_carets_merges_them_when_they_meet() {
        let text = "abcd";
        let cs = carets(&[(1, 1), (2, 2)]);
        let (edits, after) = delete_each(text, &cs, |t, at| prev_char(t, at)..at);
        let mut t = text.to_string();
        code_buffer::apply(&mut t, &edits);
        assert_eq!(t, "cd");
        assert_eq!(after.sels, vec![Sel::one(0)], "the carets met and merged");
    }

    #[test]
    fn occurrences_select_next_and_all() {
        let text = "let speed = speed + speedy; speed";
        let mut st = EditState::default();
        st.observe(text, Language::Rust, 0.0);
        st.carets = Carets::one(5);
        assert!(add_next_occurrence(&mut st, text), "the first press selects the word");
        assert_eq!(st.carets.sels, vec![Sel::new(4, 9)]);
        assert!(add_next_occurrence(&mut st, text));
        assert!(add_next_occurrence(&mut st, text));
        let ranges: Vec<_> = st.carets.sels.iter().map(Sel::range).collect();
        assert_eq!(ranges, vec![4..9, 12..17, 28..33], "whole words only: `speedy` is skipped");
        st.carets = Carets::one(13);
        assert!(select_all_occurrences(&mut st, text));
        assert_eq!(st.carets.sels.len(), 3);
        assert_eq!(st.carets.primary().range(), 12..17);
    }
}
