//! Компоненты интерфейса. Все — обычные данные с reflection: сохраняются в сценах,
//! правятся в инспекторе. Поведение (раскладка, ввод, отрисовка) — в `UiSystem`.
//!
//! Координаты интерфейса: (0, 0) — левый верхний угол, ось Y направлена **вниз**
//! (как у `input.mouse_position()`). Единицы — «пиксели холста»: при
//! `ScaleMode::ScaleWithScreenSize` они масштабируются под размер экрана.

use engine_assets::{AssetHandle, Font, Texture};
use engine_core::prelude::*;

use crate::theme;

reflect_enum! {
    /// Как холст переводит свои единицы в пиксели экрана.
    ///
    /// * `ScaleWithScreenSize` — масштаб = размер экрана / `reference_resolution`
    ///   (Unity: Scale With Screen Size);
    /// * `ConstantPixelSize` — постоянный масштаб: `scale_factor` пикселей на единицу.
    pub enum ScaleMode {
        ScaleWithScreenSize,
        ConstantPixelSize,
    }
}

reflect_enum! {
    /// Направление раскладки детей.
    pub enum LayoutDirection {
        Vertical,
        Horizontal,
    }
}

reflect_enum! {
    /// Куда прижимать детей внутри раскладки.
    pub enum Alignment {
        TopLeft,
        Top,
        TopRight,
        Left,
        Center,
        Right,
        BottomLeft,
        Bottom,
        BottomRight,
    }
}

impl Alignment {
    /// Доли по осям: (0, 0) — левый верх, (1, 1) — правый низ.
    pub fn fractions(self) -> Vec2 {
        let (x, y) = match self {
            Alignment::TopLeft => (0.0, 0.0),
            Alignment::Top => (0.5, 0.0),
            Alignment::TopRight => (1.0, 0.0),
            Alignment::Left => (0.0, 0.5),
            Alignment::Center => (0.5, 0.5),
            Alignment::Right => (1.0, 0.5),
            Alignment::BottomLeft => (0.0, 1.0),
            Alignment::Bottom => (0.5, 1.0),
            Alignment::BottomRight => (1.0, 1.0),
        };
        Vec2::new(x, y)
    }
}

reflect_enum! {
    /// Выравнивание строк текста по горизонтали.
    pub enum TextAlign {
        Left,
        Center,
        Right,
    }
}

reflect_enum! {
    /// Выравнивание блока текста по вертикали.
    pub enum VerticalAlign {
        Top,
        Middle,
        Bottom,
    }
}

reflect_enum! {
    /// Вид переключателя: `Checkbox` — квадратный флажок с галочкой, `Switch` —
    /// овальный тумблер с бегунком.
    pub enum ToggleStyle {
        Checkbox,
        Switch,
    }
}

reflect_enum! {
    /// Откуда заполняется полоса прогресса.
    pub enum FillDirection {
        LeftToRight,
        RightToLeft,
        BottomToTop,
        TopToBottom,
    }
}

component! {
    /// Экранный холст — корень интерфейса. Всё под ним (по иерархии) рисуется поверх
    /// кадра. Холсты рисуются по возрастанию `sort_order`, внутри — в порядке иерархии.
    pub struct Canvas {
        /// Порядок среди холстов: больший рисуется выше и первым получает клики.
        pub sort_order: i32 = 0,
        /// Как единицы холста переводятся в пиксели.
        pub scale_mode: ScaleMode = ScaleMode::ScaleWithScreenSize,
        /// Разрешение, под которое свёрстан интерфейс (для ScaleWithScreenSize).
        pub reference_resolution: Vec2 = Vec2::new(1280.0, 720.0),
        /// 0 — подгонять по ширине экрана, 1 — по высоте, 0.5 — поровну.
        pub match_width_or_height: f32 = 0.5,
        /// Пикселей на единицу в режиме ConstantPixelSize.
        pub scale_factor: f32 = 1.0,
        /// Перемещение фокуса клавишами (Tab, стрелки) и нажатие Enter. Выключите
        /// для HUD, чтобы стрелки управляли игрой, а не кнопками.
        pub keyboard_navigation: bool = true,
    }
}

impl Component for Canvas {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Canvas {
    /// Пикселей экрана на единицу холста.
    pub fn scale_for(&self, screen: Vec2) -> f32 {
        crate::layout::canvas_scale(self, screen)
    }
}

component! {
    /// Прямоугольник элемента (Unity: RectTransform). Якоря — доли родительского
    /// прямоугольника, (0, 0) — левый верх, (1, 1) — правый низ.
    ///
    /// Если `anchor_min == anchor_max`, `size` — размер, а `position` — смещение точки
    /// `pivot` от якоря. Если якоря разведены, `size` добавляется к растянутой области
    /// (`size = (0, 0)` — ровно по якорям, `(-20, -20)` — с отступом 10 со всех сторон).
    pub struct RectTransform {
        /// Левый верхний якорь (доли родителя).
        pub anchor_min: Vec2 = Vec2::splat(0.5),
        /// Правый нижний якорь (доли родителя).
        pub anchor_max: Vec2 = Vec2::splat(0.5),
        /// Точка опоры элемента (доли его размера): к ней относится `position`.
        pub pivot: Vec2 = Vec2::splat(0.5),
        /// Смещение опоры от якоря в единицах холста (Y — вниз).
        pub position: Vec2 = Vec2::ZERO,
        /// Размер (или добавка к растянутой области — см. описание компонента).
        pub size: Vec2 = Vec2::new(160.0, 40.0),
    }
}

impl Component for RectTransform {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Готовые сочетания якорей и опоры — как пресеты в инспекторе Unity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    /// Во всю ширину вдоль верхнего края (`size.x` — добавка к ширине, `size.y` — высота).
    StretchTop,
    /// Во всю ширину вдоль нижнего края.
    StretchBottom,
    /// Во всю высоту вдоль левого края.
    StretchLeft,
    /// Во всю высоту вдоль правого края.
    StretchRight,
    /// Растянуть на весь родительский прямоугольник.
    Stretch,
}

impl Anchor {
    /// (anchor_min, anchor_max, pivot).
    pub fn values(self) -> (Vec2, Vec2, Vec2) {
        let point = |x: f32, y: f32| (Vec2::new(x, y), Vec2::new(x, y), Vec2::new(x, y));
        match self {
            Anchor::TopLeft => point(0.0, 0.0),
            Anchor::Top => point(0.5, 0.0),
            Anchor::TopRight => point(1.0, 0.0),
            Anchor::Left => point(0.0, 0.5),
            Anchor::Center => point(0.5, 0.5),
            Anchor::Right => point(1.0, 0.5),
            Anchor::BottomLeft => point(0.0, 1.0),
            Anchor::Bottom => point(0.5, 1.0),
            Anchor::BottomRight => point(1.0, 1.0),
            Anchor::StretchTop => (Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.5, 0.0)),
            Anchor::StretchBottom => (Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(0.5, 1.0)),
            Anchor::StretchLeft => (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, 0.5)),
            Anchor::StretchRight => (Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.5)),
            Anchor::Stretch => (Vec2::ZERO, Vec2::ONE, Vec2::splat(0.5)),
        }
    }
}

impl RectTransform {
    /// Элемент с пресетом якорей, смещением и размером.
    pub fn anchored(anchor: Anchor, position: Vec2, size: Vec2) -> Self {
        let (anchor_min, anchor_max, pivot) = anchor.values();
        Self {
            anchor_min,
            anchor_max,
            pivot,
            position,
            size,
        }
    }

    /// На весь родительский прямоугольник.
    pub fn stretch() -> Self {
        Self::anchored(Anchor::Stretch, Vec2::ZERO, Vec2::ZERO)
    }

    /// На весь родительский прямоугольник с отступами от краёв.
    pub fn inset(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            position: Vec2::new((left - right) * 0.5, (top - bottom) * 0.5),
            size: Vec2::new(-(left + right), -(top + bottom)),
            ..Self::stretch()
        }
    }

    /// Сменить пресет якорей, сохранив смещение и размер.
    pub fn set_anchor(&mut self, anchor: Anchor) {
        let (anchor_min, anchor_max, pivot) = anchor.values();
        self.anchor_min = anchor_min;
        self.anchor_max = anchor_max;
        self.pivot = pivot;
    }
}

component! {
    /// Раскладывает детей столбиком или строкой (Unity: Vertical / Horizontal Layout Group).
    /// Дети берут размер вдоль направления из своего `RectTransform.size`; их якоря и
    /// смещения при этом не используются.
    pub struct LayoutGroup {
        pub direction: LayoutDirection = LayoutDirection::Vertical,
        /// Промежуток между детьми.
        pub spacing: f32 = 8.0,
        pub padding_left: f32 = 0.0,
        pub padding_right: f32 = 0.0,
        pub padding_top: f32 = 0.0,
        pub padding_bottom: f32 = 0.0,
        /// Куда прижимать детей, если места больше, чем нужно.
        pub child_alignment: Alignment = Alignment::TopLeft,
        /// Растянуть детей поперёк направления (ширину — в столбце, высоту — в строке).
        pub stretch_children: bool = true,
        /// Делить свободное место (или его нехватку) вдоль направления поровну между
        /// детьми: они заполняют группу точно.
        pub expand_children: bool = false,
        /// Подогнать собственный размер под содержимое (Unity: Content Size Fitter):
        /// вдоль направления всегда, поперёк — если дети не растягиваются.
        pub fit_content: bool = false,
    }
}

impl Component for LayoutGroup {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl LayoutGroup {
    pub fn vertical(spacing: f32) -> Self {
        Self {
            spacing,
            ..Self::default()
        }
    }

    pub fn horizontal(spacing: f32) -> Self {
        Self {
            direction: LayoutDirection::Horizontal,
            spacing,
            ..Self::default()
        }
    }

    /// Одинаковые отступы со всех сторон.
    pub fn with_padding(mut self, padding: f32) -> Self {
        self.padding_left = padding;
        self.padding_right = padding;
        self.padding_top = padding;
        self.padding_bottom = padding;
        self
    }
}

component! {
    /// Отсекает всё, что дети рисуют за пределами этого прямоугольника (Unity: RectMask2D).
    pub struct RectMask {
        /// Сжать область отсечения внутрь на столько единиц.
        pub padding: f32 = 0.0,
    }
}

impl Component for RectMask {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Прозрачность и доступность целой ветки интерфейса (Unity: CanvasGroup).
    pub struct CanvasGroup {
        /// Множитель прозрачности для себя и всех потомков.
        pub alpha: f32 = 1.0,
        /// `false` — виджеты ветки не реагируют на ввод (но видны).
        pub interactable: bool = true,
        /// `false` — ветка «прозрачна» для мыши: клики проходят сквозь неё.
        pub blocks_raycasts: bool = true,
    }
}

impl Component for CanvasGroup {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Прямоугольник цвета или картинки: панели, фоны, иконки. Умеет скругление,
    /// рамку, мягкую тень и 9-slice.
    pub struct Image {
        /// Цвет (множится на текстуру). У кнопки заменяется цветом её состояния.
        pub color: Color = Color::WHITE,
        /// Текстура из базы ассетов; пусто — сплошной цвет.
        pub texture: AssetHandle<Texture> = AssetHandle::none(),
        /// Радиус скругления углов.
        pub corner_radius: f32 = 0.0,
        /// Толщина рамки (внутрь прямоугольника).
        pub border_width: f32 = 0.0,
        pub border_color: Color = theme::panel_border(),
        /// Размер мягкой тени под элементом; 0 — без тени.
        pub shadow_size: f32 = 0.0,
        pub shadow_color: Color = theme::shadow(),
        /// Сдвиг тени (Y — вниз).
        pub shadow_offset: Vec2 = Vec2::new(0.0, 6.0),
        /// 9-slice: ширина нерастягиваемой рамки текстуры в её пикселях; 0 — растянуть целиком.
        pub slice_border: f32 = 0.0,
        /// Ловит ли элемент мышь (закрывает собой игру и элементы под ним).
        pub raycast_target: bool = true,
    }
}

impl Component for Image {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Image {
    pub fn colored(color: Color) -> Self {
        Self {
            color,
            ..Self::default()
        }
    }

    /// Панель в стиле темы: тёмный фон, скругление, рамка, тень.
    pub fn panel() -> Self {
        Self {
            color: theme::panel(),
            corner_radius: 16.0,
            border_width: 1.0,
            border_color: theme::panel_border(),
            shadow_size: 28.0,
            ..Self::default()
        }
    }
}

component! {
    /// Текст. Растеризуется под итоговый размер в пикселях, поэтому чёткий при любом
    /// масштабе холста. Встроенный шрифт (Ubuntu Light) знает латиницу и кириллицу.
    pub struct Text {
        pub text: String = String::new(),
        /// Шрифт `.ttf`/`.otf` из базы ассетов; пусто — встроенный.
        pub font: AssetHandle<Font> = AssetHandle::none(),
        /// Кегль в единицах холста (как font-size в CSS).
        pub size: f32 = 24.0,
        pub color: Color = theme::text(),
        pub align: TextAlign = TextAlign::Left,
        pub vertical_align: VerticalAlign = VerticalAlign::Middle,
        /// Переносить слова, не влезающие в ширину прямоугольника.
        pub wrap: bool = true,
        /// Полужирное начертание (синтезируется из обычного).
        pub bold: bool = false,
        /// Множитель межстрочного интервала.
        pub line_spacing: f32 = 1.0,
        /// Обводка для читаемости поверх игры; 0 — без обводки.
        pub outline_width: f32 = 0.0,
        pub outline_color: Color = Color::rgba(0.0, 0.0, 0.0, 0.85),
        /// Сдвиг тени; (0, 0) — без тени.
        pub shadow_offset: Vec2 = Vec2::ZERO,
        pub shadow_color: Color = Color::rgba(0.0, 0.0, 0.0, 0.6),
    }
}

impl Component for Text {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Заменить текст, только если он другой (раскладка пересчитывается лишь при изменении).
    pub fn set(&mut self, text: &str) {
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
        }
    }
}

component! {
    /// Кнопка. Красит `Image` на своём объекте цветом состояния (обычное, наведение,
    /// нажатие, недоступна) и по клику шлёт сообщение `message` (пусто — имя объекта)
    /// себе и всем предкам, а также событие `UiEventKind::Click`.
    pub struct Button {
        pub interactable: bool = true,
        pub normal_color: Color = theme::button(),
        pub hover_color: Color = theme::button_hover(),
        pub pressed_color: Color = theme::button_pressed(),
        pub disabled_color: Color = theme::button_disabled(),
        /// Имя сообщения о клике; пусто — имя объекта.
        pub message: String = String::new(),
    }
}

impl Component for Button {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Button {
    /// Кнопка с именем сообщения.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }

    /// Акцентная (основная) кнопка.
    pub fn primary(message: impl Into<String>) -> Self {
        Self {
            normal_color: theme::accent(),
            hover_color: theme::accent_hover(),
            pressed_color: theme::accent_pressed(),
            ..Self::new(message)
        }
    }
}

component! {
    /// Переключатель (флажок или тумблер) у левого края своего прямоугольника; подпись —
    /// дочерний `Text`. По клику меняет `is_on` и шлёт сообщение со значением `Bool`.
    pub struct Toggle {
        pub is_on: bool = false,
        pub interactable: bool = true,
        pub style: ToggleStyle = ToggleStyle::Checkbox,
        /// Фон выключенного.
        pub off_color: Color = theme::field(),
        /// Фон включённого.
        pub on_color: Color = theme::accent(),
        /// Галочка / бегунок.
        pub check_color: Color = theme::handle(),
        /// Имя сообщения; пусто — имя объекта.
        pub message: String = String::new(),
    }
}

impl Component for Toggle {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Ползунок: дорожка, заполненная часть и ручка. Мышь тянет ручку, стрелки
    /// влево/вправо двигают её в фокусе. Шлёт сообщение со значением `Float`.
    pub struct Slider {
        pub value: f32 = 0.5,
        pub min: f32 = 0.0,
        pub max: f32 = 1.0,
        /// Только целые значения.
        pub whole_numbers: bool = false,
        pub interactable: bool = true,
        pub track_color: Color = theme::field(),
        pub fill_color: Color = theme::accent(),
        pub handle_color: Color = theme::handle(),
        /// Имя сообщения; пусто — имя объекта.
        pub message: String = String::new(),
    }
}

impl Component for Slider {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Slider {
    /// Значение в долях диапазона (0..=1).
    pub fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span.abs() < f32::EPSILON {
            0.0
        } else {
            ((self.value - self.min) / span).clamp(0.0, 1.0)
        }
    }

    /// Привести значение к диапазону (и к целым, если нужно).
    pub fn clamp_value(&self, value: f32) -> f32 {
        let (lo, hi) = if self.min <= self.max { (self.min, self.max) } else { (self.max, self.min) };
        let v = value.clamp(lo, hi);
        if self.whole_numbers {
            v.round()
        } else {
            v
        }
    }
}

component! {
    /// Полоса прогресса (здоровье, загрузка). Только отображает `value` (0..=1).
    pub struct ProgressBar {
        /// Заполненность, 0..=1.
        pub value: f32 = 1.0,
        pub background_color: Color = theme::field(),
        pub fill_color: Color = theme::good(),
        pub corner_radius: f32 = 6.0,
        pub direction: FillDirection = FillDirection::LeftToRight,
        /// Отступ заполнения от края фона.
        pub inset: f32 = 2.0,
    }
}

impl Component for ProgressBar {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Однострочное поле ввода. Клик (или Enter в фокусе) начинает ввод, Enter —
    /// событие «отправлено», Escape — выход из поля. Каждое изменение текста шлёт
    /// сообщение `message` со значением `Str`, Enter — `"<message>:submit"`.
    pub struct InputField {
        pub text: String = String::new(),
        /// Подсказка в пустом поле.
        pub placeholder: String = String::new(),
        /// Наибольшая длина в символах; 0 — без ограничения.
        pub max_length: u32 = 0,
        pub font_size: f32 = 22.0,
        pub interactable: bool = true,
        pub text_color: Color = theme::text(),
        pub placeholder_color: Color = theme::text_muted(),
        pub background_color: Color = theme::field(),
        pub border_color: Color = theme::field_border(),
        /// Рамка в фокусе.
        pub focus_color: Color = theme::accent(),
        pub corner_radius: f32 = 10.0,
        /// Отступ текста от краёв по горизонтали.
        pub padding: f32 = 14.0,
        /// Имя сообщения; пусто — имя объекта.
        pub message: String = String::new(),
    }
}

impl Component for InputField {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}
