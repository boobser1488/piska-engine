//! Перевод интерфейса: `tr("Add Component")`, `trf!`, `trn!`, язык процесса.
//!
//! Слой живёт в крейте `engine-i18n` (без зависимостей, чтобы им пользовались и редактор
//! моделей, который не может зависеть от редактора, и хаб); здесь он только
//! подключается: внутри редактора — `crate::i18n::{tr, trf}`, снаружи — через
//! `engine_editor::kit::i18n`. Каталог русского — `crates/engine-i18n/locale/ru.po`.

pub use engine_i18n::*;

#[cfg(test)]
mod tests {
    use egui::{Context, FontId};

    /// Все символы русского каталога (кроме переводов строки) — их должны уметь рисовать шрифты.
    fn catalogue_chars() -> Vec<char> {
        let catalogue = engine_i18n::Language::Russian.catalogue().expect("the Russian catalogue is embedded");
        let mut chars: Vec<char> = catalogue
            .entries()
            .iter()
            .flat_map(|e| e.forms.iter())
            .flat_map(|f| f.chars())
            .filter(|c| !c.is_control())
            .collect();
        chars.sort_unstable();
        chars.dedup();
        chars
    }

    fn missing(ctx: &Context, font: &FontId, chars: &[char]) -> Vec<char> {
        ctx.fonts(|fonts| chars.iter().copied().filter(|c| !fonts.has_glyph(font, *c)).collect())
    }

    fn run_frames(ctx: &Context) {
        // Шрифты встают при первом проходе кадра.
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |_| {});
        }
    }

    /// Шрифты egui «из коробки» (то, что есть на Linux / Android / в вебе, где системных
    /// шрифтов Segoe UI нет): весь каталог рисуется без «тофу».
    #[test]
    fn bundled_fonts_cover_the_russian_catalogue() {
        let ctx = Context::default();
        run_frames(&ctx);
        let chars = catalogue_chars();
        assert!(chars.contains(&'ё') && chars.contains(&'я') && chars.contains(&'«'));
        for (name, font) in [("proportional", FontId::proportional(12.5)), ("monospace", FontId::monospace(12.0))] {
            // Буквы, кавычки и типографские знаки — обязательно. Стрелки и прочие символы
            // из английского текста («Preferences → General») в шрифтах egui «из коробки»
            // есть не везде — они не новость русского каталога, поэтому только сообщаются.
            let lost = missing(&ctx, &font, &chars);
            let (letters, symbols): (Vec<char>, Vec<char>) = lost.into_iter().partition(|c| c.is_alphabetic() || "«»—–…×·°".contains(*c));
            assert!(letters.is_empty(), "bundled {name} font has no glyphs for {letters:?}");
            if !symbols.is_empty() {
                eprintln!("bundled {name} font lacks the symbols {symbols:?} (they render as boxes without system fonts)");
            }
        }
    }

    /// Шрифты редактора (системные Segoe UI / Cascadia, а если их нет — те же встроенные)
    /// на всех используемых начертаниях: обычный, полужирный, моноширинный.
    #[test]
    fn editor_fonts_cover_the_russian_catalogue() {
        let ctx = Context::default();
        crate::theme::apply(&ctx, true);
        run_frames(&ctx);
        let chars = catalogue_chars();
        for (name, font) in [
            ("regular", FontId::proportional(12.5)),
            ("strong", crate::theme::strong(14.0)),
            ("mono", crate::theme::mono()),
        ] {
            let lost = missing(&ctx, &font, &chars);
            assert!(lost.is_empty(), "the editor {name} font has no glyphs for {lost:?}");
        }
    }
}
