//! Экспорт в браузер: `wasm32-unknown-unknown` + wasm-bindgen + страница + пак ассетов.
//!
//! Результат (`dist/<game>/web/`):
//! * `index.html` — холст на всё окно, индикатор загрузки, «Play» (браузеры не дают
//!   звуку играть без щелчка), понятное сообщение, если нет ни WebGPU, ни WebGL2;
//! * `game.js` + `game_bg.wasm` — обвязка и код (wasm-bindgen встроен в экспорт как
//!   библиотека той же версии, что в `Cargo.lock`, — отдельный CLI не нужен);
//! * `assets.pak` — все ассеты с `.meta` одним файлом; игра скачивает его при старте.

use std::io::Write;
use std::path::Path;

use crate::project::{fresh_dir, Project};
use crate::{cargo, step, Error, Options, Platform, Report, Result};

const TARGET: &str = "wasm32-unknown-unknown";
/// Версия `wasm-bindgen-cli-support` в Cargo.toml экспорта (`=` — ровно эта).
const BINDGEN_VERSION: &str = "0.2.130";
/// Профиль cargo для `--release`: размер важнее скорости (см. корневой Cargo.toml).
pub const RELEASE_PROFILE: &str = "web-release";

pub fn export(project: &Project, options: &Options) -> Result<Report> {
    const STEPS: u32 = 5;
    let out = project.out_dir(options.out.as_deref(), Platform::Web);

    step(1, STEPS, "checking the toolchain");
    if !cargo::has_target(TARGET) {
        return Err(Error(format!(
            "the Rust target `{TARGET}` is not installed. Run `rustup target add {TARGET}`"
        )));
    }
    match cargo::locked_version(&project.root, "wasm-bindgen") {
        Some(locked) if locked != BINDGEN_VERSION => {
            return Err(Error(format!(
                "Cargo.lock has wasm-bindgen {locked}, but piska-export embeds {BINDGEN_VERSION}; \
                 set `wasm-bindgen-cli-support = \"={locked}\"` in tools/piska-export/Cargo.toml"
            )))
        }
        _ => {}
    }

    step(2, STEPS, "packing assets");
    fresh_dir(&out)?;
    let pack = project.pack_assets()?;
    if !pack.is_empty() {
        std::fs::write(out.join("assets.pak"), &pack)?;
    }

    step(3, STEPS, &format!("compiling `{}` for {TARGET}", project.package));
    let profile = if options.release { RELEASE_PROFILE } else { "dev" };
    let args = [
        "rustc",
        "-p",
        &project.package,
        "--lib",
        "--target",
        TARGET,
        "--profile",
        profile,
        "--crate-type",
        "cdylib",
    ];
    // WebGL2 — запасной бэкенд для браузеров без WebGPU: возможность `webgl` движка
    // включаем здесь, а не в манифесте игры, чтобы `--no-webgl` мог её не включать.
    let mut args = args.to_vec();
    if !options.no_webgl {
        args.extend(["--features", "engine/webgl"]);
    }
    // Отладочная информация в wasm раздувает файл в разы и браузеру не нужна.
    cargo::run(&project.root, &args, &[("CARGO_PROFILE_DEV_DEBUG", "0")])?;
    let profile_dir = if profile == "dev" { "debug" } else { profile };
    let wasm = cargo::target_dir(&project.root)?
        .join(TARGET)
        .join(profile_dir)
        .join(format!("{}.wasm", project.lib_name));
    if !wasm.is_file() {
        return Err(Error(format!("cargo finished but `{}` is missing", wasm.display())));
    }

    step(4, STEPS, "generating JavaScript bindings (wasm-bindgen)");
    let mut bindgen = wasm_bindgen_cli_support::Bindgen::new();
    bindgen
        .input_path(&wasm)
        .out_name("game")
        .web(true)
        .map_err(|e| Error(format!("wasm-bindgen: {e}")))?
        .typescript(false)
        .debug(!options.release)
        .remove_name_section(options.release)
        .remove_producers_section(options.release)
        .generate(&out)
        .map_err(|e| Error(format!("wasm-bindgen failed: {e:#}")))?;
    let js = std::fs::read_to_string(out.join("game.js"))?;
    if !js.contains("piska_start") {
        return Err(Error(format!(
            "`{}` does not export `piska_start`: add `engine::game_main!(build, …);` to games/{}/src/lib.rs",
            project.package, project.game
        )));
    }

    // `wasm-opt` (binaryen) ужимает wasm ещё на 5–15 %. Он не обязателен: нашёлся в PATH
    // (или в `PISKA_WASM_OPT`) — применяем к release-сборке, нет — молча пропускаем.
    let mut opt_note = None;
    if options.release {
        opt_note = run_wasm_opt(&out.join("game_bg.wasm"));
    }

    step(5, STEPS, "writing index.html and the favicon");
    std::fs::write(out.join("index.html"), index_html(&project.title, !options.no_webgl))?;
    // Вкладка браузера показывает значок игры (или движка): PNG 64 px хватает и для Retina.
    std::fs::write(out.join("favicon.png"), crate::icon::app_icon(project, 64)?)?;

    let mut files = Vec::new();
    for name in ["index.html", "favicon.png", "game.js", "game_bg.wasm", "assets.pak"] {
        if let Ok(meta) = std::fs::metadata(out.join(name)) {
            files.push((name.to_string(), meta.len()));
        }
    }
    let wasm_gz = gzip_size(&out.join("game_bg.wasm"))?;
    let mut notes = Vec::new();
    notes.extend(opt_note);
    notes.push(if options.no_webgl {
        "built without the WebGL2 fallback: only browsers with WebGPU run this game".to_string()
    } else {
        "built with the WebGL2 fallback; `--no-webgl` makes the wasm smaller".to_string()
    });
    Ok(Report {
        notes: [notes, vec![
            format!(
                "game_bg.wasm is {} with gzip (what a server with compression sends)",
                crate::human_size(wasm_gz)
            ),
            format!(
                "test locally: cargo run -p piska-export -- serve {} --open",
                out.display()
            ),
        ]].concat(),
        out_dir: out,
        files,
    })
}

/// `wasm-opt -Oz` на готовом `game_bg.wasm`, если инструмент установлен. Возвращает строку
/// для отчёта; неудача не ломает экспорт — остаётся неужатый файл.
fn run_wasm_opt(wasm: &Path) -> Option<String> {
    let tool = std::env::var_os("PISKA_WASM_OPT").map(std::path::PathBuf::from).or_else(|| {
        let name = if cfg!(windows) { "wasm-opt.exe" } else { "wasm-opt" };
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
        })
    })?;
    let before = std::fs::metadata(wasm).ok()?.len();
    let optimized = wasm.with_extension("opt.wasm");
    let status = std::process::Command::new(&tool)
        .args(["-Oz", "--all-features", "-o"])
        .arg(&optimized)
        .arg(wasm)
        .status();
    match status {
        Ok(status) if status.success() => {
            let after = std::fs::metadata(&optimized).ok()?.len();
            if after < before && std::fs::rename(&optimized, wasm).is_ok() {
                Some(format!("wasm-opt: game_bg.wasm {} -> {}", crate::human_size(before), crate::human_size(after)))
            } else {
                let _ = std::fs::remove_file(&optimized);
                Some("wasm-opt did not make the file smaller; kept the original".to_string())
            }
        }
        _ => {
            let _ = std::fs::remove_file(&optimized);
            Some(format!("wasm-opt (`{}`) failed; kept the original game_bg.wasm", tool.display()))
        }
    }
}

fn gzip_size(path: &Path) -> Result<u64> {
    let bytes = std::fs::read(path)?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes)?;
    Ok(encoder.finish()?.len() as u64)
}

/// Страница игры. Состояние рантайм сообщает атрибутами `<body data-piska-*>`.
/// Пак ассетов игра скачивает сама после «Play» (предзагрузка `<link rel=preload>`
/// не годится: браузер ругается на неиспользованный за несколько секунд ресурс).
pub fn index_html(title: &str, webgl: bool) -> String {
    let title = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    // Без запасного WebGL2 страница не должна обещать, что игра пойдёт на нём.
    let (check, message) = if webgl {
        (
            r#"if (navigator.gpu) return true;
  try { return !!document.createElement("canvas").getContext("webgl2"); } catch (_) { return false; }"#,
            "it supports neither WebGPU nor WebGL2.",
        )
    } else {
        ("return !!navigator.gpu;", "it does not support WebGPU (this build has no WebGL2 fallback).")
    };
    INDEX_HTML.replace("{{TITLE}}", &title).replace("{{GPU_CHECK}}", check).replace("{{GPU_MESSAGE}}", message)
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>{{TITLE}}</title>
<link rel="icon" type="image/png" href="favicon.png">
<style>
  html, body { margin: 0; height: 100%; background: #0d1018; overflow: hidden; }
  body { font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; color: #e7ecf5; }
  #piska-canvas { position: fixed; inset: 0; width: 100%; height: 100%; display: block;
                  outline: none; touch-action: none; }
  #overlay { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center;
             background: #0d1018; transition: opacity .25s; }
  #overlay.hidden { opacity: 0; pointer-events: none; }
  .card { text-align: center; max-width: 34em; padding: 24px; }
  h1 { font-size: 28px; margin: 0 0 18px; font-weight: 600; letter-spacing: .01em; }
  .bar { width: 260px; height: 6px; margin: 0 auto 10px; border-radius: 3px; background: #262d3d; overflow: hidden; }
  .bar > div { height: 100%; width: 0; background: #5b8cff; transition: width .15s; }
  #status { color: #9aa6bf; min-height: 1.5em; }
  #play { display: none; margin: 8px auto 0; padding: 12px 40px; font: inherit; font-size: 18px; font-weight: 600;
          color: #fff; background: #5b8cff; border: 0; border-radius: 8px; cursor: pointer; }
  #play:hover { background: #7aa2ff; }
  #play:focus-visible { outline: 2px solid #fff; outline-offset: 3px; }
  .error { color: #ff8a80; white-space: pre-wrap; text-align: left; font-family: ui-monospace, Consolas, monospace;
           font-size: 13px; background: #1c1418; border-radius: 8px; padding: 12px 14px; }
</style>
</head>
<body>
<canvas id="piska-canvas" tabindex="0"></canvas>
<div id="overlay">
  <div class="card">
    <h1>{{TITLE}}</h1>
    <div class="bar" id="bar"><div id="progress"></div></div>
    <div id="status">Loading…</div>
    <button id="play">Play</button>
  </div>
</div>
<script type="module">
import init, { piska_start } from "./game.js";

const overlay = document.getElementById("overlay");
const status = document.getElementById("status");
const bar = document.getElementById("bar");
const progress = document.getElementById("progress");
const play = document.getElementById("play");

function fail(message) {
  bar.style.display = "none";
  play.style.display = "none";
  overlay.classList.remove("hidden");
  status.className = "error";
  status.textContent = message;
}

function hasGpu() {
  {{GPU_CHECK}}
}

// Рантайм сообщает о себе атрибутами <body>: loading → starting → running | error.
new MutationObserver(() => {
  const state = document.body.dataset.piskaState;
  if (state === "error") fail(document.body.dataset.piskaError || "The game stopped with an error.");
  else if (state === "running") overlay.classList.add("hidden");
  else if (state === "loading") status.textContent = "Loading assets…";
  else if (state === "starting") status.textContent = "Starting…";
}).observe(document.body, { attributes: true });

async function fetchWithProgress(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const total = Number(response.headers.get("Content-Length")) || 0;
  if (!response.body || !total) return response;
  const reader = response.body.getReader();
  const chunks = [];
  let loaded = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    loaded += value.length;
    progress.style.width = `${Math.round(loaded / total * 100)}%`;
  }
  return new Response(new Blob(chunks), { headers: { "Content-Type": "application/wasm" } });
}

async function main() {
  if (!hasGpu()) {
    fail("This browser cannot run the game: {{GPU_MESSAGE}}\n" +
         "Use a current Chrome, Edge, Firefox or Safari, and check that hardware acceleration is enabled.");
    return;
  }
  try {
    await init({ module_or_path: fetchWithProgress("game_bg.wasm") });
  } catch (error) {
    fail(`Cannot load the game: ${error}`);
    return;
  }
  progress.style.width = "100%";
  bar.style.display = "none";
  status.textContent = "";
  play.style.display = "block";
  play.focus();
}

// Звук в браузере разрешён только после действия игрока — поэтому старт по «Play».
play.addEventListener("click", () => {
  play.style.display = "none";
  status.textContent = "Starting…";
  document.getElementById("piska-canvas").focus();
  try {
    piska_start();
  } catch (error) {
    fail(`The game stopped with an error: ${error}`);
  }
});

main();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::index_html;

    #[test]
    fn page_links_the_favicon_that_the_export_writes() {
        let html = index_html("My <Game>", true);
        assert!(html.contains(r#"<link rel="icon" type="image/png" href="favicon.png">"#));
        assert!(html.contains("<title>My &lt;Game&gt;</title>"));
    }
}
