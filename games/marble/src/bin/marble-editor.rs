//! Игра в редакторе: `cargo run -p marble --features editor --bin marble-editor`.
//!
//! Скрипты на Rust — в `src/scripts/` (Assets → New Rust Script…). Build (Ctrl+B)
//! пересобирает эту программу и перезапускает редактор с новым кодом; сцена,
//! выделение и открытые файлы остаются. Подробно — docs/api/editor.md.

engine::editor_main!(marble::build);
