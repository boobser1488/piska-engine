# Хранитель фонаря

2D-платформер на Piska Engine и образец того, как устроен проект: уровни, персонажи,
интерфейс, звук и данные лежат в ассетах и открываются в редакторе, код — это скрипты
на Rust по одному компоненту в файле.

Маленький дух-фонарь спускается по шести тёмным пещерам. В каждой — парящие светлячки:
соберите нужное число, и арка выхода загорится. Пропасти, шипы, хрупкие и движущиеся
платформы, пружины, контрольные точки, четыре вида врагов и босс в конце.

## Правила в пяти строках

1. У духа три сердца. Враг, снаряд, шипы или пропасть отнимают сердце; после удара —
   полторы секунды неуязвимости. Шипы и пропасть возвращают на последнюю безопасную землю.
2. Нет сердец — «фонарь погас»: можно продолжить с контрольной точки или начать заново.
3. Светлячки светят и тянутся к игроку. Нужное число (счётчик слева вверху) открывает выход;
   запертые ворота тоже требуют светлячков. В каждом уровне спрятан один золотой.
4. Врагов (кроме тяжеляков) можно раздавить прыжком сверху. Тяжеляка не раздавить:
   обойти, перепрыгнуть или пробежать рывком.
5. Звёзды за уровень: ★ дошли до выхода; ★★ не менее 60 % светлячков и не больше двух
   падений; ★★★ ещё и золотой светлячок, и время не хуже контрольного
   (числа — `assets/data/`).

Двойной прыжок открывается в конце 2-го уровня, рывок — в конце 4-го. Следующий уровень
открывается, когда пройден предыдущий.

## Управление

| | Клавиатура | Геймпад |
|---|---|---|
| Бег | A / D, ← / → | левый стик, крестовина |
| Прыжок (держите — выше) | Пробел, W, ↑ | A (South) |
| Второй прыжок в воздухе | то же (после 2-го уровня) | то же |
| Рывок (после 4-го уровня) | Shift, X | X (West), RB, RT |
| Вниз сквозь «облачко» | S / ↓ + прыжок | стик вниз + A |
| Пауза | Esc, P | Start |
| Меню | мышь, Tab / стрелки, Enter | крестовина / стик, A, B (назад) |

Касания (телефон, планшет): меню и кнопки работают касанием как мышью; после первого
касания на игровом экране появляются кнопки «влево», «вправо», «рывок» и «прыжок».
Карта ввода — `assets/project.settings` (Edit → Project Settings → Input в редакторе).

## Структура проекта

```text
games/lantern/
├── assets/
│   ├── scenes/        menu.scene и level1..6.scene: тайлмапы, фон, камера, враги, предметы
│   ├── prefabs/       player, crawler, brute, bat, spitter, boss, firefly, golden_firefly,
│   │                  checkpoint, spring, moving_platform, crumble_platform, gate, exit,
│   │                  projectile, rock, fx_dust / fx_sparkle / fx_burst
│   ├── ui/            hud, pause, results, menu_canvas, main_menu, level_select,
│   │                  options, help — объекты engine-ui, сохранённые префабами
│   ├── sprites/       листы спрайтов, тайлсет, фоны, значки интерфейса (PNG + .meta)
│   ├── audio/         эффекты и музыка — .wav, отрендеренные из синтезатора движка
│   ├── data/          levels.ron (названия, нужные светлячки, контрольное время, музыка),
│   │                  rules.ron (правила звёзд)
│   ├── scenarios/     level1..6.scenario: заставка уровня и подсказки (язык Scenario)
│   └── project.settings   карта ввода, окно, теги, порядок скриптов
├── src/
│   ├── lib.rs         build(): подключить ассеты, зарегистрировать скрипты, загрузить меню
│   ├── scripts/       по компоненту на файл: player, crawler, brute, bat, spitter, boss,
│   │                  firefly, checkpoint, spring, moving_platform, crumble_platform, gate,
│   │                  exit, projectile, rock, parallax, level_manager, menu_controller
│   │                  (mod.rs пишет редактор)
│   ├── phys.rs        кинематика по сетке тайлов; combat.rs — касание с врагами;
│   │                  common.rs, ui.rs, sfx.rs, audio_bank.rs, data.rs, controls.rs — помощники
│   ├── main.rs, bin/lantern-editor.rs
├── examples/
│   ├── bake_assets/   печь префабов, интерфейса и звука (инструмент разработчика)
│   └── shots.rs       снимки экрана без окна
├── tests/             gameplay.rs (игра без окна), editor_assets.rs (проект в редакторе)
└── tools/             gen_assets.py (картинки), gen_levels.py (сцены, данные, сценарии)
```

## Как открыть в редакторе

```bash
cargo run -p lantern --features editor --bin lantern-editor      # или из Project Hub
```

Откроется `scenes/menu.scene`; уровни — в окне Project (`scenes/levelN.scene`). В иерархии
уровня: `Tiles` (карта), `Deco`, фон `bg_*`, группы `Enemies`, `Fireflies`, `Checkpoints`,
`Mechanisms`, `Player`, `Exit`, `Level` (менеджер + сценарий), экземпляры `Hud`,
`PauseOverlay`, `ResultsOverlay`.

## Как править ассетами

* **Новый уровень.** Скопируйте `level6.scene`/`level1.scene` как `level7.scene`, правьте
  `Tiles`, расставляйте префабы из `assets/prefabs` (перетащить из Project в сцену; числа
  врагов и платформ — поля компонентов в Inspector). Добавьте строку в `data/levels.ron`
  (и увеличьте `LEVEL_COUNT` в `src/data.rs`, если уровней больше шести), скопируйте
  `scenarios/levelN.scenario`.
* **Новый враг.** Файл скрипта в `src/scripts/` (Assets → New Rust Script), спрайт-лист
  в `assets/sprites`, объект с `SpriteRenderer` + `SpriteAnimator` + скриптом → Save as
  Prefab в `assets/prefabs`. Контакт с игроком — `combat::contact` / `combat::resolve`.
* **Новый экран интерфейса.** Соберите объекты через GameObject → UI, сохраните префабом в
  `assets/ui`, поставьте экземпляр в сцену; скрипту достаточно найти кнопки по имени
  (`ctx.ui().clicked("имя")`).
* **Подсказки и заставка.** `assets/scenarios/levelN.scenario` — перезагружается при
  сохранении, даже во время игры.
* **Баланс.** `data/levels.ron`, `data/rules.ron`; физика героя — константы в
  `src/scripts/player.rs`.

## Запуск, тесты, экспорт

```bash
cargo run -p lantern                                    # играть
ENGINE_FRAMES=600 cargo run -p lantern                  # 600 кадров и выход; FPS — в логе
cargo test -p lantern                                   # игра без окна
cargo test -p lantern --features editor --test editor_assets   # проект в редакторе
cargo check -p lantern --target wasm32-unknown-unknown  # веб-сборка
cargo run -p piska-export -- web --game lantern --release
cargo run -p piska-export -- windows --game lantern --release
```

Перегенерировать содержимое (обычно не нужно — готовые файлы лежат в `assets/`):

```bash
python games/lantern/tools/gen_assets.py                 # картинки
python games/lantern/tools/gen_levels.py                 # сцены, data/*.ron, сценарии, project.settings
ENGINE_AUDIO=off cargo run -p lantern --example bake_assets   # префабы, ui/*.prefab, audio/*.wav
```

Снимки экрана без окна: `LANTERN_SCREEN=level3 LANTERN_PLAYER=40,9 ENGINE_SCREENSHOT=shot.png
ENGINE_FRAMES=30 cargo run -p lantern --example shots`.

Игра не читает файлы напрямую и не использует потоки и системное время, поэтому одна и та
же `build()` работает на десктопе, в браузере и на Android. В браузере кнопки «Выход» нет.
