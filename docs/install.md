# Установка Piska Engine и его зависимостей

Установщики ставят и **настраивают** всё, что нужно разработчику: Rust, компилятор C++,
CMake, Android SDK / NDK, zig, цели Rust, переменные окружения и пути в Preferences →
Build & Tools редактора. Версии, ссылки, контрольные суммы и списки пакетов лежат в одном
файле — [`install/packages.toml`](../install/packages.toml); его читают оба установщика,
`piska-export doctor` и тесты, а таблицы ниже сверяются с ним тестом
(`install/tests`), поэтому расходиться не могут.

## Графический установщик

**`piska-installer`** — одна программа с окном (Windows, Linux, macOS) для машины, на
которой ещё нет ни Rust, ни чего-либо другого: скачали, запустили, нажали «Далее».
Устанавливает она не сама: внутри неё лежат те же `install.ps1` / `install.sh` /
`packages.toml`, при запуске они пишутся во временный каталог (и читаются обратно для
проверки), а окно запускает их в машиночитаемом режиме (`-Json` / `--json`, см. ниже) и
показывает их события. Поэтому план, проверки, контрольные суммы, журнал, манифест и
`-Resume` — ровно те же, что у скриптов.

```text
piska-installer                         # окно
piska-installer --dry-run               # всё то же, но скрипты только показывают план
piska-installer --components core,web --install-dir D:\Piska --lang ru
piska-installer --source D:\src\engine  # исходники движка — этот каталог, а не клон
piska-installer --uninstall             # сразу на страницу удаления
piska-installer --unattended --dry-run --components core   # без окна: события JSON (CI)
piska-installer --unattended --components core --accept-license vs-buildtools
```

Собрать из исходников: `cargo build --release -p piska-installer` (готовые файлы для
Windows, Linux и macOS собирает CI — артефакты `piska-installer-*` в workflow `install`).

Страницы мастера:

1. **Welcome** — что будет сделано, язык (English / Русский, по умолчанию — язык системы),
   лицензия движка. Сразу в фоне идёт проверка «что уже стоит» (сухой прогон скрипта по
   всем компонентам — только чтение) и `doctor`. Если найден манифест прежней установки —
   выбор **Modify** (добавить компоненты), **Repair** (повторить недоделанное, `-Repair`) или
   **Uninstall**.
2. **Components** — таблица: компонент, зачем он, объём скачивания и на диске (из
   `packages.toml`), состояние на этой машине (установлен с версией / не установлен /
   установлен не полностью / старая версия). Зависимости отмечаются сами и не снимаются
   (с пояснением, кто их требует). Наборы: Minimal (`core`), Typical (`core`, `cpp`, `web`),
   Everything. Внизу — нужно места / свободно; не хватает — предупреждение.
3. **Options** — каталог установки (встроенный обзор каталогов хаба); откуда исходники
   движка: **существующий каталог** (если установщик запущен из копии репозитория — по
   умолчанию она) или **скачать из репозитория** (сейчас он приватный: окно предупреждает,
   что нужен доступ — `gh auth login` или Git Credential Manager); ярлыки (меню «Пуск» /
   меню приложений, рабочий стол); ассоциации файлов (`.pmodel`, `.scene`, `.scenario` и файл
   проекта — расширение из `engine_i18n::brand`, открывает страницу Import хаба); какие
   переменные окружения будут заданы; «записать найденные пути в настройки редактора»;
   дополнительно — сухой прогон, каталог кэша, прокси (`HTTPS_PROXY` для скриптов).
4. **Licences** — каждая сторонняя лицензия (Visual Studio Build Tools, Android SDK) со
   ссылкой и **своей** галочкой. Ничего не принимается за вас: без галочек «Далее» и
   «Install» недоступны; скрипту передаются только отмеченные (`-AcceptLicense <id>`).
5. **Review** — точный план из сухого прогона скрипта с выбранными параметрами: каждая
   команда, ссылка скачивания с размером и статусом контрольной суммы («закреплена» /
   «сверяется с издателем при установке»), каждое изменение окружения, реестра и файлов,
   какие шаги попросят права администратора и зачем. «Copy Plan» / «Save Plan…».
6. **Installing** — общий ход и ход шага, текущее действие, скорость и оставшееся время
   скачивания, раскрываемый журнал. Перед запросом прав администратора окно показывает,
   зачем он (UAC на Windows; на Linux команда сначала показывается, затем `pkexec` спрашивает
   пароль в своём окне, а без `pkexec` вы выполняете эту одну команду в терминале — пароль
   через установщик не проходит никогда). Долгая тишина — «Still working…» (скрипт не
   убивается). **Cancel** спрашивает, останавливает скрипт со всеми его процессами; сделанное
   записано в файле состояния — **Resume** продолжает. Сбой: имя шага, последние строки
   журнала, **Retry Step** (`-Resume`), **Skip Step** (только там, где скрипт разрешает),
   **Open Log**, **Copy Error**.
7. **Finish** — таблица `doctor` (найдено / версия / путь / чего не хватает и чем
   починить), что установлено и куда, «Launch Piska Hub», «Open the Documentation», «Show the
   Log».

Окно не ждёт ни скрипт, ни диск (всё в фоновых потоках); переживает падение скрипта,
строки, которые не являются событиями (они показываются в журнале как есть), и зависание.
Свои настройки (размер окна, язык, последний выбор) — в `<каталог настроек>/installer.ron`
(`%APPDATA%\piska`, `~/.config/piska`); второй запуск выводит вперёд уже открытое окно.
На Windows у программы нет окна консоли (подсистема GUI; к консоли терминала она
подключается только для `--help`, `--version` и `--unattended`), PowerShell запускается
скрытым, по умолчанию Windows PowerShell 5.1 (`--pwsh` — PowerShell 7), политика выполнения
меняется только для этого процесса (`-ExecutionPolicy Bypass`).

Коды выхода `piska-installer` (окно — по итогу последнего прогона): `0` — готово или ничего
не запускалось, `1` — сбой шага, `2` — неверные аргументы, `3` — нужна перезагрузка, `4` —
лицензия или права не приняты, `5` — остановлено человеком, `6` — скрипты не удалось
подготовить или запустить.

Ниже — **скрипты**: тот же путь без окна (и альтернатива, если окно не подходит).

Что делают все установщики:

* сначала **ищут**, что уже стоит (версия показывается), и пропускают найденное;
* предпочитают менеджер пакетов (winget, apt / dnf / pacman / zypper, Homebrew); прямые
  загрузки — только с **закреплённой версией и проверкой SHA-256** (не совпало — файл
  удаляется и не используется);
* никогда не запускают скачанные скрипты молча; администратор — только для шага, которому
  он нужен, с объяснением и одним запросом;
* лицензии (Visual Studio Build Tools, Android SDK) принимаются **только** флагом
  `-AcceptLicenses` / `--accept-licenses` или вашим ответом (`-Yes` сам лицензию не принимает);
* не затирают ваши настройки: перед правкой `editor.ron`, `~/.cargo/config.toml`,
  профиля оболочки делается копия `*.piska-backup-<время>`, непустые значения остаются;
* ведут журнал, файл состояния (продолжение после сбоя или перезагрузки: `-Resume` /
  `--resume`) и **манифест установленного** — по нему работает удаление;
* ничего не отправляют наружу (нет телеметрии), понимают `HTTPS_PROXY`, TLS 1.2+,
  работают без сети для уже скачанных файлов (`-Cache` / `--cache`);
* `-DryRun` / `--dry-run` печатает каждую команду, ссылку, контрольную сумму и изменение
  окружения и **ничего не делает**.

## Требования

| | |
|---|---|
| Windows | Windows 10 / 11, PowerShell 5.1 или 7, winget («Установщик приложений» из Microsoft Store) |
| Linux | Debian / Ubuntu (apt), Fedora (dnf), Arch (pacman), openSUSE (zypper), `curl`, `sudo` |
| macOS | Homebrew (https://brew.sh — установщик его сам не ставит), Xcode Command Line Tools |
| Диск | см. таблицу компонентов; на Windows короткий путь установки (лимит 260 символов) |
| GPU | драйвер с Vulkan (или DX12 на Windows, Metal на macOS); драйверы установщик не ставит |

## Быстрый запуск

Основной путь — **из копии репозитория** (так работает и сейчас, пока репозиторий закрытый):

```powershell
# Windows: двойной щелчок по install\Install-PiskaEngine.cmd — меню; или в PowerShell:
powershell -ExecutionPolicy Bypass -File install\install.ps1                       # меню
powershell -ExecutionPolicy Bypass -File install\install.ps1 -Components core,web -DryRun
powershell -ExecutionPolicy Bypass -File install\install.ps1 -Components all -AcceptLicenses
```

```bash
# Linux / macOS
bash install/install.sh                              # меню
bash install/install.sh --components core,web --dry-run
bash install/install.sh --components all --accept-licenses --yes
```

`-ExecutionPolicy Bypass` действует только на этот запуск, политика системы не меняется.

**Одной строкой из сети** (только когда репозиторий станет публичным; адрес каталога
`install/` — поле `project.raw_base` в `install/packages.toml`; сначала прочитайте скрипт,
установщик покажет SHA-256 скачанного `packages.toml`):

```powershell
& ([scriptblock]::Create((irm <RAW_BASE>/install.ps1))) -Components core -DryRun
```

```bash
curl -fsSL <RAW_BASE>/install.sh -o install.sh && less install.sh && bash install.sh --components core --dry-run
```

Пока репозиторий закрытый, шаг `engine` клонирует его через ваш `git` — войдите в GitHub
(`gh auth login` или Git Credential Manager).

### Параметры

| Windows (`install.ps1`) | Linux / macOS (`install.sh`) | что |
|---|---|---|
| `-Components core,cpp,…\|all` | `--components …` | какие компоненты (зависимости добавляются сами) |
| `-Yes` | `--yes` | без вопросов (лицензии — отдельно) |
| `-AcceptLicenses` | `--accept-licenses` | принять лицензии (Build Tools, Android SDK) |
| `-DryRun` | `--dry-run` | только показать план |
| `-InstallDir`, `-Cache` | `--prefix`, `--cache` | где живёт установка и скачанное |
| `-AndroidSdk` | `--android-sdk` | свой каталог Android SDK |
| `-Resume` / `-Repair` | `--resume` / `--repair` | продолжить после сбоя / перепроверить всё |
| `-Uninstall` | `--uninstall` | удалить то, что поставил установщик |
| `-NoPrefs` | `--no-prefs` | не писать пути в настройки редактора |
| `-FileAssociations`, `-DesktopShortcut`, `-NoShortcuts` | `--file-associations` | ассоциации `.pmodel` `.scene` `.scenario` и файла проекта (`project_extension` в `packages.toml`; открывает страницу Import хаба), ярлыки |
| `-CargoTargetDir <папка>` | `--cargo-target-dir <папка>` | общий `target-dir` в `~/.cargo/config.toml` (блок с копией файла) |
| `-ListComponents` | `--list-components` | список компонентов |
| `-Json` | `--json` | машиночитаемый вывод (ниже) |
| `-AcceptLicense vs-buildtools,android-sdk` | `--accept-license android-sdk` | принять **эти** лицензии (по id) |
| `-ConfirmElevation` | `--confirm-elevation` | ответить «да» на «Continue?» перед шагом с правами администратора (запрос Windows / `pkexec` всё равно появится) |
| `-SkipSteps id,id` | `--skip-steps id,id` | пропустить шаги (только те, без которых следующие работают) |
| `-SourceDir <папка>` | `--source <папка>` | исходники движка — этот checkout, а не клон |
| — | `--elevate pkexec\|sudo\|none` | как получить права для менеджера пакетов в `--json` (`none` — сообщить команду для терминала) |
| — | `--no-shortcuts`, `--desktop-shortcut` | без пунктов меню приложений / ярлык на рабочем столе |

Коды выхода: `0` — готово, `1` — сбой шага (его имя в сообщении; повторите с `-Resume`),
`2` — неверные аргументы, `3` — нужна перезагрузка / системный диалог, `4` — вы отказались.

### Машиночитаемый режим (`-Json` / `--json`)

С этим флагом stdout — **только** события: по одному объекту JSON в строке (в PowerShell —
чистый ASCII, всё остальное как `\uXXXX`, чтобы кодовая страница консоли ничего не
испортила). Без флага вывод для людей не меняется ни на символ (golden-файлы тестов).
Вопросы приходят событием `prompt`, ответ — одна строка в stdin: `yes` (или `{"answer":true}`)
либо что угодно другое / конец ввода — «нет». Поток всегда заканчивается событием `summary`
(и после ошибки).

| событие | поля |
|---|---|
| `hello` | `protocol` (1), `script`, `shell`, `os`, `mode`, `dry_run`, `components`, `install_dir`, `repo_dir`, `in_checkout`, `cache`, `state_file`, `manifest_file`, `prefs_file`, `log`, `free_bytes`, `is_admin` (bash — ещё `family`, `arch`, `elevate`) |
| `plan` | шаг сухого прогона: `n`, `total`, `id`, `component`, `title`, `skip`, `reason` (`found` / `done-earlier` / `requested`), `found`, `version`, `path`, `elevation` (зачем права или `null`), `licence` (`{id, name, url}`), `skippable`, `download_mb`, `downloads` (`url`, `size_mb`, `sha256`, `sha256_url`, `pinned`, `pin_status`, `cross_check`), `lines` (`{kind, text}` — те же строки, что в плане для людей) |
| `step` | `phase`: `start` / `skip` / `done` / `failed` / `declined`; `step`, `n`, `total`, `title`; у `failed` — `error`, `log`, `skippable`, `elevation_refused`, `manual_command` (Linux без `pkexec`) |
| `progress` | `step`, `url`, `bytes`, `total` (или `null`), `estimated` (bash: размер из `packages.toml`) |
| `log` | `level` (`info` / `warn` / `error` / `output` — вывод запущенной программы), `step`, `text` |
| `prompt` | `kind` (`licence` / `confirm`), `step`, `licence`, `name`, `url`; у `confirm` — `key` (`prefs-overwrite`, `cargo-config`, `file-associations`, `elevation`, `uninstall-proceed`, `uninstall-item`), `subject`, `question`, `default` |
| `licence` | лицензия принята: `step`, `licence`, `name`, `url` |
| `elevation` | сейчас появится запрос прав: `step`, `reason`, `command` (bash), `confirmed` |
| `reboot` | нужна перезагрузка после `step` |
| `uninstall-plan` | `manifest`, `dry_run`, `items` (`action`, `target`, `ask`) |
| `error` | `text` (неверные аргументы, исключение) |
| `summary` | `exit`, `ok`, `dry_run`, `failed_step`, `error`, `reboot`, `elevation_refused`, `steps` (`total`, `done`, `skipped`, `failed`), `log`, `state_file`, `manifest_file`, `repo_dir`, `path` (PATH после установки), `env` (переменные пользователя) |

Отмена — завершить процесс скрипта вместе с потомками: шаг, который не закончился, не
отмечен в файле состояния, и `-Resume` выполнит его снова. Тесты (`install/tests`) сверяют
поток `-Json` с планом для людей для всех 63 сочетаний компонентов (те же шаги в том же
порядке) и хранят его golden-файлы (`windows-json-*.jsonl`, `unix-json-*.jsonl`).

## Компоненты

| компонент | что ставит | зачем | Windows | Linux / macOS |
|---|---|---|---|---|
| `core` | Rust (rustup, stable, MSVC), Visual Studio Build Tools (C++ и Windows SDK), Git, проверка Vulkan; на Linux — компилятор, pkg-config, ALSA, udev, Mesa Vulkan | собрать и запустить движок, редактор, хаб | 6 GB | 1.5 GB |
| `cpp` | CMake + Ninja | горячо перезагружаемые C++-плагины | 200 MB | 150 MB |
| `web` | цель `wasm32-unknown-unknown` | экспорт в браузер | 60 MB | 60 MB |
| `android` | JDK 17, Android command-line tools, SDK-пакеты, cargo-ndk 4.1.2, цель `aarch64-linux-android`, `ANDROID_HOME`, `ANDROID_NDK_HOME` | экспорт APK | 5 GB | 5 GB |
| `linux-cross` | zig 0.17.1 (SHA-256 закреплён и сверяется с index.json), цель `x86_64-unknown-linux-gnu`, `PISKA_ZIG` | экспорт под Linux с Windows / macOS | 400 MB | 400 MB |
| `engine` | клон репозитория (с подмодулем `modeler/`), сборка хаба, редактора, `piska-export` и модельера (release), ярлыки со значком, скрипт удаления | запускать движок без ручных команд | 3 GB | 3 GB |

Версии (из `packages.toml`): платформа Android `android-35`, build-tools `36.0.0`, NDK
`28.2.13676358` — те, что предлагает `piska-export`; Windows: winget-пакеты
`Microsoft.VisualStudio.2022.BuildTools` и `EclipseAdoptium.Temurin.17.JDK`.

### Пакеты Linux / macOS

| дистрибутив | сборка | запуск | cpp | JDK |
|---|---|---|---|---|
| Debian / Ubuntu | `build-essential` `pkg-config` `libasound2-dev` `libudev-dev` | `mesa-vulkan-drivers` `libvulkan1` `libxkbcommon0` `libwayland-client0` `libx11-6` | `cmake` `ninja-build` | `openjdk-17-jdk-headless` |
| Fedora | `gcc` `gcc-c++` `pkgconf-pkg-config` `alsa-lib-devel` `systemd-devel` | `mesa-vulkan-drivers` `vulkan-loader` `libxkbcommon` `libwayland-client` `libX11` | `cmake` `ninja-build` | `java-17-openjdk-devel` |
| Arch | `base-devel` `pkgconf` `alsa-lib` `systemd-libs` | `vulkan-icd-loader` `vulkan-radeon` `libxkbcommon` `wayland` `libx11` | `cmake` `ninja` | `jdk17-openjdk` |
| openSUSE | `gcc` `gcc-c++` `pkgconf-pkg-config` `alsa-devel` `systemd-devel` | `libvulkan1` `Mesa-vulkan-device-select` `libxkbcommon0` `libwayland-client0` `libX11-6` | `cmake` `ninja` | `java-17-openjdk-devel` |

macOS: `brew install pkg-config cmake ninja`, JDK — `brew install --cask temurin@17`,
компилятор — `xcode-select --install`. Список сборочных пакетов совпадает с
[`docs/api/export.md`](api/export.md) §10.1 (это проверяет тест). На NVIDIA вместо Mesa
нужен драйвер NVIDIA; на Intel — `vulkan-intel` (Arch). Остальные дистрибутивы:
поставьте эквиваленты вручную и запустите установщик с `--skip-packages`.

## Диагностика: `piska-export doctor`

```bash
cargo run -p piska-export -- doctor                 # таблица
cargo run -p piska-export -- doctor --json          # для хаба / редактора
cargo run -p piska-export -- doctor --components android,web --strict
```

Только читает: запускает `--version` у найденного и смотрит каталоги. Для каждой проблемы
печатается точная команда установщика, которая её чинит. Код выхода: `1` — не хватает
чего-то из выбранных компонентов (без `--components` — из `core`; с `--strict` — из любых),
`2` — ошибка запуска. Установщик запускает `doctor` в конце, когда собран `piska-export`.

Схема `--json` (стабильна):

```json
{ "os": "windows|linux|macos", "ok": true, "failed": 0,
  "checks": [ { "component": "core", "id": "rustc", "label": "rustc",
                "status": "ok|old|missing|info", "version": "1.97.1" ,
                "path": "…", "detail": "…", "fix": "команда установщика или null" } ] }
```

Хаб и редактор могут добавить кнопку «Install missing tools…»: запустить
`piska-export doctor --json` и для каждой проверки со `status` `old` / `missing` показать
`fix`.

## Что куда записывается

* **Preferences → Build & Tools** (`%APPDATA%\PiskaEngine\editor.ron`, на Linux / macOS
  `~/.config/PiskaEngine/editor.ron`): `tool_cargo`, `tool_android_sdk`, `tool_android_ndk`,
  `tool_java`, `tool_zig`, `tool_cmake`, `tool_cxx` — только пустые или отсутствующие
  значения; чужое установщик **спрашивает**, прежде чем заменить, и делает копию.
* Переменные окружения пользователя (Windows) / блок в `~/.profile` или `~/.zprofile`,
  подключающий `env.sh` (Linux / macOS): `ANDROID_HOME`, `ANDROID_NDK_HOME`, `JAVA_HOME`
  (если не задана), `PISKA_ZIG`.
* `~/.cargo/config.toml` — **только** с `-CargoTargetDir`: добавляется отдельный блок
  `# >>> piska-engine installer … <<<`, остальной файл не трогается.
* Манифест и состояние: `%LOCALAPPDATA%\PiskaEngine\install\` (Linux / macOS
  `~/.local/state/PiskaEngine/install/`) — `manifest.json` / `manifest.tsv`, `state.json` /
  `state.tsv`, `logs/`.

## Установка вручную

Если установщик не подходит — то же самое руками (команды совпадают с планом `-DryRun`):

1. Rust: https://rustup.rs → `rustup default stable`, `rustup component add clippy rustfmt`.
2. Windows: Visual Studio Build Tools с рабочей нагрузкой «Разработка классических
   приложений на C++» и Windows SDK; Git; Linux: пакеты из таблицы выше.
3. Веб: `rustup target add wasm32-unknown-unknown`.
4. Android: JDK 17; command-line tools с https://developer.android.com/studio →
   `sdkmanager --licenses`, затем `sdkmanager "platform-tools" "platforms;android-35"
   "build-tools;36.0.0" "ndk;28.2.13676358"`; `cargo install cargo-ndk --version 4.1.2 --locked`;
   `rustup target add aarch64-linux-android`; переменные `ANDROID_HOME`, `ANDROID_NDK_HOME`.
5. Linux-экспорт с Windows / macOS: zig с https://ziglang.org/download/ (сверьте SHA-256),
   `rustup target add x86_64-unknown-linux-gnu`, `PISKA_ZIG=<путь к zig>`.
6. Движок: `git clone --recurse-submodules <repo>`; `cargo build --release -p piska-hub
   -p piska-editor -p piska-export`; модельер —
   `cargo build --release --manifest-path modeler/Cargo.toml -p piska-modeler`.
7. В редакторе: Preferences → Build & Tools → Detect для каждого инструмента.

## Если что-то не получилось

| симптом | что делать |
|---|---|
| «winget was not found» | поставьте «Установщик приложений» из Microsoft Store или обновите Windows |
| Build Tools поставились, но `link.exe` не находится | откройте «Visual Studio Installer» → Modify → «Разработка классических приложений на C++»; проверьте `piska-export doctor` |
| установщик остановился с кодом 3 | перезагрузите Windows (или завершите системный диалог Xcode) и запустите с `-Resume` / `--resume` |
| `needs you to accept its licence` | добавьте `-AcceptLicenses` / `--accept-licenses` или запустите без `-Yes` |
| `checksum mismatch` | файл повреждён или подменён и **не использован**; проверьте сеть / прокси, удалите папку кэша и повторите |
| за прокси | задайте `HTTPS_PROXY=http://хост:порт` |
| `sdkmanager` пишет про Java | задайте `JAVA_HOME` на JDK 17 и откройте новый терминал |
| команды не видны после установки | откройте **новый** терминал: PATH и переменные читаются при старте |
| `git clone failed` | репозиторий приватный: `gh auth login` или Git Credential Manager |
| путь длиннее 260 символов | укажите короткий `-InstallDir` (например `C:\Piska`) |
| незнакомый дистрибутив Linux | поставьте пакеты вручную, затем `--skip-packages` |

## Удаление и ремонт

```powershell
powershell -ExecutionPolicy Bypass -File install\install.ps1 -Uninstall -DryRun   # что будет удалено
powershell -ExecutionPolicy Bypass -File install\install.ps1 -Uninstall
powershell -ExecutionPolicy Bypass -File install\install.ps1 -Repair               # перепроверить и доделать
```

Удаляется только записанное в манифесте (то, что **поставил установщик**; найденное
готовым не трогается): ярлыки, переменные (возвращаются прежние значения), блоки в
конфигурации, цели Rust, cargo-ndk, каталоги SDK / zig. Установленные программы (winget,
rustup) и непустые пользовательские файлы удаляются только после вопроса по каждому;
системные пакеты Linux не удаляются — установщик только перечисляет их.
