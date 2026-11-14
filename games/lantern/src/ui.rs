//! Мелкие помощники для интерфейса из `engine-ui`: найти объект по имени и поменять текст,
//! картинку, видимость. Сам интерфейс — префабы `assets/ui/*.prefab` в сценах.

use engine::prelude::*;

pub fn set_text(ctx: &mut Ctx, name: &str, text: &str) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
            t.set(text);
        }
    }
}

pub fn set_text_color(ctx: &mut Ctx, name: &str, color: Color) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
            t.color = color;
        }
    }
}

pub fn show(ctx: &mut Ctx, name: &str, visible: bool) {
    if let Some(id) = ctx.find_by_name(name) {
        ctx.scene_mut().ui_set_visible(id, visible);
    }
}

pub fn set_image_alpha(ctx: &mut Ctx, name: &str, alpha: f32) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(i) = ctx.scene_mut().get_component_mut::<Image>(id) {
            i.color = i.color.with_alpha(alpha);
        }
    }
}

pub fn set_image_texture(ctx: &mut Ctx, name: &str, tex: AssetHandle<Texture>) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(i) = ctx.scene_mut().get_component_mut::<Image>(id) {
            i.texture = tex;
        }
    }
}

pub fn set_progress(ctx: &mut Ctx, name: &str, value: f32) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(b) = ctx.scene_mut().get_component_mut::<ProgressBar>(id) {
            b.value = value.clamp(0.0, 1.0);
        }
    }
}

/// Подпись кнопки (её дочерний `Label`).
pub fn set_button_label(ctx: &mut Ctx, button: &str, text: &str) {
    if let Some(id) = ctx.find_by_name(button) {
        if let Some(label) = ctx.scene().find_child(id, "Label") {
            if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(label) {
                t.set(text);
            }
        }
    }
}

/// Перенять вид кнопки `from` у `to` (цвета состояний и цвет картинки).
pub fn copy_button_style(ctx: &mut Ctx, from: &str, to: &str) {
    let (Some(a), Some(b)) = (ctx.find_by_name(from), ctx.find_by_name(to)) else { return };
    let style = ctx.scene().get_component::<Button>(a).cloned();
    let color = ctx.scene().get_component::<Image>(a).map(|i| i.color);
    if let Some(style) = style {
        if let Some(button) = ctx.scene_mut().get_component_mut::<Button>(b) {
            button.normal_color = style.normal_color;
            button.hover_color = style.hover_color;
            button.pressed_color = style.pressed_color;
            button.disabled_color = style.disabled_color;
        }
    }
    if let (Some(color), Some(image)) = (color, ctx.scene_mut().get_component_mut::<Image>(b)) {
        image.color = color;
    }
}

pub fn set_interactable(ctx: &mut Ctx, name: &str, on: bool) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(b) = ctx.scene_mut().get_component_mut::<Button>(id) {
            b.interactable = on;
        }
    }
}

pub fn set_slider(ctx: &mut Ctx, name: &str, value: f32) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(s) = ctx.scene_mut().get_component_mut::<Slider>(id) {
            s.value = value;
        }
    }
}

pub fn set_toggle(ctx: &mut Ctx, name: &str, on: bool) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(t) = ctx.scene_mut().get_component_mut::<Toggle>(id) {
            t.is_on = on;
        }
    }
}
