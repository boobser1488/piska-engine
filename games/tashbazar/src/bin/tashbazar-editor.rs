//! Игра в редакторе: `cargo run -p tashbazar --features editor --bin tashbazar-editor`.
//!
//! Скрипты на Rust — в `src/scripts/` (Assets → New Rust Script…). Build (Ctrl+B)
//! пересобирает эту программу и перезапускает редактор с новым кодом. Подробно —
//! docs/api/editor.md.

engine::editor_main!(tashbazar::build);
