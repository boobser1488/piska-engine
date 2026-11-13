//! Прямоугольники спрайтов в атласе `sprites/atlas.png`.
//! Файл создаёт `tools/gen_assets.py` — не правьте его руками.

/// Прямоугольник спрайта в пикселях атласа (от левого верхнего угла).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const ATLAS_W: f32 = 1024.0;
pub const ATLAS_H: f32 = 1024.0;

pub const BAR: Rect = Rect { x: 750.0, y: 652.0, w: 32.0, h: 32.0 };
pub const BOMBER: Rect = Rect { x: 867.0, y: 521.0, w: 88.0, h: 88.0 };
pub const BOSS_CONDUCTOR: Rect = Rect { x: 521.0, y: 3.0, w: 256.0, h: 256.0 };
pub const BOSS_HIVE: Rect = Rect { x: 3.0, y: 3.0, w: 256.0, h: 256.0 };
pub const BOSS_OVERMIND: Rect = Rect { x: 3.0, y: 262.0, w: 256.0, h: 256.0 };
pub const BOSS_RAM: Rect = Rect { x: 262.0, y: 3.0, w: 256.0, h: 256.0 };
pub const CHASER: Rect = Rect { x: 3.0, y: 652.0, w: 80.0, h: 80.0 };
pub const CORE: Rect = Rect { x: 521.0, y: 262.0, w: 192.0, h: 192.0 };
pub const CORE_RING: Rect = Rect { x: 716.0, y: 262.0, w: 192.0, h: 192.0 };
pub const DIGIT0: Rect = Rect { x: 365.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT1: Rect = Rect { x: 400.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT2: Rect = Rect { x: 435.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT3: Rect = Rect { x: 470.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT4: Rect = Rect { x: 505.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT5: Rect = Rect { x: 540.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT6: Rect = Rect { x: 575.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT7: Rect = Rect { x: 610.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT8: Rect = Rect { x: 645.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DIGIT9: Rect = Rect { x: 680.0, y: 652.0, w: 32.0, h: 48.0 };
pub const DOT: Rect = Rect { x: 922.0, y: 652.0, w: 16.0, h: 16.0 };
pub const EBULLET: Rect = Rect { x: 715.0, y: 652.0, w: 32.0, h: 32.0 };
pub const FIELD: Rect = Rect { x: 262.0, y: 262.0, w: 256.0, h: 256.0 };
pub const GLOW: Rect = Rect { x: 677.0, y: 521.0, w: 96.0, h: 96.0 };
pub const MINE: Rect = Rect { x: 145.0, y: 652.0, w: 56.0, h: 56.0 };
pub const MINELAYER: Rect = Rect { x: 578.0, y: 521.0, w: 96.0, h: 96.0 };
pub const MISSILE: Rect = Rect { x: 820.0, y: 652.0, w: 56.0, h: 28.0 };
pub const PBULLET: Rect = Rect { x: 879.0, y: 652.0, w: 40.0, h: 16.0 };
pub const PLAYER: Rect = Rect { x: 380.0, y: 521.0, w: 96.0, h: 96.0 };
pub const REPAIR: Rect = Rect { x: 314.0, y: 652.0, w: 48.0, h: 48.0 };
pub const RING: Rect = Rect { x: 134.0, y: 521.0, w: 128.0, h: 128.0 };
pub const SCRAP: Rect = Rect { x: 785.0, y: 652.0, w: 32.0, h: 32.0 };
pub const SHIELD: Rect = Rect { x: 204.0, y: 652.0, w: 56.0, h: 56.0 };
pub const SHOOTER: Rect = Rect { x: 776.0, y: 521.0, w: 88.0, h: 88.0 };
pub const SPLITTER: Rect = Rect { x: 479.0, y: 521.0, w: 96.0, h: 96.0 };
pub const SPLITTER_MINI: Rect = Rect { x: 86.0, y: 652.0, w: 56.0, h: 56.0 };
pub const SWARMLING: Rect = Rect { x: 263.0, y: 652.0, w: 48.0, h: 48.0 };
pub const TANK: Rect = Rect { x: 3.0, y: 521.0, w: 128.0, h: 128.0 };
pub const TELEGRAPH: Rect = Rect { x: 265.0, y: 521.0, w: 112.0, h: 112.0 };

/// Цифры 0–9 (семисегментные).
pub const DIGITS: [Rect; 10] = [DIGIT0, DIGIT1, DIGIT2, DIGIT3, DIGIT4, DIGIT5, DIGIT6, DIGIT7, DIGIT8, DIGIT9];
