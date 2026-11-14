//! Интерфейс «Роя»: экраны из UI-префабов `assets/ui/*.prefab` (HUD, главное меню, выбор
//! режима, настройки, пауза, карточки улучшений, результаты), которые лежат в сцене
//! `arena.scene` как экземпляры префабов. Здесь — только привязка к ним по именам и
//! обновление содержимого; сам интерфейс собирает запекатель (`examples/bake_assets`).
//!
//! Экраны — отдельные холсты; показывается один за раз (`Ui::show`), поэтому навигация
//! клавиатурой и геймпадом ходит только по кнопкам текущего экрана.

use engine::prelude::*;

use crate::data::*;
use crate::profile::{Options, RunRecord};
use crate::sim::{Mode, Sim};

/// Экраны с кнопками (HUD живёт всегда, пока идёт игра).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    ModeSelect,
    Options,
    Playing,
    Paused,
    Upgrade,
    Results,
}

pub struct Card {
    pub root: GameObjectId,
    pub icon: GameObjectId,
    pub name: GameObjectId,
    pub level: GameObjectId,
    pub desc: GameObjectId,
}

pub struct Ui {
    // холсты экранов
    pub hud: GameObjectId,
    pub menu: GameObjectId,
    pub mode: GameObjectId,
    pub options: GameObjectId,
    pub pause: GameObjectId,
    pub upgrade: GameObjectId,
    pub results: GameObjectId,
    // HUD
    pub hp_text: GameObjectId,
    pub hp_bar: GameObjectId,
    pub core_text: GameObjectId,
    pub core_bar: GameObjectId,
    pub wave_text: GameObjectId,
    pub left_text: GameObjectId,
    pub boss_name: GameObjectId,
    pub boss_bar: GameObjectId,
    pub score_text: GameObjectId,
    pub mult_text: GameObjectId,
    pub combo_bar: GameObjectId,
    pub xp_bar: GameObjectId,
    pub level_text: GameObjectId,
    pub dash_bar: GameObjectId,
    pub dash_text: GameObjectId,
    pub banner: GameObjectId,
    pub banner_sub: GameObjectId,
    pub vignette: GameObjectId,
    pub pause_btn: GameObjectId,
    pub dash_btn: GameObjectId,
    pub touch_nodes: [GameObjectId; 4],
    // меню
    pub continue_btn: GameObjectId,
    pub continue_label: GameObjectId,
    pub quit_btn: GameObjectId,
    pub menu_best: GameObjectId,
    // выбор режима
    pub endless_btn: GameObjectId,
    pub endless_sub: GameObjectId,
    pub board_text: GameObjectId,
    // настройки
    pub music_slider: GameObjectId,
    pub sfx_slider: GameObjectId,
    pub music_label: GameObjectId,
    pub sfx_label: GameObjectId,
    pub shake_toggle: GameObjectId,
    pub numbers_toggle: GameObjectId,
    // улучшения
    pub cards: [Card; 3],
    pub upgrade_sub: GameObjectId,
    // результаты
    pub res_title: GameObjectId,
    pub res_mode: GameObjectId,
    pub res_stats: GameObjectId,
    pub res_record: GameObjectId,
    pub icons: Vec<AssetHandle<Texture>>,
    pub last_vignette: f32,
}

// ---------------------------------------------------------------------------
// Помощники
// ---------------------------------------------------------------------------

pub fn set_text(scene: &mut Scene, id: GameObjectId, text: &str) {
    if let Some(t) = scene.get_component_mut::<Text>(id) {
        t.set(text);
    }
}

pub(crate) fn set_bar(scene: &mut Scene, id: GameObjectId, v: f32) {
    if let Some(b) = scene.get_component_mut::<ProgressBar>(id) {
        let v = v.clamp(0.0, 1.0);
        if (b.value - v).abs() > 0.0005 {
            b.value = v;
        }
    }
}

pub(crate) fn set_bar_color(scene: &mut Scene, id: GameObjectId, c: Color) {
    if let Some(b) = scene.get_component_mut::<ProgressBar>(id) {
        b.fill_color = c;
    }
}

pub(crate) fn set_text_color(scene: &mut Scene, id: GameObjectId, c: Color) {
    if let Some(t) = scene.get_component_mut::<Text>(id) {
        t.color = c;
    }
}

/// Число с пробелами между тысячами: 12 345.
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{a0}');
        }
        out.push(ch);
    }
    out
}

pub fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}


fn find(scene: &Scene, name: &str) -> GameObjectId {
    scene.find_by_name(name).unwrap_or_else(|| {
        log::error!("UI object `{name}` is missing from the scene: re-bake assets (cargo run -p swarm --example bake_assets)");
        GameObjectId::from_bits(0)
    })
}

fn child(scene: &Scene, parent: GameObjectId, name: &str) -> GameObjectId {
    scene.find_child(parent, name).unwrap_or_else(|| {
        log::error!("UI object `{name}` is missing under its parent");
        GameObjectId::from_bits(0)
    })
}

impl Ui {
    /// Найти объекты интерфейса в сцене по именам (их создают префабы из `assets/ui`).
    pub fn bind(scene: &Scene, db: &AssetDatabase) -> Ui {
        let icons: Vec<AssetHandle<Texture>> = Up::ALL
            .iter()
            .map(|u| db.handle::<Texture>(u.info().icon).unwrap_or_else(|_| AssetHandle::none()))
            .collect();
        let card = |i: usize| {
            let root = find(scene, &format!("card{i}"));
            Card { root, icon: child(scene, root, "Icon"), name: child(scene, root, "Name"), level: child(scene, root, "Level"), desc: child(scene, root, "Desc") }
        };
        let continue_btn = find(scene, "continue");
        let endless_btn = find(scene, "endless");
        Ui {
            hud: find(scene, "HUD"),
            menu: find(scene, "MenuScreen"),
            mode: find(scene, "ModeScreen"),
            options: find(scene, "OptionsScreen"),
            pause: find(scene, "PauseScreen"),
            upgrade: find(scene, "UpgradeScreen"),
            results: find(scene, "ResultsScreen"),
            hp_text: find(scene, "HpText"),
            hp_bar: find(scene, "HpBar"),
            core_text: find(scene, "CoreText"),
            core_bar: find(scene, "CoreBar"),
            wave_text: find(scene, "WaveText"),
            left_text: find(scene, "LeftText"),
            boss_name: find(scene, "BossName"),
            boss_bar: find(scene, "BossBar"),
            score_text: find(scene, "ScoreText"),
            mult_text: find(scene, "MultText"),
            combo_bar: find(scene, "ComboBar"),
            xp_bar: find(scene, "XpBar"),
            level_text: find(scene, "LevelText"),
            dash_bar: find(scene, "DashBar"),
            dash_text: find(scene, "DashText"),
            banner: find(scene, "Banner"),
            banner_sub: find(scene, "BannerSub"),
            vignette: find(scene, "LowHealth"),
            pause_btn: find(scene, "pause"),
            dash_btn: find(scene, "DashButton"),
            touch_nodes: [find(scene, "Stick0"), find(scene, "Stick1"), find(scene, "Stick2"), find(scene, "Stick3")],
            continue_btn,
            continue_label: child(scene, continue_btn, "Label"),
            menu_best: find(scene, "Best"),
            quit_btn: find(scene, "quit"),
            endless_btn,
            endless_sub: child(scene, endless_btn, "Sub"),
            board_text: find(scene, "Board"),
            music_slider: find(scene, "music"),
            sfx_slider: find(scene, "sfx"),
            music_label: find(scene, "MusicLabel"),
            sfx_label: find(scene, "SfxLabel"),
            shake_toggle: find(scene, "shake"),
            numbers_toggle: find(scene, "numbers"),
            cards: [card(0), card(1), card(2)],
            upgrade_sub: find(scene, "UpgradeSub"),
            res_title: find(scene, "ResTitle"),
            res_mode: find(scene, "ResMode"),
            res_stats: find(scene, "ResStats"),
            res_record: find(scene, "ResRecord"),
            icons,
            last_vignette: -1.0,
        }
    }

    /// Показать нужные холсты для экрана `screen`.
    pub fn show(&self, scene: &mut Scene, screen: Screen) {
        let all = [
            (self.menu, Screen::Menu),
            (self.mode, Screen::ModeSelect),
            (self.options, Screen::Options),
            (self.pause, Screen::Paused),
            (self.upgrade, Screen::Upgrade),
            (self.results, Screen::Results),
        ];
        for (id, s) in all {
            scene.ui_set_visible(id, s == screen);
        }
        let hud = matches!(screen, Screen::Playing | Screen::Paused);
        // Из главного меню (экран «Настройки» поверх меню) HUD не нужен.
        scene.ui_set_visible(self.hud, hud && screen != Screen::Options);
    }

    /// Плавное появление холста.
    pub fn fade_in(&self, ctx: &mut Ctx, canvas: GameObjectId) {
        if let Some(g) = ctx.scene_mut().get_component_mut::<CanvasGroup>(canvas) {
            g.alpha = 0.0;
        }
        ctx.start_tween(Tween::field(canvas, "CanvasGroup.alpha", 1.0f32).duration(0.22).ease(Ease::OutQuad).unscaled());
    }

    pub fn screen_canvas(&self, screen: Screen) -> Option<GameObjectId> {
        match screen {
            Screen::Menu => Some(self.menu),
            Screen::ModeSelect => Some(self.mode),
            Screen::Options => Some(self.options),
            Screen::Paused => Some(self.pause),
            Screen::Upgrade => Some(self.upgrade),
            Screen::Results => Some(self.results),
            Screen::Playing => None,
        }
    }

    // -----------------------------------------------------------------------
    // Обновление содержимого
    // -----------------------------------------------------------------------

    pub fn refresh_menu(&self, scene: &mut Scene, continue_wave: Option<u32>, best: &str) {
        match continue_wave {
            Some(w) => {
                scene.ui_set_visible(self.continue_btn, true);
                set_text(scene, self.continue_label, &format!("Продолжить (волна {w})"));
            }
            None => scene.ui_set_visible(self.continue_btn, false),
        }
        set_text(scene, self.menu_best, best);
    }

    pub fn refresh_mode(&self, scene: &mut Scene, endless_unlocked: bool, board: &[RunRecord]) {
        if let Some(b) = scene.get_component_mut::<Button>(self.endless_btn) {
            b.interactable = endless_unlocked;
        }
        set_text(
            scene,
            self.endless_sub,
            if endless_unlocked { "Без конца: волны растут, пока цел реактор" } else { "Откроется после 10-й волны кампании" },
        );
        let mut s = String::new();
        if board.is_empty() {
            s.push_str("Пока пусто. Сыграй первый забег!");
        }
        for (i, r) in board.iter().enumerate() {
            let tag = if r.mode == Mode::Endless { "бесконечный" } else if r.won { "победа" } else { "кампания" };
            s.push_str(&format!("{}.  {:>9}   волна {:>2}  {}  {}\n", i + 1, grouped(r.score), r.wave, clock(r.secs), tag));
        }
        set_text(scene, self.board_text, s.trim_end());
    }

    pub fn refresh_options(&self, scene: &mut Scene, o: &Options) {
        if let Some(s) = scene.get_component_mut::<Slider>(self.music_slider) {
            s.value = o.music;
        }
        if let Some(s) = scene.get_component_mut::<Slider>(self.sfx_slider) {
            s.value = o.sfx;
        }
        if let Some(t) = scene.get_component_mut::<Toggle>(self.shake_toggle) {
            t.is_on = o.shake;
        }
        if let Some(t) = scene.get_component_mut::<Toggle>(self.numbers_toggle) {
            t.is_on = o.numbers;
        }
        self.refresh_option_labels(scene, o);
    }

    pub fn refresh_option_labels(&self, scene: &mut Scene, o: &Options) {
        set_text(scene, self.music_label, &format!("Музыка: {:.0}%", o.music * 100.0));
        set_text(scene, self.sfx_label, &format!("Звуки: {:.0}%", o.sfx * 100.0));
    }

    /// Заполнить три карточки улучшений.
    pub fn fill_cards(&self, scene: &mut Scene, offer: &[Up], levels: &[u8; UP_COUNT], reason: &str) {
        set_text(scene, self.upgrade_sub, reason);
        for (i, card) in self.cards.iter().enumerate() {
            match offer.get(i) {
                Some(up) => {
                    let info = up.info();
                    let lvl = levels[up.index()];
                    scene.ui_set_visible(card.root, true);
                    set_text(scene, card.name, info.name);
                    set_text(scene, card.level, &if lvl == 0 { "НОВОЕ".to_string() } else { format!("Уровень {} → {}", lvl, lvl + 1) });
                    set_text(scene, card.desc, &up.describe(lvl + 1));
                    if let Some(img) = scene.get_component_mut::<Image>(card.icon) {
                        img.texture = self.icons[up.index()];
                    }
                }
                None => scene.ui_set_visible(card.root, false),
            }
        }
    }

    pub fn fill_results(&self, scene: &mut Scene, won: bool, sim: &Sim, record: Option<usize>, new_best: bool) {
        set_text(scene, self.res_title, if won { "ПОБЕДА!" } else { "РЕАКТОР УНИЧТОЖЕН" });
        set_text_color(scene, self.res_title, if won { Color::hex(0x7dffb0) } else { Color::hex(0xff6a7a) });
        set_text(
            scene,
            self.res_mode,
            &if sim.mode == Mode::Campaign { format!("Кампания — волна {} из {}", sim.wave, CAMPAIGN_WAVES) } else { format!("Бесконечный режим — волна {}", sim.wave) },
        );
        set_text(
            scene,
            self.res_stats,
            &format!(
                "Счёт: {}\nУничтожено врагов: {}\nВремя: {}\nЛучший множитель: ×{:.1}\nУлучшений взято: {}",
                grouped(sim.score),
                sim.run.kills,
                clock(sim.time as u32),
                sim.run.best_mult.max(1.0),
                sim.run.upgrades
            ),
        );
        let rec = if new_best {
            "Новый рекорд!".to_string()
        } else if let Some(p) = record {
            format!("Место в таблице лучших: {}", p + 1)
        } else {
            String::new()
        };
        set_text(scene, self.res_record, &rec);
    }
}

pub(crate) fn cyan() -> Color {
    Color::hex(0x5df0ff)
}

pub(crate) fn muted() -> Color {
    Color::hex(0x8fa3c7)
}
