//! Интерфейс хаба: левая навигация (Projects, New Project, Samples, Settings),
//! страницы, окно хода сборки и диалоги. Всё, что меняет состояние, — [`HubAction`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{pos2, vec2, Align, Align2, Color32, Id, Layout, Margin, Rect, Sense, Stroke, Ui, UiBuilder};
use engine_editor::kit::icons::{self, Icon};
use engine_editor::kit::theme::{self, item};

use crate::config::{AfterOpen, SortOrder, ViewMode};
use crate::hub::{BrowseTarget, Dialog, FolderBrowser, Hub, HubAction, Page};
use crate::jobs::{JobKind, JobState};
use crate::project::{self, Listed, Source};
use crate::samples::SAMPLES;
use crate::templates::Template;
use crate::widgets::{self, Tone};
use crate::paths;

/// Ширина левой навигации.
const NAV_WIDTH: f32 = 232.0;
/// Поля страниц слева и справа.
const PAD: f32 = 28.0;
/// Высота строки списка проектов.
const ROW: f32 = 64.0;
/// Миниатюра в строке (16:9).
const THUMB: egui::Vec2 = vec2(80.0, 45.0);
/// Ширина содержимого диалогов.
const DIALOG_WIDTH: f32 = 560.0;
/// Масштабы интерфейса в настройках (общие с редактором).
const SCALES: [(u32, &str); 6] = [(90, "90%"), (100, "100%"), (110, "110%"), (125, "125%"), (150, "150%"), (175, "175%")];

impl Hub {
    /// Нарисовать хаб в контексте egui (окно или снимок без окна).
    pub fn ui(&mut self, ctx: &egui::Context) {
        if self.apply_style(ctx) {
            // Шрифты (и семейство «strong») подключаются со следующего кадра.
            ctx.request_repaint();
            return;
        }
        // Картинки, декодированные в фоне.
        while let Ok((key, image)) = self.images.1.try_recv() {
            self.images_loading.remove(&key);
            let texture = image.map(|image| ctx.load_texture(&key, image, egui::TextureOptions::LINEAR));
            self.textures.insert(key, texture);
        }
        // Окно снова в фокусе — файлы могли поменять в другой программе: обход в фоне.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            self.refresh();
        }
        egui::SidePanel::left("hub-nav")
            .exact_width(NAV_WIDTH)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(Margin {
                left: 12.0,
                right: 12.0,
                top: 14.0,
                bottom: 12.0,
            }))
            .show(ctx, |ui| self.nav(ui));
        egui::TopBottomPanel::bottom("hub-status")
            .exact_height(28.0)
            .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(Margin::symmetric(PAD - 8.0, 0.0)))
            .show(ctx, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG_PANEL))
            .show(ctx, |ui| {
                if self.job.is_some() {
                    self.job_view(ui);
                } else if self.prebuild.as_ref().is_some_and(|p| p.finished) {
                    self.prebuild_summary(ui);
                } else {
                    match self.page {
                        Page::Projects => self.projects_page(ui),
                        Page::NewProject => self.wizard_page(ui),
                        Page::Samples => self.samples_page(ui),
                        Page::Settings => self.settings_page(ui),
                    }
                }
            });
        self.dialogs(ctx);
        for action in std::mem::take(&mut self.actions) {
            self.apply(action);
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Тема и масштаб; `true` — шрифты только что поставлены (кадр рисовать рано).
    fn apply_style(&mut self, ctx: &egui::Context) -> bool {
        let zoom = self.prefs.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
        }
        // Тема ставится в каждый новый контекст (окно, снимок) и заново при смене акцента.
        let key = Id::new("hub-style-accent");
        let applied: Option<usize> = ctx.data(|d| d.get_temp(key));
        if applied.is_none() {
            // Новый контекст (окно или снимок): текстуры прежнего ему не годятся.
            self.textures.clear();
        }
        if applied != Some(self.prefs.accent) {
            theme::apply(ctx, applied.is_none());
            ctx.data_mut(|d| d.insert_temp(key, self.prefs.accent));
        }
        applied.is_none()
    }

    fn push(&mut self, action: HubAction) {
        self.actions.push(action);
    }

    // -----------------------------------------------------------------------
    // Картинки
    // -----------------------------------------------------------------------

    /// Текстура по ключу. Чтение и декодирование — в фоновом потоке: пока картинки нет,
    /// `None` (рисуется плитка), кадр не ждёт диск. В `dry_run` — сразу (снимки).
    fn texture(
        &mut self,
        ctx: &egui::Context,
        key: String,
        load: impl FnOnce() -> Option<Vec<u8>> + Send + 'static,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.textures.get(&key) {
            return cached.clone();
        }
        let decode = |bytes: Vec<u8>| {
            let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
        };
        if self.dry_run {
            let texture = load()
                .and_then(decode)
                .map(|image| ctx.load_texture(&key, image, egui::TextureOptions::LINEAR));
            self.textures.insert(key, texture.clone());
            return texture;
        }
        if self.images_loading.insert(key.clone()) {
            let sender = self.images.0.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let image = load().and_then(decode);
                let _ = sender.send((key, image));
                ctx.request_repaint();
            });
        }
        None
    }

    /// Миниатюра проекта: значок из ассетов, кадр примера движка — или ничего
    /// (тогда рисуется плитка из имени).
    fn thumbnail(&mut self, ctx: &egui::Context, listed: &Listed) -> Option<egui::TextureHandle> {
        if let Ok(info) = &listed.info {
            if let Some(icon) = info.icon.clone() {
                return self.texture(ctx, format!("icon:{}", icon.display()), move || std::fs::read(&icon).ok());
            }
        }
        let sample = SAMPLES.iter().find(|s| Some(s.folder) == listed.sample)?;
        self.texture(ctx, format!("sample:{}", sample.folder), move || Some(sample.picture.to_vec()))
    }

    fn logo(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let file = self.engine_root.as_ref()?.join("assets").join("branding").join("icon-64.png");
        self.texture(ctx, "logo".to_string(), move || std::fs::read(file).ok())
    }

    fn paint_thumb(&mut self, ui: &Ui, rect: Rect, listed: &Listed) {
        let greyed = listed.info.is_err();
        match self.thumbnail(ui.ctx(), listed) {
            Some(texture) => {
                let tint = if greyed { Color32::from_gray(90) } else { Color32::WHITE };
                widgets::paint_cover(ui, rect, &texture, theme::R_S, tint);
            }
            None => widgets::paint_tile(ui.painter(), rect, &listed.title(), greyed),
        }
    }

    // -----------------------------------------------------------------------
    // Навигация и строка состояния
    // -----------------------------------------------------------------------

    fn nav(&mut self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let mark = Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 15.0), vec2(30.0, 30.0));
        match self.logo(ui.ctx()) {
            Some(logo) => {
                egui::Image::new(egui::load::SizedTexture::new(logo.id(), mark.size()))
                    .rounding(theme::R_M)
                    .paint_at(ui, mark);
            }
            None => {
                ui.painter().rect_filled(mark, theme::R_M, theme::accent());
                ui.painter()
                    .text(mark.center(), Align2::CENTER_CENTER, "P", theme::strong(17.0), Color32::WHITE);
            }
        }
        let painter = ui.painter();
        painter.text(pos2(mark.right() + 11.0, rect.center().y - 8.0), Align2::LEFT_CENTER, "Piska Hub", theme::strong(15.0), theme::TEXT);
        painter.text(
            pos2(mark.right() + 11.0, rect.center().y + 10.0),
            Align2::LEFT_CENTER,
            format!("Piska Engine {}", env!("CARGO_PKG_VERSION")),
            theme::caption(),
            theme::TEXT_MUTED,
        );
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let items = [
            (Page::Projects, Icon::Folder, "Projects", Some(self.listed.len())),
            (Page::NewProject, Icon::Plus, "New Project", None),
            (Page::Samples, Icon::Tiles, "Samples & Docs", None),
            (Page::Settings, Icon::Gear, "Settings", None),
        ];
        let current = if self.job.is_some() { None } else { Some(self.page) };
        for (page, icon, label, count) in items {
            if widgets::nav_item(ui, Hub::nav_id(page), icon, label, count, current == Some(page)).clicked() {
                if self.job.as_ref().is_some_and(|job| !job.is_building()) {
                    self.push(HubAction::CloseJob);
                }
                self.push(HubAction::Navigate(page));
            }
        }

        // Низ: что запущено и где движок.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            if let Some(root) = self.engine_root.clone() {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
                widgets::paint_elided(
                    ui.painter(),
                    pos2(rect.left() + 6.0, rect.center().y),
                    &root.display().to_string(),
                    theme::caption(),
                    theme::TEXT_DISABLED,
                    rect.width() - 8.0,
                    Align2::LEFT_CENTER,
                );
                response.on_hover_text(format!("Engine checkout: {}", root.display()));
                nav_caption(ui, "ENGINE");
            }
            let running: Vec<(String, &'static str)> = self
                .listed
                .iter()
                .filter_map(|l| {
                    if self.processes.running_key(&l.key, &JobKind::Open).is_some() {
                        Some((l.title(), "Editor"))
                    } else if self.processes.running_key(&l.key, &JobKind::Play).is_some() {
                        Some((l.title(), "Game"))
                    } else {
                        None
                    }
                })
                .collect();
            if !running.is_empty() {
                ui.add_space(10.0);
                for (title, what) in running.iter().rev() {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                    ui.painter().circle_filled(pos2(rect.left() + 10.0, rect.center().y), 3.0, theme::OK);
                    ui.painter()
                        .text(pos2(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, title, theme::caption(), theme::TEXT);
                    ui.painter()
                        .text(pos2(rect.right() - 6.0, rect.center().y), Align2::RIGHT_CENTER, *what, theme::caption(), theme::TEXT_MUTED);
                }
                nav_caption(ui, "RUNNING");
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let y = rect.center().y;
        let recent = self.status.as_ref().filter(|s| s.error || s.at.elapsed() < Duration::from_secs(12));
        let (icon, color, text) = match recent {
            Some(status) if status.error => (Icon::Warn, theme::WARN, status.text.clone()),
            Some(status) => (Icon::Check, theme::OK, status.text.clone()),
            None => (Icon::Info, theme::TEXT_DISABLED, "Ready".to_string()),
        };
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 8.0, y), vec2(12.0, 12.0)), icon, color);
        let right = format!(
            "{} projects  ·  config {}",
            self.listed.len(),
            self.config_dir.as_ref().map(|d| d.join(paths::CONFIG_FILE).display().to_string()).unwrap_or_else(|| "not saved".into())
        );
        let right_rect = widgets::paint_elided(
            ui.painter(),
            pos2(rect.right() - 4.0, y),
            &right,
            theme::caption(),
            theme::TEXT_DISABLED,
            rect.width() * 0.4,
            Align2::RIGHT_CENTER,
        );
        let text_color = if recent.is_some_and(|s| s.error) { theme::TEXT } else { theme::TEXT_MUTED };
        let status_rect = widgets::paint_elided(
            ui.painter(),
            pos2(rect.left() + 20.0, y),
            &text,
            theme::caption(),
            text_color,
            (right_rect.left() - rect.left() - 40.0).max(40.0),
            Align2::LEFT_CENTER,
        );
        let response = ui.interact(status_rect, Id::new("hub-status-text"), Sense::click());
        if response.clicked() {
            self.status = None;
        }
        response.on_hover_text(text);
    }

    // -----------------------------------------------------------------------
    // Projects
    // -----------------------------------------------------------------------

    fn page_header(&mut self, ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Self, &mut Ui)) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::hover());
        let inner = rect.shrink2(vec2(PAD, 0.0));
        let painter = ui.painter();
        painter.text(pos2(inner.left(), rect.top() + 30.0), Align2::LEFT_CENTER, title, theme::strong(21.0), theme::TEXT);
        widgets::paint_elided(
            painter,
            pos2(inner.left(), rect.top() + 54.0),
            subtitle,
            theme::caption(),
            theme::TEXT_MUTED,
            inner.width() * 0.6,
            Align2::LEFT_CENTER,
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(inner.left(), rect.top() + 18.0), pos2(inner.right(), rect.top() + 50.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 8.0;
        right(self, &mut child);
    }

    fn projects_page(&mut self, ui: &mut Ui) {
        let total = self.listed.len();
        let pinned = self.listed.iter().filter(|l| l.favorite).count();
        let missing = self.listed.iter().filter(|l| l.missing()).count();
        let mut subtitle = format!("{total} project{}", if total == 1 { "" } else { "s" });
        if pinned > 0 {
            subtitle.push_str(&format!("  ·  {pinned} pinned"));
        }
        if missing > 0 {
            subtitle.push_str(&format!("  ·  {missing} missing"));
        }
        subtitle.push_str("  ·  engine games/ and folders you added");
        self.page_header(ui, "Projects", &subtitle, |hub, ui| {
            if widgets::large_button(ui, Id::new("hub-new-project"), Some(Icon::Plus), "New Project", true).clicked() {
                hub.push(HubAction::Navigate(Page::NewProject));
            }
            if widgets::large_button(ui, Id::new("hub-add-existing"), Some(Icon::FolderOpen), "Add Existing…", false).clicked() {
                hub.push(HubAction::ShowAdd { locate: None });
            }
            let idle = hub.job.is_none();
            let prebuild = ui
                .add_enabled_ui(idle, |ui| widgets::large_button(ui, Id::new("hub-prebuild"), Some(Icon::Build), "Prebuild All", false))
                .inner
                .on_hover_text("Build the editor of every project that needs it, one after another, so Open is instant later");
            if prebuild.clicked() {
                hub.push(HubAction::PrebuildAll);
            }
        });

        // Поиск, порядок, вид.
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
        let mut left = ui.new_child(
            UiBuilder::new()
                .max_rect(bar.shrink2(vec2(PAD, 0.0)).with_max_y(bar.bottom() - 9.0))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let search = theme::search_field(&mut left, &mut self.search, "Search by name, folder or path", 340.0);
        self.search_id = Some(search.id);
        let mut right = ui.new_child(
            UiBuilder::new()
                .max_rect(bar.shrink2(vec2(PAD, 0.0)).with_max_y(bar.bottom() - 9.0))
                .layout(Layout::right_to_left(Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 6.0;
        let mut view = self.config.settings.view;
        if icon_segmented(&mut right, &mut view, &[(ViewMode::Grid, Icon::Tiles, "Grid"), (ViewMode::List, Icon::List, "List")]) {
            self.push(HubAction::SetView(view));
        }
        right.add_space(10.0);
        let mut sort = self.config.settings.sort;
        if theme::segmented(&mut right, &mut sort, &[(SortOrder::LastOpened, "Last opened", "Recently opened first"), (SortOrder::Name, "Name", "Alphabetical")]) {
            self.push(HubAction::SetSort(sort));
        }
        right.label(egui::RichText::new("Sort").font(theme::caption()).color(theme::TEXT_MUTED));

        let visible: Vec<Listed> = self.visible_projects().into_iter().cloned().collect();
        if visible.is_empty() {
            if self.refreshing() && self.listed.is_empty() {
                theme::empty_state(ui, Icon::Refresh, "Looking for projects…", "Reading the engine's games/ and the folders you added.");
            } else if self.search.trim().is_empty() {
                theme::empty_state(ui, Icon::Folder, "No projects yet", "Create one with New Project or add a folder with Add Existing…");
            } else {
                theme::empty_state(
                    ui,
                    Icon::Search,
                    &format!("Nothing matches “{}”", self.search.trim()),
                    "Search looks at project names, folders and paths.",
                );
            }
            return;
        }
        let view = self.config.settings.view;
        egui::ScrollArea::vertical()
            .id_salt("hub-projects")
            .auto_shrink([false, false])
            .show(ui, |ui| match view {
                ViewMode::List => self.list_view(ui, &visible),
                ViewMode::Grid => self.grid_view(ui, &visible),
            });
    }

    fn list_view(&mut self, ui: &mut Ui, visible: &[Listed]) {
        ui.spacing_mut().item_spacing.y = 0.0;
        // Заголовки колонок.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let columns = Columns::new(head);
        let painter = ui.painter();
        painter.text(pos2(columns.text, head.center().y), Align2::LEFT_CENTER, "NAME", theme::strong(10.5), theme::TEXT_DISABLED);
        painter.text(pos2(columns.opened, head.center().y), Align2::LEFT_CENTER, "LAST OPENED", theme::strong(10.5), theme::TEXT_DISABLED);
        painter.hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
        for listed in visible {
            self.project_row(ui, listed);
        }
        ui.add_space(24.0);
    }

    fn project_row(&mut self, ui: &mut Ui, listed: &Listed) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let id = Hub::row_id_key(&listed.key);
        let row = ui.interact(rect, id, Sense::click());
        let columns = Columns::new(rect);
        let selected = self.selected_key.as_deref() == Some(listed.key.as_str());
        let ok = listed.info.is_ok();
        let editor_pid = self.processes.running_key(&listed.key, &JobKind::Open);
        let can_open = listed.info.as_ref().is_ok_and(|info| info.editor_bin.is_some()) && editor_pid.is_none();
        {
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, 0.0, theme::mix(theme::BG_PANEL, theme::accent(), 0.16));
                painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, theme::accent());
            } else if row.hovered() {
                painter.rect_filled(rect, 0.0, theme::hover_fill());
            }
            painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
        }

        // Звезда «закрепить».
        let star_rect = Rect::from_center_size(pos2(columns.star, rect.center().y), vec2(22.0, 22.0));
        let star = ui.interact(star_rect, id.with("pin"), Sense::click());
        if listed.favorite || star.hovered() || row.hovered() {
            let color = if listed.favorite {
                theme::WARN
            } else if star.hovered() {
                theme::TEXT
            } else {
                theme::TEXT_DISABLED
            };
            widgets::paint_star(ui.painter(), star_rect.center(), 6.5, listed.favorite, color);
        }
        if star.on_hover_text(if listed.favorite { "Unpin" } else { "Pin to top" }).clicked() {
            self.push(HubAction::TogglePin(listed.path.clone()));
        }

        let thumb = Rect::from_min_size(pos2(columns.thumb, rect.center().y - THUMB.y * 0.5), THUMB);
        self.paint_thumb(ui, thumb, listed);

        // Имя, метки, путь.
        let name_color = if ok { theme::TEXT } else { theme::TEXT_DISABLED };
        let text_width = columns.opened - columns.text - 16.0;
        let name_rect = widgets::paint_elided(
            ui.painter(),
            pos2(columns.text, rect.center().y - 10.0),
            &listed.title(),
            theme::strong(theme::FONT_TITLE),
            name_color,
            text_width * 0.6,
            Align2::LEFT_CENTER,
        );
        let mut x = name_rect.right() + 8.0;
        for (text, tone) in badges(listed, editor_pid.is_some()) {
            if x + widgets::badge_width(ui.painter(), &text) > columns.opened - 12.0 {
                break;
            }
            x = widgets::paint_badge(ui.painter(), x, rect.center().y - 10.0, &text, tone).right() + 5.0;
        }
        let second = match &listed.info {
            Err(_) => Hub::problem(listed).unwrap_or_default(),
            Ok(_) => match &listed.location {
                Some(location) => format!("{location}  ·  {}", listed.path.display()),
                None => format!("External  ·  {}", listed.path.display()),
            },
        };
        let second_color = match &listed.info {
            Err(_) if listed.missing() => theme::WARN,
            Err(_) => theme::ERROR,
            Ok(_) => theme::TEXT_MUTED,
        };
        let path_rect = widgets::paint_elided(
            ui.painter(),
            pos2(columns.text, rect.center().y + 11.0),
            &second,
            theme::caption(),
            second_color,
            text_width,
            Align2::LEFT_CENTER,
        );
        if listed.info.is_err() {
            ui.interact(path_rect, id.with("problem"), Sense::hover()).on_hover_text(second.clone());
        }

        // Когда открывали.
        let (opened, opened_color) = match editor_pid {
            Some(_) => ("Open in the editor".to_string(), theme::OK),
            None => (crate::config::ago(listed.last_opened, self.now()), theme::TEXT_MUTED),
        };
        let now = self.now();
        let (built, built_color) = build_label(listed, now);
        let has_build = !built.is_empty();
        let opened_y = if has_build { rect.center().y - 9.0 } else { rect.center().y };
        ui.painter()
            .text(pos2(columns.opened, opened_y), Align2::LEFT_CENTER, opened, theme::caption(), opened_color);
        if has_build {
            ui.painter()
                .text(pos2(columns.opened, rect.center().y + 10.0), Align2::LEFT_CENTER, built, theme::caption(), built_color);
        }

        // Кнопки справа налево.
        let actions_rect = Rect::from_min_max(pos2(columns.actions, rect.top()), pos2(columns.right, rect.bottom()));
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(actions_rect).layout(Layout::right_to_left(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 6.0;
        let more_id = id.with("more");
        if listed.missing() {
            if widgets::compact_button(&mut buttons, id.with("remove"), None, "Remove", false).clicked() {
                self.push(HubAction::Remove(listed.path.clone()));
            }
            if widgets::compact_button(&mut buttons, id.with("locate"), Some(Icon::Search), "Locate…", true).clicked() {
                self.push(HubAction::ShowAdd {
                    locate: Some(listed.path.clone()),
                });
            }
        } else {
            let more = widgets::icon_button(&mut buttons, more_id, Icon::More, "More actions");
            self.more_menu(&buttons, &more, listed);
            if ok {
                let play_tip = match listed.builds.game {
                    crate::builds::BuildState::Fresh { .. } => "Play: run the game (already built)",
                    _ => "Play: build and run the game",
                };
                let play = buttons
                    .add_enabled_ui(listed.info.as_ref().is_ok_and(|i| i.game_bin.is_some()), |ui| {
                        widgets::icon_button(ui, id.with("play"), Icon::Play, play_tip)
                    })
                    .inner;
                if play.clicked() {
                    self.push(HubAction::Play(listed.path.clone()));
                }
                let open_tooltip = match (&listed.info, editor_pid) {
                    (_, Some(pid)) => format!("Already open in the editor (process {pid})"),
                    (Ok(info), None) => info.editor_problem().unwrap_or_else(|| match listed.builds.editor {
                        crate::builds::BuildState::Fresh { .. } => "Open the editor now (it is built and up to date)".to_string(),
                        crate::builds::BuildState::Stale { .. } => {
                            "The project changed since the last build: build, then open (⋯ → Open Last Build skips the build)".to_string()
                        }
                        crate::builds::BuildState::Never => "Build the editor program, then open it".to_string(),
                    }),
                    _ => String::new(),
                };
                let open = buttons
                    .add_enabled_ui(can_open, |ui| {
                        widgets::compact_button(ui, id.with("open"), None, "Open", selected || row.hovered())
                    })
                    .inner;
                if open.on_hover_text(open_tooltip).clicked() {
                    self.push(HubAction::Open(listed.path.clone()));
                }
            } else if widgets::compact_button(&mut buttons, id.with("remove"), None, "Remove", false).clicked() {
                self.push(HubAction::Remove(listed.path.clone()));
            }
        }

        if row.clicked() {
            self.push(HubAction::Select(Some(listed.path.clone())));
        }
        if row.double_clicked() && can_open {
            self.push(HubAction::Open(listed.path.clone()));
        }
        let mut menu_actions = Vec::new();
        row.context_menu(|ui| {
            theme::menu_body(ui);
            project_menu(ui, listed, self.processes.running_key(&listed.key, &JobKind::Open), &mut menu_actions);
        });
        self.actions.extend(menu_actions);
    }

    fn more_menu(&mut self, ui: &Ui, button: &egui::Response, listed: &Listed) {
        let popup = button.id.with("popup");
        if button.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        let mut actions = Vec::new();
        let editor = self.processes.running_key(&listed.key, &JobKind::Open);
        egui::popup_below_widget(ui, popup, button, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            theme::menu_body(ui);
            project_menu(ui, listed, editor, &mut actions);
        });
        self.actions.extend(actions);
    }

    fn grid_view(&mut self, ui: &mut Ui, visible: &[Listed]) {
        const GAP: f32 = 16.0;
        let width = ui.available_width() - PAD * 2.0;
        let columns = ((width + GAP) / (236.0 + GAP)).floor().max(1.0) as usize;
        let card_w = (width - GAP * (columns as f32 - 1.0)) / columns as f32;
        let thumb_h = card_w * 9.0 / 16.0;
        let card_h = thumb_h + 86.0;
        ui.add_space(18.0);
        for chunk in visible.chunks(columns) {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), card_h), Sense::hover());
            for (i, listed) in chunk.iter().enumerate() {
                let left = row.left() + PAD + i as f32 * (card_w + GAP);
                let rect = Rect::from_min_size(pos2(left, row.top()), vec2(card_w, card_h));
                if ui.is_rect_visible(rect) {
                    self.project_card(ui, rect, thumb_h, listed);
                }
            }
            ui.add_space(GAP);
        }
        ui.add_space(12.0);
    }

    fn project_card(&mut self, ui: &mut Ui, rect: Rect, thumb_h: f32, listed: &Listed) {
        let id = Hub::row_id_key(&listed.key);
        let card = ui.interact(rect, id, Sense::click());
        let selected = self.selected_key.as_deref() == Some(listed.key.as_str());
        let editor_pid = self.processes.running_key(&listed.key, &JobKind::Open);
        let can_open = listed.info.as_ref().is_ok_and(|info| info.editor_bin.is_some()) && editor_pid.is_none();
        let stroke = if selected {
            theme::accent()
        } else if card.hovered() {
            theme::mix(theme::LINE_STRONG, Color32::WHITE, 0.12)
        } else {
            theme::LINE
        };
        ui.painter().rect(rect, theme::R_M, theme::BG_RAISED, Stroke::new(1.0_f32, stroke));
        let thumb = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(rect.width() - 12.0, thumb_h - 6.0));
        self.paint_thumb(ui, thumb, listed);
        if listed.favorite {
            let badge = Rect::from_min_size(thumb.right_top() + vec2(-26.0, 6.0), vec2(20.0, 20.0));
            ui.painter().rect_filled(badge, theme::R_S, Color32::from_black_alpha(150));
            widgets::paint_star(ui.painter(), badge.center(), 6.0, true, theme::WARN);
        }
        let name_y = thumb.bottom() + 20.0;
        let name_color = if listed.info.is_ok() { theme::TEXT } else { theme::TEXT_DISABLED };
        widgets::paint_elided(
            ui.painter(),
            pos2(rect.left() + 12.0, name_y),
            &listed.title(),
            theme::strong(theme::FONT_TITLE),
            name_color,
            rect.width() - 24.0,
            Align2::LEFT_CENTER,
        );
        let mut x = rect.left() + 12.0;
        for (text, tone) in badges(listed, editor_pid.is_some()) {
            if x + widgets::badge_width(ui.painter(), &text) > rect.right() - 12.0 {
                break;
            }
            x = widgets::paint_badge(ui.painter(), x, name_y + 22.0, &text, tone).right() + 5.0;
        }
        let caption = if listed.info.is_err() {
            Hub::problem(listed).unwrap_or_default()
        } else {
            match editor_pid {
                Some(_) => "Open in the editor".to_string(),
                None => format!("Opened {}", crate::config::ago(listed.last_opened, self.now()).to_lowercase()),
            }
        };
        widgets::paint_elided(
            ui.painter(),
            pos2(rect.left() + 12.0, rect.bottom() - 17.0),
            &caption,
            theme::caption(),
            if listed.info.is_err() { theme::WARN } else { theme::TEXT_MUTED },
            rect.width() - 110.0,
            Align2::LEFT_CENTER,
        );
        let actions_rect = Rect::from_min_max(pos2(rect.left() + 12.0, rect.bottom() - 32.0), pos2(rect.right() - 8.0, rect.bottom() - 4.0));
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(actions_rect).layout(Layout::right_to_left(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 4.0;
        if listed.missing() {
            if widgets::compact_button(&mut buttons, id.with("locate"), None, "Locate…", true).clicked() {
                self.push(HubAction::ShowAdd {
                    locate: Some(listed.path.clone()),
                });
            }
        } else {
            let more = widgets::icon_button(&mut buttons, id.with("more"), Icon::More, "More actions");
            self.more_menu(&buttons, &more, listed);
            if listed.info.is_ok() {
                let open = buttons
                    .add_enabled_ui(can_open, |ui| widgets::compact_button(ui, id.with("open"), None, "Open", selected || card.hovered()))
                    .inner;
                if open.clicked() {
                    self.push(HubAction::Open(listed.path.clone()));
                }
            }
        }
        if card.clicked() {
            self.push(HubAction::Select(Some(listed.path.clone())));
        }
        if card.double_clicked() && can_open {
            self.push(HubAction::Open(listed.path.clone()));
        }
        let mut menu_actions = Vec::new();
        card.context_menu(|ui| {
            theme::menu_body(ui);
            project_menu(ui, listed, editor_pid, &mut menu_actions);
        });
        self.actions.extend(menu_actions);
    }

    // -----------------------------------------------------------------------
    // Ход сборки
    // -----------------------------------------------------------------------

    fn job_view(&mut self, ui: &mut Ui) {
        let Some(job) = &self.job else {
            return;
        };
        let title = format!("{} {}", job.kind.verb(), job.title);
        let command = job.command.display();
        let state = job.state.clone();
        let progress = job.progress.clone();
        let elapsed = job.elapsed().as_secs();
        let kind = job.kind.clone();
        let cwd = job.command.cwd.clone();
        let project = job.project.clone();
        let failed = matches!(state, JobState::Failed(_) | JobState::Cancelled);
        let diagnosis = self.job_diagnosis();
        let last_build = self.last_build(&project, &kind);
        let last_built = self
            .listed(&project)
            .and_then(|l| if kind == JobKind::Play { l.builds.game.built() } else { l.builds.editor.built() });
        let prebuild = self.prebuild.clone().filter(|p| !p.finished);
        let title = match &prebuild {
            Some(p) => format!("Prebuilding editors: {} of {}", p.index, p.queue.len()),
            None => title,
        };
        let subtitle = match &prebuild {
            Some(_) => format!("{}  ·  {command}", self.job.as_ref().map(|j| j.title.clone()).unwrap_or_default()),
            None => format!("{command}   (in {})", cwd.display()),
        };
        self.page_header(ui, &title, &subtitle, |_, _| {});
        widgets::separator(ui);

        let body = ui.available_rect_before_wrap().shrink2(vec2(PAD, 0.0));
        let mut ui = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        ui.add_space(20.0);
        if let Some(p) = &prebuild {
            // Общий ход: сколько проектов готово и где сейчас.
            let done = p.results.len();
            let failed_count = p.results.iter().filter(|r| !r.1).count();
            let current = progress.fraction().unwrap_or(0.0);
            let overall = (done as f32 + current) / p.queue.len().max(1) as f32;
            let text = format!(
                "{done} of {} done{}  ·  {} already up to date",
                p.queue.len(),
                if failed_count > 0 { format!(", {failed_count} failed") } else { String::new() },
                p.skipped
            );
            ui.label(egui::RichText::new(text).font(theme::caption()).color(theme::TEXT_MUTED));
            ui.add_space(4.0);
            widgets::progress_bar(ui, Some(overall), false);
            ui.add_space(14.0);
        }

        // Карточка состояния.
        egui::Frame::none()
            .fill(theme::BG_RAISED)
            .stroke(Stroke::new(1.0_f32, theme::LINE_STRONG))
            .rounding(theme::R_M)
            .inner_margin(Margin::symmetric(18.0, 16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                let (headline, color) = match &state {
                    JobState::Building => match (&progress.step, &progress.current) {
                        (Some((done, total, step)), _) => (format!("Step {done} of {total}: {step}"), theme::TEXT),
                        (None, _) if progress.waiting_for_lock => (
                            "Waiting for another cargo build to release the build directory…".to_string(),
                            theme::TEXT,
                        ),
                        (None, Some(current)) => (format!("Compiling {current}"), theme::TEXT),
                        (None, None) => ("Starting cargo…".to_string(), theme::TEXT),
                    },
                    JobState::Failed(reason) => (reason.clone(), theme::ERROR),
                    JobState::Cancelled => ("Cancelled".to_string(), theme::WARN),
                    JobState::Succeeded => ("Finished".to_string(), theme::OK),
                    JobState::Launched(pid) => (format!("Started (process {pid})"), theme::OK),
                };
                let icon = match &state {
                    JobState::Building => Icon::Build,
                    JobState::Failed(_) => Icon::Error,
                    JobState::Cancelled => Icon::Warn,
                    _ => Icon::Check,
                };
                icons::paint(ui.painter(), Rect::from_center_size(pos2(line.left() + 8.0, line.center().y), vec2(15.0, 15.0)), icon, color);
                widgets::paint_elided(
                    ui.painter(),
                    pos2(line.left() + 26.0, line.center().y),
                    &headline,
                    theme::strong(theme::FONT_TITLE),
                    theme::TEXT,
                    line.width() - 220.0,
                    Align2::LEFT_CENTER,
                );
                let counts = match progress.total {
                    Some(total) => format!("{} / {} units   {}", progress.artifacts, total.max(progress.artifacts), widgets::clock(elapsed)),
                    None => format!("{} units   {}", progress.artifacts, widgets::clock(elapsed)),
                };
                ui.painter()
                    .text(line.right_center(), Align2::RIGHT_CENTER, counts, theme::mono(), theme::TEXT_MUTED);
                ui.add_space(10.0);
                let fraction = if state == JobState::Building { progress.fraction() } else { Some(progress.fraction().unwrap_or(1.0)) };
                widgets::progress_bar(ui, fraction, failed);
                ui.add_space(10.0);
                let fresh = progress.fresh;
                let stats = format!(
                    "{} compiled  ·  {} up to date  ·  {} warning{}  ·  {} error{}",
                    progress.compiled,
                    fresh,
                    progress.warnings,
                    if progress.warnings == 1 { "" } else { "s" },
                    progress.errors.len(),
                    if progress.errors.len() == 1 { "" } else { "s" }
                );
                ui.label(egui::RichText::new(stats).font(theme::caption()).color(theme::TEXT_MUTED));
                if state == JobState::Building && kind == JobKind::Open {
                    ui.label(
                        egui::RichText::new("The hub hides while the editor is open and comes back when you close it.")
                            .font(theme::caption())
                            .color(theme::TEXT_DISABLED),
                    );
                }
            });

        // Что происходит — простыми словами.
        if let Some(diagnosis) = &diagnosis {
            ui.add_space(12.0);
            let built = last_built.map(|t| crate::config::ago(Some(t), self.now()).to_lowercase());
            let last_hint = match (&last_build, &built) {
                (Some(_), Some(when)) => format!(" Open Last Build starts the editor built {when}."),
                (Some(_), None) => " Open Last Build starts the previous build.".to_string(),
                _ => String::new(),
            };
            match diagnosis {
                crate::hub::Diagnosis::WaitingForLock { what, seconds } => alert(
                    ui,
                    Tone::Warn,
                    Icon::Warn,
                    &format!("Waiting for another build ({})", widgets::clock(*seconds)),
                    &format!(
                        "Another cargo build is using the {what}. This build continues by itself when that one finishes; nothing is wrong with your project. You can wait, cancel, or open what you built before.{last_hint}"
                    ),
                    None,
                ),
                crate::hub::Diagnosis::EngineBroken { package, error } => alert(
                    ui,
                    Tone::Error,
                    Icon::Error,
                    "The engine source does not compile right now",
                    &format!(
                        "The error is in `{package}`, not in your project: the engine is probably being edited at the moment. Try Again once it compiles.{last_hint}"
                    ),
                    Some(error),
                ),
                crate::hub::Diagnosis::ProjectError { error } => alert(
                    ui,
                    Tone::Error,
                    Icon::Error,
                    &format!("Error in {}", if error.location().is_empty() { "the project".to_string() } else { error.location() }),
                    &format!("Fix it in your code and press Try Again.{last_hint}"),
                    Some(error),
                ),
            }
        }

        // Кнопки — снизу, поэтому место под них отводится заранее.
        let buttons_h = 52.0;
        let rest = ui.available_rect_before_wrap();
        let content = Rect::from_min_max(rest.min, pos2(rest.right(), rest.bottom() - buttons_h));
        let mut content_ui = ui.new_child(UiBuilder::new().max_rect(content).layout(Layout::top_down(Align::Min)));
        content_ui.add_space(18.0);
        if !progress.errors.is_empty() {
            widgets::section_label(&mut content_ui, &format!("Errors ({})", progress.errors.len()));
            content_ui.add_space(4.0);
            let errors_h = (progress.errors.len() as f32 * 44.0 + 8.0).min(content.height() * 0.42);
            egui::ScrollArea::vertical()
                .id_salt("hub-errors")
                .max_height(errors_h)
                .auto_shrink([false, true])
                .show(&mut content_ui, |ui| {
                    for (i, error) in progress.errors.iter().enumerate().take(200) {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
                        let row = ui.interact(rect, Id::new("hub-error").with(i), Sense::click());
                        if row.hovered() {
                            ui.painter().rect_filled(rect, theme::R_S, theme::hover_fill());
                        }
                        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 10.0, rect.top() + 13.0), vec2(13.0, 13.0)), Icon::Error, theme::ERROR);
                        let location = error.location();
                        let loc_rect = widgets::paint_elided(
                            ui.painter(),
                            pos2(rect.left() + 24.0, rect.top() + 13.0),
                            if location.is_empty() { "(no location)" } else { &location },
                            theme::mono(),
                            theme::mix(theme::accent(), Color32::WHITE, 0.3),
                            rect.width() * 0.5,
                            Align2::LEFT_CENTER,
                        );
                        let _ = loc_rect;
                        widgets::paint_elided(
                            ui.painter(),
                            pos2(rect.left() + 24.0, rect.top() + 31.0),
                            &error.message,
                            theme::caption(),
                            theme::TEXT,
                            rect.width() - 30.0,
                            Align2::LEFT_CENTER,
                        );
                        if let Some(file) = &error.file {
                            let resolved = resolve_source(file, &cwd, self.engine_root.as_deref());
                            if row.on_hover_text(format!("Open {}", resolved.display())).clicked() {
                                self.actions.push(HubAction::OpenFile(resolved));
                            }
                        }
                    }
                });
            content_ui.add_space(14.0);
        }
        widgets::section_label(&mut content_ui, "Output");
        content_ui.add_space(4.0);
        let out_rect = content_ui.available_rect_before_wrap();
        egui::Frame::none()
            .fill(theme::BG_INSET)
            .stroke(Stroke::new(1.0_f32, theme::LINE))
            .rounding(theme::R_S)
            .inner_margin(Margin::symmetric(10.0, 8.0))
            .show(&mut content_ui, |ui| {
                ui.set_min_size(vec2(out_rect.width() - 22.0, (out_rect.height() - 18.0).max(40.0)));
                if progress.tail.is_empty() {
                    ui.label(egui::RichText::new("Waiting for cargo…").font(theme::mono()).color(theme::TEXT_DISABLED));
                }
                // Только видимые строки: хвост в сотни строк не раскладывается каждый кадр.
                let row_height = ui.fonts(|f| f.row_height(&egui::FontId::monospace(12.5))) + 1.0;
                egui::ScrollArea::both()
                    .id_salt("hub-output")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show_rows(ui, row_height, progress.tail.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for line in progress.tail.range(range) {
                            let color = if line.starts_with("error") {
                                theme::ERROR
                            } else if line.starts_with("warning") {
                                theme::WARN
                            } else {
                                theme::TEXT_MUTED
                            };
                            ui.add(egui::Label::new(egui::RichText::new(line).font(egui::FontId::monospace(12.5)).color(color)).extend());
                        }
                    });
            });

        let bar = Rect::from_min_max(pos2(rest.left(), rest.bottom() - buttons_h), rest.max);
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(0.0, 10.0))).layout(Layout::right_to_left(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 8.0;
        let open_last_label = if kind == JobKind::Play { "Play Last Build" } else { "Open Last Build" };
        let last_action = |path: PathBuf| {
            if kind == JobKind::Play {
                HubAction::PlayLastBuild(path)
            } else {
                HubAction::OpenLastBuild(path)
            }
        };
        if prebuild.is_some() {
            if widgets::large_button(&mut buttons, Id::new("hub-prebuild-stop"), None, "Stop Prebuild", false).clicked() {
                self.push(HubAction::CancelPrebuild);
            }
            return;
        }
        match state {
            JobState::Building => {
                if widgets::large_button(&mut buttons, Id::new("hub-job-cancel"), None, "Cancel", false).clicked() {
                    self.push(HubAction::CancelJob);
                }
                if last_build.is_some()
                    && widgets::large_button(&mut buttons, Id::new("hub-job-last"), Some(Icon::Play), open_last_label, diagnosis.is_some())
                        .on_hover_text("Stop this build and start the program from the previous build")
                        .clicked()
                {
                    self.push(last_action(project.clone()));
                }
            }
            JobState::Succeeded => {
                if widgets::large_button(&mut buttons, Id::new("hub-job-done"), None, "Done", true).clicked() {
                    self.push(HubAction::CloseJob);
                }
                if let JobKind::Export { platform, .. } = &kind {
                    let folder = project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(root) = &self.engine_root {
                        let out = root.join("dist").join(folder).join(platform);
                        if widgets::large_button(&mut buttons, Id::new("hub-job-reveal"), Some(Icon::FolderOpen), "Show Output Folder", false).clicked() {
                            self.actions.push(HubAction::Reveal(out));
                        }
                    }
                }
            }
            _ => {
                let last = last_build.is_some() && matches!(kind, JobKind::Open | JobKind::Play);
                if last
                    && widgets::large_button(&mut buttons, Id::new("hub-job-last"), Some(Icon::Play), open_last_label, true)
                        .on_hover_text("Start the program from the previous successful build")
                        .clicked()
                {
                    self.push(last_action(project.clone()));
                }
                if widgets::large_button(&mut buttons, Id::new("hub-job-retry"), Some(Icon::Refresh), "Try Again", !last).clicked() {
                    self.push(HubAction::RetryJob);
                }
                if widgets::large_button(&mut buttons, Id::new("hub-job-back"), None, "Back to Projects", false).clicked() {
                    self.push(HubAction::CloseJob);
                }
            }
        }
    }

    /// Итог «Prebuild all»: что собралось, что нет и почему.
    fn prebuild_summary(&mut self, ui: &mut Ui) {
        let Some(prebuild) = self.prebuild.clone() else {
            return;
        };
        let failed = prebuild.results.iter().filter(|r| !r.1).count();
        let subtitle = format!(
            "{} built, {failed} failed, {} were already up to date. Open is now instant for the built ones.",
            prebuild.results.len() - failed,
            prebuild.skipped
        );
        self.page_header(ui, "Prebuild finished", &subtitle, |_, _| {});
        widgets::separator(ui);
        let body = ui.available_rect_before_wrap().shrink2(vec2(PAD, 0.0));
        let mut ui = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for (name, ok, detail) in &prebuild.results {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
            let (icon, color) = if *ok { (Icon::Check, theme::OK) } else { (Icon::Error, theme::ERROR) };
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(14.0, 14.0)), icon, color);
            ui.painter()
                .text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
            let text = if *ok { "Built".to_string() } else { detail.clone() };
            widgets::paint_elided(
                ui.painter(),
                pos2(rect.left() + 260.0, rect.center().y),
                &text,
                theme::caption(),
                if *ok { theme::TEXT_MUTED } else { theme::ERROR },
                rect.width() - 270.0,
                Align2::LEFT_CENTER,
            );
        }
        ui.add_space(16.0);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if widgets::large_button(ui, Id::new("hub-prebuild-done"), None, "Done", true).clicked() {
                self.actions.push(HubAction::CloseJob);
            }
        });
    }

    // -----------------------------------------------------------------------
    // New Project
    // -----------------------------------------------------------------------

    fn wizard_page(&mut self, ui: &mut Ui) {
        self.page_header(
            ui,
            "New Project",
            "A game crate with its own editor program, assets with .meta files, a starting scene, a headless test and a README.",
            |_, _| {},
        );
        widgets::separator(ui);
        if let Some(created) = self.wizard.created.clone() {
            self.wizard_done(ui, &created);
            return;
        }
        let body = ui.available_rect_before_wrap().shrink2(vec2(PAD, 0.0));
        let left_w = (body.width() * 0.44).clamp(340.0, 470.0);
        let left = Rect::from_min_max(body.min + vec2(0.0, 20.0), pos2(body.left() + left_w, body.bottom()));
        let right = Rect::from_min_max(pos2(left.right() + 32.0, body.top() + 20.0), body.max);

        // Шаблоны.
        let mut column = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::top_down(Align::Min)));
        widgets::section_label(&mut column, "Template");
        column.add_space(6.0);
        for (index, template) in Template::ALL.into_iter().enumerate() {
            let (rect, _) = column.allocate_exact_size(vec2(column.available_width(), 104.0), Sense::hover());
            let response = column.interact(rect, Id::new("hub-template").with(index), Sense::click());
            let selected = self.wizard.template == template;
            let (fill, stroke) = if selected {
                (theme::mix(theme::BG_PANEL, theme::accent(), 0.12), theme::accent())
            } else if response.hovered() {
                (theme::BG_RAISED, theme::mix(theme::LINE_STRONG, Color32::WHITE, 0.12))
            } else {
                (theme::BG_RAISED, theme::LINE)
            };
            let painter = column.painter();
            painter.rect(rect, theme::R_M, fill, Stroke::new(1.0_f32, stroke));
            let preview = Rect::from_min_size(rect.min + vec2(12.0, 16.0), vec2(128.0, 72.0));
            widgets::paint_template_preview(painter, preview, template);
            let text_left = preview.right() + 14.0;
            let text_w = rect.right() - text_left - 12.0;
            painter.text(pos2(text_left, rect.top() + 24.0), Align2::LEFT_CENTER, template.title(), theme::strong(theme::FONT_TITLE), theme::TEXT);
            let galley = painter.layout(template.description().to_string(), theme::caption(), theme::TEXT_MUTED, text_w);
            painter.galley(pos2(text_left, rect.top() + 36.0), galley, theme::TEXT_MUTED);
            widgets::paint_elided(
                painter,
                pos2(text_left, rect.bottom() - 15.0),
                template.contents(),
                theme::caption(),
                theme::TEXT_DISABLED,
                text_w,
                Align2::LEFT_CENTER,
            );
            if response.clicked() {
                self.wizard.template = template;
            }
            column.add_space(10.0);
        }

        // Поля.
        let plan = self.wizard_plan();
        let mut form = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::top_down(Align::Min)));
        let ui = &mut form;
        let full = ui.available_width();
        widgets::section_label(ui, "Name");
        ui.add_space(4.0);
        widgets::text_field(ui, Id::new(Hub::WIZARD_NAME_ID), &mut self.wizard.name, "my_game", full);
        ui.add_space(4.0);
        let name_problem = (!self.wizard.name.trim().is_empty())
            .then(|| crate::templates::validate_name(self.wizard.name.trim()).err())
            .flatten();
        let (hint, hint_color) = match (&name_problem, &plan) {
            (Some(problem), _) => (problem.to_string(), theme::ERROR),
            (None, Ok((dir, _, _))) => {
                let shown = self
                    .engine_root
                    .as_deref()
                    .and_then(|root| paths::relative_to(dir, root))
                    .map(|relative| format!("{relative}  (in the engine workspace)"))
                    .unwrap_or_else(|| dir.display().to_string());
                (format!("Folder: {shown}"), theme::TEXT_MUTED)
            }
            (None, Err(_)) if self.wizard.name.trim().is_empty() => (
                "Lowercase letters, digits, _ and -. It becomes the crate and folder name.".to_string(),
                theme::TEXT_MUTED,
            ),
            (None, Err(problem)) => (problem.clone(), theme::ERROR),
        };
        ui.add(egui::Label::new(egui::RichText::new(hint).font(theme::caption()).color(hint_color)).wrap());
        ui.add_space(14.0);

        widgets::section_label(ui, "Location");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let browse_w = 92.0;
            widgets::text_field(ui, Id::new("hub-wizard-location"), &mut self.wizard.location, "Folder for the project", full - browse_w - 6.0);
            if widgets::sized_like_field(ui, Id::new("hub-wizard-browse"), "Browse…", browse_w).clicked() {
                self.actions.push(HubAction::Browse(BrowseTarget::WizardLocation));
            }
        });
        ui.add_space(6.0);
        if let Some(root) = self.engine_root.clone() {
            let games = root.join("games");
            if !paths::same(Path::new(self.wizard.location.trim()), &games) {
                let link = ui.add(egui::Label::new(egui::RichText::new("Use the engine's games/ folder").font(theme::caption()).color(theme::accent())).sense(Sense::click()));
                if link.clicked() {
                    self.wizard.location = games.display().to_string();
                }
            }
        }
        ui.add_space(8.0);
        match &plan {
            Ok((_, true, _)) => widgets::note(
                ui,
                Icon::Info,
                Tone::Neutral,
                "Inside the engine workspace: the project shares the engine's build cache, so the first build only compiles the game.",
            ),
            Ok((_, false, _)) => widgets::note(
                ui,
                Icon::Warn,
                Tone::Warn,
                "Outside the engine: the project gets its own Cargo workspace with a path to this engine checkout. \
                 Its first build compiles the whole engine — about 10–15 minutes and several GB of disk.",
            ),
            Err(_) => {}
        }
        ui.add_space(14.0);
        widgets::section_label(ui, "Options");
        ui.add_space(4.0);
        ui.checkbox(&mut self.wizard.cpp, egui::RichText::new("C++ scripts folder (cpp/)").color(theme::TEXT));
        ui.add(
            egui::Label::new(
                egui::RichText::new("Adds build.rs and cpp/Wobble.cpp; components in cpp/*.cpp are compiled into the game. Needs a C++ compiler (MSVC or GCC).")
                    .font(theme::caption())
                    .color(theme::TEXT_MUTED),
            )
            .wrap(),
        );
        ui.add_space(14.0);

        if let Ok((_, _, files)) = &plan {
            widgets::section_label(ui, &format!("Files ({})", files.len()));
            ui.add_space(4.0);
            let shown = 10usize;
            let mut text = files.iter().take(shown).cloned().collect::<Vec<_>>().join("\n");
            if files.len() > shown {
                text.push_str(&format!("\n… and {} more", files.len() - shown));
            }
            ui.label(egui::RichText::new(text).font(egui::FontId::monospace(12.0)).color(theme::TEXT_MUTED));
        }
        if let Some(error) = self.wizard.error.clone() {
            ui.add_space(8.0);
            widgets::note(ui, Icon::Error, Tone::Error, &error);
        }

        // Кнопки — внизу колонки.
        let bar = Rect::from_min_max(pos2(right.left(), right.bottom() - 56.0), pos2(right.right(), right.bottom() - 12.0));
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(bar).layout(Layout::right_to_left(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 8.0;
        let ready = plan.is_ok();
        let create = buttons
            .add_enabled_ui(ready, |ui| widgets::large_button(ui, Id::new(Hub::CREATE_BUTTON_ID), Some(Icon::Plus), "Create Project", true))
            .inner;
        if create.clicked() {
            self.push(HubAction::CreateProject);
        }
        if widgets::large_button(&mut buttons, Id::new("hub-wizard-cancel"), None, "Cancel", false).clicked() {
            self.push(HubAction::Navigate(Page::Projects));
        }
    }

    fn wizard_done(&mut self, ui: &mut Ui, created: &Path) {
        ui.add_space(48.0);
        let title = self
            .listed(created)
            .map(Listed::title)
            .unwrap_or_else(|| created.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let workspace = self.listed(created).is_some_and(|l| l.source == Source::Workspace);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 22.0, theme::mix(theme::BG_PANEL, theme::OK, 0.2));
            icons::paint(ui.painter(), rect.shrink(12.0), Icon::Check, theme::OK);
            ui.add_space(12.0);
            ui.label(egui::RichText::new(format!("“{title}” is ready")).font(theme::strong(19.0)).color(theme::TEXT));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(created.display().to_string()).font(theme::mono()).color(theme::TEXT_MUTED));
            ui.add_space(6.0);
            let first_build = if workspace {
                "Open builds the game's editor program; the engine is already compiled in the workspace."
            } else {
                "The first Open compiles the engine for this project: expect 10–15 minutes."
            };
            ui.label(egui::RichText::new(first_build).font(theme::caption()).color(theme::TEXT_MUTED));
            ui.add_space(20.0);
            // Три кнопки по центру.
            let widths = 360.0;
            let (row, _) = ui.allocate_exact_size(vec2(widths + 120.0, 32.0), Sense::hover());
            let mut buttons = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
            buttons.spacing_mut().item_spacing.x = 8.0;
            if widgets::large_button(&mut buttons, Id::new("hub-open-now"), Some(Icon::Play), "Open Now", true).clicked() {
                self.actions.push(HubAction::Open(created.to_path_buf()));
            }
            if widgets::large_button(&mut buttons, Id::new("hub-created-reveal"), Some(Icon::FolderOpen), "Show in File Manager", false).clicked() {
                self.actions.push(HubAction::Reveal(created.to_path_buf()));
            }
            if widgets::large_button(&mut buttons, Id::new("hub-created-back"), None, "Back to Projects", false).clicked() {
                self.wizard.created = None;
                self.wizard.name.clear();
                self.actions.push(HubAction::Navigate(Page::Projects));
            }
        });
    }

    // -----------------------------------------------------------------------
    // Samples & Docs
    // -----------------------------------------------------------------------

    fn samples_page(&mut self, ui: &mut Ui) {
        self.page_header(
            ui,
            "Samples & Docs",
            "Complete games made with the engine, and the API guides they were written from.",
            |_, _| {},
        );
        widgets::separator(ui);
        egui::ScrollArea::vertical()
            .id_salt("hub-samples")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(20.0);
                const GAP: f32 = 18.0;
                let width = ui.available_width() - PAD * 2.0;
                let card_w = (width - GAP * 2.0) / 3.0;
                let pic_h = card_w * 9.0 / 16.0;
                let card_h = pic_h + 132.0;
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), card_h), Sense::hover());
                for i in 0..SAMPLES.len() {
                    let rect = Rect::from_min_size(pos2(row.left() + PAD + i as f32 * (card_w + GAP), row.top()), vec2(card_w, card_h));
                    self.sample_card(ui, rect, pic_h, i);
                }
                ui.add_space(30.0);
                let (label, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                ui.painter().text(
                    pos2(label.left() + PAD, label.center().y),
                    Align2::LEFT_CENTER,
                    "DOCUMENTATION",
                    theme::strong(11.0),
                    theme::TEXT_MUTED,
                );
                ui.add_space(6.0);
                let docs = self.docs.clone();
                if docs.is_empty() {
                    ui.label(egui::RichText::new("      docs/api was not found in the engine checkout.").color(theme::TEXT_MUTED));
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, doc) in docs.iter().enumerate() {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
                    let inner = rect.shrink2(vec2(PAD, 0.0));
                    let response = ui.interact(inner, Id::new("hub-doc").with(i), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(inner, theme::R_S, theme::hover_fill());
                    }
                    ui.painter().hline(inner.x_range(), inner.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(inner.left() + 14.0, inner.center().y), vec2(14.0, 14.0)), Icon::File, theme::TEXT_MUTED);
                    widgets::paint_elided(
                        ui.painter(),
                        pos2(inner.left() + 34.0, inner.center().y),
                        &doc.title,
                        theme::body(),
                        theme::TEXT,
                        inner.width() - 260.0,
                        Align2::LEFT_CENTER,
                    );
                    ui.painter().text(
                        pos2(inner.right() - 36.0, inner.center().y),
                        Align2::RIGHT_CENTER,
                        format!("docs/api/{}", doc.file),
                        theme::mono(),
                        theme::TEXT_DISABLED,
                    );
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(inner.right() - 14.0, inner.center().y), vec2(12.0, 12.0)), Icon::External, theme::TEXT_DISABLED);
                    if response.on_hover_text("Open with the system's default app").clicked() {
                        self.actions.push(HubAction::OpenFile(doc.path.clone()));
                    }
                }
                ui.add_space(24.0);
            });
    }

    fn sample_card(&mut self, ui: &mut Ui, rect: Rect, pic_h: f32, index: usize) {
        let sample = &SAMPLES[index];
        // Сведения — из списка проектов (он прочитан в фоне), не с диска в каждом кадре.
        let listed = self.listed.iter().find(|l| l.sample == Some(sample.folder));
        let path = listed.map(|l| l.path.clone());
        let key = listed.map(|l| l.key.clone()).unwrap_or_default();
        let info = listed.and_then(|l| l.info.as_ref().ok()).cloned();
        ui.painter().rect(rect, theme::R_M, theme::BG_RAISED, Stroke::new(1.0_f32, theme::LINE));
        let picture = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(rect.width() - 12.0, pic_h - 6.0));
        let texture = self.texture(ui.ctx(), format!("sample:{}", sample.folder), move || Some(sample.picture.to_vec()));
        if let Some(texture) = texture {
            widgets::paint_cover(ui, picture, &texture, theme::R_S, Color32::WHITE);
        }
        let y = picture.bottom() + 22.0;
        let title_rect = ui
            .painter()
            .text(pos2(rect.left() + 14.0, y), Align2::LEFT_CENTER, sample.title, theme::strong(15.0), theme::TEXT);
        let mut x = title_rect.right() + 8.0;
        if let Some(info) = &info {
            let mut tags: Vec<String> = info.dimension.label().map(str::to_string).into_iter().collect();
            if info.has_cpp {
                tags.push("C++".into());
            }
            for tag in tags {
                x = widgets::paint_badge(ui.painter(), x, y, &tag, Tone::Neutral).right() + 5.0;
            }
        }
        let galley = ui
            .painter()
            .layout(sample.summary.to_string(), theme::caption(), theme::TEXT_MUTED, rect.width() - 28.0);
        ui.painter().galley(pos2(rect.left() + 14.0, y + 14.0), galley, theme::TEXT_MUTED);
        let bar = Rect::from_min_max(pos2(rect.left() + 14.0, rect.bottom() - 42.0), pos2(rect.right() - 14.0, rect.bottom() - 12.0));
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 6.0;
        let id = Id::new("hub-sample").with(index);
        match (path, &info) {
            (Some(path), Some(info)) => {
                let editor_open = self.processes.running_key(&key, &JobKind::Open).is_some();
                let open = buttons
                    .add_enabled_ui(info.editor_bin.is_some() && !editor_open, |ui| {
                        widgets::compact_button(ui, id.with("open"), None, "Open", true)
                    })
                    .inner;
                if open.clicked() {
                    self.actions.push(HubAction::Open(path.clone()));
                }
                if widgets::compact_button(&mut buttons, id.with("play"), Some(Icon::Play), "Play", false).clicked() {
                    self.actions.push(HubAction::Play(path.clone()));
                }
                if widgets::icon_button(&mut buttons, id.with("folder"), Icon::FolderOpen, &format!("Show games/{} in the file manager", sample.folder))
                    .clicked()
                {
                    self.actions.push(HubAction::Reveal(path));
                }
            }
            _ => {
                buttons.label(
                    egui::RichText::new(format!("games/{} is not in this engine checkout", sample.folder))
                        .font(theme::caption())
                        .color(theme::TEXT_DISABLED),
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Settings
    // -----------------------------------------------------------------------

    fn settings_page(&mut self, ui: &mut Ui) {
        self.page_header(ui, "Settings", "Interface scale and accent are shared with the editor's Preferences.", |_, _| {});
        widgets::separator(ui);
        let body = ui.available_rect_before_wrap().shrink2(vec2(PAD, 0.0));
        let mut ui = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        ui.add_space(8.0);

        let games = self.engine_root.as_ref().map(|r| r.join("games"));
        let location = self.config.settings.default_location.clone().or(games.clone());
        setting_row(ui, "Default project location", "New Project starts here.", |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let text = location.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(not set)".into());
            let (rect, _) = ui.allocate_exact_size(vec2((ui.available_width() - 200.0).clamp(160.0, 460.0), 30.0), Sense::hover());
            ui.painter().rect(rect, theme::R_S + 1.0, theme::BG_INSET, Stroke::new(1.0_f32, theme::LINE_STRONG));
            widgets::paint_elided(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), &text, theme::body(), theme::TEXT, rect.width() - 16.0, Align2::LEFT_CENTER);
            if widgets::sized_like_field(ui, Id::new("hub-settings-browse"), "Browse…", 92.0).clicked() {
                self.actions.push(HubAction::Browse(BrowseTarget::DefaultLocation));
            }
            let custom = self.config.settings.default_location.is_some();
            let reset = ui.add_enabled_ui(custom, |ui| widgets::sized_like_field(ui, Id::new("hub-settings-reset"), "Reset", 70.0)).inner;
            if reset.on_hover_text("Back to the engine's games/ folder").clicked() {
                self.actions.push(HubAction::SetDefaultLocation(None));
            }
        });
        let mut after = self.config.settings.after_open;
        setting_row(ui, "After opening a project", "Hidden, the hub comes back when the editor closes.", |ui| {
            if theme::segmented(ui, &mut after, &[(AfterOpen::Hide, "Hide the hub", "Hide and come back when the editor closes"), (AfterOpen::Close, "Close the hub", "Close the window; the process exits with the editor")]) {
                self.actions.push(HubAction::SetAfterOpen(after));
            }
        });
        let mut scale = (self.prefs.ui_scale * 100.0).round() as u32;
        setting_row(ui, "Interface scale", "Also used by the editor.", |ui| {
            let options: Vec<(u32, &str, &str)> = SCALES.iter().map(|(value, label)| (*value, *label, *label)).collect();
            if theme::segmented(ui, &mut scale, &options) {
                self.actions.push(HubAction::SetUiScale(scale as f32 / 100.0));
            }
        });
        let accent = self.prefs.accent;
        setting_row(ui, "Accent colour", "Selection, focus and main buttons. Also used by the editor.", |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (index, (name, color)) in theme::ACCENTS.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                let response = ui.interact(rect, Id::new("hub-accent").with(index), Sense::click());
                if index == accent {
                    ui.painter().circle_stroke(rect.center(), 11.5, Stroke::new(1.5_f32, theme::TEXT));
                } else if response.hovered() {
                    ui.painter().circle_stroke(rect.center(), 11.5, Stroke::new(1.0_f32, theme::LINE_STRONG));
                }
                ui.painter().circle_filled(rect.center(), 8.0, *color);
                if response.on_hover_text(*name).clicked() {
                    self.actions.push(HubAction::SetAccent(index));
                }
            }
        });
        let hidden = self.config.projects.iter().filter(|p| p.hidden).count();
        setting_row(ui, "Removed engine games", "Games from games/ that you removed from the list.", |ui| {
            ui.label(egui::RichText::new(format!("{hidden} removed")).color(theme::TEXT_MUTED));
            let restore = ui.add_enabled_ui(hidden > 0, |ui| widgets::sized_like_field(ui, Id::new("hub-settings-restore"), "Restore", 80.0)).inner;
            if restore.clicked() {
                self.actions.push(HubAction::RestoreHidden);
            }
        });
        let file = self.config_dir.as_ref().map(|d| d.join(paths::CONFIG_FILE));
        setting_row(ui, "Project list file", "Projects, pins and times; RON, safe to edit while the hub is closed.", |ui| {
            let text = file.as_ref().map(|f| f.display().to_string()).unwrap_or_else(|| "no config directory (set PISKA_HUB_CONFIG_DIR)".into());
            ui.label(egui::RichText::new(text).font(theme::mono()).color(theme::TEXT_MUTED));
            if let Some(dir) = file.as_ref().and_then(|f| f.parent()).filter(|d| d.exists()) {
                if widgets::icon_button(ui, Id::new("hub-settings-reveal"), Icon::FolderOpen, "Show in File Manager").clicked() {
                    self.actions.push(HubAction::Reveal(dir.to_path_buf()));
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Диалоги
    // -----------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut actions: Vec<HubAction> = Vec::new();
        let frame = egui::Frame::popup(&ctx.style())
            .inner_margin(Margin::same(22.0))
            .rounding(theme::R_L);
        let modal = egui::Modal::new(Id::new("hub-dialog")).frame(frame).show(ctx, |ui| {
            // Своя колонка шириной 560: содержимое окна (прокрутка, плашки) не растягивает его.
            let height = ui.available_height().max(200.0);
            ui.allocate_ui_with_layout(vec2(DIALOG_WIDTH, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(DIALOG_WIDTH);
            ui.spacing_mut().item_spacing.y = 8.0;
            match &mut dialog {
                Dialog::Add { locate, browser } => {
                    let title = match locate {
                        Some(old) => format!("Locate “{}”", old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                        None => "Add Existing Project".to_string(),
                    };
                    dialog_title(ui, Icon::FolderOpen, theme::accent(), &title);
                    dialog_text(ui, "Choose the game folder: the one with Cargo.toml, src/lib.rs and an `engine` dependency.");
                    folder_browser(ui, browser);
                    let chosen = PathBuf::from(browser.path.trim());
                    let check = browser.check();
                    match &check {
                        Ok(info) => {
                            let mut what = vec![info.dimension.label().unwrap_or("Game").to_string()];
                            if info.has_cpp {
                                what.push("C++".into());
                            }
                            let editor = if info.editor_bin.is_some() { "editor program found" } else { "no editor program — Open will explain" };
                            widgets::note_width(ui, Icon::Check, Tone::Ok, &format!("“{}” — {}, {editor}.", info.title, what.join(", ")), DIALOG_WIDTH);
                        }
                        Err(project::ProjectError::NoManifest(_)) if browser.entries.iter().any(|(_, _, crate_dir)| *crate_dir) => {
                            widgets::note_width(ui, Icon::Info, Tone::Neutral, "Choose one of the crates below (marked with a cube).", DIALOG_WIDTH)
                        }
                        Err(err) => widgets::note_width(ui, Icon::Warn, Tone::Warn, &err.to_string(), DIALOG_WIDTH),
                    }
                    let confirm = if locate.is_some() { "Use This Folder" } else { "Add Project" };
                    match dialog_buttons(ui, confirm, check.is_ok(), false) {
                        Some(true) => {
                            actions.push(match locate.take() {
                                Some(old) => HubAction::Locate { old, new: chosen },
                                None => HubAction::Add(chosen),
                            });
                            keep = false;
                        }
                        Some(false) => keep = false,
                        None => {}
                    }
                }
                Dialog::Browse { target, browser } => {
                    dialog_title(ui, Icon::FolderOpen, theme::accent(), "Choose a Folder");
                    dialog_text(ui, "New projects are created inside this folder.");
                    folder_browser(ui, browser);
                    let chosen = PathBuf::from(browser.path.trim());
                    let valid = chosen.is_dir();
                    match dialog_buttons(ui, "Select Folder", valid, false) {
                        Some(true) => {
                            actions.push(HubAction::SetBrowsed { target: *target, path: chosen });
                            keep = false;
                        }
                        Some(false) => keep = false,
                        None => {}
                    }
                }
                Dialog::Delete { path, title, verdict } => {
                    dialog_title(ui, Icon::Trash, theme::ERROR, &format!("Delete “{title}” from disk?"));
                    match verdict.clone() {
                        Ok(()) => {
                            dialog_text(ui, "This folder and everything in it is deleted permanently. This cannot be undone:");
                            ui.label(egui::RichText::new(path.display().to_string()).font(theme::mono()).color(theme::TEXT));
                            match dialog_buttons(ui, "Delete", true, true) {
                                Some(true) => {
                                    actions.push(HubAction::Delete(path.clone()));
                                    keep = false;
                                }
                                Some(false) => keep = false,
                                None => {}
                            }
                        }
                        Err(reason) => {
                            widgets::note_width(ui, Icon::Warn, Tone::Warn, &reason, DIALOG_WIDTH);
                            if dialog_buttons(ui, "", false, false).is_some() {
                                keep = false;
                            }
                        }
                    }
                }
                Dialog::Export { path, title, platform, release } => {
                    dialog_title(ui, Icon::Build, theme::accent(), &format!("Export “{title}”"));
                    let folder = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let mut platforms: Vec<(&'static str, &str, &str)> = vec![("web", "Web", "WebGPU / WebGL2 in the browser")];
                    if cfg!(windows) {
                        platforms.push(("windows", "Windows", "A .exe with the assets next to it"));
                    }
                    if cfg!(target_os = "linux") {
                        platforms.push(("linux", "Linux", "x86_64 build"));
                    }
                    platforms.push(("android", "Android", "APK; needs the SDK, NDK, Java and cargo-ndk"));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Platform").color(theme::TEXT_MUTED));
                        ui.add_space(8.0);
                        theme::segmented(ui, platform, &platforms);
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Build").color(theme::TEXT_MUTED));
                        ui.add_space(24.0);
                        theme::segmented(ui, release, &[(false, "Debug", "Faster to build"), (true, "Release", "Optimized, smaller")]);
                    });
                    dialog_text(
                        ui,
                        &format!(
                            "Runs `cargo run -p piska-export -- {platform} --game {folder}{}`. The result goes to dist/{folder}/{platform}/. \
                             Same as File → Build… in the editor (docs/api/export.md).",
                            if *release { " --release" } else { "" }
                        ),
                    );
                    match dialog_buttons(ui, "Export", true, false) {
                        Some(true) => {
                            actions.push(HubAction::Export {
                                path: path.clone(),
                                platform: platform.to_string(),
                                release: *release,
                            });
                            keep = false;
                        }
                        Some(false) => keep = false,
                        None => {}
                    }
                }
            }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
        self.actions.extend(actions);
    }
}

/// Колонки строки списка.
struct Columns {
    star: f32,
    thumb: f32,
    text: f32,
    opened: f32,
    actions: f32,
    right: f32,
}

impl Columns {
    fn new(rect: Rect) -> Self {
        let left = rect.left() + PAD - 8.0;
        let right = rect.right() - PAD;
        let actions = right - 200.0;
        Self {
            star: left + 8.0,
            thumb: left + 24.0,
            text: left + 24.0 + THUMB.x + 14.0,
            opened: actions - 150.0,
            actions,
            right,
        }
    }
}

/// Метки проекта: 2D / 3D, C++, Scenario, Running, Missing…
fn badges(listed: &Listed, editor_open: bool) -> Vec<(String, Tone)> {
    let mut badges = Vec::new();
    match &listed.info {
        Ok(info) => {
            if editor_open {
                badges.push(("Running".to_string(), Tone::Ok));
            }
            if let Some(d) = info.dimension.label() {
                badges.push((d.to_string(), Tone::Neutral));
            }
            if info.has_cpp {
                badges.push(("C++".to_string(), Tone::Neutral));
            }
            if info.has_scenario {
                badges.push(("Scenario".to_string(), Tone::Neutral));
            }
            if info.standalone {
                badges.push(("Standalone".to_string(), Tone::Neutral));
            }
            if info.editor_bin.is_none() {
                badges.push(("No editor".to_string(), Tone::Warn));
            }
        }
        Err(_) if listed.missing() => badges.push(("Missing".to_string(), Tone::Warn)),
        Err(_) => badges.push(("Not a Piska project".to_string(), Tone::Error)),
    }
    badges
}

/// Меню проекта (правая кнопка и «⋯»).
fn project_menu(ui: &mut Ui, listed: &Listed, editor_pid: Option<u32>, actions: &mut Vec<HubAction>) {
    let path = listed.path.clone();
    let info = listed.info.as_ref().ok();
    let open_reason = match (editor_pid, info) {
        (Some(_), _) => "Already open in the editor".to_string(),
        (None, Some(info)) => info.editor_problem().unwrap_or_default(),
        (None, None) => Hub::problem(listed).unwrap_or_default(),
    };
    let can_open = editor_pid.is_none() && info.is_some_and(|i| i.editor_bin.is_some());
    if item("Open in Editor").icon(Icon::External).enabled_if(can_open, &open_reason).show(ui) {
        actions.push(HubAction::Open(path.clone()));
    }
    let has_editor_build = listed.builds.editor_exe.is_some();
    if item("Open Last Build")
        .icon(Icon::Play)
        .enabled_if(can_open && has_editor_build, "There is no previous build of the editor yet")
        .show(ui)
    {
        actions.push(HubAction::OpenLastBuild(path.clone()));
    }
    if item("Rebuild and Open").icon(Icon::Build).enabled_if(can_open, &open_reason).show(ui) {
        actions.push(HubAction::RebuildAndOpen(path.clone()));
    }
    theme::menu_separator(ui);
    let can_play = info.is_some_and(|i| i.game_bin.is_some());
    if item("Play").icon(Icon::Play).enabled_if(can_play, "No game program (src/main.rs)").show(ui) {
        actions.push(HubAction::Play(path.clone()));
    }
    if item("Rebuild and Play").icon(Icon::Build).enabled_if(can_play, "No game program (src/main.rs)").show(ui) {
        actions.push(HubAction::RebuildAndPlay(path.clone()));
    }
    let exportable = info.is_some() && listed.source == Source::Workspace;
    if item("Export…")
        .icon(Icon::Build)
        .enabled_if(exportable, "piska-export builds games from the engine's games/ folder")
        .show(ui)
    {
        actions.push(HubAction::ShowExport(path.clone()));
    }
    theme::menu_separator(ui);
    if item("Show in File Manager").icon(Icon::FolderOpen).enabled_if(path.exists(), "The folder does not exist").show(ui) {
        actions.push(HubAction::Reveal(path.clone()));
    }
    if item(if listed.favorite { "Unpin" } else { "Pin to Top" }).icon(Icon::ChevronUp).show(ui) {
        actions.push(HubAction::TogglePin(path.clone()));
    }
    if listed.missing() && item("Locate…").icon(Icon::Search).show(ui) {
        actions.push(HubAction::ShowAdd { locate: Some(path.clone()) });
    }
    theme::menu_separator(ui);
    if item("Remove from List").icon(Icon::Close).show(ui) {
        actions.push(HubAction::Remove(path.clone()));
    }
    if item("Delete from Disk…")
        .tinted(Icon::Trash, theme::ERROR)
        .enabled_if(info.is_some(), "Only a valid project folder can be deleted from here")
        .show(ui)
    {
        actions.push(HubAction::AskDelete(path));
    }
}

/// Переключатель из иконок (вид списка).
fn icon_segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, Icon, &str)]) -> bool {
    let mut changed = false;
    let cell = vec2(30.0, 26.0);
    let (rect, _) = ui.allocate_exact_size(vec2(cell.x * options.len() as f32, cell.y), Sense::hover());
    ui.painter().rect(rect, theme::R_S + 1.0, theme::BG_INSET, Stroke::new(1.0_f32, theme::LINE_STRONG));
    // Справа налево раскладка кладёт ячейки в обратном порядке — рисуем слева направо.
    for (i, (option, icon, tooltip)) in options.iter().rev().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * cell.x, rect.top()), cell);
        let response = ui.interact(r, ui.id().with(("icon-seg", *tooltip)), Sense::click()).on_hover_text(*tooltip);
        let selected = *value == *option;
        if selected {
            ui.painter().rect_filled(r.shrink(2.0), theme::R_S, theme::accent_soft());
        } else if response.hovered() {
            ui.painter().rect_filled(r.shrink(2.0), theme::R_S, theme::BG_HOVER);
        }
        let color = if selected { Color32::WHITE } else { theme::TEXT_MUTED };
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), *icon, color);
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// Строка настроек: подпись с пояснением слева, управление справа.
fn setting_row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    let label_w = 280.0;
    ui.painter().text(pos2(rect.left(), rect.center().y - 9.0), Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    widgets::paint_elided(
        ui.painter(),
        pos2(rect.left(), rect.center().y + 11.0),
        hint,
        theme::caption(),
        theme::TEXT_MUTED,
        label_w - 16.0,
        Align2::LEFT_CENTER,
    );
    let control = Rect::from_min_max(pos2(rect.left() + label_w, rect.top()), rect.max);
    let mut child = ui.new_child(UiBuilder::new().max_rect(control).layout(Layout::left_to_right(Align::Center)));
    add(&mut child);
}

fn dialog_title(ui: &mut Ui, icon: Icon, color: Color32, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(20.0, 22.0), Sense::hover());
        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(17.0, 17.0)), icon, color);
        ui.label(egui::RichText::new(title).font(theme::strong(theme::FONT_HEADING)).color(theme::TEXT));
    });
    ui.add_space(2.0);
}

fn dialog_text(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT_MUTED)).wrap());
}

/// Кнопки диалога: `Some(true)` — подтвердили, `Some(false)` — отменили.
/// Пустое `confirm` — только кнопка Close.
fn dialog_buttons(ui: &mut Ui, confirm: &str, enabled: bool, danger: bool) -> Option<bool> {
    ui.add_space(10.0);
    let mut result = None;
    ui.allocate_ui_with_layout(vec2(DIALOG_WIDTH, 34.0), Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if !confirm.is_empty() {
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    if danger {
                        theme::danger_button(ui, confirm)
                    } else {
                        theme::primary_button(ui, confirm)
                    }
                })
                .inner;
            if response.clicked() {
                result = Some(true);
            }
        }
        let cancel = if confirm.is_empty() { "Close" } else { "Cancel" };
        if theme::secondary_button(ui, cancel).clicked() {
            result = Some(false);
        }
    });
    if enabled && !confirm.is_empty() && !danger && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        result = Some(true);
    }
    result
}

/// Обзор каталогов: путь, «вверх», список подкаталогов (проекты Piska отмечены).
fn folder_browser(ui: &mut Ui, browser: &mut FolderBrowser) {
    // Ширина задаётся явно: внутри окна-диалога прокрутка иначе растягивается на экран.
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if widgets::icon_button(ui, Id::new("hub-browse-up"), Icon::ChevronUp, "Up one level").clicked() {
            browser.up();
        }
        let width = ui.available_width();
        widgets::text_field(ui, Id::new("hub-browse-path"), &mut browser.path, "Folder path", width);
    });
    browser.refresh();
    let entries = browser.entries.clone();
    let mut enter: Option<PathBuf> = None;
    let mut pick: Option<PathBuf> = None;
    egui::Frame::none()
        .fill(theme::BG_INSET)
        .stroke(Stroke::new(1.0_f32, theme::LINE))
        .rounding(theme::R_S)
        .inner_margin(Margin::same(4.0))
        .show(ui, |ui| {
            ui.set_width(width - 10.0);
            egui::ScrollArea::vertical()
                .id_salt("hub-browse-list")
                .max_width(width - 10.0)
                .min_scrolled_height(240.0)
                .max_height(240.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if entries.is_empty() {
                        ui.label(egui::RichText::new("  No subfolders").color(theme::TEXT_DISABLED));
                    }
                    for (i, (name, path, is_project)) in entries.iter().enumerate() {
                        let (rect, _) = ui.allocate_exact_size(vec2(width - 18.0, 26.0), Sense::hover());
                        let response = ui.interact(rect, Id::new("hub-browse-entry").with(i), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect, theme::R_S, theme::hover_fill());
                        }
                        let icon = if *is_project { Icon::Cube } else { Icon::Folder };
                        let tint = if *is_project { theme::accent() } else { theme::TEXT_MUTED };
                        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(14.0, 14.0)), icon, tint);
                        ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
                        if *is_project {
                            ui.painter().text(
                                pos2(rect.right() - 8.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                "Rust crate",
                                theme::caption(),
                                theme::TEXT_MUTED,
                            );
                        }
                        if response.double_clicked() {
                            enter = Some(path.clone());
                        } else if response.clicked() {
                            pick = Some(path.clone());
                        }
                    }
                });
        });
    ui.label(egui::RichText::new("Click a folder to choose it, double-click to open it.").font(theme::caption()).color(theme::TEXT_DISABLED));
    if let Some(dir) = enter {
        browser.enter(&dir);
    } else if let Some(dir) = pick {
        // Выбор показывает путь в поле; содержимое — после двойного щелчка.
        browser.path = dir.display().to_string();
        browser.listed = Some(dir);
    }
}

/// «Built 12 min ago» / «Needs build» / «Never built» — состояние программы редактора.
fn build_label(listed: &Listed, now: u64) -> (String, Color32) {
    use crate::builds::BuildState;
    if listed.info.as_ref().map_or(true, |i| i.editor_bin.is_none()) {
        return (String::new(), theme::TEXT_MUTED);
    }
    match listed.builds.editor {
        BuildState::Fresh { built } => (
            format!("Built {}", crate::config::ago(Some(built), now).to_lowercase()),
            theme::TEXT_DISABLED,
        ),
        BuildState::Stale { .. } => ("Needs build".to_string(), theme::WARN),
        BuildState::Never => ("Never built".to_string(), theme::TEXT_DISABLED),
    }
}

/// Плашка с заголовком и пояснением; `detail` — первая ошибка компилятора (место и текст).
fn alert(ui: &mut Ui, tone: Tone, icon: Icon, title: &str, body: &str, detail: Option<&crate::cargo::Diagnostic>) {
    let color = match tone {
        Tone::Error => theme::ERROR,
        Tone::Warn => theme::WARN,
        Tone::Ok => theme::OK,
        _ => theme::accent(),
    };
    let width = ui.available_width();
    egui::Frame::none()
        .fill(theme::mix(theme::BG_PANEL, color, 0.10))
        .stroke(Stroke::new(1.0_f32, theme::mix(theme::BG_PANEL, color, 0.45)))
        .rounding(theme::R_M)
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(width - 30.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), icon, color);
                ui.label(egui::RichText::new(title).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
            });
            ui.add_space(2.0);
            ui.add(egui::Label::new(egui::RichText::new(body).color(theme::TEXT_MUTED)).wrap());
            if let Some(error) = detail {
                ui.add_space(6.0);
                let place = match (&error.package, error.location()) {
                    (Some(package), location) if !location.is_empty() => format!("{package}  ·  {location}"),
                    (Some(package), _) => package.clone(),
                    (None, location) => location,
                };
                ui.label(egui::RichText::new(place).font(theme::mono()).color(theme::mix(color, Color32::WHITE, 0.35)));
                ui.add(egui::Label::new(egui::RichText::new(&error.message).color(theme::TEXT)).wrap());
            }
        });
}

/// Подпись группы внизу навигации.
fn nav_caption(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    ui.painter()
        .text(pos2(rect.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, text, theme::strong(10.5), theme::TEXT_DISABLED);
}

/// Файл из сообщения компилятора: абсолютный, относительно каталога сборки или движка.
fn resolve_source(file: &str, cwd: &Path, engine_root: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(file);
    if path.is_absolute() {
        return path;
    }
    let candidates = [Some(cwd.join(&path)), engine_root.map(|root| root.join(&path))];
    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| cwd.join(path))
}
