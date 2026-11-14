//! Заставка при открытии окна редактора: знак движка, имя проекта, текущий шаг и тонкая
//! «бегущая» полоса. Окно показывается уже с ней (см. `runner.rs`: окно невидимо до
//! первого кадра в цветах темы), она держится, пока сцена впервые рисуется и готовятся
//! миниатюры, и плавно растворяется в редактор. Ввод не перехватывает.
//!
//! Её включает только оконный запуск ([`Editor::enable_splash`]); снимки и тесты рисуют
//! редактор без неё.

use crate::i18n::{tr_s, trf};
use egui::{pos2, vec2, Align2, Color32, Rect};

use crate::state::Editor;
use crate::theme;

/// Не короче (иначе мелькает) и не дольше (если что-то так и не догрузилось).
const MIN_SECONDS: f64 = 0.7;
const MAX_SECONDS: f64 = 6.0;
/// Растворение в редактор.
const FADE_SECONDS: f64 = 0.35;

#[derive(Default)]
pub(crate) struct Splash {
    enabled: bool,
    started: Option<f64>,
    /// Когда загрузка кончилась и началось растворение.
    fading_since: Option<f64>,
    done: bool,
}

impl Editor {
    /// Нарисован ли уже хоть один кадр с темой и шрифтами (окно можно показывать).
    pub fn themed(&self) -> bool {
        self.themed_frames > 0
    }

    /// Показывать заставку при первых кадрах (оконный запуск).
    pub fn enable_splash(&mut self) {
        self.splash.enabled = true;
    }

    /// Заставка ещё на экране.
    pub fn splash_visible(&self) -> bool {
        self.splash.enabled && !self.splash.done
    }

    /// Текущий шаг загрузки — что заставка пишет под именем проекта.
    pub(crate) fn splash_step(&self) -> Option<String> {
        if self.viewport.texture.is_none() && self.dock.game.texture.is_none() {
            return Some(tr_s("Compiling shaders and drawing the scene…").to_string());
        }
        if self.previews.busy() {
            return Some(tr_s("Rendering asset previews…").to_string());
        }
        None
    }

    /// Нарисовать заставку поверх всего (слой Foreground, без ввода).
    pub(crate) fn splash_ui(&mut self, ctx: &egui::Context) {
        if !self.splash.enabled || self.splash.done {
            return;
        }
        let now = ctx.input(|i| i.time);
        let started = *self.splash.started.get_or_insert(now);
        let step = self.splash_step();
        let elapsed = now - started;
        if self.splash.fading_since.is_none() && ((step.is_none() && elapsed >= MIN_SECONDS) || elapsed >= MAX_SECONDS) {
            self.splash.fading_since = Some(now);
        }
        let alpha = match self.splash.fading_since {
            Some(since) => 1.0 - ((now - since) / FADE_SECONDS).clamp(0.0, 1.0) as f32,
            None => 1.0,
        };
        if alpha <= 0.0 {
            self.splash.done = true;
            return;
        }
        ctx.request_repaint();
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("editor_splash")));
        let fade = |c: Color32| c.gamma_multiply(alpha);
        painter.rect_filled(screen, 0.0, fade(theme::BG_APP));

        let center = screen.center() - vec2(0.0, 24.0);
        let mark = Rect::from_center_size(center - vec2(0.0, 28.0), vec2(48.0, 48.0));
        crate::ui::paint_mark_faded(&painter, mark, alpha);
        let project = self.app.world.scene.name.clone();
        let title = self.config.title.clone();
        painter.text(
            pos2(center.x, mark.bottom() + 18.0),
            Align2::CENTER_CENTER,
            if title.is_empty() { tr_s("Piska Engine").to_string() } else { title },
            theme::strong(theme::FONT_HEADING),
            fade(theme::TEXT_STRONG),
        );
        painter.text(
            pos2(center.x, mark.bottom() + 38.0),
            Align2::CENTER_CENTER,
            step.unwrap_or_else(|| trf!("Opening “{project}”", project = project)),
            theme::caption(),
            fade(theme::TEXT_MUTED),
        );
        // Тонкая полоса с бегущим отрезком: движется, пока идёт работа.
        let bar = Rect::from_center_size(pos2(center.x, mark.bottom() + 58.0), vec2(180.0, 2.0));
        painter.rect_filled(bar, 1.0, fade(theme::BG_FIELD));
        let t = (elapsed * 0.9).fract() as f32;
        let eased = t * t * (3.0 - 2.0 * t);
        let width = bar.width() * 0.3;
        let x = bar.left() - width + (bar.width() + width) * eased;
        let segment = Rect::from_min_max(pos2(x.max(bar.left()), bar.top()), pos2((x + width).min(bar.right()), bar.bottom()));
        if segment.width() > 0.0 {
            painter.rect_filled(segment, 1.0, fade(theme::accent()));
        }
        painter.text(
            pos2(center.x, screen.bottom() - 20.0),
            Align2::CENTER_CENTER,
            trf!("Piska Engine {}", env!("CARGO_PKG_VERSION")),
            theme::caption(),
            fade(theme::TEXT_DISABLED),
        );
    }
}
