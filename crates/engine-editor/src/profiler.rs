//! Окно Profiler: график времени кадра по частям с осью в миллисекундах,
//! средние / худшие значения и счётчики рендера.

use crate::i18n::{n_, tr_s, trf};
use std::collections::VecDeque;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_render::RenderStats;

use crate::icons::Icon;
use crate::state::Editor;
use crate::theme;

const HISTORY: usize = 300;

/// Куда ушло время одного кадра редактора, в миллисекундах.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTimings {
    /// Весь кадр, от начала до начала следующего (вместе с ожиданием).
    pub frame_ms: f32,
    /// Скрипты и физика игры (`App::tick`); в режиме Edit — подсистемы в простое.
    pub update_ms: f32,
    /// Отрисовка сцены в текстуру вьюпорта (подготовка команд на CPU).
    pub render_ms: f32,
    /// Построение интерфейса egui.
    pub ui_ms: f32,
    /// Разбиение интерфейса на треугольники.
    pub tessellate_ms: f32,
    /// Загрузка буферов интерфейса и отправка кадра видеокарте.
    pub submit_ms: f32,
    /// Ожидание свободного кадра окна: видеокарта занята или ждёт синхронизации.
    pub wait_ms: f32,
}

impl FrameTimings {
    /// Время работы кадра — всё, кроме простоя между кадрами.
    pub fn work_ms(&self) -> f32 {
        self.update_ms + self.render_ms + self.ui_ms + self.tessellate_ms + self.submit_ms + self.wait_ms
    }
}

#[derive(Default)]
pub(crate) struct ProfilerState {
    samples: VecDeque<FrameTimings>,
    pub stats: RenderStats,
    pub objects: usize,
    /// Части кадра, скрытые щелчком по легенде.
    hidden: [bool; PARTS.len()],
    /// Кадр, выбранный щелчком по графику: таблица показывает его, а не среднее.
    selected: Option<FrameTimings>,
    /// Длина истории в кадрах (Preferences → Console & Diagnostics); 0 — обычная.
    history: usize,
}

impl ProfilerState {
    pub fn push(&mut self, timings: FrameTimings, stats: RenderStats, objects: usize) {
        while self.samples.len() >= self.capacity() {
            self.samples.pop_front();
        }
        self.samples.push_back(timings);
        self.stats = stats;
        self.objects = objects;
    }

    /// Сколько последних кадров помнит история.
    pub fn capacity(&self) -> usize {
        if self.history == 0 { HISTORY } else { self.history }
    }

    /// Задать длину истории; лишние старые кадры забываются сразу.
    pub fn set_history(&mut self, frames: usize) {
        self.history = frames.max(2);
        while self.samples.len() > self.capacity() {
            self.samples.pop_front();
        }
    }

    /// Сколько кадров в истории сейчас.
    pub fn recorded(&self) -> usize {
        self.samples.len()
    }

    /// Среднее по последним `count` кадрам.
    fn average(&self, count: usize) -> FrameTimings {
        let recent: Vec<&FrameTimings> = self.samples.iter().rev().take(count).collect();
        let n = recent.len().max(1) as f32;
        let sum = |f: fn(&FrameTimings) -> f32| recent.iter().map(|t| f(t)).sum::<f32>() / n;
        FrameTimings {
            frame_ms: sum(|t| t.frame_ms),
            update_ms: sum(|t| t.update_ms),
            render_ms: sum(|t| t.render_ms),
            ui_ms: sum(|t| t.ui_ms),
            tessellate_ms: sum(|t| t.tessellate_ms),
            submit_ms: sum(|t| t.submit_ms),
            wait_ms: sum(|t| t.wait_ms),
        }
    }

    /// Среднее по всей истории (последние несколько секунд).
    pub fn summary(&self) -> FrameTimings {
        self.average(self.capacity())
    }

    /// Кадров в секунду, сглаженно.
    pub fn fps(&self) -> f32 {
        let frame = self.average(30).frame_ms;
        if frame > 1e-3 { 1000.0 / frame } else { 0.0 }
    }

    /// Среднее время кадра за последние полсекунды.
    pub fn recent_frame_ms(&self) -> f32 {
        self.average(30).frame_ms
    }

    /// Среднее время работы кадра (без простоя).
    pub fn recent_work_ms(&self) -> f32 {
        self.average(30).work_ms()
    }

    /// Худший кадр и «1 % худших» по истории.
    fn worst(&self) -> (f32, f32) {
        let mut frames: Vec<f32> = self.samples.iter().map(|t| t.frame_ms).collect();
        frames.sort_by(|a, b| b.total_cmp(a));
        let worst = frames.first().copied().unwrap_or(0.0);
        let one_percent = frames.get(frames.len() / 100).copied().unwrap_or(worst);
        (worst, one_percent)
    }
}

/// Часть кадра на графике: подпись, цвет, откуда взять значение.
type Part = (&'static str, Color32, fn(&FrameTimings) -> f32);

// Приглушённые цвета серий: различимы, но не «конфетные».
const PARTS: [Part; 6] = [
    (n_("Game (scripts, physics)"), Color32::from_rgb(0xc9, 0x86, 0x52), |t| t.update_ms),
    (n_("Scene render"), Color32::from_rgb(0x5b, 0x86, 0xc4), |t| t.render_ms),
    (n_("Editor UI"), Color32::from_rgb(0x6a, 0xa8, 0x74), |t| t.ui_ms),
    (n_("Tessellation"), Color32::from_rgb(0x8f, 0x7b, 0xc0), |t| t.tessellate_ms),
    (n_("GPU submit"), Color32::from_rgb(0x4c, 0x9c, 0xa3), |t| t.submit_ms),
    (n_("Waiting for GPU / vsync"), Color32::from_rgb(0x6c, 0x6c, 0x6c), |t| t.wait_ms),
];

/// «Красивые» шаги оси: 0.5, 1, 2, 5, 10… мс.
fn axis_step(top: f32) -> f32 {
    let raw = top / 4.0;
    let magnitude = 10.0_f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|step| *step >= raw)
        .unwrap_or(raw)
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let continuous = editor.wants_continuous_redraw();
    let profiler = &mut editor.profiler;
    let average = profiler.average(60);
    let (worst, one_percent) = profiler.worst();

    // Полоса сверху: итог и сброс выбранного кадра.
    let selected = profiler.selected;
    let mut clear_selection = false;
    theme::toolbar_strip(ui, |ui| {
        let headline = if continuous {
            trf!("{} fps   {} ms", format!("{:.0}", profiler.fps()), format!("{:.2}", average.frame_ms))
        } else {
            trf!("{} ms of work per frame   (redrawing on demand)", format!("{:.2}", average.work_ms()))
        };
        ui.label(egui::RichText::new(headline).font(theme::mono()).color(theme::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if selected.is_some() && theme::icon_text_button(ui, Icon::Close, tr_s("Clear Selection")).clicked() {
                clear_selection = true;
            }
            ui.label(
                egui::RichText::new(trf!("worst {worst} ms  ·  1% low {one_percent} ms", worst = format!("{:.1}", worst), one_percent = format!("{:.1}", one_percent)))
                    .font(theme::mono())
                    .color(theme::TEXT_MUTED),
            );
        });
    });
    if clear_selection {
        profiler.selected = None;
    }

    // Занимаем всё место окна (иначе панель с каждым кадром «усыхала» бы на отступы).
    let available = ui.available_rect_before_wrap();
    ui.allocate_rect(available, Sense::hover());
    let full = available.shrink2(vec2(theme::SP_M, theme::SP_S + 2.0));
    let left = Rect::from_min_size(full.min, vec2(320.0_f32.min(full.width() * 0.45), full.height()));
    let graph = Rect::from_min_max(pos2(left.right() + theme::SP_L, full.top()), full.max);

    // --- Таблица слева: части кадра (щелчок — скрыть на графике), счётчики ---
    let shown = profiler.selected.unwrap_or(average);
    let total = if continuous { shown.frame_ms } else { shown.work_ms() }.max(1e-3);
    let mut summary = ui.new_child(egui::UiBuilder::new().max_rect(left));
    egui::ScrollArea::vertical()
        .id_salt("profiler_summary")
        .auto_shrink([false, false])
        .show(&mut summary, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = if profiler.selected.is_some() { tr_s("Selected frame") } else { tr_s("Average of 60 frames") };
            table_row(ui, None, title, "ms", "%", true);
            for (index, (label, color, value)) in PARTS.iter().enumerate() {
                let label = tr_s(label);
                let ms = value(&shown);
                let hidden = profiler.hidden[index];
                let row = table_row(
                    ui,
                    Some((*color, hidden)),
                    label,
                    &format!("{ms:.2}"),
                    &format!("{:.0}", ms / total * 100.0),
                    false,
                );
                if row.on_hover_text(tr_s("Click to hide or show this part on the graph")).clicked() {
                    profiler.hidden[index] = !hidden;
                }
            }
            table_row(ui, None, tr_s("Frame"), &format!("{:.2}", shown.frame_ms), "", false);
            ui.add_space(theme::SP_M);
            table_row(ui, None, tr_s("Rendering"), "", "", true);
            table_row(ui, None, tr_s("Objects"), &profiler.objects.to_string(), "", false);
            table_row(ui, None, tr_s("Draw calls"), &profiler.stats.draw_calls.to_string(), "", false);
            table_row(ui, None, tr_s("Sprites"), &profiler.stats.sprites.to_string(), "", false);
            table_row(ui, None, tr_s("Textures"), &profiler.stats.textures.to_string(), "", false);
        });

    // --- График ---
    if graph.width() < 80.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(graph.expand(2.0));
    let axis_width = 44.0;
    let plot = Rect::from_min_max(pos2(graph.left() + axis_width, graph.top() + 2.0), pos2(graph.right(), graph.bottom() - 16.0));
    crate::controls::paint_frame(&painter, plot, theme::R_S, theme::BG_INSET, theme::LINE_STRONG);

    // Шкала: по 95-му перцентилю, чтобы один долгий кадр не сплющил всё остальное.
    let mut frames: Vec<f32> = profiler
        .samples
        .iter()
        .map(|t| if continuous { t.frame_ms } else { t.work_ms() })
        .collect();
    frames.sort_by(f32::total_cmp);
    let p95 = frames.get(frames.len() * 95 / 100).copied().unwrap_or(16.7);
    let top = (p95 * 1.3).clamp(2.0, 120.0);
    let step = axis_step(top);
    let y_of = |ms: f32| plot.bottom() - (ms / top).min(1.0) * plot.height();

    let mut tick = 0.0;
    while tick <= top + 1e-3 {
        let y = y_of(tick);
        painter.hline(plot.x_range().shrink(1.0), y, Stroke::new(1.0_f32, theme::BG_PANEL));
        painter.text(
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{tick} ms"),
            theme::caption(),
            theme::TEXT_DISABLED,
        );
        tick += step;
    }
    for (ms, label) in [(1000.0 / 60.0, "60 fps"), (1000.0 / 30.0, "30 fps"), (1000.0 / 144.0, "144 fps")] {
        if ms < top {
            let y = y_of(ms);
            painter.add(egui::Shape::dashed_line(
                &[pos2(plot.left(), y), pos2(plot.right(), y)],
                Stroke::new(1.0_f32, theme::LINE_HOVER),
                4.0,
                4.0,
            ));
            painter.text(
                pos2(plot.right() - 4.0, y - 2.0),
                Align2::RIGHT_BOTTOM,
                label,
                theme::caption(),
                theme::TEXT_MUTED,
            );
        }
    }

    let bar = plot.width() / profiler.capacity() as f32;
    let first = profiler.capacity().saturating_sub(profiler.samples.len());
    let response = ui.interact(plot, egui::Id::new("profiler_plot"), Sense::click());
    let pointer = response.hover_pos();
    let mut hovered = None;
    for (index, sample) in profiler.samples.iter().enumerate() {
        let x = plot.left() + (first + index) as f32 * bar;
        let column = |from: f32, to: f32, color: Color32| {
            painter.rect_filled(
                Rect::from_min_max(pos2(x, y_of(to)), pos2(x + (bar - 0.5).max(1.0), y_of(from))),
                0.0,
                color,
            );
        };
        if continuous {
            column(0.0, sample.frame_ms, theme::BG_FIELD);
        }
        let mut base = 0.0;
        for (part, (_, color, value)) in PARTS.iter().enumerate() {
            if profiler.hidden[part] {
                continue;
            }
            let ms = value(sample);
            column(base, base + ms, *color);
            base += ms;
        }
        if profiler.selected == Some(*sample) {
            painter.rect_filled(
                Rect::from_min_max(pos2(x, plot.top()), pos2(x + bar.max(1.0), plot.bottom())),
                0.0,
                theme::accent().gamma_multiply(0.35),
            );
            painter.vline(x + bar * 0.5, plot.y_range(), Stroke::new(1.0_f32, theme::accent()));
        }
        if pointer.is_some_and(|p| p.x >= x && p.x < x + bar) {
            hovered = Some(*sample);
            painter.vline(x + bar * 0.5, plot.y_range(), Stroke::new(1.0_f32, theme::TEXT_MUTED));
        }
    }
    painter.text(
        pos2(plot.left(), graph.bottom()),
        Align2::LEFT_BOTTOM,
        trf!("last {} frames · click a frame to inspect it", profiler.samples.len()),
        theme::caption(),
        theme::TEXT_DISABLED,
    );
    if let Some(sample) = hovered {
        if response.clicked() {
            profiler.selected = Some(sample);
        }
        let mut text = format!("{}\n", trf!("Frame {} ms", format!("{:.2}", sample.frame_ms)));
        for (label, _, value) in PARTS {
            let label = tr_s(label);
            text.push_str(&format!("{label}: {:.2} ms\n", value(&sample)));
        }
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("profiler_tip"), |ui| {
            ui.label(egui::RichText::new(text.trim_end()).font(theme::mono()));
        });
    }
}

/// Строка таблицы профайлера: образец цвета (щелчок прячет серию), подпись, мс, доля.
fn table_row(ui: &mut egui::Ui, swatch: Option<(Color32, bool)>, label: &str, ms: &str, share: &str, header: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
    if header {
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE_SOFT));
    } else if swatch.is_some() && response.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::hover_fill());
    }
    let hidden = swatch.is_some_and(|(_, hidden)| hidden);
    let mut x = rect.left() + theme::SP_XS;
    if let Some((color, hidden)) = swatch {
        let square = Rect::from_center_size(pos2(x + 5.0, rect.center().y), vec2(9.0, 9.0));
        if hidden {
            ui.painter().rect_stroke(square, 1.0, Stroke::new(1.0_f32, color));
        } else {
            ui.painter().rect_filled(square, 1.0, color);
        }
        x += 16.0;
    }
    let text_color = if header {
        theme::TEXT_MUTED
    } else if hidden {
        theme::TEXT_DISABLED
    } else {
        theme::TEXT
    };
    let font = if header { theme::strong(theme::FONT_CAPTION) } else { theme::body() };
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font.clone(), text_color);
    let number_font = if header { font } else { theme::mono() };
    ui.painter().text(
        pos2(rect.right() - 40.0, rect.center().y),
        Align2::RIGHT_CENTER,
        ms,
        number_font.clone(),
        text_color,
    );
    ui.painter().text(
        pos2(rect.right() - theme::SP_XS, rect.center().y),
        Align2::RIGHT_CENTER,
        share,
        number_font,
        if header { theme::TEXT_MUTED } else { theme::TEXT_DISABLED },
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_steps_are_round_numbers() {
        assert_eq!(axis_step(20.0), 5.0);
        assert_eq!(axis_step(4.0), 1.0);
        assert_eq!(axis_step(60.0), 20.0);
    }

    #[test]
    fn fps_and_worst_frames_come_from_history() {
        let mut profiler = ProfilerState::default();
        for i in 0..100 {
            let frame_ms = if i == 50 { 40.0 } else { 2.0 };
            profiler.push(FrameTimings { frame_ms, ..FrameTimings::default() }, RenderStats::default(), 0);
        }
        assert!((profiler.fps() - 500.0).abs() < 1.0);
        assert_eq!(profiler.worst().0, 40.0);
    }

    #[test]
    fn history_length_follows_the_preference() {
        let mut profiler = ProfilerState::default();
        let frame = || FrameTimings { frame_ms: 1.0, ..FrameTimings::default() };
        for _ in 0..400 {
            profiler.push(frame(), RenderStats::default(), 0);
        }
        assert_eq!(profiler.recorded(), HISTORY);
        profiler.set_history(50);
        assert_eq!(profiler.recorded(), 50, "shrinking drops the oldest frames at once");
        for _ in 0..100 {
            profiler.push(frame(), RenderStats::default(), 0);
        }
        assert_eq!(profiler.recorded(), 50);
        profiler.set_history(1000);
        for _ in 0..900 {
            profiler.push(frame(), RenderStats::default(), 0);
        }
        assert_eq!(profiler.recorded(), 950);
    }
}
