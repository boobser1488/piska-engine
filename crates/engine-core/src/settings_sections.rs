//! Разделы настроек проекта (Unity: Project Settings): игрок и окно, время, физика 2D / 3D,
//! теги и слои, действия ввода, графика, звук. Это чистые данные с `serde(default)`:
//! старый файл `project.settings`, где разделов нет, читается как «всё по умолчанию».
//!
//! Применяет разделы не это место, а подсистема, которую раздел настраивает
//! (физика — `engine-physics`, звук — `engine-audio`, рендер — `engine-render`,
//! окно — `engine`), см. `docs/api/settings.md`.

use serde::{Deserialize, Serialize};

use crate::input::{AnalogBinding, AxisBindings, Binding, GamepadAxis, Input};

// ----------------------------------------------------------------------------
// Player
// ----------------------------------------------------------------------------

/// Как окно игры открывается при старте.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FullscreenMode {
    /// Обычное окно.
    #[default]
    Windowed,
    /// Окно без рамки на весь экран (borderless fullscreen).
    Borderless,
}

impl FullscreenMode {
    pub const ALL: [FullscreenMode; 2] = [FullscreenMode::Windowed, FullscreenMode::Borderless];

    pub fn label(self) -> &'static str {
        match self {
            FullscreenMode::Windowed => "Windowed",
            FullscreenMode::Borderless => "Fullscreen (borderless)",
        }
    }
}

/// Unity: Player Settings → окно, название, версия.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// Заголовок окна. Пусто — берётся заголовок из кода игры (`WindowConfig::titled`).
    pub title: String,
    pub company: String,
    pub version: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub fullscreen: FullscreenMode,
    pub vsync: bool,
    /// Ограничение частоты кадров; 0 — без ограничения.
    pub fps_cap: u32,
    /// Значок окна относительно корня ассетов (PNG). Пусто — `icon.png`, если он есть.
    pub icon: String,
    /// Сцена, которая загружается, если `build()` игры не создала ни одного объекта.
    pub default_scene: String,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            title: String::new(),
            company: String::new(),
            version: "0.1.0".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
            fullscreen: FullscreenMode::Windowed,
            // Включена: без неё камера движется рывками даже при высоком FPS.
            vsync: true,
            fps_cap: 0,
            icon: String::new(),
            default_scene: String::new(),
        }
    }
}

// ----------------------------------------------------------------------------
// Time
// ----------------------------------------------------------------------------

/// Unity: Project Settings → Time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeSettings {
    /// Шаг `on_fixed_update` и физики, секунд (`Time.fixedDeltaTime`).
    pub fixed_timestep: f32,
    /// Самый длинный кадр, который игра «видит»: дольше (подвисание) — обрезается
    /// (`Time.maximumDeltaTime`).
    pub max_timestep: f32,
    /// Скорость времени при старте (`Time.timeScale`).
    pub time_scale: f32,
}

impl Default for TimeSettings {
    fn default() -> Self {
        Self { fixed_timestep: 1.0 / 60.0, max_timestep: 1.0 / 3.0, time_scale: 1.0 }
    }
}

impl TimeSettings {
    /// Значения, с которыми физика и таймеры не ломаются.
    pub fn sanitized(&self) -> Self {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        Self {
            fixed_timestep: finite(self.fixed_timestep, 1.0 / 60.0).clamp(0.001, 0.5),
            max_timestep: finite(self.max_timestep, 1.0 / 3.0).clamp(0.01, 1.0),
            time_scale: finite(self.time_scale, 1.0).clamp(0.0, 100.0),
        }
    }
}

// ----------------------------------------------------------------------------
// Physics
// ----------------------------------------------------------------------------

/// Матрица столкновений слоёв: бит `j` строки `i` — слои `i` и `j` сталкиваются.
/// Всегда симметрична; по умолчанию сталкиваются все со всеми.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<u32>", into = "Vec<u32>")]
pub struct LayerMatrix {
    rows: [u32; 32],
}

impl Default for LayerMatrix {
    fn default() -> Self {
        Self { rows: [u32::MAX; 32] }
    }
}

impl From<Vec<u32>> for LayerMatrix {
    /// Короткий или лишний список не ошибка: недостающие строки — «сталкивается со всеми».
    fn from(rows: Vec<u32>) -> Self {
        let mut matrix = Self::default();
        for (slot, row) in matrix.rows.iter_mut().zip(rows) {
            *slot = row;
        }
        // Файл могли править руками: приводим к симметричному виду (сталкиваются, только
        // если разрешено в обеих строках).
        for i in 0..32 {
            for j in 0..i {
                let both = matrix.rows[i] >> j & 1 == 1 && matrix.rows[j] >> i & 1 == 1;
                matrix.set(i as u32, j as u32, both);
            }
        }
        matrix
    }
}

impl From<LayerMatrix> for Vec<u32> {
    fn from(matrix: LayerMatrix) -> Self {
        matrix.rows.to_vec()
    }
}

impl LayerMatrix {
    pub fn collides(&self, a: u32, b: u32) -> bool {
        self.rows[a.min(31) as usize] >> b.min(31) & 1 == 1
    }

    /// Разрешить / запретить столкновения слоёв `a` и `b` (в обе стороны).
    pub fn set(&mut self, a: u32, b: u32, collide: bool) {
        let (a, b) = (a.min(31), b.min(31));
        for (x, y) in [(a, b), (b, a)] {
            if collide {
                self.rows[x as usize] |= 1 << y;
            } else {
                self.rows[x as usize] &= !(1 << y);
            }
        }
    }

    /// С чем сталкивается слой `layer` — маска для фильтра коллизий.
    pub fn mask(&self, layer: u32) -> u32 {
        self.rows[layer.min(31) as usize]
    }

    /// Все ли пары разрешены (матрица по умолчанию).
    pub fn is_default(&self) -> bool {
        self.rows.iter().all(|row| *row == u32::MAX)
    }
}

/// Unity: Project Settings → Physics 2D.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Physics2dSettings {
    pub gravity: [f32; 2],
    /// Итерации решателя контактов (чем больше, тем жёстче стопки тел).
    pub solver_iterations: u32,
    /// Порог линейной скорости, ниже которого тело засыпает.
    pub sleep_linear_threshold: f32,
    /// Порог угловой скорости (рад/с), ниже которого тело засыпает.
    pub sleep_angular_threshold: f32,
    pub layer_matrix: LayerMatrix,
}

impl Default for Physics2dSettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81],
            solver_iterations: 4,
            sleep_linear_threshold: 0.4,
            sleep_angular_threshold: 0.5,
            layer_matrix: LayerMatrix::default(),
        }
    }
}

/// Unity: Project Settings → Physics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Physics3dSettings {
    pub gravity: [f32; 3],
    pub solver_iterations: u32,
    pub sleep_linear_threshold: f32,
    pub sleep_angular_threshold: f32,
    pub layer_matrix: LayerMatrix,
}

impl Default for Physics3dSettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            solver_iterations: 4,
            sleep_linear_threshold: 0.4,
            sleep_angular_threshold: 0.5,
            layer_matrix: LayerMatrix::default(),
        }
    }
}

// ----------------------------------------------------------------------------
// Tags and layers
// ----------------------------------------------------------------------------

/// Число слоёв (как в `GameObject::layer`: 0–31).
pub const LAYER_COUNT: usize = 32;

/// Unity: Project Settings → Tags and Layers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagsAndLayers {
    /// Свои теги (встроенный `Untagged` не хранится).
    pub tags: Vec<String>,
    /// Имена 32 слоёв; пустая строка — слой без имени.
    pub layers: Vec<String>,
}

impl Default for TagsAndLayers {
    fn default() -> Self {
        let mut layers = vec![String::new(); LAYER_COUNT];
        layers[0] = "Default".to_string();
        layers[1] = "TransparentFX".to_string();
        layers[2] = "Ignore Raycast".to_string();
        layers[4] = "Water".to_string();
        layers[5] = "UI".to_string();
        Self { tags: Vec::new(), layers }
    }
}

impl TagsAndLayers {
    /// Всегда ровно 32 имени (файл мог быть короче или длиннее).
    pub fn normalized(mut self) -> Self {
        self.layers.resize(LAYER_COUNT, String::new());
        self
    }

    /// Имя слоя; пустое — `Layer N`.
    pub fn layer_name(&self, layer: u32) -> String {
        match self.layers.get(layer as usize) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => format!("Layer {layer}"),
        }
    }

    /// Номер слоя по имени.
    pub fn layer_by_name(&self, name: &str) -> Option<u32> {
        self.layers.iter().position(|n| !n.is_empty() && n == name).map(|i| i as u32)
    }

    /// Тег есть в списке (`Untagged` и пустая строка — всегда).
    pub fn has_tag(&self, tag: &str) -> bool {
        tag.is_empty() || tag == "Untagged" || self.tags.iter().any(|t| t == tag)
    }

    /// Добавить тег; `false` — уже есть, пустой или зарезервированный.
    pub fn add_tag(&mut self, tag: &str) -> bool {
        let tag = tag.trim();
        if tag.is_empty() || tag == "Untagged" || self.tags.iter().any(|t| t == tag) {
            return false;
        }
        self.tags.push(tag.to_string());
        true
    }
}

// ----------------------------------------------------------------------------
// Input
// ----------------------------------------------------------------------------

/// Вид действия ввода.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    #[default]
    Button,
    /// Ось −1..1 из пары «минус / плюс» и аналоговых осей геймпада.
    Axis,
}

/// Одно действие ввода: `Input::button("Jump")` / `Input::axis("Horizontal")`.
/// Источники записаны текстом [`Binding::to_text`] (`Key:Space`, `Pad:South`,
/// `PadAxis:LeftStickX+`), аналоговые оси — `LeftStickX` или `-LeftStickY` (инверсия).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputAction {
    pub name: String,
    pub kind: ActionKind,
    /// Кнопка: источники нажатия.
    pub bindings: Vec<String>,
    /// Ось: источники «минус» и «плюс».
    pub negative: Vec<String>,
    pub positive: Vec<String>,
    pub analog: Vec<String>,
    pub dead_zone: f32,
}

impl InputAction {
    pub fn button(name: &str, bindings: &[Binding]) -> Self {
        Self {
            name: name.to_string(),
            kind: ActionKind::Button,
            bindings: bindings.iter().map(|b| b.to_text()).collect(),
            dead_zone: crate::input::DEFAULT_DEAD_ZONE,
            ..Self::default()
        }
    }

    pub fn axis(name: &str, bindings: &AxisBindings) -> Self {
        Self {
            name: name.to_string(),
            kind: ActionKind::Axis,
            negative: bindings.negative.iter().map(|b| b.to_text()).collect(),
            positive: bindings.positive.iter().map(|b| b.to_text()).collect(),
            analog: bindings
                .analog
                .iter()
                .map(|a| format!("{}{:?}", if a.invert { "-" } else { "" }, a.axis))
                .collect(),
            dead_zone: bindings.dead_zone,
            ..Self::default()
        }
    }

    fn parse_all(texts: &[String]) -> Vec<Binding> {
        texts.iter().filter_map(|t| Binding::parse(t)).collect()
    }

    /// Привязки оси из этого действия (незнакомые строки пропускаются).
    pub fn axis_bindings(&self) -> AxisBindings {
        let analog = self
            .analog
            .iter()
            .filter_map(|text| {
                let (invert, name) = match text.strip_prefix('-') {
                    Some(rest) => (true, rest),
                    None => (false, text.as_str()),
                };
                GamepadAxis::ALL
                    .into_iter()
                    .find(|a| format!("{a:?}") == name)
                    .map(|axis| AnalogBinding { axis, invert })
            })
            .collect();
        AxisBindings {
            negative: Self::parse_all(&self.negative),
            positive: Self::parse_all(&self.positive),
            analog,
            dead_zone: self.dead_zone.clamp(0.0, 0.99),
        }
    }

    pub fn button_bindings(&self) -> Vec<Binding> {
        Self::parse_all(&self.bindings)
    }
}

/// Unity: Project Settings → Input Manager / Input Actions.
///
/// Список по умолчанию — те самые действия, что движок создаёт сам (`Horizontal`,
/// `Vertical`, `Jump`, `Fire1`, `Submit`, `Cancel`): проект без своих настроек ведёт себя
/// как раньше.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSettings {
    pub actions: Vec<InputAction>,
}

impl Default for InputSettings {
    fn default() -> Self {
        let defaults = Input::default();
        let mut actions = Vec::new();
        for name in ["Horizontal", "Vertical"] {
            if let Some(bindings) = defaults.axis_bindings(name) {
                actions.push(InputAction::axis(name, bindings));
            }
        }
        for name in ["Jump", "Fire1", "Submit", "Cancel"] {
            actions.push(InputAction::button(name, defaults.button_bindings(name)));
        }
        Self { actions }
    }
}

impl InputSettings {
    /// Записать действия в `input`. Действие из настроек заменяет одноимённое; действия,
    /// которых в настройках нет (заданные кодом игры), остаются как есть — «код дополняет».
    /// Порядок: настройки применяет `mount_assets`, а код игры, который вызывает
    /// `bind_*` позже, перекрывает настройки — «код побеждает».
    ///
    /// Встроенное действие (`Horizontal`, `Jump`…), которого нет в списке, убирается из
    /// `input`: его удалили на странице Input, и игра не должна получать его привязки.
    pub fn apply(&self, input: &mut Input) {
        for name in ["Horizontal", "Vertical", "Jump", "Fire1", "Submit", "Cancel"] {
            if self.find(name).is_none() {
                input.remove_action(name);
            }
        }
        for action in &self.actions {
            if action.name.is_empty() {
                continue;
            }
            match action.kind {
                ActionKind::Button => {
                    input.remove_action(&action.name);
                    input.set_button_bindings(&action.name, action.button_bindings());
                }
                ActionKind::Axis => {
                    input.remove_action(&action.name);
                    input.set_axis_bindings(&action.name, action.axis_bindings());
                }
            }
        }
    }

    pub fn find(&self, name: &str) -> Option<&InputAction> {
        self.actions.iter().find(|a| a.name == name)
    }
}

// ----------------------------------------------------------------------------
// Graphics
// ----------------------------------------------------------------------------

/// Пресет качества: `ctx.set_quality(QualityLevel::Low)` в игре и список в редакторе.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityLevel {
    Low,
    Medium,
    #[default]
    High,
}

impl QualityLevel {
    pub const ALL: [QualityLevel; 3] = [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High];

    pub fn label(self) -> &'static str {
        match self {
            QualityLevel::Low => "Low",
            QualityLevel::Medium => "Medium",
            QualityLevel::High => "High",
        }
    }
}

/// Unity: Project Settings → Quality. Читает рендерер из `world.resources` каждый кадр,
/// поэтому смена на лету (`ctx.set_quality`) видна со следующего кадра.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    /// Выбранный пресет; поля ниже — его значения, их можно подправить вручную.
    pub quality: QualityLevel,
    /// Тени от солнца.
    pub shadows: bool,
    /// Потолок дальности теней; меньшее из него и `Environment::shadow_distance`.
    pub shadow_distance: f32,
    /// Блум (свечение ярких участков).
    pub bloom: bool,
    /// Множитель силы блума из `Environment`.
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    /// Множитель плотности тумана из `Environment`; 0 — без тумана.
    pub fog_scale: f32,
    /// Масштаб разрешения 3D-сцены (0.25…2): 0.75 — сцена рисуется в 75 % ширины и
    /// высоты и растягивается при сведении кадра (интерфейс остаётся чётким). Главный
    /// рычаг для слабых видеокарт: время кадра на рельефе почти пропорционально числу
    /// пикселей. Больше 1 — сверхвыборка.
    pub render_scale: f32,
    /// Сколько каскадов теней разрешено компоненту `SoftShadows` (1–4).
    pub shadow_cascades: u32,
    /// Самый дорогой фильтр края теней `SoftShadows`: 0 — PCF 3×3, 1 — диск Пуассона,
    /// 2 — контактное упрочнение.
    pub shadow_filter: u32,
    /// Экранное затенение (компонент `AmbientOcclusion`) разрешено.
    pub ambient_occlusion: bool,
    /// Блум «13 выборок» (компонент `PostQuality`) разрешён.
    pub high_quality_bloom: bool,
    /// FXAA (компонент `PostQuality`) разрешено.
    pub fxaa: bool,
    /// Полная детализация рельефа: вторая выборка против повторов плиток и трипланарная
    /// скала. Выключено — рельеф заметно дешевле (слабые видеокарты, браузер, телефоны).
    pub terrain_detail: bool,
    /// Сколько точечных ламп и прожекторов может освещать кадр с компонентом `ManyLights`
    /// (4…255; ближние к камере — первыми).
    pub max_lights: u32,
    /// Сколько ламп с компонентом `LightShadows` отбрасывают тени (ближние — первыми;
    /// точечная лампа — шесть проходов теней, прожектор — один).
    pub light_shadows: u32,
    /// Детализация воды с компонентом `WaterEffects`: 0 — волны и блик, 1 — + преломление,
    /// цвет по глубине и пена, 2 — + отражения сцены по экрану.
    pub water_detail: u32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self::preset(QualityLevel::High)
    }
}

impl GraphicsSettings {
    /// Значения пресета.
    pub fn preset(level: QualityLevel) -> Self {
        match level {
            QualityLevel::Low => Self {
                quality: level,
                shadows: false,
                shadow_distance: 20.0,
                bloom: false,
                bloom_intensity: 1.0,
                bloom_threshold: 1.0,
                fog_scale: 1.0,
                render_scale: 1.0,
                shadow_cascades: 1,
                shadow_filter: 0,
                ambient_occlusion: false,
                high_quality_bloom: false,
                fxaa: true,
                terrain_detail: false,
                max_lights: 16,
                light_shadows: 0,
                water_detail: 0,
            },
            QualityLevel::Medium => Self {
                quality: level,
                shadows: true,
                shadow_distance: 30.0,
                bloom: true,
                bloom_intensity: 0.6,
                bloom_threshold: 1.0,
                fog_scale: 1.0,
                render_scale: 1.0,
                shadow_cascades: 2,
                shadow_filter: 0,
                ambient_occlusion: true,
                high_quality_bloom: true,
                fxaa: true,
                terrain_detail: true,
                max_lights: 64,
                light_shadows: 1,
                water_detail: 1,
            },
            QualityLevel::High => Self {
                quality: level,
                shadows: true,
                shadow_distance: 1000.0,
                bloom: true,
                bloom_intensity: 1.0,
                bloom_threshold: 1.0,
                fog_scale: 1.0,
                render_scale: 1.0,
                shadow_cascades: 4,
                shadow_filter: 2,
                ambient_occlusion: true,
                high_quality_bloom: true,
                fxaa: true,
                terrain_detail: true,
                max_lights: 255,
                light_shadows: 2,
                water_detail: 2,
            },
        }
    }
}

// ----------------------------------------------------------------------------
// Audio
// ----------------------------------------------------------------------------

/// Unity: Project Settings → Audio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub ui_volume: f32,
    /// Заглушать звук, пока окно игры не в фокусе.
    pub mute_when_unfocused: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master_volume: 1.0, music_volume: 1.0, sfx_volume: 1.0, ui_volume: 1.0, mute_when_unfocused: false }
    }
}
