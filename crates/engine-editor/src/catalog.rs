//! Каталог: иконки и категории компонентов, заготовки объектов для меню GameObject.
//!
//! Редактор не зависит от крейтов звука, интерфейса и эффектов: он знает только имена
//! их компонентов. Заготовка показывается в меню, лишь если все её компоненты есть в
//! `TypeRegistry` во время работы — нет крейта, нет и пункта.

use egui::Color32;
use engine_core::prelude::*;

use crate::icons::Icon;
use crate::theme;

/// Грубая категория по имени типа (иконки объектов). Меню Add Component берёт категории
/// из реестра типов (`TypeRegistry::category_of`). Всё, чего движок не знает, — скрипты игры.
pub(crate) fn category(type_name: &str) -> &'static str {
    match type_name {
        "MeshRenderer" | "Light" | "Environment" | "Terrain" | "Water" => "Rendering",
        "SpriteRenderer" | "Tilemap" => "2D",
        "Rigidbody" | "Collider" | "CharacterController" | "Rigidbody2D" | "Collider2D" => "Physics",
        "AudioSource" | "AudioListener" => "Audio",
        "Canvas" | "RectTransform" | "LayoutGroup" | "RectMask" | "CanvasGroup" | "Image" | "Text"
        | "Button" | "Toggle" | "Slider" | "ProgressBar" | "InputField" => "UI",
        "ParticleSystem" | "TrailRenderer" => "Effects",
        "SpriteAnimator" | "Animator" | "Tween" => "Animation",
        "Camera" | "CameraController2D" => "Camera",
        _ => "Scripts",
    }
}

/// Иконка компонента в инспекторе и в окне Add Component.
pub(crate) fn component_icon(type_name: &str) -> Icon {
    match type_name {
        "Transform" => Icon::Transform,
        "Camera" | "CameraController2D" => Icon::Camera,
        "Light" => Icon::Bulb,
        "Environment" => Icon::Environment,
        "MeshRenderer" => Icon::Cube,
        "SpriteRenderer" => Icon::Sprite,
        "Tilemap" => Icon::Tilemap,
        "Rigidbody" | "Rigidbody2D" => Icon::Physics,
        "Collider" | "Collider2D" | "CharacterController" => Icon::Collider,
        "AudioSource" | "AudioListener" => Icon::Audio,
        "Text" | "InputField" => Icon::Text,
        "Image" => Icon::Image,
        "ParticleSystem" | "TrailRenderer" => Icon::Particles,
        "SpriteAnimator" | "Animator" => Icon::Animation,
        "Tween" => Icon::Tween,
        other => match category(other) {
            "UI" => Icon::Ui,
            _ => Icon::Script,
        },
    }
}

/// Иконка и цвет строки объекта (иерархия, маркеры во вьюпорте).
pub(crate) fn object_icon(object: &GameObject) -> (Icon, Color32) {
    if object.prefab().is_some() {
        return (Icon::Prefab, theme::PREFAB);
    }
    let names: Vec<&'static str> = object.components().map(|c| c.type_name()).collect();
    let has = |name: &str| names.contains(&name);
    let icon = if has("Camera") {
        Icon::Camera
    } else if has("Light") {
        match object.components().find(|c| c.type_name() == "Light").and_then(|c| c.get_field("kind")) {
            Some(Value::Enum(kind)) if kind == "Directional" => Icon::Sun,
            _ => Icon::Bulb,
        }
    } else if has("Canvas") || names.iter().any(|n| category(n) == "UI") {
        Icon::Ui
    } else if has("AudioSource") || has("AudioListener") {
        Icon::Audio
    } else if has("ParticleSystem") || has("TrailRenderer") {
        Icon::Particles
    } else if has("Tilemap") {
        Icon::Tilemap
    } else if has("MeshRenderer") {
        Icon::Cube
    } else if has("SpriteRenderer") {
        Icon::Sprite
    } else if has("Environment") {
        Icon::Environment
    } else {
        Icon::Object
    };
    let color = if icon == Icon::Object { theme::TEXT_MUTED } else { theme::TEXT };
    (icon, color)
}

/// Заготовка объекта: имя, компоненты и дети (кнопке нужна подпись).
pub(crate) struct Preset {
    /// Идентификатор для [`crate::Action::Create`]: `"UI/Button"`.
    pub id: &'static str,
    pub label: &'static str,
    pub icon: Icon,
    pub name: &'static str,
    pub components: &'static [&'static str],
    pub children: &'static [(&'static str, &'static [&'static str], &'static str)],
}

impl Preset {
    /// Подменю, в котором показывается заготовка.
    pub fn menu(&self) -> &'static str {
        self.id.split('/').next().unwrap_or("")
    }

    /// Все ли нужные компоненты зарегистрированы.
    pub fn available(&self, registry: &TypeRegistry) -> bool {
        self.components.iter().all(|kind| registry.contains(kind))
            && self
                .children
                .iter()
                .all(|(_, kinds, _)| kinds.iter().all(|kind| registry.contains(kind)))
    }

    /// Объект интерфейса: живёт под холстом.
    pub fn is_ui(&self) -> bool {
        self.menu() == "UI" && self.id != "UI/Canvas"
    }
}

/// Подменю GameObject и их иконки — в этом порядке.
pub(crate) const PRESET_MENUS: [(&str, Icon); 6] = [
    ("2D", Icon::Sprite),
    ("Audio", Icon::Audio),
    ("UI", Icon::Ui),
    ("Effects", Icon::Particles),
    ("Physics", Icon::Physics),
    ("Animation", Icon::Animation),
];

/// Заготовки компонентов других крейтов. Встроенные (меши, свет, спрайт, камера)
/// создаются своими действиями, потому что им нужна особая начальная поза.
pub(crate) const PRESETS: &[Preset] = &[
    Preset { id: "2D/Tilemap", label: "Tilemap", icon: Icon::Tilemap, name: "Tilemap", components: &["Tilemap"], children: &[] },
    Preset {
        id: "2D/Animated Sprite",
        label: "Animated Sprite",
        icon: Icon::Animation,
        name: "Animated Sprite",
        components: &["SpriteRenderer", "SpriteAnimator"],
        children: &[],
    },
    Preset {
        id: "2D/Sprite with Physics",
        label: "Sprite with Physics",
        icon: Icon::Physics,
        name: "Body",
        components: &["SpriteRenderer", "Rigidbody2D", "Collider2D"],
        children: &[],
    },
    Preset { id: "Audio/Audio Source", label: "Audio Source", icon: Icon::Audio, name: "Audio Source", components: &["AudioSource"], children: &[] },
    Preset { id: "Audio/Audio Listener", label: "Audio Listener", icon: Icon::Audio, name: "Audio Listener", components: &["AudioListener"], children: &[] },
    Preset { id: "UI/Canvas", label: "Canvas", icon: Icon::Ui, name: "Canvas", components: &["Canvas"], children: &[] },
    Preset { id: "UI/Text", label: "Text", icon: Icon::Text, name: "Text", components: &["RectTransform", "Text"], children: &[] },
    Preset { id: "UI/Image", label: "Image", icon: Icon::Image, name: "Image", components: &["RectTransform", "Image"], children: &[] },
    Preset {
        id: "UI/Button",
        label: "Button",
        icon: Icon::Ui,
        name: "Button",
        components: &["RectTransform", "Image", "Button"],
        children: &[("Text", &["RectTransform", "Text"], "Button")],
    },
    Preset {
        id: "UI/Toggle",
        label: "Toggle",
        icon: Icon::Check,
        name: "Toggle",
        components: &["RectTransform", "Toggle"],
        children: &[("Label", &["RectTransform", "Text"], "Toggle")],
    },
    Preset { id: "UI/Slider", label: "Slider", icon: Icon::Inspector, name: "Slider", components: &["RectTransform", "Slider"], children: &[] },
    Preset { id: "UI/Progress Bar", label: "Progress Bar", icon: Icon::Chart, name: "Progress Bar", components: &["RectTransform", "ProgressBar"], children: &[] },
    Preset { id: "UI/Input Field", label: "Input Field", icon: Icon::Text, name: "Input Field", components: &["RectTransform", "InputField"], children: &[] },
    Preset { id: "Effects/Particle System", label: "Particle System", icon: Icon::Particles, name: "Particle System", components: &["ParticleSystem"], children: &[] },
    Preset { id: "Effects/Trail", label: "Trail", icon: Icon::Tween, name: "Trail", components: &["TrailRenderer"], children: &[] },
    Preset {
        id: "Physics/Rigidbody Cube",
        label: "Rigidbody Cube",
        icon: Icon::Physics,
        name: "Rigidbody Cube",
        components: &["MeshRenderer", "Rigidbody", "Collider"],
        children: &[],
    },
    Preset {
        id: "Physics/Character",
        label: "Character Controller",
        icon: Icon::Collider,
        name: "Character",
        components: &["CharacterController"],
        children: &[],
    },
];

pub(crate) fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_menu_is_listed_and_ids_are_unique() {
        for preset in PRESETS {
            assert!(PRESET_MENUS.iter().any(|(menu, _)| *menu == preset.menu()), "{}", preset.id);
            assert_eq!(PRESETS.iter().filter(|p| p.id == preset.id).count(), 1);
        }
        assert!(preset("UI/Button").unwrap().is_ui());
        assert!(!preset("UI/Canvas").unwrap().is_ui());
    }

    #[test]
    fn unknown_components_are_scripts() {
        assert_eq!(category("PlayerController"), "Scripts");
        assert_eq!(category("Rigidbody2D"), "Physics");
    }
}
