//! Покадровая анимация спрайтов: клипы поверх листа спрайтов.
//!
//! Клип в сцене — одна строка, её удобно править руками и в инспекторе:
//!
//! ```text
//! "run: 8-13 @12"            кадры 8…13, 12 кадров/с, по кругу (по умолчанию)
//! "jump: 14,15,16 @10 once"  один проход, затем остановка
//! "idle: 0-3,2,1 @6 pingpong" произвольный список; pingpong — туда и обратно
//! ```

use engine_core::prelude::*;

use crate::components::SpriteRenderer;

/// Как проигрывается клип.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipMode {
    /// По кругу.
    Loop,
    /// Один раз; на последнем кадре остановка и сообщение `animation_finished`.
    Once,
    /// Вперёд и назад по кругу.
    PingPong,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ClipError {
    #[error("animation clip `{0}` has no `name:` prefix; write it like `run: 8-13 @12 loop`")]
    NoName(String),
    #[error("animation clip `{name}` has no frames; list them after the colon, e.g. `{name}: 0-3`")]
    NoFrames { name: String },
    #[error("animation clip `{name}`: cannot read `{token}`; use a frame number (`4`) or a range (`4-7`)")]
    BadFrame { name: String, token: String },
    #[error("animation clip `{name}`: fps must be a positive number after `@`, got `{token}`")]
    BadFps { name: String, token: String },
    #[error("animation clip `{name}`: unknown mode `{token}`; use `loop`, `once` or `pingpong`")]
    BadMode { name: String, token: String },
}

/// Именованная последовательность кадров.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClip {
    pub name: String,
    pub frames: Vec<u32>,
    pub fps: f32,
    pub mode: ClipMode,
}

impl AnimationClip {
    pub fn new(name: &str, frames: impl IntoIterator<Item = u32>, fps: f32, mode: ClipMode) -> Self {
        Self {
            name: name.to_string(),
            frames: frames.into_iter().collect(),
            fps,
            mode,
        }
    }

    /// Разобрать строку вида `"run: 8-13 @12 loop"`.
    pub fn parse(text: &str) -> Result<AnimationClip, ClipError> {
        let (name, rest) = text
            .split_once(':')
            .ok_or_else(|| ClipError::NoName(text.trim().to_string()))?;
        let name = name.trim().to_string();
        let mut frames = Vec::new();
        let mut fps = 10.0;
        let mut mode = ClipMode::Loop;
        // Токены разделяются пробелами; кадры внутри токена — запятыми.
        for token in rest.split_whitespace() {
            if let Some(value) = token.strip_prefix('@') {
                fps = value
                    .parse::<f32>()
                    .ok()
                    .filter(|f| *f > 0.0 && f.is_finite())
                    .ok_or_else(|| ClipError::BadFps {
                        name: name.clone(),
                        token: value.to_string(),
                    })?;
            } else if token.chars().next().is_some_and(|c| c.is_alphabetic()) {
                mode = match token.to_ascii_lowercase().as_str() {
                    "loop" => ClipMode::Loop,
                    "once" => ClipMode::Once,
                    "pingpong" | "ping-pong" => ClipMode::PingPong,
                    _ => {
                        return Err(ClipError::BadMode {
                            name,
                            token: token.to_string(),
                        })
                    }
                };
            } else {
                for part in token.split(',').filter(|p| !p.is_empty()) {
                    let bad = || ClipError::BadFrame {
                        name: name.clone(),
                        token: part.to_string(),
                    };
                    match part.split_once('-') {
                        Some((a, b)) => {
                            let a: u32 = a.parse().map_err(|_| bad())?;
                            let b: u32 = b.parse().map_err(|_| bad())?;
                            if a <= b {
                                frames.extend(a..=b);
                            } else {
                                frames.extend((b..=a).rev());
                            }
                        }
                        None => frames.push(part.parse().map_err(|_| bad())?),
                    }
                }
            }
        }
        if frames.is_empty() {
            return Err(ClipError::NoFrames { name });
        }
        Ok(AnimationClip {
            name,
            frames,
            fps,
            mode,
        })
    }

    /// Длительность одного прохода, секунд.
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.fps.max(1e-3)
    }

    /// Кадр листа в момент `time` секунд от начала клипа и признак «клип закончился»
    /// (только для `Once`).
    pub fn frame_at(&self, time: f32) -> (u32, bool) {
        let count = self.frames.len();
        if count == 0 {
            return (0, true);
        }
        let step = (time.max(0.0) * self.fps).floor() as usize;
        match self.mode {
            ClipMode::Loop => (self.frames[step % count], false),
            ClipMode::Once => (self.frames[step.min(count - 1)], step >= count),
            ClipMode::PingPong => {
                if count == 1 {
                    return (self.frames[0], false);
                }
                let period = 2 * count - 2;
                let k = step % period;
                (self.frames[if k < count { k } else { period - k }], false)
            }
        }
    }
}

impl std::fmt::Display for AnimationClip {
    /// Та же запись, которую понимает [`AnimationClip::parse`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let frames: Vec<String> = self.frames.iter().map(u32::to_string).collect();
        let mode = match self.mode {
            ClipMode::Loop => "loop",
            ClipMode::Once => "once",
            ClipMode::PingPong => "pingpong",
        };
        write!(f, "{}: {} @{} {}", self.name, frames.join(","), self.fps, mode)
    }
}

/// Событие «клип `Once` доиграл до конца». Читается через `world.events.read`.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationFinished {
    pub object: GameObjectId,
    pub clip: String,
}

/// Имя сообщения, которое получают компоненты объекта, когда клип закончился
/// (значение — `Value::Str(имя клипа)`).
pub const ANIMATION_FINISHED: &str = "animation_finished";

/// Разобранные клипы: пересобираются, когда меняется исходный текст.
#[derive(Clone, Debug, Default)]
pub struct AnimatorRuntime {
    source: Vec<String>,
    clips: Vec<AnimationClip>,
    bad: Vec<String>,
}

component_rt! {
    /// Проигрывает клипы по листу спрайтов: каждый кадр пишет номер кадра в
    /// `SpriteRenderer` того же объекта (лист — `sheet_columns` × `sheet_rows`).
    /// Клипы — строки `"имя: кадры @fps режим"`, например `"run: 8-13 @12"`.
    pub struct SpriteAnimator {
        /// Клипы: `"idle: 0-3 @6"`, `"jump: 14,15 @10 once"`, `"wave: 0-4 @8 pingpong"`.
        pub clips: Vec<String> = Vec::new(),
        /// Текущий клип. Пусто — первый из списка.
        pub clip: String = String::new(),
        pub playing: bool = true,
        /// Множитель скорости; 2 — вдвое быстрее.
        pub speed: f32 = 1.0,
        /// Секунд от начала текущего клипа.
        pub time: f32 = 0.0,
        /// Закончился ли клип `Once`.
        pub finished: bool = false,
        /// Номер кадра, записанный в спрайт на последнем обновлении.
        pub frame: u32 = 0,
    }
    runtime: AnimatorRuntime
}

impl Component for SpriteAnimator {
    /// Остальные хуки движок не вызывает.
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::UPDATE
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        self.ensure_clips();
        if self.clip.is_empty() {
            if let Some(first) = self.rt.clips.first() {
                self.clip = first.name.clone();
            }
        }
        self.sample();
        self.apply(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if let Some(clip) = self.advance(dt) {
            ctx.send_message(ctx.id, ANIMATION_FINISHED, Value::Str(clip.clone()));
            ctx.emit(AnimationFinished {
                object: ctx.id,
                clip,
            });
        }
        self.apply(ctx);
    }
}

impl SpriteAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавить клип: `SpriteAnimator::new().with_clip("idle: 0-3 @6")`.
    pub fn with_clip(mut self, spec: &str) -> Self {
        self.clips.push(spec.to_string());
        self
    }

    /// Добавить готовый клип.
    pub fn with_clip_data(mut self, clip: &AnimationClip) -> Self {
        self.clips.push(clip.to_string());
        self
    }

    /// Какой клип играть первым.
    pub fn starting_with(mut self, name: &str) -> Self {
        self.clip = name.to_string();
        self
    }

    fn ensure_clips(&mut self) {
        if self.rt.source == self.clips {
            return;
        }
        self.rt.source = self.clips.clone();
        self.rt.clips.clear();
        self.rt.bad.clear();
        for spec in &self.clips {
            match AnimationClip::parse(spec) {
                Ok(clip) => self.rt.clips.push(clip),
                Err(error) => {
                    log::error!("SpriteAnimator: {error}");
                    self.rt.bad.push(error.to_string());
                }
            }
        }
    }

    /// Ошибки разбора клипов (для редактора и тестов).
    pub fn clip_errors(&mut self) -> &[String] {
        self.ensure_clips();
        &self.rt.bad
    }

    /// Разобранный клип по имени.
    pub fn clip_named(&mut self, name: &str) -> Option<&AnimationClip> {
        self.ensure_clips();
        self.rt.clips.iter().find(|c| c.name == name)
    }

    pub fn has_clip(&mut self, name: &str) -> bool {
        self.clip_named(name).is_some()
    }

    /// Запустить клип. Если он уже играет, ничего не происходит — можно звать
    /// каждый кадр: `anim.play(if running { "run" } else { "idle" })`.
    /// Неизвестное имя пишется в лог один раз и игнорируется.
    pub fn play(&mut self, name: &str) {
        if self.clip == name && self.playing && !self.finished {
            return;
        }
        self.restart(name);
    }

    /// То же, что [`play`](Self::play), но всегда с первого кадра.
    pub fn restart(&mut self, name: &str) {
        if !self.has_clip(name) {
            if self.clip != name {
                log::warn!(
                    "SpriteAnimator: no clip `{name}`; defined: {}",
                    self.rt.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
            return;
        }
        self.clip = name.to_string();
        self.time = 0.0;
        self.playing = true;
        self.finished = false;
        self.sample();
    }

    pub fn stop(&mut self) {
        self.playing = false;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn current_clip(&self) -> &str {
        &self.clip
    }

    /// Кадр листа в текущий момент.
    pub fn current_frame(&self) -> u32 {
        self.frame
    }

    /// Обновить `frame` по `clip` и `time`; `true`, если `Once`-клип дошёл до конца.
    fn sample(&mut self) -> bool {
        self.ensure_clips();
        let Some(clip) = self.rt.clips.iter().find(|c| c.name == self.clip) else {
            return false;
        };
        let (frame, done) = clip.frame_at(self.time);
        self.frame = frame;
        done
    }

    /// Продвинуть время. Возвращает имя клипа, если он только что закончился.
    pub fn advance(&mut self, dt: f32) -> Option<String> {
        self.ensure_clips();
        if self.clip.is_empty() {
            self.clip = self.rt.clips.first()?.name.clone();
        }
        if !self.playing {
            return None;
        }
        self.time += dt.max(0.0) * self.speed.max(0.0);
        if self.sample() && !self.finished {
            self.finished = true;
            self.playing = false;
            return Some(self.clip.clone());
        }
        None
    }

    fn apply(&self, ctx: &mut Ctx) {
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            if sprite.frame != self.frame {
                sprite.frame = self.frame;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_lists_fps_and_mode() {
        let clip = AnimationClip::parse("run: 8-10,2 @12 once").unwrap();
        assert_eq!(clip.frames, vec![8, 9, 10, 2]);
        assert_eq!(clip.fps, 12.0);
        assert_eq!(clip.mode, ClipMode::Once);
        assert_eq!(AnimationClip::parse(&clip.to_string()).unwrap(), clip);
        assert_eq!(AnimationClip::parse("back: 3-1").unwrap().frames, vec![3, 2, 1]);
    }

    #[test]
    fn reports_readable_errors() {
        assert!(matches!(AnimationClip::parse("0-3"), Err(ClipError::NoName(_))));
        assert!(matches!(AnimationClip::parse("a:"), Err(ClipError::NoFrames { .. })));
        assert!(matches!(AnimationClip::parse("a: 1-x"), Err(ClipError::BadFrame { .. })));
        assert!(matches!(AnimationClip::parse("a: 1 @0"), Err(ClipError::BadFps { .. })));
        assert!(matches!(AnimationClip::parse("a: 1 spin"), Err(ClipError::BadMode { .. })));
    }

    #[test]
    fn frame_selection_per_mode() {
        let looped = AnimationClip::parse("a: 0-3 @10").unwrap();
        assert_eq!(looped.frame_at(0.0).0, 0);
        assert_eq!(looped.frame_at(0.25).0, 2);
        assert_eq!(looped.frame_at(0.4).0, 0);
        let once = AnimationClip::parse("a: 0-3 @10 once").unwrap();
        assert_eq!(once.frame_at(0.35), (3, false));
        assert_eq!(once.frame_at(0.45), (3, true));
        let ping = AnimationClip::parse("a: 0-3 @10 pingpong").unwrap();
        let seq: Vec<u32> = (0..8).map(|i| ping.frame_at(i as f32 * 0.1 + 0.01).0).collect();
        assert_eq!(seq, vec![0, 1, 2, 3, 2, 1, 0, 1]);
    }
}
