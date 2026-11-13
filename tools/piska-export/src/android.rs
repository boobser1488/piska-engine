//! Экспорт для Android: подписанный APK без Gradle.
//!
//! Шаги: проверка SDK / NDK / Java / cargo-ndk → пак ассетов → `cargo ndk … rustc
//! --crate-type cdylib` (библиотека `lib<пакет>.so` с `android_main` из
//! `engine::game_main!`) → проверка библиотеки (все `NEEDED` есть в Android или в APK)
//! → `AndroidManifest.xml` + значок → `aapt2 compile` / `aapt2 link` против
//! `android.jar` → в архив добавляются `lib/<abi>/*.so` и `assets/assets.pak` (без
//! сжатия: Android читает их прямо из APK) → `zipalign` (страницы по 16 КиБ) →
//! `apksigner sign` → `apksigner verify`.
//!
//! Результат — `dist/<game>/android/<game>.apk`; `adb install -r <game>.apk`.
//! Промежуточные файлы — в `target/piska-android/<game>/`, в `dist/` попадает только APK.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::archive::{self, Method, ZipWriter};
use crate::project::{fresh_dir, Project};
use crate::{cargo, elf, icon, step, Error, Options, Platform, Report, Result};

/// Минимальная версия Android: API 26 (8.0) — с неё есть AAudio, через который
/// играет звук (cpal). Ниже библиотека игры просто не загрузится.
pub const MIN_SDK: u32 = 26;
/// ABI по умолчанию: все современные телефоны — 64-битный ARM.
pub const DEFAULT_ABI: &str = "arm64-v8a";
/// Пароль отладочного хранилища ключей Android — общеизвестный (`android`), как у
/// Android Studio; хранилище и ключ — только для отладки.
const DEBUG_PASSWORD: &str = "android";
const DEBUG_ALIAS: &str = "androiddebugkey";
/// Переменные окружения для пароля своего ключа (пароль не пишется в командную строку).
pub const KEYSTORE_PASSWORD_ENV: &str = "PISKA_KEYSTORE_PASSWORD";
pub const KEY_PASSWORD_ENV: &str = "PISKA_KEY_PASSWORD";

/// Библиотеки, которые есть в самом Android (стабильный NDK API, API ≥ 26).
pub const SYSTEM_LIBS: &[&str] = &[
    "libc.so",
    "libm.so",
    "libdl.so",
    "liblog.so",
    "libandroid.so",
    "libaaudio.so",
    "libOpenSLES.so",
    "libEGL.so",
    "libGLESv1_CM.so",
    "libGLESv2.so",
    "libGLESv3.so",
    "libvulkan.so",
    "libz.so",
    "libjnigraphics.so",
    "libmediandk.so",
    "libnativewindow.so",
    "libcamera2ndk.so",
    "libneuralnetworks.so",
    "libsync.so",
    "libamidi.so",
    "libbinder_ndk.so",
];
/// Стандартная библиотека C++ из NDK: её кладём в APK сами (её тянет C++-мост).
const CXX_SHARED: &str = "libc++_shared.so";

/// ABI Android ↔ цель Rust ↔ каталог библиотек в sysroot NDK.
pub fn abi_target(abi: &str) -> Option<(&'static str, &'static str)> {
    Some(match abi {
        "arm64-v8a" => ("aarch64-linux-android", "aarch64-linux-android"),
        "armeabi-v7a" => ("armv7-linux-androideabi", "arm-linux-androideabi"),
        "x86_64" => ("x86_64-linux-android", "x86_64-linux-android"),
        "x86" => ("i686-linux-android", "i686-linux-android"),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Поиск SDK, NDK, build-tools, платформы
// ---------------------------------------------------------------------------

/// Найденные инструменты Android.
#[derive(Clone, Debug)]
pub struct Sdk {
    pub root: PathBuf,
    pub ndk: PathBuf,
    /// `<ndk>/toolchains/llvm/prebuilt/<host>`.
    pub ndk_prebuilt: PathBuf,
    pub build_tools: PathBuf,
    pub android_jar: PathBuf,
    /// API установленной платформы (`platforms/android-37.0` → 37) — targetSdk.
    pub platform_api: u32,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn script(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.bat")
    } else {
        name.to_string()
    }
}

/// Числа в имени версии — для сортировки (`36.0.0`, `android-37.0`, `28.2.13676358`).
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// Подкаталоги `dir`, от новой версии к старой.
fn versions_desc(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort_by_key(|p| std::cmp::Reverse(version_key(&p.file_name().unwrap_or_default().to_string_lossy())));
    dirs
}

impl Sdk {
    /// Найти SDK по переменным окружения и стандартным местам установки.
    pub fn detect() -> Result<Sdk> {
        Self::detect_with(&|name| std::env::var_os(name).filter(|v| !v.is_empty()))
    }

    /// То же с подставляемым окружением (для тестов).
    pub fn detect_with(env: &dyn Fn(&str) -> Option<OsString>) -> Result<Sdk> {
        let path = |name: &str| env(name).map(PathBuf::from);
        let mut candidates: Vec<(String, PathBuf)> = Vec::new();
        for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            if let Some(dir) = path(var) {
                candidates.push((format!("${var}"), dir));
            }
        }
        let home = path("HOME").or_else(|| path("USERPROFILE"));
        if let Some(local) = path("LOCALAPPDATA") {
            candidates.push(("the default location".into(), local.join("Android").join("Sdk")));
        }
        if let Some(home) = &home {
            candidates.push(("the default location".into(), home.join("Library").join("Android").join("sdk")));
            candidates.push(("the default location".into(), home.join("Android").join("Sdk")));
        }
        let root = candidates
            .iter()
            .find(|(_, dir)| dir.join("platforms").is_dir() || dir.join("build-tools").is_dir())
            .map(|(_, dir)| dir.clone())
            .ok_or_else(|| {
                let looked: Vec<String> = candidates.iter().map(|(why, d)| format!("  {} ({why})", d.display())).collect();
                Error(format!(
                    "the Android SDK was not found. Install Android Studio (or the command-line tools) and set \
                     ANDROID_HOME to the SDK folder. Looked in:\n{}",
                    if looked.is_empty() { "  (nowhere: neither ANDROID_HOME nor a home folder is set)".to_string() } else { looked.join("\n") }
                ))
            })?;

        // NDK: переменная или самый новый `ndk/<версия>`.
        let ndk = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK"]
            .iter()
            .find_map(|var| path(var))
            .or_else(|| versions_desc(&root.join("ndk")).into_iter().next())
            .or_else(|| Some(root.join("ndk-bundle")).filter(|d| d.is_dir()))
            .ok_or_else(|| {
                Error(format!(
                    "the Android NDK was not found in `{}`. Install it with the SDK Manager \
                     (Android Studio → SDK Manager → SDK Tools → NDK (Side by side)) or \
                     `sdkmanager \"ndk;28.2.13676358\"`, or set ANDROID_NDK_HOME",
                    root.join("ndk").display()
                ))
            })?;
        let ndk_prebuilt = versions_desc(&ndk.join("toolchains").join("llvm").join("prebuilt"))
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error(format!(
                    "`{}` is not an NDK (no toolchains/llvm/prebuilt); NDK r23 or newer is required",
                    ndk.display()
                ))
            })?;

        let build_tools = versions_desc(&root.join("build-tools"))
            .into_iter()
            .find(|dir| {
                dir.join(exe("aapt2")).is_file() && dir.join(exe("zipalign")).is_file() && dir.join(script("apksigner")).is_file()
            })
            .ok_or_else(|| {
                Error(format!(
                    "no Android build-tools with aapt2, zipalign and apksigner in `{}`. Install them with the \
                     SDK Manager or `sdkmanager \"build-tools;36.0.0\"`",
                    root.join("build-tools").display()
                ))
            })?;

        let (android_jar, platform_api) = versions_desc(&root.join("platforms"))
            .into_iter()
            .filter_map(|dir| {
                let name = dir.file_name()?.to_string_lossy().into_owned();
                let api = *version_key(name.strip_prefix("android-")?).first()? as u32;
                let jar = dir.join("android.jar");
                jar.is_file().then_some((jar, api))
            })
            .max_by_key(|(_, api)| *api)
            .ok_or_else(|| {
                Error(format!(
                    "no Android platform (android.jar) in `{}`. Install one with the SDK Manager or \
                     `sdkmanager \"platforms;android-35\"`",
                    root.join("platforms").display()
                ))
            })?;
        if platform_api < MIN_SDK {
            return Err(Error(format!(
                "the newest installed Android platform is API {platform_api}; API {MIN_SDK} or newer is needed"
            )));
        }
        Ok(Sdk {
            root,
            ndk,
            ndk_prebuilt,
            build_tools,
            android_jar,
            platform_api,
        })
    }

    pub fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(exe(name))
    }

    pub fn apksigner(&self) -> PathBuf {
        self.build_tools.join(script("apksigner"))
    }

    fn llvm_strip(&self) -> PathBuf {
        self.ndk_prebuilt.join("bin").join(exe("llvm-strip"))
    }

    fn cxx_shared(&self, ndk_triple: &str) -> PathBuf {
        self.ndk_prebuilt
            .join("sysroot")
            .join("usr")
            .join("lib")
            .join(ndk_triple)
            .join(CXX_SHARED)
    }
}

/// Java (для apksigner и keytool): `JAVA_HOME/bin` или PATH.
fn java_tool(name: &str) -> PathBuf {
    if let Some(home) = std::env::var_os("JAVA_HOME").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(home).join("bin").join(exe(name));
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from(exe(name))
}

fn check_java() -> Result<()> {
    let ok = Command::new(java_tool("java"))
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success());
    if ok {
        Ok(())
    } else {
        Err(Error(
            "Java was not found: apksigner and keytool need a JDK 11 or newer. Install one \
             (for example Eclipse Temurin or the JDK bundled with Android Studio) and set JAVA_HOME"
                .to_string(),
        ))
    }
}

fn check_cargo_ndk() -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let ok = Command::new(cargo)
        .args(["ndk", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if ok {
        Ok(())
    } else {
        Err(Error(
            "cargo-ndk is not installed (it compiles Rust and the C++ bridge with the NDK's clang). \
             Install it once: `cargo install cargo-ndk`"
                .to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Манифест
// ---------------------------------------------------------------------------

/// Что пишется в `AndroidManifest.xml`.
#[derive(Clone, Debug, PartialEq)]
pub struct AppConfig {
    /// Идентификатор приложения (`org.piska.template`).
    pub app_id: String,
    pub label: String,
    /// Имя библиотеки без `lib` и `.so` (`android.app.lib_name`).
    pub lib_name: String,
    pub version_code: u32,
    pub version_name: String,
    /// Значение `android:screenOrientation`.
    pub orientation: &'static str,
    pub min_sdk: u32,
    pub target_sdk: u32,
}

/// `[package.metadata.piska.android] orientation` → `android:screenOrientation`.
pub fn orientation(value: &str) -> Option<&'static str> {
    Some(match value {
        "landscape" => "sensorLandscape",
        "portrait" => "sensorPortrait",
        "any" | "auto" => "fullUser",
        _ => return None,
    })
}

/// Проверить идентификатор приложения: хотя бы две части через точку, каждая —
/// буква, затем буквы, цифры или `_`.
pub fn validate_app_id(id: &str) -> Result<()> {
    let parts: Vec<&str> = id.split('.').collect();
    let valid_part = |p: &&str| {
        let mut chars = p.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if parts.len() >= 2 && parts.iter().all(valid_part) {
        Ok(())
    } else {
        Err(Error(format!(
            "`{id}` is not a valid Android application id: use at least two dot-separated parts, \
             each starting with a letter and containing only letters, digits and `_` \
             (for example `org.example.my_game`)"
        )))
    }
}

impl AppConfig {
    pub fn from_project(project: &Project, target_sdk: u32) -> Result<AppConfig> {
        let default_id = {
            let clean: String = project
                .game
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
                .collect();
            let clean = if clean.starts_with(|c: char| c.is_ascii_alphabetic()) { clean } else { format!("g{clean}") };
            format!("org.piska.{clean}")
        };
        let app_id = project.setting_str("android.package")?.map(str::to_string).unwrap_or(default_id);
        validate_app_id(&app_id)?;
        let int = |key: &str, default: u32| -> Result<u32> {
            match project.setting(key) {
                None => Ok(default),
                Some(toml::Value::Integer(n)) if *n > 0 && *n <= i64::from(u32::MAX >> 1) => Ok(*n as u32),
                Some(other) => Err(Error(format!(
                    "`{key}` in [package.metadata.piska] must be a positive integer, not `{other}`"
                ))),
            }
        };
        let min_sdk = int("android.min_sdk", MIN_SDK)?;
        if min_sdk < MIN_SDK {
            return Err(Error(format!(
                "`android.min_sdk = {min_sdk}` is too low: the engine plays sound through AAudio, \
                 which Android has since API {MIN_SDK} (8.0)"
            )));
        }
        let orientation_name = project.setting_str("android.orientation")?.unwrap_or("landscape");
        let orientation = orientation(orientation_name).ok_or_else(|| {
            Error(format!(
                "`android.orientation = \"{orientation_name}\"` is not supported: use \"landscape\", \"portrait\" or \"any\""
            ))
        })?;
        Ok(AppConfig {
            app_id,
            label: project.title.clone(),
            lib_name: project.lib_name.clone(),
            version_code: int("android.version_code", 1)?,
            version_name: project
                .setting_str("android.version_name")?
                .map(str::to_string)
                .unwrap_or_else(|| project.version.clone()),
            orientation,
            min_sdk,
            target_sdk: target_sdk.max(min_sdk),
        })
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `AndroidManifest.xml`: одна NativeActivity без Java-кода, на весь экран, не
/// перезапускается при повороте и смене клавиатуры (игра получает `Resized`).
pub fn manifest_xml(config: &AppConfig) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{app_id}"
    android:versionCode="{version_code}"
    android:versionName="{version_name}">
    <uses-sdk android:minSdkVersion="{min_sdk}" android:targetSdkVersion="{target_sdk}" />
    <!-- Рендер: Vulkan, а без него OpenGL ES 3.0 (wgpu выбирает сам). -->
    <uses-feature android:glEsVersion="0x00030000" android:required="true" />
    <uses-feature android:name="android.hardware.touchscreen" android:required="false" />
    <uses-feature android:name="android.hardware.gamepad" android:required="false" />
    <application
        android:label="{label}"
        android:icon="@mipmap/ic_launcher"
        android:hasCode="false"
        android:extractNativeLibs="false"
        android:appCategory="game"
        android:isGame="true"
        android:allowBackup="true"
        android:theme="@android:style/Theme.Black.NoTitleBar.Fullscreen">
        <activity
            android:name="android.app.NativeActivity"
            android:label="{label}"
            android:exported="true"
            android:launchMode="singleTask"
            android:screenOrientation="{orientation}"
            android:configChanges="orientation|screenSize|smallestScreenSize|screenLayout|keyboard|keyboardHidden|navigation|uiMode|density|fontScale|layoutDirection|locale|mcc|mnc|touchscreen"
            android:resizeableActivity="true"
            android:windowSoftInputMode="adjustNothing">
            <meta-data android:name="android.app.lib_name" android:value="{lib_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        app_id = config.app_id,
        version_code = config.version_code,
        version_name = xml_escape(&config.version_name),
        min_sdk = config.min_sdk,
        target_sdk = config.target_sdk,
        label = xml_escape(&config.label),
        orientation = config.orientation,
        lib_name = config.lib_name,
    )
}

// ---------------------------------------------------------------------------
// Проверка библиотеки и сборка архива
// ---------------------------------------------------------------------------

/// Проверить `NEEDED` библиотеки игры. `Ok(true)` — нужна `libc++_shared.so`
/// (её надо положить в APK); неизвестная библиотека — ошибка.
pub fn check_needed(needed: &[String]) -> Result<bool> {
    let mut cxx = false;
    let mut missing = Vec::new();
    for lib in needed {
        if lib == CXX_SHARED {
            cxx = true;
        } else if !SYSTEM_LIBS.contains(&lib.as_str()) {
            missing.push(lib.as_str());
        }
    }
    if missing.is_empty() {
        Ok(cxx)
    } else {
        Err(Error(format!(
            "the game library needs {} which Android does not provide and the APK would not contain; \
             a dependency links a native library that is not available on Android",
            missing.join(", ")
        )))
    }
}

/// Собрать APK (ещё не выровненный и не подписанный): записи из результата
/// `aapt2 link` + библиотеки `lib/<abi>/…` + `assets/assets.pak`.
pub fn assemble(base_apk: &[u8], libs: &[(String, Vec<u8>)], pack: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new();
    let base = archive::read_zip(base_apk)?;
    // Манифест — первым (так делают все сборщики APK).
    let (manifest, rest): (Vec<_>, Vec<_>) = base.into_iter().partition(|e| e.name == "AndroidManifest.xml");
    if manifest.is_empty() {
        return Err(Error("aapt2 produced an APK without AndroidManifest.xml".to_string()));
    }
    for entry in manifest.iter().chain(&rest) {
        zip.add_raw(entry)?;
    }
    // Без сжатия: с `extractNativeLibs="false"` Android загружает .so прямо из APK.
    for (path, bytes) in libs {
        zip.add(path, bytes, Method::Stored)?;
    }
    if let Some(pack) = pack {
        zip.add(&format!("assets/{}", PACK_FILE), pack, Method::Stored)?;
    }
    zip.finish()
}

/// Имя пака ассетов в `assets/` APK (его ищет `engine::android`).
pub const PACK_FILE: &str = "assets.pak";

// ---------------------------------------------------------------------------
// Подпись
// ---------------------------------------------------------------------------

/// Аргументы `apksigner sign` и переменные окружения с паролем.
type SignArgs = (Vec<OsString>, Vec<(String, String)>);

/// Чем подписать APK.
#[derive(Clone, Debug)]
pub enum Signing {
    /// Отладочный ключ Android (`~/.android/debug.keystore`, создаётся при первой сборке).
    Debug(PathBuf),
    /// Свой ключ: хранилище, псевдоним, откуда взять пароль.
    Release {
        keystore: PathBuf,
        alias: String,
        password_file: Option<PathBuf>,
    },
}

/// Каталог `.android` пользователя (`ANDROID_USER_HOME` или `~/.android`).
pub fn android_user_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ANDROID_USER_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".android"))
}

impl Signing {
    pub fn from_options(options: &Options) -> Result<Signing> {
        match (&options.keystore, &options.key_alias) {
            (Some(keystore), Some(alias)) => {
                if !keystore.is_file() {
                    return Err(Error(format!("the keystore `{}` does not exist", keystore.display())));
                }
                if let Some(file) = &options.keystore_pass_file {
                    if !file.is_file() {
                        return Err(Error(format!("the password file `{}` does not exist", file.display())));
                    }
                }
                Ok(Signing::Release {
                    keystore: keystore.clone(),
                    alias: alias.clone(),
                    password_file: options.keystore_pass_file.clone(),
                })
            }
            (Some(_), None) => Err(Error("--keystore needs --key-alias <alias> (the key inside the keystore)".to_string())),
            (None, Some(_)) => Err(Error("--key-alias needs --keystore <file>".to_string())),
            (None, None) => {
                let dir = android_user_home().ok_or_else(|| {
                    Error("cannot find the home folder for the debug keystore; set ANDROID_USER_HOME".to_string())
                })?;
                Ok(Signing::Debug(dir.join("debug.keystore")))
            }
        }
    }

    /// Создать отладочное хранилище ключей, если его ещё нет (`keytool`).
    fn ensure_debug_keystore(path: &Path) -> Result<()> {
        if path.is_file() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        println!("piska-export: creating the Android debug keystore {}", path.display());
        // Пароль — через переменную окружения, не в командной строке.
        let status = Command::new(java_tool("keytool"))
            .args(["-genkeypair", "-noprompt", "-keystore"])
            .arg(path)
            .args(["-storetype", "PKCS12", "-alias", DEBUG_ALIAS, "-keyalg", "RSA", "-keysize", "2048"])
            .args(["-validity", "10000", "-dname", "CN=Android Debug,O=Android,C=US"])
            .args(["-storepass:env", "PISKA_DEBUG_KEYSTORE_PASS", "-keypass:env", "PISKA_DEBUG_KEYSTORE_PASS"])
            .env("PISKA_DEBUG_KEYSTORE_PASS", DEBUG_PASSWORD)
            .status()
            .map_err(|e| Error(format!("cannot run keytool: {e}. Install a JDK and set JAVA_HOME")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error(format!("keytool could not create `{}` ({status})", path.display())))
        }
    }

    /// Аргументы `apksigner sign` для ключа и окружение с паролем.
    fn apksigner_args(&self) -> Result<SignArgs> {
        let mut args: Vec<OsString> = Vec::new();
        let mut env = Vec::new();
        match self {
            Signing::Debug(keystore) => {
                Self::ensure_debug_keystore(keystore)?;
                args.extend(["--ks".into(), keystore.clone().into_os_string()]);
                args.extend(["--ks-key-alias".into(), DEBUG_ALIAS.into()]);
                args.extend(["--ks-pass".into(), "env:PISKA_DEBUG_KEYSTORE_PASS".into()]);
                args.extend(["--key-pass".into(), "env:PISKA_DEBUG_KEYSTORE_PASS".into()]);
                env.push(("PISKA_DEBUG_KEYSTORE_PASS".to_string(), DEBUG_PASSWORD.to_string()));
            }
            Signing::Release {
                keystore,
                alias,
                password_file,
            } => {
                args.extend(["--ks".into(), keystore.clone().into_os_string()]);
                args.extend(["--ks-key-alias".into(), alias.into()]);
                if let Some(file) = password_file {
                    let mut pass = OsString::from("file:");
                    pass.push(file.as_os_str());
                    args.extend(["--ks-pass".into(), pass]);
                } else if std::env::var_os(KEYSTORE_PASSWORD_ENV).is_some() {
                    args.extend(["--ks-pass".into(), format!("env:{KEYSTORE_PASSWORD_ENV}").into()]);
                }
                // Без файла и переменной apksigner спросит пароль в терминале.
                if std::env::var_os(KEY_PASSWORD_ENV).is_some() {
                    args.extend(["--key-pass".into(), format!("env:{KEY_PASSWORD_ENV}").into()]);
                }
            }
        }
        Ok((args, env))
    }
}

// ---------------------------------------------------------------------------
// Экспорт
// ---------------------------------------------------------------------------

fn run_tool(what: &str, command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .map_err(|e| Error(format!("cannot run {what}: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(Error(format!(
            "{what} failed ({}):\n{}{}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub fn export(project: &Project, options: &Options) -> Result<Report> {
    const STEPS: u32 = 7;
    let out = project.out_dir(options.out.as_deref(), Platform::Android);
    let abis: Vec<String> = options
        .abi
        .as_deref()
        .unwrap_or(DEFAULT_ABI)
        .split(',')
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();

    step(1, STEPS, "checking the Android toolchain");
    let mut targets = Vec::new();
    for abi in &abis {
        let (rust_target, ndk_triple) = abi_target(abi).ok_or_else(|| {
            Error(format!("unknown Android ABI `{abi}`: use arm64-v8a, armeabi-v7a, x86_64 or x86"))
        })?;
        if !cargo::has_target(rust_target) {
            return Err(Error(format!(
                "the Rust target `{rust_target}` (ABI {abi}) is not installed. Run `rustup target add {rust_target}`"
            )));
        }
        targets.push((abi.clone(), rust_target, ndk_triple));
    }
    let sdk = Sdk::detect()?;
    check_cargo_ndk()?;
    check_java()?;
    let signing = Signing::from_options(options)?;
    let app = AppConfig::from_project(project, sdk.platform_api)?;
    println!(
        "piska-export: SDK {}, NDK {}, build-tools {}, platform API {}",
        sdk.root.display(),
        sdk.ndk.file_name().unwrap_or_default().to_string_lossy(),
        sdk.build_tools.file_name().unwrap_or_default().to_string_lossy(),
        sdk.platform_api
    );

    step(2, STEPS, "packing assets");
    let pack = project.pack_assets()?;
    let target_dir = cargo::target_dir(&project.root)?;
    let work = target_dir.join("piska-android").join(&project.game);
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;

    let profile = if options.release { "release" } else { "dev" };
    let profile_dir = if profile == "dev" { "debug" } else { profile };
    let mut libs: Vec<(String, Vec<u8>)> = Vec::new();
    for (abi, rust_target, ndk_triple) in &targets {
        step(3, STEPS, &format!("compiling `{}` for Android {abi} ({rust_target})", project.package));
        let min_sdk = app.min_sdk.to_string();
        let ndk_home = sdk.ndk.display().to_string();
        let sdk_home = sdk.root.display().to_string();
        cargo::run(
            &project.root,
            &[
                "ndk", "-t", abi, "-P", &min_sdk, "rustc", "-p", &project.package, "--lib", "--profile", profile,
                "--crate-type", "cdylib",
            ],
            &[("ANDROID_NDK_HOME", &ndk_home), ("ANDROID_HOME", &sdk_home)],
        )?;
        let built = target_dir.join(rust_target).join(profile_dir).join(format!("lib{}.so", project.lib_name));
        if !built.is_file() {
            return Err(Error(format!("cargo finished but `{}` is missing", built.display())));
        }

        step(4, STEPS, &format!("checking lib{}.so ({abi})", project.lib_name));
        // Отладочная информация в APK не нужна (бэктрейсы в logcat остаются по символам).
        let stripped = work.join(format!("lib{}-{abi}.so", project.lib_name));
        run_tool(
            "llvm-strip",
            Command::new(sdk.llvm_strip()).arg("--strip-debug").arg("-o").arg(&stripped).arg(&built),
        )?;
        let info = elf::parse_file(&stripped)?;
        if !info.exports("ANativeActivity_onCreate") {
            return Err(Error(format!(
                "lib{}.so does not export ANativeActivity_onCreate: the game crate needs \
                 `engine::game_main!(build, …)` in src/lib.rs",
                project.lib_name
            )));
        }
        let needs_cxx = check_needed(&info.needed)?;
        println!("piska-export:   NEEDED: {}", info.needed.join(", "));
        libs.push((format!("lib/{abi}/lib{}.so", project.lib_name), std::fs::read(&stripped)?));
        if needs_cxx {
            let cxx = sdk.cxx_shared(ndk_triple);
            let bytes = std::fs::read(&cxx).map_err(|e| Error(format!("cannot read `{}` from the NDK: {e}", cxx.display())))?;
            libs.push((format!("lib/{abi}/{CXX_SHARED}"), bytes));
        }
    }

    step(5, STEPS, "linking the manifest and resources (aapt2)");
    let manifest = work.join("AndroidManifest.xml");
    std::fs::write(&manifest, manifest_xml(&app))?;
    let res = work.join("res").join("mipmap-xxxhdpi");
    std::fs::create_dir_all(&res)?;
    std::fs::write(res.join("ic_launcher.png"), icon::app_icon(project, 192)?)?;
    let compiled = work.join("res.zip");
    run_tool(
        "aapt2 compile",
        Command::new(sdk.tool("aapt2")).arg("compile").arg("--dir").arg(work.join("res")).arg("-o").arg(&compiled),
    )?;
    let base = work.join("base.apk");
    run_tool(
        "aapt2 link",
        Command::new(sdk.tool("aapt2"))
            .arg("link")
            .arg("-o")
            .arg(&base)
            .arg("-I")
            .arg(&sdk.android_jar)
            .arg("--manifest")
            .arg(&manifest)
            .arg(&compiled),
    )?;

    step(6, STEPS, "assembling and aligning the APK");
    let unaligned = work.join("unaligned.apk");
    let apk_bytes = assemble(&std::fs::read(&base)?, &libs, (!pack.is_empty()).then_some(&pack[..]))?;
    std::fs::write(&unaligned, apk_bytes)?;
    let aligned = work.join("aligned.apk");
    // -P 16: .so выровнены по страницам 16 КиБ (новые устройства Android 15+).
    run_tool(
        "zipalign",
        Command::new(sdk.tool("zipalign")).args(["-f", "-P", "16", "4"]).arg(&unaligned).arg(&aligned),
    )?;

    step(7, STEPS, "signing the APK (apksigner)");
    fresh_dir(&out)?;
    let apk_name = format!("{}.apk", project.game);
    let apk = out.join(&apk_name);
    let (sign_args, sign_env) = signing.apksigner_args()?;
    let mut sign = Command::new(sdk.apksigner());
    sign.arg("sign")
        .args(&sign_args)
        .args(["--v4-signing-enabled", "false", "--out"])
        .arg(&apk)
        .arg(&aligned);
    for (key, value) in &sign_env {
        sign.env(key, value);
    }
    // Свой ключ без файла и переменной: apksigner спрашивает пароль в терминале.
    let status = sign
        .status()
        .map_err(|e| Error(format!("cannot run apksigner: {e}")))?;
    if !status.success() {
        return Err(Error(format!(
            "apksigner could not sign the APK ({status}). For your own key give the password in a file \
             (--keystore-pass-file) or in {KEYSTORE_PASSWORD_ENV} / {KEY_PASSWORD_ENV}"
        )));
    }
    let verify = run_tool(
        "apksigner verify",
        Command::new(sdk.apksigner()).args(["verify", "--verbose"]).arg(&apk),
    )?;
    let schemes: Vec<&str> = verify
        .lines()
        .filter(|l| l.starts_with("Verified using") && l.ends_with("true"))
        .collect();

    let size = std::fs::metadata(&apk)?.len();
    let adb = sdk.root.join("platform-tools").join(exe("adb"));
    let mut notes = vec![
        format!(
            "application id {}, version {} ({}), min SDK {}, target SDK {}, ABI {}",
            app.app_id,
            app.version_name,
            app.version_code,
            app.min_sdk,
            app.target_sdk,
            abis.join(", ")
        ),
        format!(
            "signed with {}: {}",
            match &signing {
                Signing::Debug(path) => format!("the debug key {}", path.display()),
                Signing::Release { keystore, alias, .. } => format!("`{alias}` from {}", keystore.display()),
            },
            if schemes.is_empty() { "verified".to_string() } else { schemes.join("; ") }
        ),
        format!("install: {} install -r {}", adb.display(), apk.display()),
        format!("logs: {} logcat -s piska RustStdoutStderr", adb.display()),
    ];
    if matches!(signing, Signing::Debug(_)) {
        notes.push("debug-signed: fine for testing; Google Play needs your own key (--keystore, see docs/api/export.md)".to_string());
    }
    Ok(Report {
        out_dir: out,
        files: vec![(apk_name, size)],
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piska-export-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    fn config() -> AppConfig {
        AppConfig {
            app_id: "org.piska.template".into(),
            label: "Star & <Fall>".into(),
            lib_name: "template".into(),
            version_code: 3,
            version_name: "0.0.1".into(),
            orientation: "sensorLandscape",
            min_sdk: 26,
            target_sdk: 37,
        }
    }

    #[test]
    fn manifest_has_native_activity_and_escaped_label() {
        let xml = manifest_xml(&config());
        assert!(xml.contains(r#"package="org.piska.template""#));
        assert!(xml.contains(r#"android:versionCode="3""#));
        assert!(xml.contains(r#"android:minSdkVersion="26" android:targetSdkVersion="37""#));
        assert!(xml.contains(r#"android:name="android.app.NativeActivity""#));
        assert!(xml.contains(r#"<meta-data android:name="android.app.lib_name" android:value="template" />"#));
        assert!(xml.contains(r#"android:hasCode="false""#));
        assert!(xml.contains(r#"android:screenOrientation="sensorLandscape""#));
        assert!(xml.contains("Fullscreen"));
        assert!(xml.contains("orientation|screenSize"), "rotation must not restart the activity");
        assert!(xml.contains(r#"android:label="Star &amp; &lt;Fall&gt;""#));
        assert!(xml.contains("android.intent.category.LAUNCHER"));
    }

    #[test]
    fn app_ids_and_orientations_are_validated() {
        assert!(validate_app_id("org.piska.template").is_ok());
        assert!(validate_app_id("com.example.my_game2").is_ok());
        for bad in ["template", "org..x", "org.1game", "org.my-game", ""] {
            let err = validate_app_id(bad).unwrap_err().0;
            assert!(err.contains("application id"), "{bad}: {err}");
        }
        assert_eq!(orientation("landscape"), Some("sensorLandscape"));
        assert_eq!(orientation("portrait"), Some("sensorPortrait"));
        assert_eq!(orientation("any"), Some("fullUser"));
        assert_eq!(orientation("sideways"), None);
    }

    #[test]
    fn needed_libraries_are_checked() {
        let libs = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!check_needed(&libs(&["libc.so", "libaaudio.so", "liblog.so"])).unwrap());
        assert!(check_needed(&libs(&["libc.so", "libc++_shared.so"])).unwrap());
        let err = check_needed(&libs(&["libc.so", "libasound.so.2"])).unwrap_err().0;
        assert!(err.contains("libasound.so.2"), "{err}");
    }

    #[test]
    fn apk_layout_puts_manifest_first_and_stores_libs_and_pack() {
        let mut base = ZipWriter::new();
        base.add("res/mipmap-xxxhdpi-v4/ic_launcher.png", b"png", Method::Stored).unwrap();
        base.add("AndroidManifest.xml", b"<binary xml>", Method::Deflated).unwrap();
        base.add("resources.arsc", b"arsc", Method::Stored).unwrap();
        let base = base.finish().unwrap();
        let libs = vec![
            ("lib/arm64-v8a/libtemplate.so".to_string(), b"\x7fELF so".to_vec()),
            ("lib/arm64-v8a/libc++_shared.so".to_string(), b"\x7fELF cxx".to_vec()),
        ];
        let apk = assemble(&base, &libs, Some(b"PISKAPK1...")).unwrap();
        let entries = archive::read_zip(&apk).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "AndroidManifest.xml",
                "res/mipmap-xxxhdpi-v4/ic_launcher.png",
                "resources.arsc",
                "lib/arm64-v8a/libtemplate.so",
                "lib/arm64-v8a/libc++_shared.so",
                "assets/assets.pak"
            ]
        );
        for entry in &entries[3..] {
            assert_eq!(entry.method, 0, "{} must be stored uncompressed", entry.name);
        }
        assert_eq!(archive::entry_bytes(&entries[0]).unwrap(), b"<binary xml>");
        assert_eq!(archive::entry_bytes(&entries[5]).unwrap(), b"PISKAPK1...");
        // Без пака — без assets/.
        let apk = assemble(&base, &libs[..1], None).unwrap();
        assert!(archive::read_zip(&apk).unwrap().iter().all(|e| !e.name.starts_with("assets/")));
    }

    #[test]
    fn missing_sdk_explains_android_home() {
        let empty = temp_dir("nosdk");
        let env = |name: &str| (name == "HOME").then(|| empty.clone().into_os_string());
        let err = Sdk::detect_with(&env).unwrap_err().0;
        assert!(err.contains("Android SDK was not found") && err.contains("ANDROID_HOME"), "{err}");
        let nothing = |_: &str| None;
        let err = Sdk::detect_with(&nothing).unwrap_err().0;
        assert!(err.contains("neither ANDROID_HOME"), "{err}");
    }

    #[test]
    fn sdk_detection_reports_each_missing_piece_and_picks_newest_versions() {
        let sdk = temp_dir("sdk");
        std::fs::create_dir_all(sdk.join("platforms")).unwrap();
        let sdk_var = sdk.clone();
        let env = move |name: &str| (name == "ANDROID_HOME").then(|| sdk_var.clone().into_os_string());

        let err = Sdk::detect_with(&env).unwrap_err().0;
        assert!(err.contains("NDK was not found"), "{err}");

        std::fs::create_dir_all(sdk.join("ndk/27.0.1/toolchains/llvm/prebuilt/windows-x86_64")).unwrap();
        std::fs::create_dir_all(sdk.join("ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64")).unwrap();
        let err = Sdk::detect_with(&env).unwrap_err().0;
        assert!(err.contains("build-tools"), "{err}");

        for version in ["34.0.0", "36.0.0"] {
            for tool in [exe("aapt2"), exe("zipalign"), script("apksigner")] {
                touch(&sdk.join("build-tools").join(version).join(tool));
            }
        }
        // Неполный каталог новее — пропускается.
        touch(&sdk.join("build-tools/37.0.0-rc1").join(exe("aapt2")));
        let err = Sdk::detect_with(&env).unwrap_err().0;
        assert!(err.contains("android.jar"), "{err}");

        touch(&sdk.join("platforms/android-34/android.jar"));
        touch(&sdk.join("platforms/android-37.0/android.jar"));
        let found = Sdk::detect_with(&env).unwrap();
        assert_eq!(found.platform_api, 37);
        assert!(found.ndk.ends_with("28.2.13676358"), "{}", found.ndk.display());
        assert!(found.build_tools.ends_with("36.0.0"), "{}", found.build_tools.display());
        assert!(found.android_jar.ends_with("android-37.0/android.jar") || found.android_jar.ends_with("android-37.0\\android.jar"));

        // ANDROID_NDK_HOME важнее каталога ndk/.
        let custom = sdk.join("ndk/27.0.1");
        let sdk_var = sdk.clone();
        let env = move |name: &str| match name {
            "ANDROID_HOME" => Some(sdk_var.clone().into_os_string()),
            "ANDROID_NDK_HOME" => Some(custom.clone().into_os_string()),
            _ => None,
        };
        assert!(Sdk::detect_with(&env).unwrap().ndk.ends_with("27.0.1"));
        let _ = std::fs::remove_dir_all(&sdk);
    }

    /// Полная сборка APK шаблона (нужны SDK, NDK, Java, cargo-ndk; ~30 минут с нуля):
    /// `cargo test -p piska-export -- --ignored builds_the_template_apk`.
    #[test]
    #[ignore = "needs the Android SDK/NDK and builds the whole engine for Android"]
    fn builds_the_template_apk() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project = Project::open(Some(root), "template").unwrap();
        let out = std::env::temp_dir().join(format!("piska-export-apk-{}", std::process::id()));
        let options = Options {
            game: "template".into(),
            release: false,
            out: Some(out.clone()),
            root: Some(root.to_path_buf()),
            abi: None,
            keystore: None,
            key_alias: None,
            keystore_pass_file: None,
            tar: false,
            no_webgl: false,
            glibc: None,
            zig: None,
        };
        let report = export(&project, &options).unwrap();
        let apk = std::fs::read(report.out_dir.join("template.apk")).unwrap();
        let entries = archive::read_zip(&apk).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        for expected in [
            "AndroidManifest.xml",
            "resources.arsc",
            "lib/arm64-v8a/libtemplate.so",
            "lib/arm64-v8a/libc++_shared.so",
            "assets/assets.pak",
        ] {
            assert!(names.contains(&expected), "{expected} missing from {names:?}");
        }
        assert!(names.iter().any(|n| n.starts_with("META-INF/")), "signed: {names:?}");
        let lib = entries.iter().find(|e| e.name == "lib/arm64-v8a/libtemplate.so").unwrap();
        let info = elf::parse(&lib.data).unwrap();
        assert_eq!(info.machine, "aarch64");
        assert!(info.exports("ANativeActivity_onCreate"));
        check_needed(&info.needed).unwrap();
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn abis_map_to_rust_targets() {
        assert_eq!(abi_target("arm64-v8a"), Some(("aarch64-linux-android", "aarch64-linux-android")));
        assert_eq!(abi_target("armeabi-v7a").unwrap().0, "armv7-linux-androideabi");
        assert_eq!(abi_target("mips"), None);
    }
}
