# Звук (`engine-audio`)

Музыка и эффекты в стиле Unity: источники и слушатель на объектах сцены, ресурс `Audio`
для скриптов, шины громкости. Всё нужное лежит в `engine::prelude::*`.

Главная идея: **ни одного файла не нужно**. Синтезатор (`synth`) делает прыжок, монету,
взрыв и музыкальную петлю прямо из кода. Файлы `.wav` и `.ogg` тоже поддерживаются.

* [Быстрый старт](#быстрый-старт)
* [Клипы](#клипы)
* [Синтезатор и рецепты](#синтезатор-и-рецепты)
* [Ресурс `Audio`](#ресурс-audio)
* [Компоненты `AudioSource` и `AudioListener`](#компоненты)
* [Пространственный звук (2D и 3D)](#пространственный-звук)
* [Музыка и переходы](#музыка)
* [Шины громкости](#шины)
* [Редактор: Play, Pause, Stop](#редактор)
* [Устройство, ошибки, платформы](#устройство-ошибки-платформы)
* [Тесты и запись в файл](#тесты-и-запись-в-файл)

## Быстрый старт

```rust
use engine::prelude::*;

component! {
    pub struct Hero {
        pub jump_speed: f32 = 8.0,
    }
}

impl Component for Hero {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.input().key_down(Key::Space) {
            // Звук играется «в никуда»: если клипа нет, в лог попадёт подсказка, игра не упадёт.
            if let Some(audio) = ctx.resource_mut::<Audio>() {
                audio.play("jump");
            }
        }
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app(); // ресурс Audio и подсистема уже подключены
    {
        let audio = app.world.resources.get_mut::<Audio>().unwrap();
        audio.add_clip("jump", synth::sfx::jump());
        audio.add_clip("theme", synth::music::adventure());
        audio.play_music("theme", 0.5); // музыка по кругу, плавное включение
    }
    app.spawn("Camera").with(Camera::default()).with(AudioListener::default());
    app.spawn("Hero").with(Hero::default());
    engine::run(app, WindowConfig::titled("Game"))
}
```

Работающий пример: `cargo run --example audio_demo` (клавиши 1-8 — эффекты, `M` — музыка,
`P` — кружащий источник).

## Клипы

`AudioClip` — готовые сэмплы `f32` (чередующиеся каналы). Клипы неизменяемы и раздаются
через `Arc`, поэтому один клип могут играть сотни голосов.

```rust
// Из кода:
let clip = AudioClip::from_samples(44_100, 1, samples_vec);   // sample_rate, channels, interleaved
let clip = AudioClip::mono(44_100, samples_vec);

// Из файла или байтов (.wav: PCM 8/16/24/32 бит и float; .ogg: Vorbis):
let clip = AudioClip::load("assets/audio/theme.ogg")?;
let clip = AudioClip::from_bytes(include_bytes!("hit.wav"))?;   // формат по заголовку

// Из синтезатора:
let clip: AudioClip = synth::sfx::coin();
let clip: AudioClip = Tone::new(Wave::Sine, 440.0).duration(0.3).render().into_clip();

// Сохранить (отладка, «запечь» сгенерированный звук):
clip.save_wav("coin.wav")?;
```

Свойства: `sample_rate()`, `channels()`, `frames()`, `duration()`, `peak()`, `samples()`.

**Регистрация по имени** — основной способ для звука из кода:

```rust
audio.add_clip("coin", synth::sfx::coin());          // повторный вызов заменяет клип
audio.load_clip("boss", "assets/audio/boss.ogg")?;   // файл с диска под именем
audio.has_clip("coin");  audio.remove_clip("coin");
```

**Из базы ассетов** (клип лежит в проекте, ссылка по GUID — так, как их назначает инспектор):

```rust
engine::mount_assets(&mut app, "assets")?;     // Audio сам подхватывает AssetDatabase
let handle = app.world.resources.get::<AssetDatabase>().unwrap()
    .handle::<engine::assets::AudioClip>("audio/theme.ogg")?;
audio.play(handle);                            // декодируется при первом использовании и кэшируется
audio.preload(handle);                         // декодировать заранее, чтобы не «подвисало» на первом звуке
```

Поддерживаются только `.wav` и `.ogg` (Vorbis). `.mp3`, `.flac`, Opus: сообщение об ошибке
подскажет переконвертировать файл. База ассетов тоже не считает `.mp3` и `.flac` звуком (импортёр
`Audio` — только у `.wav` и `.ogg`; в окне Project такие файлы видны как `Raw`).
Клипы-ассеты читаются через базу ассетов, поэтому одинаково работают из каталога и из пака
(браузер, Android). Большие `.ogg` декодируются целиком в память, потоковой
подгрузки нет: для музыки на несколько минут это десятки мегабайт, учитывайте.

Куда передавать клип: везде, где в API написано `impl Into<ClipRef>`, подойдёт имя
(`"coin"`, `&str`/`String`), `AssetHandle<engine::assets::AudioClip>`, `Arc<AudioClip>`, `&Arc<AudioClip>`
или сам `AudioClip`.

## Синтезатор и рецепты

Модуль `synth` (в prelude: `synth`, `Tone`, `Wave`, `Sound`, `Melody`).

### Кирпичики

| Тип | Что делает |
|---|---|
| `Wave` | `Sine`, `Square`, `Saw`, `Triangle`, `Noise` |
| `Tone` | один тон: волна + свип частоты + огибающая ADSR + вибрато + фильтр → `render() -> Sound` |
| `Envelope` | ADSR: `Envelope::adsr(attack, decay, sustain, release)`, `Envelope::percussive(decay)` |
| `Sound` | моно-буфер с цепочками: `gain`, `then`, `mix`, `mix_at`, `pad`, `repeat`, `reverse`, `fade_in`, `fade_out`, `lowpass`, `echo`, `normalize`, `into_clip` |
| `Melody` | строка нот → `Sound`; `render_loop()` даёт бесшовную петлю |
| `synth::sfx` | готовые эффекты |
| `synth::music` | готовые музыкальные петли |

`Tone` строится цепочкой (все методы возвращают `Tone`):

```rust
Tone::new(Wave::Square, 220.0)   // волна и начальная частота, Гц
    .sweep_to(660.0)             // свип: к концу тона частота 660 Гц (по умолчанию экспоненциальный,
                                 //   .linear_sweep() — равномерный по герцам)
    .duration(0.15)              // секунд, пока «клавиша зажата» (звук длится ещё release)
    .adsr(0.005, 0.05, 0.6, 0.04)// attack, decay, sustain (уровень 0..1), release — секунды
    .volume(0.4)
    .duty(0.35)                  // скважность квадрата: 0.5 меандр, 0.125 «жужжащий»
    .vibrato(6.0, 0.3)           // Гц модуляции, глубина в полутонах
    .lowpass(3000.0)             // ФНЧ; .lowpass_sweep(5000.0, 200.0) — «скользящий» срез
    .render()                    // -> Sound длиной duration + release
```

Особые случаи: `Tone::noise()` — белый шум (частота шума — «зернистость», 0 — обычный белый);
`Tone::note(Wave::Triangle, "A4")` — по имени ноты; `.percussive()` — быстрый спад за `duration`;
`.seed(n)` — другой, но воспроизводимый шум. Сэмплы детерминированы: один и тот же код
всегда даёт тот же звук.

Ноты: `synth::note_freq("A4") == Some(440.0)`, `synth::midi_to_freq(60.0)`.

### Готовые эффекты

`synth::sfx::` — `jump`, `land`, `coin`, `powerup`, `hit`, `hurt`, `laser`, `explosion`, `blip`,
`ui_click`, `ui_hover`, `ui_confirm`, `ui_error`, `footstep`, `win`, `lose`. Каждый возвращает `AudioClip`:

```rust
audio.add_clip("jump", synth::sfx::jump());
audio.add_clip("boom", synth::sfx::explosion());
```

Чтобы однотипные звуки не звучали «машинно», варьируйте высоту: `PlayParams::new().pitch(0.9 + rand * 0.2)`.

### Рецепты своих звуков

**Прыжок** — быстрый подъём квадратной волны:

```rust
let jump = Tone::new(Wave::Square, 260.0).sweep_to(620.0).duration(0.16)
    .duty(0.35).adsr(0.004, 0.06, 0.5, 0.05).render().normalize(0.8).into_clip();
```

**Монетка** — две ноты вверх (вторая звонче и дольше):

```rust
let coin = Tone::new(Wave::Square, 987.8).duration(0.07).volume(0.6).render()
    .then(Tone::new(Wave::Square, 1318.5).duration(0.2).adsr(0.002, 0.15, 0.1, 0.08).volume(0.6).render())
    .into_clip();
```

**Удар** — щелчок шума плюс низкий «тум»:

```rust
let hit = Tone::noise().duration(0.12).lowpass_sweep(5000.0, 400.0).percussive().render()
    .mix(Tone::new(Wave::Sine, 180.0).sweep_to(70.0).duration(0.1).percussive().volume(0.8).render())
    .into_clip();
```

**Взрыв** — шум с падающим срезом и басовый «бум»:

```rust
let boom = Tone::noise().duration(0.9).adsr(0.002, 0.8, 0.0, 0.1).lowpass_sweep(2800.0, 90.0).render()
    .mix(Tone::new(Wave::Sine, 90.0).sweep_to(28.0).duration(0.7).adsr(0.002, 0.6, 0.0, 0.1).render())
    .normalize(0.8).into_clip();
```

**Лазер** — быстро падающая пила: `Tone::new(Wave::Saw, 2000.0).sweep_to(180.0).duration(0.2).lowpass(6000.0)`.

**Клик интерфейса** — очень короткий тон: `Tone::new(Wave::Triangle, 1500.0).sweep_to(900.0).duration(0.03).percussive()`.

**Бонус (арпеджио)**:

```rust
let powerup = Sound::concat(["C5", "E5", "G5", "C6"].iter().map(|n|
    Tone::note(Wave::Square, n).duration(0.055).duty(0.25).render())).into_clip();
```

**Эхо, реверс, повтор**: `sound.echo(0.25, 0.4, 0.5)` (задержка с, обратная связь, громкость повторов),
`sound.reverse()`, `sound.repeat(3)`.

### Музыкальная петля

`Melody` читает строку нот. Один токен — один шаг (по умолчанию восьмая: 2 шага на долю):
`C4`, `F#3`, `Bb5` — нота, `_` — пауза, `-` — продлить предыдущую ноту на шаг.

```rust
let bpm = 120.0;
let bass = Melody::new(Wave::Triangle, bpm).volume(0.5)
    .notes("A2 _ E3 _ A2 _ E3 _")          // такт 1
    .notes("F2 _ C3 _ F2 _ C3 _")          // такт 2: notes() дописывает шаги
    .render_loop();
let lead = Melody::new(Wave::Square, bpm).volume(0.2).duty(0.25)
    .notes("E5 - C5 E5 A5 - E5 -")
    .notes("C5 - A4 C5 F5 - C5 _")
    .render_loop();
// Ударные: 'x' — удар, остальное пауза; шаг сетки в секундах.
let step = 60.0 / bpm / 2.0;
let kick = Tone::new(Wave::Sine, 150.0).sweep_to(45.0).duration(0.12).percussive().render();
let drums = Sound::pattern(&kick, "x...x...x...x...", step);
// Смешать (все петли одной длины — 16 шагов) и сделать клипом.
let music = Sound::mix_all([bass, lead, drums]).normalize(0.7).into_clip();
audio.add_clip("level1", music);
audio.play_music("level1", 1.0);
```

`render_loop()` даёт звук ровно длиной в число шагов, а хвосты нот заворачиваются в начало,
поэтому петля **без щелчка** на стыке. `render()` оставляет хвост последней ноты — для
одноразовых фраз и джинглов (победа, поражение). Параметры `Melody`: `steps_per_beat(n)`,
`adsr(..)`, `volume`, `duty`, `gate` (доля шага, пока нота звучит).

Готовые петли: `synth::music::adventure()` (бодрая, ~7 с) и `synth::music::calm()` (спокойная).

## Ресурс Audio

`world.resources.get_mut::<Audio>()`; из хука компонента — `ctx.resource_mut::<Audio>()`
(возвращает `Option`).

```rust
// Запуск
audio.play("coin") -> VoiceId                        // один раз, шина Sfx
audio.play_ui("ui_click") -> VoiceId                 // шина Ui
audio.play_at("boom", Vec3::new(5.0, 0.0, 0.0))      // в точке мира: громкость и панорама от слушателя
audio.play_with("engine", PlayParams::new()          // со всеми параметрами
    .volume(0.6).pitch(1.2).pan(-0.3).looping(true).bus(Bus::Sfx)
    .fade_in(0.5).start_at(1.0)                      // нарастание с 0 за 0.5 с, начать с 1-й секунды клипа
    .at(position).range(2.0, 25.0));                 // пространственный: min/max расстояние

// Управление играющим голосом (VoiceId из play*; устаревший безвреден)
audio.stop(voice);                 // мгновенно, с «антищёлчковым» затуханием за 4 мс
audio.fade_out(voice, 1.0);        // затухнуть за 1 с и остановить
audio.fade_in(voice, 0.5);         // вернуть громкость после fade_out
audio.set_volume(voice, 0.5);  audio.set_pitch(voice, 1.5);  audio.set_pan(voice, 0.8);
audio.set_looping(voice, true);
audio.is_playing(voice) -> bool;   audio.position(voice) -> Option<f32>   // секунда клипа
audio.voice_count() -> usize

// Всё сразу
audio.pause_all();  audio.resume_all();  audio.is_paused();
audio.stop_all();   audio.stop_bus(Bus::Sfx);
```

`VoiceId::NONE` возвращается, если клип не найден (в лог один раз пишется подсказка:
«клип `x` не зарегистрирован, вызовите `audio.add_clip`…»). Все операции с `NONE` безвредны.
Одновременно играет не больше 64 голосов: при переполнении вытесняется самый тихий.

**Игровая логика не зависит от звуковой карты**: без устройства голоса всё равно «идут»
(по времени кадров), `is_playing` честно становится `false` по окончании клипа, затухания
работают. Можно ждать конца звука и не бояться зависнуть.

## Компоненты

Объявлены через `component!`: сохраняются в сценах, видны в инспекторе редактора.

```rust
AudioSource {
    clip: AssetHandle<AudioClip>,   // аудиофайл проекта (в инспекторе — выбор из .wav/.ogg)
    clip_name: String,              // имя клипа из кода (audio.add_clip); если задано, берётся вместо clip
    volume: f32 = 1.0,
    pitch: f32 = 1.0,
    looping: bool = false,
    play_on_start: bool = true,     // начать играть, когда объект появился в сцене
    spatial: bool = false,          // 3D/2D-звук: расстояние и панорама относительно слушателя
    min_distance: f32 = 1.0,        // ближе — полная громкость
    max_distance: f32 = 30.0,       // дальше — тишина
    bus: Bus = Bus::Sfx,
}
AudioListener { volume: f32 = 1.0 }   // «уши»; обычно на камере или персонаже
```

Конструкторы `AudioSource`: `named("coin")`, `from_asset(handle)`, цепочка `with_volume`, `with_pitch`,
`with_looping`, `with_bus`, `with_spatial(min, max)`, `manual()` (без автозапуска).

```rust
app.spawn("Campfire").at(3.0, 0.0)
    .with(AudioSource::named("fire").with_looping(true).with_spatial(1.0, 12.0));
app.spawn("Camera").with(Camera::default()).with(AudioListener::default());
```

Что делает подсистема каждый кадр:

* `play_on_start`-источник запускается на первом кадре после появления (один раз; закончившийся
  одноразовый звук сам не перезапускается);
* поля компонента (`volume`, `pitch`, `looping`, `bus`, позиция) **применяются на лету** к играющему голосу;
* объект уничтожен, выключен (`active = false`) или у него удалили `AudioSource` — голос
  останавливается. Выключенный и снова включённый объект запускается заново;
* клип, который ещё не зарегистрирован, не ошибка: источник молчит и пробует снова каждый кадр.

Ручное управление источником (нужно при `play_on_start = false` и для перезапуска):

```rust
audio.play_source(object_id);       // с начала; звук начнётся в ближайшем кадре
audio.stop_source(object_id);
audio.is_source_playing(object_id) -> bool
audio.source_voice(object_id) -> Option<VoiceId>   // например, для set_pitch
```

В сцене (`.scene`) компонент записывается так же, как любой другой: `AudioSource(clip_name: "coin", ...)`;
типы зарегистрированы в `engine::app()`.

## Пространственный звук

`spatial: true` (или `PlayParams::at`/`play_at`) включает расчёт от слушателя — первого активного
`AudioListener` в сцене:

* **Громкость**: 1 до `min_distance`, плавно (квадратично) спадает до 0 на `max_distance`;
* **Панорама**: проекция направления на источник на «правую» ось слушателя (его поворот
  учитывается). Источник справа от слушателя звучит в правом канале, слева — в левом. Внутри
  `min_distance` панорама ослабляется, чтобы звук «у самого уха» не метался.

Работает одинаково в 2D (камера с `AudioListener`, источники в плоскости XY: панорама по оси X, расстояние
по XY) и в 3D (слушатель на камере: «вправо» — её правая ось). Единицы — мировые (метры).

Если слушателя в сцене нет, пространственные источники звучат как обычные: полная громкость, по центру.
Нужен второй слушатель — берётся первый активный (по порядку иерархии).

Допустимы и «позиционные» разовые звуки без компонента: `audio.play_at("boom", pos)`. Позиция
фиксируется в момент вызова; для движущегося источника используйте `AudioSource`.

Формулу можно взять отдельно: `engine::audio::spatialize(&ListenerState, source_pos, min, max) -> (gain, pan)`.

## Музыка

```rust
audio.play_music("level1", 1.5);   // по кругу, шина Music. Предыдущая музыка затухает 1.5 с, новая нарастает
audio.play_music("level1", 1.5);   // тот же клип уже играет: ничего не происходит
audio.stop_music(1.0);             // плавно выключить
audio.is_music_playing();  audio.music_voice() -> Option<VoiceId>
audio.set_bus_volume(Bus::Music, 0.5);   // ползунок «громкость музыки»
```

`crossfade` — секунды; `0.0` — переключение без перехода. Пример смены музыки при входе в бой:

```rust
audio.play_music("battle", 0.8);
// …бой закончился:
audio.play_music("level1", 2.0);
```

## Шины

Четыре шины: `Bus::Master`, `Bus::Music`, `Bus::Sfx`, `Bus::Ui`. У каждого голоса есть шина
(`AudioSource::bus`, `PlayParams::bus`); итоговая громкость = голос × шина × `Master`.

```rust
audio.set_bus_volume(Bus::Master, 0.8);
audio.set_bus_volume(Bus::Music, 0.4);
audio.set_bus_muted(Bus::Sfx, true);       // mute не теряет значение громкости
audio.bus_volume(Bus::Music);  audio.is_bus_muted(Bus::Sfx);
```

Громкости меняются плавно (без щелчков). `Bus` — перечисление для инспектора, его можно
использовать полем своих компонентов.

## Редактор

Подсистема понимает режимы редактора:

* **Play** — звук работает как в игре; `play_on_start`-источники стартуют;
* **Pause** (редактор вызывает `idle`) — звук замирает вместе с игрой и продолжается после Play;
* **Stop** — сцена восстанавливается из снимка, **все звуки обрываются** (в том числе разовые
  и музыка). Новый Play стартует источники сцены заново.

В Edit-режиме звуковая карта не открывается, пока ничего не играет.

## Устройство, ошибки, платформы

Устройство вывода (`cpal` 0.18) открывается **лениво, при первом звуке** (в браузере этого и требует политика
автовоспроизведения: звук должен начаться после жеста пользователя). Нет устройства, оно занято, формат не
поддерживается, устройство пропало во время игры — в лог пишется **одно** предупреждение, игра идёт дальше
без звука. Состояние: `audio.device_status()` → `DeviceStatus::{NotOpened, Active{..}, Unavailable(причина), Disabled}`.

* `ENGINE_AUDIO=off` (или `0`/`none`) — не открывать звуковую карту; в режиме `ENGINE_SCREENSHOT` звук выключен сам.
* `audio.set_device_enabled(false)` — то же из кода (в тестах).
* Фича `device` (включена по умолчанию) подключает `cpal`; без неё крейт собирается на любой платформе и
  работает «вхолостую».

Платформы: Windows (WASAPI), Linux (ALSA; нужен `libasound2-dev`), Android (AAudio, без NDK C++-кода),
macOS/iOS (CoreAudio), `wasm32-unknown-unknown` (Web Audio через `cpal/wasm-bindgen`). Windows проверен на практике; остальные — сборкой.

Поток звука: `cpal` вызывает микшер из своего потока; игровой код общается с ним через короткие замки,
микшер при воспроизведении не выделяет память.

## Тесты и запись в файл

Микшер не зависит от устройства — его можно прогнать и проверить по сэмплам:

```rust
let mut app = engine::app();
let audio = app.world.resources.get_mut::<Audio>().unwrap();
audio.set_device_enabled(false);                 // тест не трогает звуковую карту
audio.add_clip("beep", Tone::new(Wave::Sine, 440.0).duration(0.5).render().into_clip());
audio.play("beep");
let mut buf = vec![0.0f32; 4800 * 2];            // интерлив. стерео, 48 кГц
audio.render(&mut buf);                          // buf — следующие 0.1 с звука
assert!(buf.iter().any(|s| s.abs() > 0.1));
app.run_headless(30, 1.0 / 60.0);                // кадры двигают микшер «вхолостую»
```

Отдельно от приложения: `engine::audio::Mixer::new(44_100)` с `play(Arc<AudioClip>, VoiceParams)` и
`render(&mut [f32])`; `Mixer::advance(seconds)` двигает время без буфера.

## Ограничения

* Только `.wav` и `.ogg` (Vorbis); `.mp3`, `.flac`, Opus не декодируются. Декодирование целиком, без стриминга.
* Один слушатель; эффекта Доплера, реверберации по зонам и фильтров на шинах нет (есть `Sound::lowpass`/`echo` при сборке клипа).
* Пространственный `AudioSource` меняет высоту только вручную (`pitch`), позиция разового `play_at` не следует за объектом.
* Нет записи с микрофона.
