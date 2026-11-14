//! Настройки игрока (экран «Настройки»): чувствительность мыши, угол обзора, автопрыжок,
//! спидометр, инверсия мыши, громкость. Хранятся в сохранениях (`SaveData`).

use engine::prelude::*;

/// Настройки игрока.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Чувствительность мыши (как `sensitivity` в классике).
    pub sensitivity: f32,
    /// Горизонтальный угол обзора на 4:3, градусы.
    pub fov: f32,
    pub auto_hop: bool,
    pub show_speed: bool,
    pub invert_y: bool,
    /// Общая громкость, 0..1.
    pub volume: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { sensitivity: 2.0, fov: 90.0, auto_hop: false, show_speed: true, invert_y: false, volume: 0.8 }
    }
}

impl Options {
    pub const SENSITIVITY: (f32, f32) = (0.2, 10.0);
    pub const FOV: (f32, f32) = (70.0, 110.0);

    /// Прочитать из сохранений (нет ключа — значение по умолчанию).
    pub fn load(save: &SaveData) -> Self {
        let d = Self::default();
        Self {
            sensitivity: save.float("opt.sensitivity", d.sensitivity).clamp(Self::SENSITIVITY.0, Self::SENSITIVITY.1),
            fov: save.float("opt.fov", d.fov).clamp(Self::FOV.0, Self::FOV.1),
            auto_hop: save.bool("opt.auto_hop", d.auto_hop),
            show_speed: save.bool("opt.show_speed", d.show_speed),
            invert_y: save.bool("opt.invert_y", d.invert_y),
            volume: save.float("opt.volume", d.volume).clamp(0.0, 1.0),
        }
    }

    /// Записать в сохранения и на диск.
    pub fn store(&self, save: &mut SaveData) {
        save.set_float("opt.sensitivity", self.sensitivity);
        save.set_float("opt.fov", self.fov);
        save.set_bool("opt.auto_hop", self.auto_hop);
        save.set_bool("opt.show_speed", self.show_speed);
        save.set_bool("opt.invert_y", self.invert_y);
        save.set_float("opt.volume", self.volume);
        if let Err(error) = save.save() {
            log::warn!("options were not saved: {error}");
        }
    }

    /// Применить к контроллеру игрока.
    pub fn apply(&self, controller: &mut FpsController) {
        controller.sensitivity = self.sensitivity;
        controller.fov_deg = self.fov;
        controller.auto_hop = self.auto_hop;
        controller.invert_y = self.invert_y;
    }
}
