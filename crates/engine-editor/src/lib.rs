//! `engine-editor` — редактор в стиле Unity на `egui` + `wgpu`.
//!
//! Редактор — это библиотека: игра сама запускает его со своими компонентами,
//! поэтому скрипты не нужно ни «подгружать», ни перекомпилировать отдельно.
//!
//! ```no_run
//! use engine_core::prelude::*;
//! use engine_editor::{Editor, EditorConfig};
//!
//! let mut app = App::new();
//! engine_render::register_components(&mut app.world.registry);
//! // app.register::<MyScript>(); …
//! let editor = Editor::new(app, EditorConfig::titled("My Game")).unwrap();
//! engine_editor::run(editor).unwrap();
//! ```
//!
//! Окна: Hierarchy, Inspector (строится по reflection), Scene/Game/Code с гизмо,
//! Project, Console, Profiler, Preferences; режимы Play / Pause / Step; Undo / Redo.
//!
//! Всё, что делает мышь, выражено значением [`Action`], а интерфейс строится
//! методом [`Editor::ui`] без GPU — поэтому редактор тестируется без окна.
#![forbid(unsafe_code)]

mod add_menu;
mod catalog;
mod code;
// Вкладка Code: проверка синтаксиса, ошибки компилятора, Error List (W15).
// `code_checks.rs` и `code_view.rs` (живая проверка при наборе и её отрисовка) написаны, но ещё не
// подключены: им нужны поля `CodeState`, которых пока нет. Объявить вместе с ними.
mod code_cargo;
mod code_check_cpp;
mod code_check_data;
mod code_check_rust;
mod code_diagnostics;
mod code_files;
mod console;
mod controls;
mod metrics;
mod build_tools;
mod dock;
mod editor_api;
mod editor_audio;
mod export;
mod gpu;
mod hub_link;
mod help;
mod hierarchy;
mod i18n;
mod prefs;
mod preview;
mod icons;
/// Один редактор на проект: метка с номером процесса (её проверяет Piska Hub).
pub mod instance;
mod lines;
mod inspector;
mod inspector_multi;
mod inspector_prefab;
mod inspector_refs;
mod material_editor;
mod modeler_link;
mod profiler;
mod project;
mod project_file;
mod project_bar;
mod project_chrome;
mod project_content;
mod project_fs;
mod project_menus;
mod project_nav;
mod project_ops;
mod project_tree;
mod runner;
mod rust_scripts;
mod scenarios;
mod scene_picker;
mod script_order;
mod selection;
mod settings;
mod shortcuts;
mod sorting;
mod splash;
mod state;
mod terrain_tools;
mod theme;
mod ui;
mod undo_history;
mod window_kit;
mod view3d;
mod viewport;

pub use build_tools::{DoctorReport, ToolCheck};
pub use code::{BuildCommand, BuildStatus};
pub use dock::{DockSide, LayoutAction, LayoutPreset, Panel};
pub use export::{ExportPlatform, ExportSettings, SERVE_PORT};
pub use egui;
pub use gpu::{capture, capture_with_input};
pub use prefs::Preferences;
pub use project_nav::ProjectPrefs;
pub use settings::SettingsScope;
pub use settings::{
    read_build_options, validate_android_package, write_build_options, AndroidOrientation, BuildOptions,
    InputBindingList, ANDROID_MAX_VERSION_CODE, ANDROID_MIN_SDK,
};
pub use shortcuts::{CommandDef, KeyBinding, Keymap, COMMANDS};
pub use preview::PreviewStats;
pub use profiler::FrameTimings;
pub use scene_picker::SceneEntry;
pub use runner::run;
pub use rust_scripts::{remove_stale_executables, session_from_args, Relaunch, SESSION_ARG, SESSION_ENV};
pub use state::{Action, BottomTab, ChromeRects, Editor, EditorConfig, Mode, Tool, ViewTab};
pub use view3d::Orbit3D;
pub use viewport::ViewTransform;

/// Визуальная система редактора для других программ движка (Piska Hub): палитра,
/// шкалы, виджеты и векторные иконки — те же, что в самом редакторе.
pub mod kit {
    /// Палитра, шрифты, отступы и виджеты редактора.
    pub mod theme {
        pub use crate::theme::*;
    }
    /// Всплывающие окна и диалоги: заголовок, перетаскивание, строка кнопок, заставка.
    pub mod window {
        pub use crate::window_kit::*;
    }
    /// Контролы с едиными метриками: кнопки, поля, числа, списки, строки, карточки.
    pub mod controls {
        pub use crate::controls::*;
    }
    /// Замер интерфейса после кадра: виджеты, рамки, сводка нарушений.
    pub mod metrics {
        pub use crate::metrics::*;
    }
    /// Иконки, нарисованные фигурами.
    pub mod icons {
        pub use crate::icons::*;
    }
    /// Перевод интерфейса: `tr`, `trf!`, `trn!`, `Language`, `set` / `current`.
    pub mod i18n {
        pub use crate::i18n::*;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error(transparent)]
    Scene(#[from] engine_core::error::SceneError),
    #[error(transparent)]
    Asset(#[from] engine_assets::AssetError),
    #[error(transparent)]
    Render(#[from] engine_render::RenderError),
    #[error("{0}")]
    Other(String),
}
