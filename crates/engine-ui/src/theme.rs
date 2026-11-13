//! Палитра по умолчанию: тёмные панели, светлый текст, синий акцент. Ею пользуются
//! значения полей по умолчанию и конструкторы (`ui_button`, `ui_panel`…), поэтому
//! интерфейс выглядит цельно без единой настройки. Цвета — в sRGB, как в графическом
//! редакторе (переводятся в линейные через [`Color::hex`]).

use engine_core::math::Color;

/// Фон панели.
pub fn panel() -> Color {
    Color::hex(0x161c27)
}
/// Рамка панели.
pub fn panel_border() -> Color {
    Color::hex(0x2c3648)
}
/// Основной текст.
pub fn text() -> Color {
    Color::hex(0xeef2f8)
}
/// Второстепенный текст: подписи, подсказки.
pub fn text_muted() -> Color {
    Color::hex(0x8e9bb0)
}
/// Акцент: основная кнопка, заполненная часть ползунка, фокус.
pub fn accent() -> Color {
    Color::hex(0x4f8cff)
}
pub fn accent_hover() -> Color {
    Color::hex(0x6a9dff)
}
pub fn accent_pressed() -> Color {
    Color::hex(0x3f74db)
}
/// Обычная кнопка.
pub fn button() -> Color {
    Color::hex(0x273142)
}
pub fn button_hover() -> Color {
    Color::hex(0x334058)
}
pub fn button_pressed() -> Color {
    Color::hex(0x1f2735)
}
pub fn button_disabled() -> Color {
    Color::hex(0x1f2531).with_alpha(0.7)
}
/// Подложка полей: дорожка ползунка, поле ввода, рамка флажка.
pub fn field() -> Color {
    Color::hex(0x0e131b)
}
pub fn field_border() -> Color {
    Color::hex(0x3a475e)
}
/// Ручка ползунка, галочка.
pub fn handle() -> Color {
    Color::hex(0xf4f7fb)
}
/// Положительный индикатор (здоровье).
pub fn good() -> Color {
    Color::hex(0x3ecf7a)
}
/// Опасность (мало здоровья, ошибка).
pub fn danger() -> Color {
    Color::hex(0xff5d5d)
}
/// Тень под панелями.
pub fn shadow() -> Color {
    Color::rgba(0.0, 0.0, 0.0, 0.5)
}
