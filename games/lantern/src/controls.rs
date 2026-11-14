//! Управление: экранные кнопки для касаний. Карта клавиш и геймпада — в
//! `assets/project.settings` (раздел `input`).

use engine::prelude::*;

/// Размеры экранных кнопок в единицах холста (холст 1280×720, масштаб по высоте).
pub const PAD: f32 = 120.0;
pub const PAD_GAP: f32 = 16.0;
pub const PAD_MARGIN: f32 = 28.0;

/// Состояние виртуальных кнопок в этом кадре.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchState {
    pub axis: f32,
    pub jump_down: bool,
    pub jump_held: bool,
    pub dash_down: bool,
    pub down: bool,
    pub any: bool,
}

/// Какие кнопки нажаты касаниями. Прямоугольники совпадают с картинками HUD (см. `level.rs`):
/// слева «←» «→», справа «рывок» и «прыжок».
pub fn read_touch(input: &Input) -> TouchState {
    let touches = input.touches();
    let mut state = TouchState { any: !touches.is_empty(), ..Default::default() };
    if touches.is_empty() {
        return state;
    }
    let screen = input.screen_size();
    let scale = screen.y / 720.0;
    if scale <= 0.0 {
        return state;
    }
    // Прямоугольник кнопки в пикселях: (x от левого края, от нижнего края).
    let left = |i: f32| {
        let x0 = (PAD_MARGIN + i * (PAD + PAD_GAP)) * scale;
        (x0 - 20.0 * scale, x0 + (PAD + 20.0) * scale)
    };
    let right = |i: f32| {
        let x1 = screen.x - (PAD_MARGIN + i * (PAD + PAD_GAP)) * scale;
        (x1 - (PAD + 20.0) * scale, x1 + 20.0 * scale)
    };
    let top = screen.y - (PAD_MARGIN + PAD + 40.0) * scale;
    for t in touches {
        if t.position.y < top {
            continue;
        }
        let began = t.phase == TouchPhase::Began;
        let x = t.position.x;
        let (l0, l1) = left(0.0);
        let (r0, r1) = left(1.0);
        if x >= l0 && x <= l1 {
            state.axis -= 1.0;
        } else if x >= r0 && x <= r1 {
            state.axis += 1.0;
        }
        let (j0, j1) = right(0.0);
        let (d0, d1) = right(1.0);
        if x >= j0 && x <= j1 {
            state.jump_held = true;
            state.jump_down |= began;
        } else if x >= d0 && x <= d1 {
            state.dash_down |= began;
        }
    }
    state.axis = state.axis.clamp(-1.0, 1.0);
    state
}
