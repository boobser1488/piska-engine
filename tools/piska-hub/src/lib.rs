//! Piska Hub — стартовое окно Piska Engine: список проектов, новый проект из
//! шаблона, открытие в редакторе с ходом сборки, примеры и документация.
//!
//! ```text
//! cargo run -p piska-hub                 # окно
//! cargo run -p piska-hub -- --open <dir> # сразу открыть проект в редакторе
//! ```
//!
//! Хаб — библиотека плюс маленькая программа: интерфейс строится методом
//! [`Hub::ui`] без GPU, а всё, что делает мышь, — значение [`HubAction`], поэтому
//! хаб проверяется тестами без окна. Подробно — `docs/api/hub.md`.
#![forbid(unsafe_code)]

pub mod builds;
pub mod capture;
pub mod cargo;
pub mod config;
mod hub;
pub mod jobs;
pub mod link;
pub mod paths;
pub mod platform;
pub mod project;
pub mod samples;
pub mod templates;
mod ui;
pub mod widgets;

pub use capture::capture;
pub use hub::{
    BrowseTarget, Diagnosis, Dialog, FolderBrowser, Hub, HubAction, HubOptions, Page, Prebuild, StartMode, Status, WindowRequest, Wizard,
    WizardPlan,
};
