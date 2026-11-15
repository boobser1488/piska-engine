//! `UiSystem` — подсистема интерфейса.
//!
//! * `pre_update` (до скриптов): мышь и клавиатура → состояние виджетов, события
//!   [`UiEvents`] и сообщения. Попадание считается по раскладке прошлого кадра — ровно
//!   по тому, что игрок видит на экране.
//! * `post_update` (после скриптов): раскладка всех холстов и сборка списка отрисовки
//!   [`Overlay`] для рендера. В редакторе в режиме Edit то же делает `idle`, поэтому
//!   вид Game показывает интерфейс и без Play.
//!
//! В установившемся режиме кадр не выделяет память: буферы переиспользуются, раскладка
//! текста пересчитывается только при изменении строки или размеров.

use std::collections::HashMap;

use engine_assets::{AssetDatabase, Guid};
use engine_core::prelude::*;
use engine_render::{Overlay, OverlayImage, OverlayTexture};

use crate::components::*;
use crate::draw::{fade, mix, Painter};
use crate::events::{submit_message, UiEventKind, UiEvents, UiEvent};
use crate::layout::{canvas_scale, content_size, layout_children, resolve_rect, Rect};
use crate::text::{Atlas, TextEngine, TextLayout, TextParams};
use crate::theme;

/// Отступ дорожки ползунка от краёв (единицы холста): там помещается ручка.
const SLIDER_MARGIN: f32 = 10.0;
/// Через сколько кадров забывать кэши невидимых элементов.
const PRUNE_FRAMES: u64 = 240;
/// Задержка и период автоповтора клавиш в поле ввода (секунды).
const REPEAT_DELAY: f32 = 0.45;
const REPEAT_RATE: f32 = 0.04;
/// Пауза между шагами фокуса при удержании стрелки или стика: меню листается медленнее,
/// чем движется курсор в тексте.
const NAV_REPEAT_RATE: f32 = 0.14;
const CARET_BLINK: f32 = 0.53;

/// Слоты кэша раскладки текста у одного объекта.
const SLOT_TEXT: u8 = 0;
const SLOT_INPUT: u8 = 1;

/// Элемент, который ловит мышь (в порядке отрисовки).
#[derive(Clone, Copy)]
struct Hit {
    rect: Rect,
    clip: Option<Rect>,
    /// Виджет, которому достаётся клик (сам элемент или ближайший предок-виджет).
    owner: Option<GameObjectId>,
    enabled: bool,
}

/// Геометрия виджета с прошлого кадра: рабочий отрезок по X
/// (дорожка ползунка, область текста поля ввода).
#[derive(Clone, Copy, Default)]
struct Geometry {
    x0: f32,
    x1: f32,
}

/// Плавные переходы состояния виджета.
#[derive(Clone, Copy, Default)]
struct Animation {
    hover: f32,
    press: f32,
    on: f32,
    seen: u64,
    started: bool,
}

/// Контекст обхода ветки.
#[derive(Clone, Copy)]
struct Walk {
    scale: f32,
    alpha: f32,
    interactive: bool,
    blocks: bool,
    clip: Option<Rect>,
    owner: Option<GameObjectId>,
    navigation: bool,
    /// Ветка внутри недоступного виджета: подпись бледнее.
    dimmed: bool,
}

/// Клавиши, которые интересны интерфейсу.
const KEYS: [Key; 13] = [
    Key::Tab,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Enter,
    Key::Escape,
    Key::Backspace,
    Key::Delete,
    Key::Home,
    Key::End,
    Key::LShift,
    Key::RShift,
];
const K_TAB: usize = 0;
const K_UP: usize = 1;
const K_DOWN: usize = 2;
const K_LEFT: usize = 3;
const K_RIGHT: usize = 4;
const K_ENTER: usize = 5;
const K_ESCAPE: usize = 6;
const K_BACKSPACE: usize = 7;
const K_DELETE: usize = 8;
const K_HOME: usize = 9;
const K_END: usize = 10;
const K_LSHIFT: usize = 11;
const K_RSHIFT: usize = 12;

/// Отклонение левого стика, после которого он считается направлением (−1..1).
const PAD_THRESHOLD: f32 = 0.5;

/// Направления геймпада в порядке `K_UP, K_DOWN, K_LEFT, K_RIGHT`: крестовина или левый
/// стик за порогом (вверх и вправо у стика — плюс).
fn pad_directions(input: &Input) -> [bool; 4] {
    let x = input.any_gamepad_axis(GamepadAxis::LeftStickX);
    let y = input.any_gamepad_axis(GamepadAxis::LeftStickY);
    [
        input.any_gamepad_button(GamepadButton::DPadUp) || y > PAD_THRESHOLD,
        input.any_gamepad_button(GamepadButton::DPadDown) || y < -PAD_THRESHOLD,
        input.any_gamepad_button(GamepadButton::DPadLeft) || x < -PAD_THRESHOLD,
        input.any_gamepad_button(GamepadButton::DPadRight) || x > PAD_THRESHOLD,
    ]
}

#[derive(Default)]
struct KeySnapshot {
    down: [bool; KEYS.len()],
    held: [bool; KEYS.len()],
}

/// Подсистема интерфейса. Подключается [`install`](crate::install).
pub struct UiSystem {
    text: TextEngine,
    layouts: HashMap<(GameObjectId, u8), TextLayout>,
    animations: HashMap<GameObjectId, Animation>,
    geometry: HashMap<GameObjectId, Geometry>,
    texture_sizes: HashMap<Guid, Option<[u32; 2]>>,
    canvases: Vec<(i32, u32, GameObjectId)>,
    hits: Vec<Hit>,
    focusables: Vec<GameObjectId>,
    child_sizes: Vec<Vec2>,
    child_rects: Vec<Rect>,
    typed: String,

    hovered: Option<GameObjectId>,
    pressed: Option<GameObjectId>,
    focused: Option<GameObjectId>,
    focus_visible: bool,
    editing: bool,
    caret: usize,
    caret_clock: f32,
    input_scroll: f32,
    repeat: Option<(usize, f32)>,
    /// Направления геймпада в прошлом кадре (для «нажатия» стика).
    pad_prev: [bool; 4],

    frame: u64,
    real_dt: f32,
}

impl Default for UiSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl UiSystem {
    pub fn new() -> Self {
        Self {
            text: TextEngine::new(),
            layouts: HashMap::new(),
            animations: HashMap::new(),
            geometry: HashMap::new(),
            texture_sizes: HashMap::new(),
            canvases: Vec::new(),
            hits: Vec::new(),
            focusables: Vec::new(),
            child_sizes: Vec::new(),
            child_rects: Vec::new(),
            typed: String::new(),
            hovered: None,
            pressed: None,
            focused: None,
            focus_visible: false,
            editing: false,
            caret: 0,
            caret_clock: 0.0,
            input_scroll: 0.0,
            repeat: None,
            pad_prev: [false; 4],
            frame: 0,
            real_dt: 1.0 / 60.0,
        }
    }

    // ==================================================================
    // Раскладка и отрисовка
    // ==================================================================

    fn build(&mut self, world: &mut World) {
        self.frame += 1;
        let screen = world.input.screen_size();
        let mut overlay = world
            .resources
            .get_mut::<Overlay>()
            .map(std::mem::take)
            .unwrap_or_default();
        overlay.clear();
        overlay.size = screen;
        if overlay.images.is_empty() {
            overlay.images.push(OverlayImage::default());
        }
        self.text.atlas.init_image(&mut overlay.images[0]);
        self.hits.clear();
        self.focusables.clear();
        self.geometry.clear();

        let scene = &world.scene;
        self.canvases.clear();
        // Холсты — по индексу сцены (уже в порядке иерархии), без обхода всех объектов.
        // Холст внутри другого холста рисуется в составе внешнего.
        let mut sequence = 0;
        for id in scene.find_with::<Canvas>() {
            if !scene.is_active(id) || has_canvas_ancestor(scene, id) {
                continue;
            }
            let order = scene.get_component::<Canvas>(id).map_or(0, |canvas| canvas.sort_order);
            self.canvases.push((order, sequence, id));
            sequence += 1;
        }
        self.canvases.sort_unstable_by_key(|&(order, seq, _)| (order, seq));

        let assets = world.resources.get::<AssetDatabase>();
        let screen_rect = Rect::from_min_size(Vec2::ZERO, screen);
        for index in 0..self.canvases.len() {
            let id = self.canvases[index].2;
            let Some(canvas) = scene.get_component::<Canvas>(id) else {
                continue;
            };
            let walk = Walk {
                scale: canvas_scale(canvas, screen),
                alpha: 1.0,
                interactive: true,
                blocks: true,
                clip: None,
                owner: None,
                navigation: canvas.keyboard_navigation,
                dimmed: false,
            };
            self.visit(scene, id, screen_rect, walk, &mut overlay, assets);
        }
        self.text.atlas.publish(&mut overlay.images[0]);

        if self.frame.is_multiple_of(PRUNE_FRAMES) {
            let frame = self.frame;
            self.layouts.retain(|_, layout| layout.seen + PRUNE_FRAMES > frame);
            self.animations.retain(|_, anim| anim.seen + PRUNE_FRAMES > frame);
        }
        match world.resources.get_mut::<Overlay>() {
            Some(slot) => *slot = overlay,
            None => {
                world.resources.insert(overlay);
            }
        }
    }

    /// Желаемый размер элемента в пикселях (для раскладки родителя).
    fn preferred_size(&mut self, scene: &Scene, id: GameObjectId, scale: f32) -> Vec2 {
        let Some(object) = scene.get(id) else {
            return Vec2::ZERO;
        };
        let own = object
            .get::<RectTransform>()
            .map_or(Vec2::ZERO, |rt| rt.size * scale);
        match object.get::<LayoutGroup>() {
            Some(group) if group.fit_content => {
                let base = self.child_sizes.len();
                for &child in object.children() {
                    if scene.get(child).is_some_and(|c| c.active) {
                        let size = self.preferred_size(scene, child, scale);
                        self.child_sizes.push(size);
                    }
                }
                let size = content_size(group, scale, &self.child_sizes[base..], own);
                self.child_sizes.truncate(base);
                size
            }
            _ => own,
        }
    }

    /// Прямоугольник ребёнка по его `RectTransform` (без раскладки родителя).
    fn child_rect(&mut self, scene: &Scene, child: GameObjectId, parent: Rect, scale: f32) -> Rect {
        let Some(object) = scene.get(child) else {
            return parent;
        };
        let Some(rt) = object.get::<RectTransform>() else {
            return parent;
        };
        let fit = match object.get::<LayoutGroup>() {
            Some(group) if group.fit_content => Some(self.preferred_size(scene, child, scale)),
            _ => None,
        };
        resolve_rect(rt, parent, scale, fit)
    }

    fn painter<'a>(atlas: &Atlas, overlay: &'a mut Overlay, clip: Option<Rect>) -> Painter<'a> {
        Painter {
            overlay,
            white: atlas.white_uv(),
            atlas_size: atlas.size() as f32,
            clip: clip.map(|c| [c.min.x, c.min.y, c.width(), c.height()]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        atlas: &Atlas,
        overlay: &mut Overlay,
        clip: Option<Rect>,
        layout: &TextLayout,
        origin: Vec2,
        color: Color,
    ) {
        if color.a <= 0.0 {
            return;
        }
        let mut painter = Self::painter(atlas, overlay, clip);
        for glyph in &layout.glyphs {
            let entry = atlas.entry(glyph.entry);
            painter.glyph(
                origin + glyph.position,
                Vec2::new(entry.size[0] as f32, entry.size[1] as f32),
                entry.origin,
                entry.size,
                color,
            );
        }
    }

    fn visit(
        &mut self,
        scene: &Scene,
        id: GameObjectId,
        rect: Rect,
        mut walk: Walk,
        out: &mut Overlay,
        assets: Option<&AssetDatabase>,
    ) {
        let Some(object) = scene.get(id) else {
            return;
        };
        if !object.active {
            return;
        }
        if let Some(group) = object.get::<CanvasGroup>() {
            walk.alpha *= group.alpha.clamp(0.0, 1.0);
            walk.interactive &= group.interactable;
            walk.blocks &= group.blocks_raycasts;
        }
        if let Some(canvas) = object.get::<Canvas>() {
            walk.navigation = canvas.keyboard_navigation;
        }
        let scale = walk.scale;
        let frame = self.frame;
        let dt = self.real_dt;
        let snapped = rect.snapped();
        let alpha = walk.alpha;

        let button = object.get::<Button>();
        let toggle = object.get::<Toggle>();
        let slider = object.get::<Slider>();
        let input = object.get::<InputField>();
        let progress = object.get::<ProgressBar>();
        let interactable = button
            .map(|b| b.interactable)
            .or(toggle.map(|t| t.interactable))
            .or(slider.map(|s| s.interactable))
            .or(input.map(|i| i.interactable));
        let is_widget = interactable.is_some();
        let enabled = interactable.unwrap_or(false) && walk.interactive;

        // Плавные состояния: наведение, нажатие, «включено».
        let mut anim = Animation::default();
        if is_widget {
            let hovered = enabled && self.hovered == Some(id);
            let pressed = hovered && self.pressed == Some(id);
            let on = toggle.is_some_and(|t| t.is_on);
            let state = self.animations.entry(id).or_default();
            if !state.started {
                state.on = on as u8 as f32;
                state.started = true;
            }
            let step = (dt * 14.0).min(1.0);
            let approach = |value: &mut f32, target: bool| {
                let target = target as u8 as f32;
                *value += (target - *value) * step;
                if (*value - target).abs() < 0.01 {
                    *value = target;
                }
            };
            approach(&mut state.hover, hovered);
            approach(&mut state.press, pressed);
            approach(&mut state.on, on);
            state.seen = frame;
            anim = *state;
        }
        let focus_ring = is_widget && self.focus_visible && self.focused == Some(id);

        // ---------------- картинка ----------------
        let mut radius = 0.0;
        let mut catches_mouse = is_widget;
        if let Some(image) = object.get::<Image>() {
            catches_mouse |= image.raycast_target;
            radius = image.corner_radius * scale;
            let mut color = image.color;
            if let Some(button) = button {
                color = if enabled {
                    mix(
                        mix(button.normal_color, button.hover_color, anim.hover),
                        button.pressed_color,
                        anim.press,
                    )
                } else {
                    button.disabled_color
                };
            }
            if alpha > 0.0 {
                let border = image.border_width * scale;
                let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
                if image.shadow_size > 0.0 {
                    painter.shadow(
                        snapped.translate(image.shadow_offset * scale),
                        radius,
                        image.shadow_size * scale,
                        fade(image.shadow_color, alpha),
                    );
                }
                if image.texture.is_none() {
                    painter.rect(snapped, fade(color, alpha), radius, border, fade(image.border_color, alpha));
                } else {
                    let guid = image.texture.guid();
                    let texture = OverlayTexture::Asset(guid);
                    let size = texture_size(&mut self.texture_sizes, guid, assets);
                    match size {
                        Some([tw, th]) if image.slice_border > 0.0 => {
                            let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
                            nine_slice(&mut painter, texture, snapped, [tw, th], image.slice_border, scale, fade(color, alpha), radius);
                        }
                        _ => {
                            let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
                            painter.textured(texture, snapped, [0.0, 0.0, 1.0, 1.0], fade(color, alpha), snapped, radius);
                        }
                    }
                    if border > 0.0 {
                        let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
                        painter.rect(snapped, Color::TRANSPARENT, radius, border, fade(image.border_color, alpha));
                    }
                }
            }
        }

        // ---------------- виджеты ----------------
        if let Some(toggle) = toggle {
            self.draw_toggle(out, walk.clip, snapped, toggle, anim, scale, alpha);
        }
        if let Some(slider) = slider {
            self.draw_slider(id, out, walk.clip, snapped, slider, anim, scale, alpha);
        }
        if let Some(bar) = progress {
            let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
            let r = bar.corner_radius * scale;
            painter.rect(snapped, fade(bar.background_color, alpha), r, 0.0, Color::TRANSPARENT);
            let inner = snapped.shrink(bar.inset * scale);
            let value = bar.value.clamp(0.0, 1.0);
            let size = inner.size();
            let fill = match bar.direction {
                FillDirection::LeftToRight => Rect::from_min_size(inner.min, Vec2::new(size.x * value, size.y)),
                FillDirection::RightToLeft => Rect {
                    min: Vec2::new(inner.max.x - size.x * value, inner.min.y),
                    max: inner.max,
                },
                FillDirection::TopToBottom => Rect::from_min_size(inner.min, Vec2::new(size.x, size.y * value)),
                FillDirection::BottomToTop => Rect {
                    min: Vec2::new(inner.min.x, inner.max.y - size.y * value),
                    max: inner.max,
                },
            };
            if value > 0.0 {
                painter.rect(fill.snapped(), fade(bar.fill_color, alpha), (r - bar.inset * scale).max(0.0), 0.0, Color::TRANSPARENT);
            }
        }
        if let Some(input) = input {
            self.draw_input(id, out, walk.clip, snapped, input, anim, scale, alpha);
        }
        if focus_ring && alpha > 0.0 {
            let grow = 3.0 * scale;
            let mut painter = Self::painter(&self.text.atlas, out, walk.clip);
            let ring_radius = if toggle.is_some() || slider.is_some() { 8.0 * scale } else { radius + grow };
            painter.rect(snapped.shrink(-grow), Color::TRANSPARENT, ring_radius, (2.0 * scale).max(1.5), fade(theme::accent(), alpha));
        }

        // ---------------- текст ----------------
        if let Some(text) = object.get::<Text>() {
            if !text.text.is_empty() && alpha > 0.0 {
                let font = self.text.fonts.resolve(text.font, assets);
                let params = TextParams {
                    font,
                    size_px: text.size * scale,
                    bold: text.bold,
                    align: text.align,
                    vertical_align: text.vertical_align,
                    wrap: text.wrap,
                    line_spacing: text.line_spacing,
                    box_size: snapped.size(),
                };
                let layout = self.layouts.entry((id, SLOT_TEXT)).or_default();
                self.text.layout(layout, &text.text, params, &mut out.images[0]);
                layout.seen = frame;
                let alpha = if walk.dimmed { alpha * 0.45 } else { alpha };
                let atlas = &self.text.atlas;
                if text.shadow_offset != Vec2::ZERO {
                    let offset = (text.shadow_offset * scale).round();
                    Self::draw_text(atlas, out, walk.clip, layout, snapped.min + offset, fade(text.shadow_color, alpha));
                }
                if text.outline_width > 0.0 {
                    let w = (text.outline_width * scale).max(1.0);
                    let d = (w * std::f32::consts::FRAC_1_SQRT_2).round();
                    let w = w.round();
                    for offset in [
                        Vec2::new(w, 0.0),
                        Vec2::new(-w, 0.0),
                        Vec2::new(0.0, w),
                        Vec2::new(0.0, -w),
                        Vec2::new(d, d),
                        Vec2::new(-d, d),
                        Vec2::new(d, -d),
                        Vec2::new(-d, -d),
                    ] {
                        Self::draw_text(atlas, out, walk.clip, layout, snapped.min + offset, fade(text.outline_color, alpha));
                    }
                }
                Self::draw_text(atlas, out, walk.clip, layout, snapped.min, fade(text.color, alpha));
            }
        }

        // ---------------- ввод ----------------
        if catches_mouse && walk.blocks {
            self.hits.push(Hit {
                rect: snapped,
                clip: walk.clip,
                owner: if is_widget { Some(id) } else { walk.owner },
                enabled: if is_widget { enabled } else { walk.owner.is_some() && walk.interactive },
            });
        }
        if enabled && walk.navigation {
            self.focusables.push(id);
        }

        // ---------------- дети ----------------
        let mut child_walk = walk;
        if is_widget {
            child_walk.owner = Some(id);
            child_walk.dimmed |= !enabled;
        }
        if let Some(mask) = object.get::<RectMask>() {
            let area = snapped.shrink(mask.padding * scale);
            child_walk.clip = Some(walk.clip.map_or(area, |clip| clip.intersect(&area)));
        }
        let children = object.children();
        if let Some(group) = object.get::<LayoutGroup>() {
            let sizes_base = self.child_sizes.len();
            for &child in children {
                if scene.get(child).is_some_and(|c| c.active) {
                    let size = self.preferred_size(scene, child, scale);
                    self.child_sizes.push(size);
                }
            }
            let rects_base = self.child_rects.len();
            layout_children(group, rect, scale, &self.child_sizes[sizes_base..], &mut self.child_rects);
            let mut index = rects_base;
            for &child in children {
                if scene.get(child).is_some_and(|c| c.active) {
                    let child_rect = self.child_rects[index];
                    index += 1;
                    self.visit(scene, child, child_rect, child_walk, out, assets);
                }
            }
            self.child_rects.truncate(rects_base);
            self.child_sizes.truncate(sizes_base);
        } else {
            for &child in children {
                let child_rect = self.child_rect(scene, child, rect, scale);
                self.visit(scene, child, child_rect, child_walk, out, assets);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_toggle(
        &mut self,
        out: &mut Overlay,
        clip: Option<Rect>,
        rect: Rect,
        toggle: &Toggle,
        anim: Animation,
        scale: f32,
        alpha: f32,
    ) {
        let mut painter = Self::painter(&self.text.atlas, out, clip);
        let center_y = rect.center().y;
        let idle_border = mix(theme::field_border(), theme::text_muted(), anim.hover * 0.6);
        let fill = fade(mix(toggle.off_color, toggle.on_color, anim.on), alpha);
        let border = fade(mix(idle_border, toggle.on_color, anim.on), alpha);
        match toggle.style {
            ToggleStyle::Checkbox => {
                let side = (24.0 * scale).min(rect.height()).round();
                let b = Rect::new(rect.min.x, (center_y - side * 0.5).round(), side, side);
                painter.rect(b, fill, 6.0 * scale, (1.5 * scale).max(1.0), border);
                if anim.on > 0.02 {
                    let at = |x: f32, y: f32| b.min + Vec2::new(x, y) * side;
                    let check = fade(toggle.check_color, alpha * anim.on);
                    let thickness = (2.6 * scale).max(1.5);
                    painter.line(at(0.27, 0.53), at(0.43, 0.69), thickness, check);
                    painter.line(at(0.43, 0.69), at(0.74, 0.36), thickness, check);
                }
            }
            ToggleStyle::Switch => {
                let height = (24.0 * scale).min(rect.height()).round();
                let width = (44.0 * scale).round();
                let track = Rect::new(rect.min.x, (center_y - height * 0.5).round(), width, height);
                painter.rect(track, fill, height * 0.5, (1.5 * scale).max(1.0), fade(mix(idle_border, toggle.on_color, anim.on), alpha));
                let pad = 3.0 * scale;
                let knob = height - pad * 2.0;
                let x = track.min.x + pad + (width - pad * 2.0 - knob) * anim.on;
                let knob_rect = Rect::new(x, track.min.y + pad, knob, knob);
                painter.shadow(knob_rect.translate(Vec2::new(0.0, scale)), knob * 0.5, 3.0 * scale, Color::rgba(0.0, 0.0, 0.0, 0.35 * alpha));
                painter.rect(knob_rect, fade(toggle.check_color, alpha), knob * 0.5, 0.0, Color::TRANSPARENT);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_slider(
        &mut self,
        id: GameObjectId,
        out: &mut Overlay,
        clip: Option<Rect>,
        rect: Rect,
        slider: &Slider,
        anim: Animation,
        scale: f32,
        alpha: f32,
    ) {
        let margin = SLIDER_MARGIN * scale;
        let x0 = rect.min.x + margin;
        let x1 = (rect.max.x - margin).max(x0);
        self.geometry.insert(id, Geometry { x0, x1 });
        let mut painter = Self::painter(&self.text.atlas, out, clip);
        let cy = rect.center().y;
        let track_h = (6.0 * scale).max(2.0).round();
        let top = (cy - track_h * 0.5).round();
        let fraction = slider.fraction();
        let hx = x0 + (x1 - x0) * fraction;
        painter.rect(
            Rect::new(x0 - track_h * 0.5, top, x1 - x0 + track_h, track_h),
            fade(slider.track_color, alpha),
            track_h * 0.5,
            (scale).max(1.0),
            fade(theme::field_border().with_alpha(0.6), alpha),
        );
        painter.rect(
            Rect::new(x0 - track_h * 0.5, top, hx - x0 + track_h, track_h),
            fade(slider.fill_color, alpha),
            track_h * 0.5,
            0.0,
            Color::TRANSPARENT,
        );
        let r = 9.0 * scale * (1.0 + 0.12 * anim.hover + 0.08 * anim.press);
        let handle = Rect::new(hx - r, cy - r, r * 2.0, r * 2.0);
        painter.shadow(handle.translate(Vec2::new(0.0, scale)), r, 4.0 * scale, Color::rgba(0.0, 0.0, 0.0, 0.45 * alpha));
        painter.rect(handle, fade(slider.handle_color, alpha), r, 0.0, Color::TRANSPARENT);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_input(
        &mut self,
        id: GameObjectId,
        out: &mut Overlay,
        clip: Option<Rect>,
        rect: Rect,
        input: &InputField,
        anim: Animation,
        scale: f32,
        alpha: f32,
    ) {
        let editing = self.editing && self.focused == Some(id);
        let pad = input.padding * scale;
        let inner = Rect {
            min: Vec2::new(rect.min.x + pad, rect.min.y),
            max: Vec2::new((rect.max.x - pad).max(rect.min.x + pad), rect.max.y),
        };
        self.geometry.insert(id, Geometry { x0: inner.min.x, x1: inner.max.x });
        {
            let mut painter = Self::painter(&self.text.atlas, out, clip);
            let (border, width) = if editing {
                (input.focus_color, (2.0 * scale).max(1.5))
            } else {
                (mix(input.border_color, theme::text_muted(), anim.hover * 0.5), scale.max(1.0))
            };
            painter.rect(rect, fade(input.background_color, alpha), input.corner_radius * scale, width, fade(border, alpha));
        }

        let empty = input.text.is_empty();
        let (shown, color) = if empty {
            (input.placeholder.as_str(), input.placeholder_color)
        } else {
            (input.text.as_str(), input.text_color)
        };
        let params = TextParams {
            font: crate::text::DEFAULT_FONT,
            size_px: input.font_size * scale,
            bold: false,
            align: TextAlign::Left,
            vertical_align: VerticalAlign::Middle,
            wrap: false,
            line_spacing: 1.0,
            box_size: inner.size(),
        };
        let layout = self.layouts.entry((id, SLOT_INPUT)).or_default();
        self.text.layout(layout, shown, params, &mut out.images[0]);
        layout.seen = self.frame;

        // Прокрутка: каретка всегда видна.
        let caret_x = if empty {
            0.0
        } else {
            layout.carets.get(self.caret).or(layout.carets.last()).copied().unwrap_or(0.0)
        };
        let mut offset = 0.0;
        if editing {
            let room = inner.width() - 2.0 * scale;
            offset = self.input_scroll.min(0.0);
            if caret_x + offset > room {
                offset = room - caret_x;
            }
            if caret_x + offset < 0.0 {
                offset = -caret_x;
            }
            self.input_scroll = offset;
        }
        let text_clip = Some(clip.map_or(inner, |c| c.intersect(&inner)));
        Self::draw_text(&self.text.atlas, out, text_clip, layout, inner.min + Vec2::new(offset.round(), 0.0), fade(color, alpha));

        if editing && (self.caret_clock % (CARET_BLINK * 2.0)) < CARET_BLINK {
            let mut painter = Self::painter(&self.text.atlas, out, text_clip);
            let x = (inner.min.x + caret_x + offset).round();
            let width = (2.0 * scale).round().max(1.0);
            let height = layout.line_height.max(4.0);
            let top = (inner.center().y - height * 0.5).round();
            painter.rect(Rect::new(x, top, width, height), fade(input.text_color, alpha), 0.0, 0.0, Color::TRANSPARENT);
        }
    }

    // ==================================================================
    // Ввод
    // ==================================================================

    fn interact(&mut self, world: &mut World) {
        // Реальное время: анимации интерфейса идут и на паузе (`time_scale = 0`).
        self.real_dt = world.time.unscaled_delta.clamp(0.0, 0.25);
        self.caret_clock += self.real_dt;

        let mut events = world
            .resources
            .get_mut::<UiEvents>()
            .map(std::mem::take)
            .unwrap_or_default();
        events.events.clear();

        // Снимок ввода: дальше мир меняется (значения виджетов, сообщения).
        let input = &world.input;
        let mouse = input.mouse_position();
        let mouse_down = input.mouse_button_down(MouseButton::Left);
        let mouse_held = input.mouse_button(MouseButton::Left);
        let mouse_up = input.mouse_button_up(MouseButton::Left);
        let scroll = input.scroll_delta().y;
        let mut keys = KeySnapshot::default();
        for (i, key) in KEYS.iter().enumerate() {
            keys.down[i] = input.key_down(*key);
            keys.held[i] = input.key(*key);
        }
        // Геймпад вводит те же события: крестовина/стик — стрелки (с автоповтором),
        // South — Enter, East — Escape.
        let pad = pad_directions(input);
        for (i, &held) in pad.iter().enumerate() {
            keys.held[K_UP + i] |= held;
            keys.down[K_UP + i] |= held && !self.pad_prev[i];
        }
        self.pad_prev = pad;
        keys.down[K_ENTER] |= input.any_gamepad_button_down(GamepadButton::South);
        keys.held[K_ENTER] |= input.any_gamepad_button(GamepadButton::South);
        keys.down[K_ESCAPE] |= input.any_gamepad_button_down(GamepadButton::East);
        keys.held[K_ESCAPE] |= input.any_gamepad_button(GamepadButton::East);
        self.typed.clear();
        self.typed.push_str(input.text_input());

        // Наведение: верхний элемент под курсором.
        let mut over_ui = false;
        let mut hovered = None;
        for hit in self.hits.iter().rev() {
            if hit.rect.contains(mouse) && hit.clip.is_none_or(|clip| clip.contains(mouse)) {
                over_ui = true;
                hovered = hit.owner.filter(|_| hit.enabled);
                break;
            }
        }
        self.hovered = hovered;

        // Фокус на исчезнувшем виджете снимается.
        if let Some(focused) = self.focused {
            if !world.scene.contains(focused) || !self.focusables.contains(&focused) && !self.geometry.contains_key(&focused) {
                self.focused = None;
                self.editing = false;
            }
        }

        // ---------------- мышь ----------------
        if mouse_down {
            self.pressed = hovered;
            self.focus_visible = false;
            self.editing = false;
            self.focused = hovered;
            if let Some(id) = hovered {
                if world.scene.get_component::<InputField>(id).is_some() {
                    self.begin_editing(world, id);
                    self.caret = self.caret_at(id, mouse.x);
                }
                if world.scene.get_component::<Slider>(id).is_some() {
                    self.drag_slider(world, &mut events, id, mouse.x);
                }
            }
        } else if mouse_held {
            if let Some(id) = self.pressed {
                if world.scene.get_component::<Slider>(id).is_some() {
                    self.drag_slider(world, &mut events, id, mouse.x);
                }
            }
        }
        if mouse_up {
            if let Some(id) = self.pressed.take() {
                if hovered == Some(id) {
                    self.activate(world, &mut events, id, false);
                }
            }
        }
        if scroll != 0.0 && !mouse_held {
            if let Some(id) = hovered {
                if world.scene.get_component::<Slider>(id).is_some() {
                    self.step_slider(world, &mut events, id, scroll.signum());
                }
            }
        }

        // ---------------- клавиатура ----------------
        if self.editing {
            self.edit_text(world, &mut events, &keys);
        } else if !self.focusables.is_empty() {
            self.navigate(world, &mut events, &keys);
        }

        events.pointer_over_ui = over_ui;
        events.hovered = self.hovered;
        events.focused = self.focused;
        events.editing_text = self.editing;
        match world.resources.get_mut::<UiEvents>() {
            Some(slot) => *slot = events,
            None => {
                world.resources.insert(events);
            }
        }
    }

    fn repeating(&mut self, keys: &KeySnapshot, key: usize) -> bool {
        self.repeating_every(keys, key, REPEAT_RATE)
    }

    /// Нажатие с автоповтором: сразу, затем после `REPEAT_DELAY` каждые `rate` секунд.
    fn repeating_every(&mut self, keys: &KeySnapshot, key: usize, rate: f32) -> bool {
        if keys.down[key] {
            self.repeat = Some((key, REPEAT_DELAY));
            return true;
        }
        match &mut self.repeat {
            Some((held, timer)) if *held == key && keys.held[key] => {
                *timer -= self.real_dt;
                if *timer <= 0.0 {
                    *timer += rate;
                    true
                } else {
                    false
                }
            }
            Some((held, _)) if *held == key => {
                self.repeat = None;
                false
            }
            _ => false,
        }
    }

    fn navigate(&mut self, world: &mut World, events: &mut UiEvents, keys: &KeySnapshot) {
        let shift = keys.held[K_LSHIFT] || keys.held[K_RSHIFT];
        let current = self
            .focused
            .and_then(|f| self.focusables.iter().position(|&id| id == f));
        let focused_slider = self
            .focused
            .filter(|&id| world.scene.get_component::<Slider>(id).is_some());

        let mut direction = 0i32;
        if keys.down[K_TAB] {
            direction = if shift { -1 } else { 1 };
        }
        if self.repeating_every(keys, K_DOWN, NAV_REPEAT_RATE) {
            direction = 1;
        }
        if self.repeating_every(keys, K_UP, NAV_REPEAT_RATE) {
            direction = -1;
        }
        match focused_slider {
            Some(id) => {
                let left = self.repeating(keys, K_LEFT);
                let right = self.repeating(keys, K_RIGHT);
                if left || right {
                    self.focus_visible = true;
                    self.step_slider(world, events, id, if right { 1.0 } else { -1.0 });
                }
            }
            None => {
                if keys.down[K_RIGHT] {
                    direction = 1;
                }
                if keys.down[K_LEFT] {
                    direction = -1;
                }
            }
        }
        if direction != 0 {
            let count = self.focusables.len() as i32;
            let next = match current {
                Some(index) if self.focus_visible => (index as i32 + direction).rem_euclid(count),
                Some(index) => index as i32,
                None if direction > 0 => 0,
                None => count - 1,
            };
            self.focused = Some(self.focusables[next as usize]);
            self.focus_visible = true;
        }
        if keys.down[K_ENTER] {
            if let Some(id) = self.focused {
                self.focus_visible = true;
                self.activate(world, events, id, true);
            }
        }
        if keys.down[K_ESCAPE] && self.focus_visible {
            self.focused = None;
            self.focus_visible = false;
        }
    }

    fn begin_editing(&mut self, world: &World, id: GameObjectId) {
        self.focused = Some(id);
        self.editing = true;
        self.caret = world
            .scene
            .get_component::<InputField>(id)
            .map_or(0, |f| f.text.chars().count());
        self.caret_clock = 0.0;
        self.input_scroll = 0.0;
    }

    /// Номер границы символа под курсором в поле ввода.
    fn caret_at(&self, id: GameObjectId, x: f32) -> usize {
        let (Some(geometry), Some(layout)) = (self.geometry.get(&id), self.layouts.get(&(id, SLOT_INPUT))) else {
            return self.caret;
        };
        let local = x - geometry.x0 - self.input_scroll;
        layout
            .carets
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - local).abs().total_cmp(&(b.1 - local).abs()))
            .map_or(self.caret, |(i, _)| i)
    }

    fn edit_text(&mut self, world: &mut World, events: &mut UiEvents, keys: &KeySnapshot) {
        let Some(id) = self.focused else {
            self.editing = false;
            return;
        };
        let backspace = self.repeating(keys, K_BACKSPACE);
        let delete = self.repeating(keys, K_DELETE);
        let left = self.repeating(keys, K_LEFT);
        let right = self.repeating(keys, K_RIGHT);
        let home = keys.down[K_HOME];
        let end = keys.down[K_END];
        let Some(field) = world.scene.get_component_mut::<InputField>(id) else {
            self.editing = false;
            return;
        };
        let mut chars = field.text.chars().count();
        self.caret = self.caret.min(chars);
        let mut changed = false;
        for c in self.typed.chars() {
            if field.max_length > 0 && chars >= field.max_length as usize {
                break;
            }
            let at = byte_index(&field.text, self.caret);
            field.text.insert(at, c);
            self.caret += 1;
            chars += 1;
            changed = true;
        }
        if backspace && self.caret > 0 {
            let at = byte_index(&field.text, self.caret - 1);
            field.text.remove(at);
            self.caret -= 1;
            chars -= 1;
            changed = true;
        }
        if delete && self.caret < chars {
            let at = byte_index(&field.text, self.caret);
            field.text.remove(at);
            chars -= 1;
            changed = true;
        }
        if left && self.caret > 0 {
            self.caret -= 1;
        }
        if right && self.caret < chars {
            self.caret += 1;
        }
        if home {
            self.caret = 0;
        }
        if end {
            self.caret = chars;
        }
        if changed || left || right || home || end {
            self.caret_clock = 0.0;
        }
        let text = changed.then(|| field.text.clone());
        let submitted = keys.down[K_ENTER].then(|| field.text.clone());
        if let Some(text) = text {
            self.emit(world, events, id, UiEventKind::TextChanged(text.clone()), Value::Str(text), None);
        }
        if let Some(text) = submitted {
            self.emit(world, events, id, UiEventKind::Submit(text.clone()), Value::Str(text), Some(":submit"));
        }
        if keys.down[K_ESCAPE] {
            self.editing = false;
        }
        if keys.down[K_TAB] {
            self.editing = false;
            self.navigate(world, events, keys);
        }
    }

    fn drag_slider(&mut self, world: &mut World, events: &mut UiEvents, id: GameObjectId, x: f32) {
        let Some(geometry) = self.geometry.get(&id).copied() else {
            return;
        };
        let Some(slider) = world.scene.get_component_mut::<Slider>(id) else {
            return;
        };
        if !slider.interactable {
            return;
        }
        let t = if geometry.x1 > geometry.x0 {
            ((x - geometry.x0) / (geometry.x1 - geometry.x0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let value = slider.clamp_value(slider.min + (slider.max - slider.min) * t);
        if value != slider.value {
            slider.value = value;
            self.emit(world, events, id, UiEventKind::Value(value), Value::Float(value), None);
        }
    }

    fn step_slider(&mut self, world: &mut World, events: &mut UiEvents, id: GameObjectId, direction: f32) {
        let Some(slider) = world.scene.get_component_mut::<Slider>(id) else {
            return;
        };
        if !slider.interactable {
            return;
        }
        let step = if slider.whole_numbers {
            1.0
        } else {
            (slider.max - slider.min) / 20.0
        };
        let value = slider.clamp_value(slider.value + step * direction);
        if value != slider.value {
            slider.value = value;
            self.emit(world, events, id, UiEventKind::Value(value), Value::Float(value), None);
        }
    }

    /// Клик или Enter по виджету.
    fn activate(&mut self, world: &mut World, events: &mut UiEvents, id: GameObjectId, keyboard: bool) {
        let scene = &mut world.scene;
        if scene.get_component::<Button>(id).is_some_and(|b| b.interactable) {
            self.emit(world, events, id, UiEventKind::Click, Value::Nil, None);
            // Хук `on_click` скриптов на самой кнопке (короткая форма сообщения о клике).
            world.dispatch_hook(id, Hooks::CLICK, |c, ctx| c.on_click(ctx));
            return;
        }
        if let Some(toggle) = scene.get_component_mut::<Toggle>(id) {
            if toggle.interactable {
                toggle.is_on = !toggle.is_on;
                let on = toggle.is_on;
                self.emit(world, events, id, UiEventKind::Toggle(on), Value::Bool(on), None);
            }
            return;
        }
        if keyboard && scene.get_component::<InputField>(id).is_some_and(|f| f.interactable) {
            self.begin_editing(world, id);
        }
    }

    /// Событие + сообщение виджету и всем его предкам.
    fn emit(
        &mut self,
        world: &mut World,
        events: &mut UiEvents,
        id: GameObjectId,
        kind: UiEventKind,
        value: Value,
        suffix: Option<&str>,
    ) {
        let name = message_name(&world.scene, id);
        let message = match suffix {
            Some(":submit") => submit_message(&name),
            Some(other) => format!("{name}{other}"),
            None => name.clone(),
        };
        let mut cursor = Some(id);
        while let Some(target) = cursor {
            world.send_message(target, &message, value.clone());
            cursor = world.scene.get(target).and_then(|o| o.parent());
        }
        events.events.push(UiEvent { source: id, name, kind });
    }
}

/// Имя сообщения виджета: поле `message` или имя объекта.
fn message_name(scene: &Scene, id: GameObjectId) -> String {
    let Some(object) = scene.get(id) else {
        return String::new();
    };
    let explicit = object
        .get::<Button>()
        .map(|b| b.message.as_str())
        .or(object.get::<Toggle>().map(|t| t.message.as_str()))
        .or(object.get::<Slider>().map(|s| s.message.as_str()))
        .or(object.get::<InputField>().map(|i| i.message.as_str()))
        .unwrap_or("");
    if explicit.is_empty() {
        object.name.clone()
    } else {
        explicit.to_string()
    }
}

/// Есть ли холст у кого-то из предков `id`.
fn has_canvas_ancestor(scene: &Scene, id: GameObjectId) -> bool {
    let mut cursor = scene.parent_of(id);
    while let Some(current) = cursor {
        if scene.get_component::<Canvas>(current).is_some() {
            return true;
        }
        cursor = scene.parent_of(current);
    }
    false
}

fn byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices().nth(char_index).map_or(text.len(), |(i, _)| i)
}

/// Размер текстуры ассета (читается из заголовка файла один раз). Байты берутся из
/// базы ассетов — так работает и каталог, и пак (браузер, Android).
fn texture_size(cache: &mut HashMap<Guid, Option<[u32; 2]>>, guid: Guid, assets: Option<&AssetDatabase>) -> Option<[u32; 2]> {
    *cache.entry(guid).or_insert_with(|| {
        let assets = assets?;
        let name = assets.relative_path_of(guid).unwrap_or("?").to_string();
        let dimensions = assets.read(guid).map_err(|err| err.to_string()).and_then(|bytes| {
            image::ImageReader::new(std::io::Cursor::new(bytes.as_ref()))
                .with_guessed_format()
                .map_err(|err| err.to_string())?
                .into_dimensions()
                .map_err(|err| err.to_string())
        });
        match dimensions {
            Ok((w, h)) => Some([w, h]),
            Err(err) => {
                log::error!("ui image `{name}`: {err}");
                None
            }
        }
    })
}

/// 9-slice: углы не растягиваются, края тянутся в одну сторону, середина — в обе.
#[allow(clippy::too_many_arguments)]
fn nine_slice(
    painter: &mut Painter<'_>,
    texture: OverlayTexture,
    rect: Rect,
    texture_size: [u32; 2],
    border: f32,
    scale: f32,
    color: Color,
    radius: f32,
) {
    let size = rect.size();
    let b = (border * scale).min(size.x * 0.5).min(size.y * 0.5).round();
    let bu = (border / texture_size[0].max(1) as f32).min(0.5);
    let bv = (border / texture_size[1].max(1) as f32).min(0.5);
    let xs = [rect.min.x, rect.min.x + b, rect.max.x - b, rect.max.x];
    let ys = [rect.min.y, rect.min.y + b, rect.max.y - b, rect.max.y];
    let us = [0.0, bu, 1.0 - bu, 1.0];
    let vs = [0.0, bv, 1.0 - bv, 1.0];
    for row in 0..3 {
        for col in 0..3 {
            let cell = Rect {
                min: Vec2::new(xs[col], ys[row]),
                max: Vec2::new(xs[col + 1], ys[row + 1]),
            };
            painter.textured(texture, cell, [us[col], vs[row], us[col + 1], vs[row + 1]], color, rect, radius);
        }
    }
}

impl Subsystem for UiSystem {
    fn name(&self) -> &'static str {
        "ui"
    }

    fn init(&mut self, world: &mut World) {
        if !world.resources.contains::<UiEvents>() {
            world.resources.insert(UiEvents::default());
        }
        if !world.resources.contains::<Overlay>() {
            world.resources.insert(Overlay::default());
        }
    }

    fn pre_update(&mut self, world: &mut World) {
        self.interact(world);
    }

    fn post_update(&mut self, world: &mut World) {
        self.build(world);
    }

    fn idle(&mut self, world: &mut World) {
        self.build(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::AssetPack;

    #[test]
    fn nine_slice_texture_size_is_read_from_a_pack() {
        let guid = Guid(0x9_51);
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(24, 12).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let mut builder = AssetPack::builder();
        builder.add("ui/frame.png", png.into_inner());
        builder.add(
            "ui/frame.png.meta",
            format!("(
    guid: \"{:032x}\",
    importer: \"Texture\",
    settings: {{}},
)
", guid.0).into_bytes(),
        );
        let database = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
        assert!(!database.path_of(guid).unwrap().exists(), "no file on disk: bytes come from the pack");
        let mut cache = HashMap::new();
        assert_eq!(texture_size(&mut cache, guid, Some(&database)), Some([24, 12]));
        assert_eq!(texture_size(&mut cache, Guid(5), Some(&database)), None);
    }
}
