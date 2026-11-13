//! Игра в редакторе: `cargo run -p template --features editor --bin template-editor`.
//!
//! Скрипты на Rust — в `src/scripts/` (Assets → New Rust Script…). Build (Ctrl+B)
//! пересобирает эту программу и перезапускает редактор с новым кодом; сцена,
//! выделение и открытые файлы остаются. Подробно — docs/api/editor.md.

engine::editor_main!(template::build, |config| config.open_scene("scenes/main.scene"));
