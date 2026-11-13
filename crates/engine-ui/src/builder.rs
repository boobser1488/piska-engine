//! Короткое создание интерфейса из кода:
//!
//! ```
//! use engine_core::prelude::*;
//! use engine_ui::prelude::*;
//!
//! let mut scene = Scene::default();
//! let canvas = scene.ui_canvas().id();
//! let menu = scene.ui_panel(canvas).anchor(Anchor::Center).width(420.0).column(14.0).padding(32.0).fit().id();
//! scene.ui_text(menu, "Главное меню").font_size(44.0).bold().align(TextAlign::Center);
//! scene.ui_button(menu, "play", "Играть").primary();
//! scene.ui_button(menu, "settings", "Настройки");
//! scene.ui_button(menu, "quit", "Выход");
//! ```

use engine_core::prelude::*;

use crate::components::*;
use crate::theme;

/// Объект интерфейса в процессе настройки. Методы цепляются: `.anchor(..).size(..)`.
pub struct UiNode<'a> {
    scene: &'a mut Scene,
    id: GameObjectId,
}

impl UiNode<'_> {
    /// Готово: id объекта.
    pub fn id(self) -> GameObjectId {
        self.id
    }

    fn object(&mut self) -> &mut GameObject {
        self.scene
            .get_mut(self.id)
            .expect("UiNode refers to a live object: it was just created or passed in by the caller")
    }

    fn component<T: Component + Default>(&mut self) -> &mut T {
        let object = self.object();
        if !object.has::<T>() {
            object.add(T::default());
        }
        object.get_mut::<T>().expect("component was just ensured")
    }

    fn rect(&mut self) -> &mut RectTransform {
        self.component::<RectTransform>()
    }

    /// `Text` самого объекта или его первого ребёнка с текстом (подпись кнопки).
    fn text(&mut self) -> Option<&mut Text> {
        if self.object().has::<Text>() {
            return self.object().get_mut::<Text>();
        }
        let child = self
            .scene
            .get(self.id)?
            .children()
            .iter()
            .copied()
            .find(|&c| self.scene.get_component::<Text>(c).is_some())?;
        self.scene.get_component_mut::<Text>(child)
    }

    /// Добавить компонент (Unity: `AddComponent`).
    pub fn with<T: Component>(mut self, component: T) -> Self {
        self.object().add(component);
        self
    }

    /// Изменить компонент, который уже есть на объекте.
    pub fn edit<T: Component>(mut self, f: impl FnOnce(&mut T)) -> Self {
        if let Some(component) = self.object().get_mut::<T>() {
            f(component);
        }
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.object().name = name.into();
        self
    }

    /// Создать выключенным (скрытым).
    pub fn hidden(mut self) -> Self {
        self.object().active = false;
        self
    }

    // ----- прямоугольник -----

    /// Пресет якорей и опоры (смещение и размер сохраняются).
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.rect().set_anchor(anchor);
        self
    }

    /// Смещение опоры от якоря (единицы холста, Y — вниз).
    pub fn pos(mut self, x: f32, y: f32) -> Self {
        self.rect().position = Vec2::new(x, y);
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.rect().size = Vec2::new(width, height);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.rect().size.x = width;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.rect().size.y = height;
        self
    }

    /// На весь родительский прямоугольник.
    pub fn stretch(mut self) -> Self {
        *self.rect() = RectTransform::stretch();
        self
    }

    /// На весь родительский прямоугольник с отступами.
    pub fn inset(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        *self.rect() = RectTransform::inset(left, top, right, bottom);
        self
    }

    /// На весь родительский прямоугольник с одинаковым отступом.
    pub fn margin(self, margin: f32) -> Self {
        self.inset(margin, margin, margin, margin)
    }

    // ----- раскладка -----

    /// Раскладывать детей столбиком.
    pub fn column(mut self, spacing: f32) -> Self {
        let group = self.component::<LayoutGroup>();
        group.direction = LayoutDirection::Vertical;
        group.spacing = spacing;
        self
    }

    /// Раскладывать детей строкой.
    pub fn row(mut self, spacing: f32) -> Self {
        let group = self.component::<LayoutGroup>();
        group.direction = LayoutDirection::Horizontal;
        group.spacing = spacing;
        self
    }

    /// Одинаковые внутренние отступы раскладки.
    pub fn padding(mut self, padding: f32) -> Self {
        let group = self.component::<LayoutGroup>();
        group.padding_left = padding;
        group.padding_right = padding;
        group.padding_top = padding;
        group.padding_bottom = padding;
        self
    }

    /// Куда прижимать детей раскладки.
    pub fn child_align(mut self, alignment: Alignment) -> Self {
        self.component::<LayoutGroup>().child_alignment = alignment;
        self
    }

    /// Не растягивать детей поперёк раскладки.
    pub fn no_stretch(mut self) -> Self {
        self.component::<LayoutGroup>().stretch_children = false;
        self
    }

    /// Раздать свободное место детям поровну.
    pub fn expand(mut self) -> Self {
        self.component::<LayoutGroup>().expand_children = true;
        self
    }

    /// Подгонять размер под содержимое.
    pub fn fit(mut self) -> Self {
        self.component::<LayoutGroup>().fit_content = true;
        self
    }

    /// Обрезать детей по своему прямоугольнику.
    pub fn mask(self) -> Self {
        self.with(RectMask::default())
    }

    // ----- внешний вид -----

    /// Цвет: `Image`, иначе `Text`, иначе заполнение `ProgressBar`.
    pub fn color(mut self, color: Color) -> Self {
        let object = self.object();
        if let Some(image) = object.get_mut::<Image>() {
            image.color = color;
        } else if let Some(text) = object.get_mut::<Text>() {
            text.color = color;
        } else if let Some(bar) = object.get_mut::<ProgressBar>() {
            bar.fill_color = color;
        }
        self
    }

    /// Радиус скругления (`Image` или `ProgressBar`).
    pub fn radius(mut self, radius: f32) -> Self {
        let object = self.object();
        if let Some(image) = object.get_mut::<Image>() {
            image.corner_radius = radius;
        }
        if let Some(bar) = object.get_mut::<ProgressBar>() {
            bar.corner_radius = radius;
        }
        if let Some(field) = object.get_mut::<InputField>() {
            field.corner_radius = radius;
        }
        self
    }

    /// Рамка `Image`.
    pub fn border(mut self, width: f32, color: Color) -> Self {
        let image = self.component::<Image>();
        image.border_width = width;
        image.border_color = color;
        self
    }

    /// Мягкая тень под `Image`.
    pub fn shadow(mut self, size: f32) -> Self {
        self.component::<Image>().shadow_size = size;
        self
    }

    // ----- текст (свой или подписи) -----

    /// Кегль текста. У одиночного текста (не растянутого по высоте якорями) заодно
    /// ставит высоту прямоугольника под одну строку; другую задайте `.height()` после.
    pub fn font_size(mut self, size: f32) -> Self {
        let own = self.object().has::<Text>() && !self.object().has::<Button>();
        if let Some(text) = self.text() {
            text.size = size;
        }
        if let Some(field) = self.object().get_mut::<InputField>() {
            field.font_size = size;
        }
        let rect = self.rect();
        if own && rect.anchor_min.y == rect.anchor_max.y {
            rect.size.y = (size * 1.4).ceil();
        }
        self
    }

    pub fn bold(mut self) -> Self {
        if let Some(text) = self.text() {
            text.bold = true;
        }
        self
    }

    pub fn text_color(mut self, color: Color) -> Self {
        if let Some(text) = self.text() {
            text.color = color;
        }
        if let Some(field) = self.object().get_mut::<InputField>() {
            field.text_color = color;
        }
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        if let Some(text) = self.text() {
            text.align = align;
        }
        self
    }

    pub fn valign(mut self, align: VerticalAlign) -> Self {
        if let Some(text) = self.text() {
            text.vertical_align = align;
        }
        self
    }

    /// Обводка текста — читаемость поверх игры.
    pub fn outline(mut self, width: f32, color: Color) -> Self {
        if let Some(text) = self.text() {
            text.outline_width = width;
            text.outline_color = color;
        }
        self
    }

    /// Тень текста со сдвигом.
    pub fn text_shadow(mut self, x: f32, y: f32) -> Self {
        if let Some(text) = self.text() {
            text.shadow_offset = Vec2::new(x, y);
        }
        self
    }

    /// Без переноса строк.
    pub fn no_wrap(mut self) -> Self {
        if let Some(text) = self.text() {
            text.wrap = false;
        }
        self
    }

    /// Строка текста (свой `Text` или подпись).
    pub fn label(mut self, label: &str) -> Self {
        if let Some(text) = self.text() {
            text.set(label);
        }
        self
    }

    // ----- виджеты -----

    /// Акцентная кнопка.
    pub fn primary(mut self) -> Self {
        if let Some(button) = self.object().get_mut::<Button>() {
            button.normal_color = theme::accent();
            button.hover_color = theme::accent_hover();
            button.pressed_color = theme::accent_pressed();
        }
        self
    }

    /// Доступность виджета.
    pub fn interactable(mut self, interactable: bool) -> Self {
        let object = self.object();
        if let Some(c) = object.get_mut::<Button>() {
            c.interactable = interactable;
        }
        if let Some(c) = object.get_mut::<Toggle>() {
            c.interactable = interactable;
        }
        if let Some(c) = object.get_mut::<Slider>() {
            c.interactable = interactable;
        }
        if let Some(c) = object.get_mut::<InputField>() {
            c.interactable = interactable;
        }
        self
    }

    /// Переключатель в виде тумблера.
    pub fn switch(mut self) -> Self {
        if let Some(toggle) = self.object().get_mut::<Toggle>() {
            toggle.style = ToggleStyle::Switch;
        }
        let label = self.scene.get(self.id).and_then(|o| o.children().first().copied());
        if let Some(rt) = label.and_then(|l| self.scene.get_component_mut::<RectTransform>(l)) {
            *rt = RectTransform::inset(TOGGLE_SWITCH_LABEL, 0.0, 0.0, 0.0);
        }
        self
    }

    /// Ползунок только с целыми значениями.
    pub fn whole_numbers(mut self) -> Self {
        if let Some(slider) = self.object().get_mut::<Slider>() {
            slider.whole_numbers = true;
            slider.value = slider.value.round();
        }
        self
    }

    /// Наибольшая длина текста в поле ввода.
    pub fn max_length(mut self, chars: u32) -> Self {
        if let Some(field) = self.object().get_mut::<InputField>() {
            field.max_length = chars;
        }
        self
    }

    /// Начальный текст поля ввода.
    pub fn value(mut self, text: &str) -> Self {
        if let Some(field) = self.object().get_mut::<InputField>() {
            field.text = text.to_string();
        }
        self
    }

    // ----- холст -----

    pub fn sort_order(mut self, order: i32) -> Self {
        self.component::<Canvas>().sort_order = order;
        self
    }

    /// Разрешение, под которое свёрстан холст.
    pub fn reference_resolution(mut self, width: f32, height: f32) -> Self {
        self.component::<Canvas>().reference_resolution = Vec2::new(width, height);
        self
    }

    /// Выключить навигацию с клавиатуры (HUD: стрелки управляют игрой).
    pub fn no_navigation(mut self) -> Self {
        self.component::<Canvas>().keyboard_navigation = false;
        self
    }
}

/// Отступ подписи переключателя от левого края (флажок 24 + зазор 12).
const TOGGLE_CHECKBOX_LABEL: f32 = 36.0;
/// То же для тумблера (44 + 12).
const TOGGLE_SWITCH_LABEL: f32 = 56.0;

/// Конструкторы интерфейса прямо на сцене: `scene.ui_button(menu, "play", "Играть")`.
/// Работают с `app.world.scene` и `ctx.scene_mut()`.
pub trait UiScene {
    /// Настроить уже существующий объект теми же методами.
    fn ui_edit(&mut self, id: GameObjectId) -> UiNode<'_>;
    /// Холст в корне сцены.
    fn ui_canvas(&mut self) -> UiNode<'_>;
    /// Пустой прямоугольник (группа, контейнер).
    fn ui_node(&mut self, parent: GameObjectId, name: &str) -> UiNode<'_>;
    /// Панель в стиле темы (фон, скругление, рамка, тень).
    fn ui_panel(&mut self, parent: GameObjectId) -> UiNode<'_>;
    /// Прозрачный столбик с раскладкой.
    fn ui_column(&mut self, parent: GameObjectId, spacing: f32) -> UiNode<'_>;
    /// Прозрачная строка с раскладкой.
    fn ui_row(&mut self, parent: GameObjectId, spacing: f32) -> UiNode<'_>;
    fn ui_text(&mut self, parent: GameObjectId, text: &str) -> UiNode<'_>;
    fn ui_image(&mut self, parent: GameObjectId, color: Color) -> UiNode<'_>;
    /// Кнопка с подписью; `message` — имя сообщения о клике и имя объекта.
    fn ui_button(&mut self, parent: GameObjectId, message: &str, label: &str) -> UiNode<'_>;
    /// Флажок с подписью.
    fn ui_toggle(&mut self, parent: GameObjectId, message: &str, label: &str, on: bool) -> UiNode<'_>;
    fn ui_slider(&mut self, parent: GameObjectId, message: &str, min: f32, max: f32, value: f32) -> UiNode<'_>;
    /// Полоса прогресса, `value` — 0..=1.
    fn ui_progress(&mut self, parent: GameObjectId, value: f32) -> UiNode<'_>;
    fn ui_input(&mut self, parent: GameObjectId, message: &str, placeholder: &str) -> UiNode<'_>;
    /// Показать или скрыть элемент (флаг `active` объекта).
    fn ui_set_visible(&mut self, id: GameObjectId, visible: bool);
}

impl UiScene for Scene {
    fn ui_edit(&mut self, id: GameObjectId) -> UiNode<'_> {
        UiNode { scene: self, id }
    }

    fn ui_canvas(&mut self) -> UiNode<'_> {
        let id = self
            .build("Canvas")
            .with(Canvas::default())
            .with(RectTransform::stretch())
            .id();
        UiNode { scene: self, id }
    }

    fn ui_node(&mut self, parent: GameObjectId, name: &str) -> UiNode<'_> {
        let id = self
            .build(name)
            .child_of(parent)
            .with(RectTransform::default())
            .id();
        UiNode { scene: self, id }
    }

    fn ui_panel(&mut self, parent: GameObjectId) -> UiNode<'_> {
        let id = self
            .build("Panel")
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(420.0, 320.0),
                ..RectTransform::default()
            })
            .with(Image::panel())
            .id();
        UiNode { scene: self, id }
    }

    fn ui_column(&mut self, parent: GameObjectId, spacing: f32) -> UiNode<'_> {
        self.ui_node(parent, "Column").size(320.0, 240.0).column(spacing)
    }

    fn ui_row(&mut self, parent: GameObjectId, spacing: f32) -> UiNode<'_> {
        self.ui_node(parent, "Row").size(320.0, 48.0).row(spacing)
    }

    fn ui_text(&mut self, parent: GameObjectId, text: &str) -> UiNode<'_> {
        let id = self
            .build("Text")
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(320.0, 34.0),
                ..RectTransform::default()
            })
            .with(Text::new(text))
            .id();
        UiNode { scene: self, id }
    }

    fn ui_image(&mut self, parent: GameObjectId, color: Color) -> UiNode<'_> {
        let id = self
            .build("Image")
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(100.0, 100.0),
                ..RectTransform::default()
            })
            .with(Image::colored(color))
            .id();
        UiNode { scene: self, id }
    }

    fn ui_button(&mut self, parent: GameObjectId, message: &str, label: &str) -> UiNode<'_> {
        let id = self
            .build(message)
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(260.0, 52.0),
                ..RectTransform::default()
            })
            .with(Image {
                corner_radius: 10.0,
                ..Image::default()
            })
            .with(Button::new(message))
            .id();
        self.build("Label")
            .child_of(id)
            .with(RectTransform::stretch())
            .with(Text {
                size: 21.0,
                bold: true,
                align: TextAlign::Center,
                wrap: false,
                ..Text::new(label)
            })
            .id();
        UiNode { scene: self, id }
    }

    fn ui_toggle(&mut self, parent: GameObjectId, message: &str, label: &str, on: bool) -> UiNode<'_> {
        let id = self
            .build(message)
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(260.0, 36.0),
                ..RectTransform::default()
            })
            .with(Toggle {
                is_on: on,
                message: message.to_string(),
                ..Toggle::default()
            })
            .id();
        self.build("Label")
            .child_of(id)
            .with(RectTransform::inset(TOGGLE_CHECKBOX_LABEL, 0.0, 0.0, 0.0))
            .with(Text {
                size: 20.0,
                wrap: false,
                ..Text::new(label)
            })
            .id();
        UiNode { scene: self, id }
    }

    fn ui_slider(&mut self, parent: GameObjectId, message: &str, min: f32, max: f32, value: f32) -> UiNode<'_> {
        let id = self
            .build(message)
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(260.0, 32.0),
                ..RectTransform::default()
            })
            .with(Slider {
                value,
                min,
                max,
                message: message.to_string(),
                ..Slider::default()
            })
            .id();
        UiNode { scene: self, id }
    }

    fn ui_progress(&mut self, parent: GameObjectId, value: f32) -> UiNode<'_> {
        let id = self
            .build("Progress")
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(260.0, 14.0),
                ..RectTransform::default()
            })
            .with(ProgressBar {
                value,
                ..ProgressBar::default()
            })
            .id();
        UiNode { scene: self, id }
    }

    fn ui_input(&mut self, parent: GameObjectId, message: &str, placeholder: &str) -> UiNode<'_> {
        let id = self
            .build(message)
            .child_of(parent)
            .with(RectTransform {
                size: Vec2::new(300.0, 48.0),
                ..RectTransform::default()
            })
            .with(InputField {
                placeholder: placeholder.to_string(),
                message: message.to_string(),
                ..InputField::default()
            })
            .id();
        UiNode { scene: self, id }
    }

    fn ui_set_visible(&mut self, id: GameObjectId, visible: bool) {
        if let Some(object) = self.get_mut(id) {
            object.active = visible;
        }
    }
}
