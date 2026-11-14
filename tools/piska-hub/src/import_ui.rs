//! Интерфейс импорта проекта и сохранения проекта в файл: страница «Import Project»
//! (источники, предварительный отчёт, настройки, ход, результат), окно «Save Project As
//! File…» и строки фоновых задач над строкой состояния.

use engine_editor::kit::i18n::brand;
use engine_editor::kit::i18n::{tr, tr_s, trf, trn};
use std::path::PathBuf;

use egui::{pos2, vec2, Align, Align2, Color32, Id, Layout, Margin, Rect, Sense, Stroke, Ui, UiBuilder};
use engine_editor::kit::icons::{self, Icon};
use engine_editor::kit::theme;

use crate::hub::{BrowseTarget, Hub, HubAction, Page};
use crate::import::{self, ChangeCode, Level, Mode, Outcome, Report, Source};
use crate::import_flow::{ImportStage, PackDialog};
use crate::tasks::{Work, WorkKind, WorkState};
use crate::ui::PAD;
use crate::widgets::{self, Tone};

/// Ширина колонки подписей формы.
const LABEL_W: f32 = 132.0;

impl Hub {
    pub const IMPORT_PATH_ID: &'static str = "hub-import-path";
    pub const IMPORT_RUN_ID: &'static str = "hub-import-run";
    pub const IMPORT_NAME_ID: &'static str = "hub-import-name";
    pub const IMPORT_FOLDER_ID: &'static str = "hub-import-browse-folder";
    pub const IMPORT_FILE_ID: &'static str = "hub-import-browse-file";
    pub const IMPORT_ADVANCED_ID: &'static str = "hub-import-advanced";
    pub const PACK_PATH_ID: &'static str = "hub-pack-path";

    /// Идентификатор строки источника на странице импорта.
    pub fn import_row_id(index: usize) -> Id {
        Id::new("hub-import-row").with(index)
    }

    /// Страница «Import Project».
    pub(crate) fn import_page_ui(&mut self, ui: &mut Ui) {
        let subtitle = match self.import.stage {
            ImportStage::Choose => tr_s("Nothing is written to disk until you press Import."),
            ImportStage::Running => tr_s("Importing in the background; the hub stays usable."),
            ImportStage::Result => tr_s("What was imported, what was changed and what to do next."),
        };
        self.page_header(ui, tr_s("Import Project"), subtitle, |_, _| {});
        widgets::separator(ui);
        match self.import.stage {
            ImportStage::Choose => self.import_choose(ui),
            ImportStage::Running => self.import_running(ui),
            ImportStage::Result => self.import_result(ui),
        }
    }

    // -----------------------------------------------------------------------
    // Выбор источников и настройки
    // -----------------------------------------------------------------------

    fn import_choose(&mut self, ui: &mut Ui) {
        // Нижняя полоса с кнопками — отдельным блоком, чтобы остальное прокручивалось.
        let full = ui.available_rect_before_wrap();
        let bar_h = 56.0;
        let body = Rect::from_min_max(full.min, pos2(full.right(), full.bottom() - bar_h)).shrink2(vec2(PAD, 0.0));
        let bar = Rect::from_min_max(pos2(full.left(), full.bottom() - bar_h), full.max);
        let left_w = (body.width() * 0.36).clamp(250.0, 380.0);
        let left = Rect::from_min_max(body.min + vec2(0.0, 16.0), pos2(body.left() + left_w, body.bottom()));
        let right = Rect::from_min_max(pos2(left.right() + 24.0, body.top() + 16.0), body.max);

        let mut column = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::top_down(Align::Min)));
        self.import_sources(&mut column);
        let mut form = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::top_down(Align::Min)));
        self.import_details(&mut form);
        let mut footer = ui.new_child(UiBuilder::new().max_rect(bar).layout(Layout::top_down(Align::Min)));
        self.import_bar(&mut footer);
        ui.allocate_rect(full, Sense::hover());
    }

    /// Левая колонка: поле пути, кнопки обзора и список источников.
    fn import_sources(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        widgets::section_label(ui, tr_s("Sources"));
        ui.add_space(theme::SP_3);
        // Путь: Enter добавляет, Esc — назад к проектам.
        let field = widgets::text_field(ui, Id::new(Self::IMPORT_PATH_ID), &mut self.import.path, tr_s("Folder or project file path"), width);
        if std::mem::take(&mut self.import.focus_path) {
            field.request_focus();
        }
        // В однострочном поле Enter снимает фокус: ловим потерю фокуса вместе с Enter.
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let text = self.import.path.trim().trim_matches('"').to_string();
            if !text.is_empty() {
                self.import.path.clear();
                self.push(HubAction::ImportAdd(PathBuf::from(text)));
                field.request_focus();
            }
        }
        if self.dialog.is_none() && !field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.push(HubAction::Navigate(Page::Projects));
        }
        ui.add_space(theme::SP_3);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let half = (width - 6.0) / 2.0;
            if widgets::button_sized(ui, Id::new(Self::IMPORT_FOLDER_ID), tr_s("Folder…"), half).clicked() {
                self.push(HubAction::Browse(BrowseTarget::ImportFolder));
            }
            if widgets::button_sized(ui, Id::new(Self::IMPORT_FILE_ID), tr_s("Project file…"), half).clicked() {
                self.push(HubAction::Browse(BrowseTarget::ImportFile));
            }
        });
        ui.add_space(theme::SP_3);
        ui.add(
            egui::Label::new(
                egui::RichText::new(trf!(
                    "Paste a path and press Enter, or drop a folder or a .{} file on the window. A folder with several projects is scanned: tick the ones to import.",
                    brand::PROJECT_EXTENSION
                ))
                .font(theme::caption())
                .color(theme::TEXT_MUTED),
            )
            .wrap(),
        );
        if let Some(message) = self.import.message.clone() {
            ui.add_space(theme::SP_3);
            widgets::note_width(ui, Icon::Warn, Tone::Warn, &message, width);
        }
        if let Some(dir) = self.import.scanning.clone() {
            ui.add_space(theme::SP_3);
            ui.label(egui::RichText::new(trf!("Looking for projects in {}…", dir.display())).font(theme::caption()).color(theme::TEXT_MUTED));
        }
        ui.add_space(theme::SP_4);
        if self.import.items.is_empty() {
            if self.import.scanning.is_none() {
                theme::empty_state(
                    ui,
                    Icon::FolderOpen,
                    tr_s("Nothing to import yet"),
                    tr_s("Add a project folder, a folder with several projects, or a project file."),
                );
            }
            return;
        }
        let count = self.import.items.len();
        ui.label(egui::RichText::new(trn!("{n} source", "{n} sources", count)).font(theme::caption()).color(theme::TEXT_DISABLED));
        ui.add_space(theme::SP_1);
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("hub-import-sources")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for index in 0..count {
                    let problems = self.import_problems(index);
                    let item = &self.import.items[index];
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                    let response = ui.interact(rect, Self::import_row_id(index), Sense::click());
                    let selected = self.import.selected == index;
                    theme::list_row(ui.painter(), rect, selected, true, response.hovered());
                    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
                    // Флажок.
                    let box_rect = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(16.0, 16.0));
                    let box_response = ui.interact(box_rect.expand(4.0), Id::new("hub-import-tick").with(index), Sense::click());
                    let ticked = item.ticked;
                    ui.painter().rect(
                        box_rect,
                        theme::R_S,
                        if ticked { theme::accent() } else { theme::BG_INSET },
                        Stroke::new(1.0_f32, if ticked { theme::accent() } else { theme::LINE_STRONG }),
                    );
                    if ticked {
                        icons::paint(ui.painter(), box_rect.shrink(2.0), Icon::Check, Color32::WHITE);
                    }
                    let icon = if item.source.is_archive() { Icon::Save } else { Icon::Folder };
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 42.0, rect.center().y), vec2(16.0, 16.0)), icon, theme::TEXT_MUTED);
                    let title = item.report.as_ref().map(|r| r.title.clone()).unwrap_or_else(|| item.name.clone());
                    let text_left = rect.left() + 58.0;
                    let right_pad = 62.0;
                    widgets::paint_elided(
                        ui.painter(),
                        pos2(text_left, rect.center().y - 8.0),
                        &title,
                        theme::body(),
                        theme::TEXT,
                        rect.right() - text_left - right_pad,
                        Align2::LEFT_CENTER,
                    );
                    widgets::paint_elided(
                        ui.painter(),
                        pos2(text_left, rect.center().y + 9.0),
                        &item.source.path().display().to_string(),
                        theme::caption(),
                        theme::TEXT_MUTED,
                        rect.right() - text_left - right_pad,
                        Align2::LEFT_CENTER,
                    );
                    // Состояние справа.
                    let status_center = pos2(rect.right() - 36.0, rect.center().y);
                    match &item.report {
                        None => {
                            ui.painter().text(status_center, Align2::CENTER_CENTER, tr_s("Reading…"), theme::caption(), theme::TEXT_DISABLED);
                        }
                        Some(_) => {
                            let worst = problems.iter().map(|p| p.level).max();
                            let (icon, color) = match worst {
                                Some(Level::Error) => (Icon::Error, theme::ERROR),
                                Some(Level::Warn) => (Icon::Warn, theme::WARN),
                                _ => (Icon::Check, theme::OK),
                            };
                            icons::paint(ui.painter(), Rect::from_center_size(status_center, vec2(14.0, 14.0)), icon, color);
                        }
                    }
                    let close = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(16.0, 16.0));
                    let close_response = ui.interact(close.expand(3.0), Id::new("hub-import-remove").with(index), Sense::click());
                    if response.hovered() || close_response.hovered() {
                        icons::paint(ui.painter(), close.shrink(3.0), Icon::Close, if close_response.hovered() { theme::TEXT } else { theme::TEXT_DISABLED });
                    }
                    if close_response.on_hover_text(tr_s("Remove from this import")).clicked() {
                        remove = Some(index);
                    } else if box_response.clicked() {
                        self.import.items[index].ticked = !ticked;
                    } else if response.clicked() {
                        self.import.selected = index;
                    }
                }
            });
        if let Some(index) = remove {
            self.push(HubAction::ImportRemove(index));
        }
    }

    /// Правая колонка: отчёт и настройки выбранного источника.
    fn import_details(&mut self, ui: &mut Ui) {
        let index = self.import.selected.min(self.import.items.len().saturating_sub(1));
        let Some(item) = self.import.items.get(index) else {
            return;
        };
        let source = item.source.clone();
        let Some(report) = item.report.clone() else {
            ui.label(egui::RichText::new(tr_s("Reading the project…")).color(theme::TEXT_MUTED));
            return;
        };
        let problems = self.import_problems(index);
        egui::ScrollArea::vertical()
            .id_salt("hub-import-details")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width() - 10.0;
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 6.0;
                let name_error = self.import.items.get(index).and_then(|i| crate::templates::validate_name(&i.name).err()).map(|e| e.to_string());
                self.import_report(ui, &report, &problems, name_error, width);
                ui.add_space(theme::SP_4);
                self.import_settings_ui(ui, index, &report, &source, width);
                ui.add_space(theme::SP_12);
            });
    }

    fn import_report(&mut self, ui: &mut Ui, report: &Report, problems: &[import::Problem], name_error: Option<String>, width: f32) {
        widgets::section_label(ui, tr_s("Pre-flight report"));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&report.title).font(theme::strong(15.0)).color(theme::TEXT));
            if let Some(d) = report.dimension {
                widgets::paint_badge_inline(ui, d, Tone::Neutral);
            }
            if report.facts.cmake || report.counts.cpp_files > 0 {
                widgets::paint_badge_inline(ui, "C++", Tone::Neutral);
            }
            if report.counts.scenarios > 0 {
                widgets::paint_badge_inline(ui, "Scenario", Tone::Neutral);
            }
        });
        let kind = if report.source.is_archive() { tr_s("Project file") } else { tr_s("Folder") };
        key_value(ui, kind, &report.source.path().display().to_string(), true);
        let made = match &report.made_with {
            Some(v) if v == import::ENGINE_VERSION => trf!("{v} (this engine)", v = v),
            Some(v) => trf!("{v}; this engine is {now}", v = v, now = import::ENGINE_VERSION),
            None => trf!("not recorded; this engine is {now}", now = import::ENGINE_VERSION),
        };
        key_value(ui, tr_s("Made with engine"), &made, false);
        let c = &report.counts;
        let mut parts = vec![
            trn!("{n} scene", "{n} scenes", c.scenes as usize),
            trn!("{n} prefab", "{n} prefabs", c.prefabs as usize),
            trn!("{n} Rust script", "{n} Rust scripts", c.rust_scripts as usize),
            trn!("{n} C++ file", "{n} C++ files", c.cpp_files as usize),
            trn!("{n} Scenario script", "{n} Scenario scripts", c.scenarios as usize),
        ];
        parts.retain(|p| !p.starts_with("0 "));
        let contents = if parts.is_empty() { tr_s("no scenes or scripts yet").to_string() } else { parts.join("  ·  ") };
        key_value(ui, tr_s("Contents"), &contents, false);
        key_value(
            ui,
            tr_s("Size"),
            &trf!("{files} files  ·  {total}  ·  assets {assets}", files = c.files, total = import::human(c.bytes), assets = import::human(c.assets_bytes)),
            false,
        );
        ui.add_space(theme::SP_1);
        if problems.is_empty() {
            widgets::note_width(ui, Icon::Check, Tone::Ok, tr_s("No problems found."), width);
        }
        let mut sorted: Vec<&import::Problem> = problems.iter().collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.level));
        for problem in sorted {
            let text = problem.text();
            // Ошибка имени уже стоит под полем имени — второй раз её не повторяем.
            if problem.code == import::Code::NameInvalid && name_error.as_deref() == Some(text.as_str()) {
                continue;
            }
            match problem.level {
                Level::Error => widgets::note_width(ui, Icon::Error, Tone::Error, &text, width),
                Level::Warn => widgets::note_width(ui, Icon::Warn, Tone::Warn, &text, width),
                // Справочные замечания — строками без рамки: отчёт не вырастает в простыню.
                Level::Info => {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), Icon::Info, theme::TEXT_DISABLED);
                        ui.add(egui::Label::new(egui::RichText::new(&text).font(theme::caption()).color(theme::TEXT_MUTED)).wrap());
                    });
                }
            }
        }
        if !report.skipped.is_empty() {
            theme::foldout(ui, Id::new("hub-import-skipped"), &trf!("Left out of the copy ({})", report.skipped.len()), false, |ui| {
                for (path, reason) in report.skipped.iter().take(40) {
                    ui.label(egui::RichText::new(format!("{path}  —  {}", tr(reason))).font(theme::mono()).color(theme::TEXT_MUTED));
                }
                if report.skipped.len() > 40 {
                    ui.label(egui::RichText::new(trf!("… and {} more", report.skipped.len() - 40)).font(theme::caption()).color(theme::TEXT_DISABLED));
                }
            });
        }
    }

    fn import_settings_ui(&mut self, ui: &mut Ui, index: usize, report: &Report, source: &Source, width: f32) {
        widgets::section_label(ui, tr_s("Settings"));
        let archive = source.is_archive();
        // Узкая колонка (маленькое окно, крупный масштаб, русский текст): подпись над полем.
        let field_w = form_control_width(width);
        // Имя.
        let original = report.package.clone();
        let mut name = self.import.items[index].name.clone();
        let in_place = self.import.settings.mode == Mode::InPlace && !archive;
        form_row(ui, tr_s("Name"), tr_s("Crate and folder name: lowercase letters, digits, _ and -. The same rules as New Project."), |ui| {
            ui.add_enabled_ui(!in_place, |ui| {
                widgets::text_field(ui, Id::new(Self::IMPORT_NAME_ID), &mut name, &original, field_w);
            });
        });
        if name != self.import.items[index].name {
            self.import.items[index].name = name.clone();
            self.import.items[index].name_touched = true;
        }
        let name_problem = (!in_place).then(|| crate::templates::validate_name(&name).err()).flatten();
        if let Some(problem) = name_problem {
            ui.horizontal(|ui| {
                ui.add_space(form_indent(width));
                ui.label(egui::RichText::new(problem.to_string()).font(theme::caption()).color(theme::ERROR));
            });
        }
        // Как.
        if archive {
            form_row(ui, tr_s("How"), tr_s("The project file is unpacked into a new folder; the file itself is not changed."), |ui| {
                ui.label(egui::RichText::new(tr_s("Unpack into a folder")).color(theme::TEXT));
            });
        } else {
            let mut mode = self.import.settings.mode;
            form_row(
                ui,
                tr_s("How"),
                tr_s("Copy makes an independent project in the folder below; Use in place only adds the existing folder to the list and changes nothing in it."),
                |ui| {
                    theme::segmented(
                        ui,
                        &mut mode,
                        &[
                            (Mode::Copy, tr_s("Copy into folder"), tr_s("Copy the project; the original is never touched")),
                            (Mode::InPlace, tr_s("Use in place"), tr_s("Reference the folder where it is")),
                        ],
                    );
                },
            );
            self.import.settings.mode = mode;
        }
        // Куда.
        let mut location = self.import.settings.location.display().to_string();
        let browse_w = 84.0;
        let hint = match &self.engine_root {
            Some(root) if paths_same(&self.import.settings.location, &root.join("games")) => {
                tr_s("Inside the engine's games/: the project joins the workspace and shares its build cache.")
            }
            _ => tr_s("A folder named after the project is created here. Outside the engine the project gets its own Cargo workspace."),
        };
        form_row(ui, tr_s("Destination"), hint, |ui| {
            ui.add_enabled_ui(!in_place, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    widgets::text_field(ui, Id::new("hub-import-location"), &mut location, tr_s("Folder for the project"), field_w - browse_w - 6.0);
                    if widgets::button_sized(ui, Id::new("hub-import-browse-location"), tr_s("Browse…"), browse_w).clicked() {
                        self.actions.push(HubAction::Browse(BrowseTarget::ImportLocation));
                    }
                });
            });
        });
        if location != self.import.settings.location.display().to_string() {
            self.import.settings.location = PathBuf::from(location.trim());
        }
        if !in_place {
            let dest = import::destination(&self.import.settings, &name);
            ui.horizontal(|ui| {
                ui.add_space(form_indent(width));
                ui.add(egui::Label::new(egui::RichText::new(trf!("Result: {}", dest.display())).font(theme::caption()).color(theme::TEXT_MUTED)).wrap());
            });
        }
        // После импорта.
        let settings = &mut self.import.settings;
        form_row(ui, tr_s("After import"), tr_s("The project is always added to the project list."), |ui| {
            ui.vertical(|ui| {
                ui.add_enabled(false, egui::Checkbox::new(&mut true.clone(), tr_s("Add to the project list")));
                ui.checkbox(&mut settings.open, tr_s("Open in the editor"));
                ui.checkbox(&mut settings.build, tr_s("Build the project's editor in the background"));
                ui.checkbox(&mut settings.test, tr_s("Run the project's tests"));
            });
        });
        ui.add_space(theme::SP_2);
        // Advanced.
        theme::foldout(ui, Id::new(Self::IMPORT_ADVANCED_ID), tr_s("Advanced"), self.import.advanced, |ui| {
            let settings = &mut self.import.settings;
            let changed_name = name != original;
            form_row(
                ui,
                tr_s("Rename inside"),
                tr_s("Update Cargo.toml (package, programs, title), `use` paths in sources and tests, GAME_NAME and the README to the new name. Applies when the name differs from the original."),
                |ui| {
                    ui.add_enabled(!in_place && changed_name, egui::Checkbox::new(&mut settings.rename_inside, tr_s("Rename in sources")));
                },
            );
            form_row(
                ui,
                tr_s("Engine dependency"),
                tr_s("“Use this engine” points the project at this engine checkout; “Keep as is” leaves Cargo.toml untouched (that path may not exist on this computer)."),
                |ui| {
                    ui.add_enabled_ui(!in_place, |ui| {
                        theme::segmented(
                            ui,
                            &mut settings.engine,
                            &[
                                (import::EngineDep::Rewrite, tr_s("Use this engine"), tr_s("Point the engine dependency at this checkout")),
                                (import::EngineDep::Keep, tr_s("Keep as is"), tr_s("Do not touch the dependency paths")),
                            ],
                        );
                    });
                },
            );
            form_row(
                ui,
                tr_s("If files exist"),
                tr_s("Only when the destination folder already has files: keep yours, replace them, or save the new file next to yours as name.imported.ext."),
                |ui| {
                    ui.add_enabled_ui(!in_place, |ui| {
                        theme::segmented(
                            ui,
                            &mut settings.conflict,
                            &[
                                (engine_project::Conflict::Skip, tr_s("Skip"), tr_s("Keep the existing file")),
                                (engine_project::Conflict::Overwrite, tr_s("Overwrite"), tr_s("Replace the existing file")),
                                (engine_project::Conflict::KeepBoth, tr_s("Keep both"), tr_s("Save the new file as name.imported.ext")),
                            ],
                        );
                    });
                },
            );
            form_row(
                ui,
                tr_s("Asset GUIDs"),
                tr_s("Give every asset a new GUID and update the references. Use it for a copy that lives next to its original."),
                |ui| {
                    ui.add_enabled(!in_place, egui::Checkbox::new(&mut settings.regenerate_guids, tr_s("Regenerate GUIDs")));
                },
            );
            let c = &report.counts;
            form_row(ui, tr_s("Include"), tr_s("Parts of the project to bring along. Source, assets and settings are always included."), |ui| {
                ui.add_enabled_ui(!in_place, |ui| {
                    ui.vertical(|ui| {
                        ui.checkbox(&mut settings.tests, trn!("Tests ({n} file)", "Tests ({n} files)", c.tests as usize));
                        ui.checkbox(&mut settings.cpp, trf!("C++ sources ({})", import::human(c.cpp_bytes)));
                        ui.checkbox(&mut settings.builds, trf!("Exported builds ({})", import::human(c.builds_bytes)));
                        ui.checkbox(&mut settings.ide, tr_s("Editor and IDE settings (.vscode, .idea)"));
                    });
                });
            });
            form_row(ui, tr_s("Line endings"), tr_s("Files are copied byte for byte; line endings are never converted."), |ui| {
                ui.label(egui::RichText::new(tr_s("Never converted")).color(theme::TEXT_MUTED));
            });
        });
        // Состояние раздела запоминаем для следующего открытия страницы.
        self.import.advanced = ui.data_mut(|d| d.get_persisted::<bool>(Id::new(Self::IMPORT_ADVANCED_ID))).unwrap_or(self.import.advanced);
    }

    /// Нижняя полоса: сводка и кнопки.
    fn import_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let ticked = self.import.ticked().count();
        let ready = self.import_ready();
        let ticked_indices: Vec<usize> = (0..self.import.items.len()).filter(|i| self.import.items[*i].ticked).collect();
        let blockers: usize = ticked_indices
            .into_iter()
            .map(|i| self.import_problems(i).iter().filter(|p| p.level == Level::Error).count())
            .sum();
        let waiting = self.import.items.iter().any(|i| i.ticked && i.report.is_none());
        let (text, color) = if self.import.items.is_empty() {
            (tr_s("Add a source to begin.").to_string(), theme::TEXT_MUTED)
        } else if ticked == 0 {
            (tr_s("Tick at least one project to import.").to_string(), theme::TEXT_MUTED)
        } else if waiting {
            (tr_s("Reading the projects…").to_string(), theme::TEXT_MUTED)
        } else if blockers > 0 {
            (trn!("{n} problem blocks the import; see the report", "{n} problems block the import; see the report", blockers), theme::ERROR)
        } else {
            (trn!("{n} project ready to import", "{n} projects ready to import", ticked), theme::TEXT_MUTED)
        };
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.left() + PAD, rect.top() + 10.0), pos2(rect.right() - PAD, rect.bottom() - 10.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 8.0;
        let label = if ticked > 1 { trf!("Import {n} Projects", n = ticked) } else { tr_s("Import").to_string() };
        let run = row
            .add_enabled_ui(ready, |ui| widgets::action_button(ui, Id::new(Self::IMPORT_RUN_ID), Some(Icon::Plus), &label, true))
            .inner
            .on_hover_text(tr_s("Copy or unpack the ticked projects (Enter)"));
        if run.clicked() {
            self.push(HubAction::ImportRun);
        }
        if widgets::action_button(&mut row, Id::new("hub-import-cancel"), None, tr_s("Cancel"), false).clicked() {
            self.push(HubAction::Navigate(Page::Projects));
        }
        row.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).font(theme::caption()).color(color)).truncate());
        });
        // Enter запускает, когда фокус не в поле ввода.
        if ready && self.dialog.is_none() && !ui.ctx().wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.push(HubAction::ImportRun);
        }
    }

    // -----------------------------------------------------------------------
    // Ход и результат
    // -----------------------------------------------------------------------

    fn import_running(&mut self, ui: &mut Ui) {
        let Some(work) = self.import.work.and_then(|id| self.works.iter().find(|w| w.id == id)) else {
            theme::empty_state(ui, Icon::Refresh, tr_s("Finishing…"), "");
            return;
        };
        let (stage, progress, fraction, elapsed) = (work.stage.clone(), work.progress.clone(), work.fraction(), work.started.elapsed().as_secs());
        let cancelling = work.control.is_cancelled();
        ui.add_space(theme::SP_12 + theme::SP_8);
        let width = (ui.available_width() - 2.0 * PAD).clamp(300.0, 640.0);
        ui.horizontal(|ui| {
            ui.add_space(PAD);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label(egui::RichText::new(if cancelling { tr_s("Cancelling…") } else { tr_s("Importing…") }).font(theme::strong(17.0)).color(theme::TEXT));
                ui.add_space(theme::SP_2);
                ui.label(egui::RichText::new(if stage.is_empty() { tr_s("Starting…").to_string() } else { stage }).color(theme::TEXT_MUTED));
                ui.add_space(theme::SP_4);
                widgets::progress_bar(ui, fraction, false);
                ui.add_space(theme::SP_3);
                if let Some(p) = &progress {
                    ui.label(
                        egui::RichText::new(trf!(
                            "{done} / {total} files  ·  {bytes} of {all}  ·  {time}",
                            done = p.files_done,
                            total = p.files_total,
                            bytes = import::human(p.bytes_done),
                            all = import::human(p.bytes_total),
                            time = widgets::clock(elapsed)
                        ))
                        .font(theme::caption())
                        .color(theme::TEXT_MUTED),
                    );
                    ui.add(egui::Label::new(egui::RichText::new(&p.current).font(theme::mono()).color(theme::TEXT_DISABLED)).truncate());
                }
                ui.add_space(theme::SP_6);
                ui.horizontal(|ui| {
                    if widgets::action_button(ui, Id::new("hub-import-stop"), None, tr_s("Cancel"), false).clicked() {
                        self.push(HubAction::ImportCancel);
                    }
                    if widgets::action_button(ui, Id::new("hub-import-background"), None, tr_s("Continue in Background"), false).clicked() {
                        self.push(HubAction::Navigate(Page::Projects));
                    }
                });
                ui.add_space(theme::SP_4);
                ui.label(
                    egui::RichText::new(tr_s("Cancelling leaves no half-written project: files are prepared in a temporary folder and moved into place at the end."))
                        .font(theme::caption())
                        .color(theme::TEXT_DISABLED),
                );
            });
        });
    }

    fn import_result(&mut self, ui: &mut Ui) {
        let Some(result) = self.import.result.clone() else {
            return;
        };
        let full = ui.available_rect_before_wrap();
        let bar_h = 56.0;
        let body = Rect::from_min_max(full.min, pos2(full.right(), full.bottom() - bar_h)).shrink2(vec2(PAD, 0.0));
        let bar = Rect::from_min_max(pos2(full.left(), full.bottom() - bar_h), full.max);
        let mut content = ui.new_child(UiBuilder::new().max_rect(body.shrink2(vec2(0.0, 14.0))).layout(Layout::top_down(Align::Min)));
        egui::ScrollArea::vertical().id_salt("hub-import-result").auto_shrink([false, false]).show(&mut content, |ui| {
            let width = ui.available_width() - 12.0;
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 8.0;
            let ok = !result.outcomes.is_empty();
            let headline = match (result.outcomes.len(), result.failures.len()) {
                (n, 0) => trn!("Imported {n} project", "Imported {n} projects", n),
                (0, f) => trn!("{n} project could not be imported", "{n} projects could not be imported", f),
                (n, f) => trf!("Imported {n}, {f} failed", n = n, f = f),
            };
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(22.0, 24.0), Sense::hover());
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
                    if ok { Icon::Check } else { Icon::Error },
                    if ok { theme::OK } else { theme::ERROR },
                );
                ui.label(egui::RichText::new(headline).font(theme::strong(17.0)).color(theme::TEXT));
                ui.label(
                    egui::RichText::new(trf!("in {value} s", value = format!("{:.1}", result.took.as_secs_f32())))
                        .font(theme::caption())
                        .color(theme::TEXT_DISABLED),
                );
            });
            for (name, reason) in &result.failures {
                widgets::note_width(ui, Icon::Error, Tone::Error, &trf!("{name}: {reason}", name = name, reason = reason), width);
            }
            for outcome in &result.outcomes {
                self.outcome_card(ui, outcome, width);
            }
        });
        let mut footer = ui.new_child(UiBuilder::new().max_rect(bar).layout(Layout::top_down(Align::Min)));
        let rect = footer.max_rect();
        footer.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let mut row = footer.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.left() + PAD, rect.top() + 10.0), pos2(rect.right() - PAD, rect.bottom() - 10.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 8.0;
        if widgets::action_button(&mut row, Id::new("hub-import-done"), Some(Icon::Folder), tr_s("Back to Projects"), true).clicked() {
            self.push(HubAction::ImportReset);
            self.push(HubAction::Navigate(Page::Projects));
        }
        if widgets::action_button(&mut row, Id::new("hub-import-more"), None, tr_s("Import More"), false).clicked() {
            self.push(HubAction::ImportReset);
        }
        ui.allocate_rect(full, Sense::hover());
    }

    fn outcome_card(&mut self, ui: &mut Ui, outcome: &Outcome, width: f32) {
        egui::Frame::none()
            .fill(theme::BG_RAISED)
            .stroke(Stroke::new(1.0_f32, theme::LINE))
            .rounding(theme::R_M)
            .inner_margin(Margin::same(14.0))
            .show(ui, |ui| {
                ui.set_width(width - 30.0);
                ui.spacing_mut().item_spacing.y = theme::SP_2;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&outcome.title).font(theme::strong(14.0)).color(theme::TEXT));
                    let how = match outcome.used {
                        import::Used::Copied => tr_s("copied"),
                        import::Used::Unpacked => tr_s("unpacked"),
                        import::Used::InPlace => tr_s("in place"),
                    };
                    widgets::paint_badge_inline(ui, how, Tone::Neutral);
                });
                ui.add(egui::Label::new(egui::RichText::new(outcome.dir.display().to_string()).font(theme::mono()).color(theme::TEXT_MUTED)).truncate());
                if outcome.used != import::Used::InPlace {
                    ui.label(
                        egui::RichText::new(trf!("{files} files  ·  {size}", files = outcome.files, size = import::human(outcome.bytes)))
                            .font(theme::caption())
                            .color(theme::TEXT_MUTED),
                    );
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let id = outcome.dir.display().to_string();
                    if widgets::button(ui, Id::new("hub-import-open").with(&id), Some(Icon::External), tr_s("Open in Editor"), true).clicked() {
                        self.push(HubAction::Open(outcome.dir.clone()));
                    }
                    if widgets::button(ui, Id::new("hub-import-reveal").with(&id), Some(Icon::FolderOpen), tr_s("Show in Folder"), false).clicked() {
                        self.push(HubAction::Reveal(outcome.dir.clone()));
                    }
                });
                if !outcome.changes.is_empty() {
                    ui.add_space(theme::SP_1);
                    widgets::section_label(ui, tr_s("What was changed"));
                    for change in &outcome.changes {
                        bullet(ui, &change.text(), theme::TEXT);
                    }
                }
                if !outcome.skipped.is_empty() {
                    widgets::section_label(ui, &trf!("Left out ({})", outcome.skipped.len()));
                    for (path, reason) in outcome.skipped.iter().take(6) {
                        bullet(ui, &format!("{path} — {}", tr(reason)), theme::TEXT_MUTED);
                    }
                    if outcome.skipped.len() > 6 {
                        bullet(ui, &trf!("… and {} more", outcome.skipped.len() - 6), theme::TEXT_DISABLED);
                    }
                }
                widgets::section_label(ui, tr_s("What to do next"));
                for step in next_steps(outcome, &self.import.settings) {
                    bullet(ui, &step, theme::TEXT);
                }
            });
    }

    // -----------------------------------------------------------------------
    // Подсказка при перетаскивании файла
    // -----------------------------------------------------------------------

    /// Файлы брошены на окно: в импорт. Пока файл над окном — подсказка.
    pub(crate) fn drop_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            self.push(HubAction::ImportAdd(path));
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("hub-drop-overlay")));
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(150));
            let frame = screen.shrink(28.0);
            painter.rect_stroke(frame, theme::R_L, Stroke::new(2.0_f32, theme::accent()));
            painter.text(
                frame.center() - vec2(0.0, 10.0),
                Align2::CENTER_CENTER,
                tr_s("Drop to import"),
                theme::strong(20.0),
                theme::TEXT_STRONG,
            );
            painter.text(
                frame.center() + vec2(0.0, 16.0),
                Align2::CENTER_CENTER,
                trf!("A project folder, a folder with projects, or a .{} file", brand::PROJECT_EXTENSION),
                theme::body(),
                theme::TEXT_MUTED,
            );
            ctx.request_repaint();
        }
    }

    // -----------------------------------------------------------------------
    // Строки фоновых задач
    // -----------------------------------------------------------------------

    /// Строки задач над полосой сборок.
    pub(crate) fn work_rows(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let count = self.works.len().min(3);
        let row_h = rect.height() / count.max(1) as f32;
        let shown: Vec<usize> = (self.works.len().saturating_sub(count)..self.works.len()).collect();
        for (n, index) in shown.into_iter().enumerate() {
            let row = Rect::from_min_size(pos2(rect.left(), rect.top() + n as f32 * row_h), vec2(rect.width(), row_h));
            self.work_row(ui, row, index);
        }
    }

    fn work_row(&mut self, ui: &mut Ui, rect: Rect, index: usize) {
        let work: &Work = &self.works[index];
        let id = work.id;
        let y = rect.center().y;
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
        let (icon, color, text, bar) = match &work.state {
            WorkState::Running => {
                let detail = match &work.progress {
                    Some(p) if p.files_total > 0 => trf!(
                        "{done} / {total} files  ·  {bytes} of {all}",
                        done = p.files_done,
                        total = p.files_total,
                        bytes = import::human(p.bytes_done),
                        all = import::human(p.bytes_total)
                    ),
                    _ => work.stage.clone(),
                };
                let current = work.progress.as_ref().map(|p| p.current.clone()).filter(|c| !c.is_empty() && work.kind == WorkKind::Pack);
                let text = match current {
                    Some(current) => format!("{}  —  {detail}  ·  {current}", work.title),
                    None if detail.is_empty() => work.title.clone(),
                    None => format!("{}  —  {detail}", work.title),
                };
                (Icon::Save, theme::accent(), text, Some(work.fraction()))
            }
            WorkState::Done => {
                let text = match &work.result {
                    Some(crate::tasks::WorkResult::Pack { out, summary }) => trf!(
                        "Saved {name}: {files} files, {size}",
                        name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        files = summary.files,
                        size = import::human(summary.archive_size)
                    ),
                    Some(crate::tasks::WorkResult::Import { outcomes, failures }) => {
                        trf!("{title}: {n} imported, {f} failed", title = work.title, n = outcomes.len(), f = failures.len())
                    }
                    Some(crate::tasks::WorkResult::Test { passed, .. }) => {
                        if *passed {
                            trf!("{}: passed", work.title)
                        } else {
                            trf!("{}: failed", work.title)
                        }
                    }
                    None => work.title.clone(),
                };
                let ok = !matches!(&work.result, Some(crate::tasks::WorkResult::Test { passed: false, .. }));
                (if ok { Icon::Check } else { Icon::Error }, if ok { theme::OK } else { theme::ERROR }, text, None)
            }
            WorkState::Failed(reason) => (Icon::Error, theme::ERROR, format!("{}: {reason}", work.title), None),
            WorkState::Cancelled => (Icon::Warn, theme::WARN, trf!("{}: cancelled", work.title), None),
        };
        let running = work.is_running();
        let kind = work.kind;
        let out = match &work.result {
            Some(crate::tasks::WorkResult::Pack { out, .. }) => Some(out.clone()),
            _ => None,
        };
        let tail = match &work.result {
            Some(crate::tasks::WorkResult::Test { tail, .. }) => Some(tail.join("\n")),
            _ => None,
        };
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 8.0, y), vec2(14.0, 14.0)), icon, color);
        // Полоса хода сжимается в узком окне, чтобы тексту осталось место.
        let bar_w = (rect.width() * 0.14).clamp(60.0, 160.0);
        // Кнопок справа столько, сколько их есть: у идущей задачи их меньше.
        let buttons_w = match (running, kind) {
            (true, WorkKind::Import) => 200.0,
            (true, _) => 110.0,
            (false, _) => 250.0,
        };
        let text_right = rect.right() - buttons_w - bar_w - 24.0;
        let text_right = if bar.is_some() { text_right } else { text_right + bar_w + 12.0 };
        let shown = widgets::paint_elided(ui.painter(), pos2(rect.left() + 24.0, y), &text, theme::caption(), theme::TEXT, (text_right - rect.left() - 24.0).max(60.0), Align2::LEFT_CENTER);
        if let Some(tip) = &tail {
            let response = ui.interact(shown, Id::new("hub-work-text").with(id), Sense::hover());
            response.on_hover_text(tip);
        }
        if let Some(fraction) = bar {
            let bar_rect = Rect::from_min_size(pos2(text_right + 12.0, y - 3.0), vec2(bar_w, 6.0));
            let mut child = ui.new_child(UiBuilder::new().max_rect(bar_rect));
            widgets::progress_bar(&mut child, fraction, false);
        }
        let area = Rect::from_min_max(pos2(rect.right() - buttons_w, rect.top()), rect.max).shrink2(vec2(8.0, 0.0));
        let mut buttons = ui.new_child(UiBuilder::new().max_rect(area).layout(Layout::right_to_left(Align::Center)));
        buttons.spacing_mut().item_spacing.x = 6.0;
        if running {
            if widgets::button(&mut buttons, Id::new("hub-work-cancel").with(id), None, tr_s("Cancel"), false).clicked() {
                self.push(HubAction::CancelWork(id));
            }
            if kind == WorkKind::Import && widgets::button(&mut buttons, Id::new("hub-work-show").with(id), None, tr_s("Details"), false).clicked() {
                self.push(HubAction::Navigate(Page::Import));
            }
        } else {
            if widgets::button(&mut buttons, Id::new("hub-work-dismiss").with(id), None, tr_s("Dismiss"), false).clicked() {
                self.push(HubAction::DismissWork(id));
            }
            if let Some(out) = out {
                if widgets::button(&mut buttons, Id::new("hub-work-reveal").with(id), Some(Icon::FolderOpen), tr_s("Show in Folder"), false).clicked() {
                    self.push(HubAction::Reveal(out));
                }
            }
            if kind == WorkKind::Import && widgets::button(&mut buttons, Id::new("hub-work-result").with(id), None, tr_s("Show Result"), false).clicked() {
                self.push(HubAction::Navigate(Page::Import));
            }
        }
    }

    /// Высота панели задач: строка на задачу, не больше трёх.
    pub(crate) fn work_panel_height(&self) -> f32 {
        36.0 * self.works.len().min(3) as f32
    }
}

fn paths_same(a: &std::path::Path, b: &std::path::Path) -> bool {
    crate::paths::same(a, b)
}

/// Ширина, с которой форма переключается на «подпись над полем».
const STACK_BELOW: f32 = 540.0;

/// Ширина управляющего элемента строки формы шириной `total`.
fn form_control_width(total: f32) -> f32 {
    if total < STACK_BELOW {
        total.max(120.0)
    } else {
        (total - LABEL_W - 8.0).max(120.0)
    }
}

/// Отступ слева, с которого начинаются пояснения под полем.
fn form_indent(total: f32) -> f32 {
    if total < STACK_BELOW {
        0.0
    } else {
        LABEL_W + 8.0
    }
}

/// «Подпись — значение» в две колонки.
fn key_value(ui: &mut Ui, key: &str, value: &str, mono: bool) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(LABEL_W, 16.0), Sense::hover());
        widgets::paint_elided(ui.painter(), pos2(rect.left(), rect.center().y), key, theme::body(), theme::TEXT_MUTED, LABEL_W - 4.0, Align2::LEFT_CENTER);
        response.on_hover_text(key);
        let font = if mono { theme::mono() } else { theme::body() };
        ui.add(egui::Label::new(egui::RichText::new(value).font(font).color(theme::TEXT)).wrap());
    });
}

/// Строка формы: подпись слева, управление справа, пояснение мелким шрифтом под ним; в узкой
/// колонке подпись стоит над управлением.
fn form_row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(theme::SP_3);
    if ui.available_width() < STACK_BELOW {
        ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED));
        ui.add_space(theme::SP_1);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme::SP_2;
            add(ui);
            if !hint.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(hint).font(theme::caption()).color(theme::TEXT_DISABLED)).wrap());
            }
        });
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_W, widgets::CONTROL_H), Sense::hover());
        widgets::paint_elided(ui.painter(), pos2(rect.left(), rect.center().y), label, theme::body(), theme::TEXT_MUTED, LABEL_W - 6.0, Align2::LEFT_CENTER);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme::SP_2;
            add(ui);
            if !hint.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(hint).font(theme::caption()).color(theme::TEXT_DISABLED)).wrap());
            }
        });
    });
}

fn bullet(ui: &mut Ui, text: &str, color: Color32) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 1.8, theme::TEXT_DISABLED);
        ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
    });
}

/// Что делать после импорта — по результату.
pub fn next_steps(outcome: &Outcome, settings: &import::ImportSettings) -> Vec<String> {
    let mut steps = Vec::new();
    if outcome.used == import::Used::InPlace {
        steps.push(tr_s("The project stays where it was; the hub never edits it. Open it from the list.").to_string());
        return steps;
    }
    steps.push(tr_s("Open the project from the list: it starts in the generic editor at once.").to_string());
    if outcome.native {
        if settings.build {
            steps.push(tr_s("The project has its own Rust or C++ code: its editor is building in the background; the first build can take several minutes.").to_string());
        } else {
            steps.push(tr_s("The project has its own Rust or C++ code: use Open in Editor to build its editor (the first build can take several minutes), or tick “Build in the background” next time.").to_string());
        }
    }
    if outcome.has_cpp {
        steps.push(tr_s("C++ code needs a C++ compiler (MSVC or GCC); run Tools → Check Build Tools in the editor if the build complains.").to_string());
    }
    if outcome.changes.iter().any(|c| c.code == ChangeCode::DependencyRewritten) {
        steps.push(tr_s("Cargo.toml now points at this engine checkout; if you move the engine folder, update the paths in Cargo.toml.").to_string());
    }
    if !settings.test {
        steps.push(trf!("Check that it builds: `cargo test -p {}` in the project folder.", outcome.name));
    }
    steps
}

// ---------------------------------------------------------------------------
// Окно «Save Project As File…»
// ---------------------------------------------------------------------------

/// Содержимое окна. `Some(true)` — сохранить, `Some(false)` — закрыть.
pub(crate) fn pack_dialog_ui(ui: &mut Ui, dialog: &mut PackDialog, actions: &mut Vec<HubAction>, width: f32) -> Option<bool> {
    crate::ui::dialog_title(ui, Icon::Save, theme::accent(), &trf!("Save “{}” As File", dialog.title));
    crate::ui::dialog_text(
        ui,
        &trf!(
            "One portable .{} file with everything needed to rebuild the project: Cargo.toml, src, assets with .meta, settings. Nothing derived: no target/, .git/ or caches.",
            brand::PROJECT_EXTENSION
        ),
    );
    ui.add_space(theme::SP_2);
    widgets::section_label(ui, tr_s("Save to"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let browse_w = 84.0;
        widgets::text_field(ui, Id::new(Hub::PACK_PATH_ID), &mut dialog.out, &brand::project_file_name(&dialog.package), width - browse_w - 6.0);
        if widgets::button_sized(ui, Id::new("hub-pack-browse"), tr_s("Browse…"), browse_w).clicked() {
            actions.push(HubAction::Browse(BrowseTarget::PackFolder));
        }
    });
    ui.add_space(theme::SP_2);
    widgets::section_label(ui, tr_s("Include"));
    let estimate = dialog.estimate.clone();
    let (tests_hint, cpp_hint, builds_hint) = (String::new(), String::new(), String::new());
    let _ = (tests_hint, cpp_hint, builds_hint);
    ui.checkbox(&mut dialog.tests, tr_s("Tests (tests/)"));
    ui.checkbox(&mut dialog.cpp, tr_s("C++ sources (cpp/)"));
    let builds_label = match &estimate {
        Some(Ok(e)) if e.builds_bytes > 0 => trf!("The last exported builds (dist/, {})", import::human(e.builds_bytes)),
        _ => tr_s("The last exported builds (dist/)").to_string(),
    };
    ui.checkbox(&mut dialog.builds, builds_label);
    ui.add(
        egui::Label::new(
            egui::RichText::new(tr_s("Always left out: target/, .git/, caches, plugin build folders, editor temp files and links. .gitignore is respected."))
                .font(theme::caption())
                .color(theme::TEXT_DISABLED),
        )
        .wrap(),
    );
    ui.add_space(theme::SP_2);
    let line = match &estimate {
        None => tr_s("Counting files…").to_string(),
        Some(Ok(e)) => trf!("{files} files, {size} before compression  ·  {left} items left out", files = e.files, size = import::human(e.bytes), left = e.left_out),
        Some(Err(err)) => err.clone(),
    };
    ui.label(egui::RichText::new(line).font(theme::caption()).color(theme::TEXT_MUTED));
    if let Some(error) = &dialog.error {
        ui.add_space(theme::SP_2);
        widgets::note_width(ui, Icon::Error, Tone::Error, error, width);
    }
    let ready = !dialog.out.trim().is_empty() && !matches!(estimate, Some(Err(_)));
    let answer = crate::ui::dialog_buttons(ui, tr_s("Save"), ready, false);
    if answer == Some(true) {
        actions.push(HubAction::StartPack(Box::new(dialog.clone())));
    }
    answer
}
