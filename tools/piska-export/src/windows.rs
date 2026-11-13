//! Экспорт для Windows: `<game>.exe` (без консоли в release) + каталог `assets/` рядом.
//! Собирается на Windows родным тулчейном; игра находит `assets/` рядом с exe
//! (`engine::assets_dir!()`).

use crate::project::{copy_dir, fresh_dir, Project};
use crate::{cargo, step, Error, Options, Platform, Report, Result};

pub fn export(project: &Project, options: &Options) -> Result<Report> {
    if !cfg!(windows) {
        return Err(Error(
            "the Windows export needs a Windows machine (no cross toolchain is set up)".to_string(),
        ));
    }
    const STEPS: u32 = 3;
    let out = project.out_dir(options.out.as_deref(), Platform::Windows);

    step(1, STEPS, &format!("compiling `{}`", project.package));
    let profile = if options.release { "release" } else { "dev" };
    // Значок из `[package.metadata.piska] icon` передаём `build.rs` игры
    // (`engine_build::windows_icon()` читает `PISKA_ICON`); без настройки он берёт
    // `assets/icon.png` игры или значок движка.
    let icon_env = project
        .setting_str("icon")?
        .map(|path| project.assets_dir().join(path).to_string_lossy().into_owned());
    let env: Vec<(&str, &str)> = icon_env.iter().map(|path| ("PISKA_ICON", path.as_str())).collect();
    cargo::run(
        &project.root,
        &["build", "-p", &project.package, "--bins", "--profile", profile],
        &env,
    )?;
    let profile_dir = if profile == "dev" { "debug" } else { profile };
    let exe = cargo::target_dir(&project.root)?
        .join(profile_dir)
        .join(format!("{}.exe", project.package));
    if !exe.is_file() {
        return Err(Error(format!(
            "`{}` is missing: the game needs `src/main.rs` calling `{}::main()`",
            exe.display(),
            project.lib_name
        )));
    }

    step(2, STEPS, "copying the program");
    fresh_dir(&out)?;
    let exe_name = format!("{}.exe", project.package);
    let exe_size = std::fs::copy(&exe, out.join(&exe_name))?;

    step(3, STEPS, "copying assets");
    let mut files = vec![(exe_name, exe_size)];
    // Проверка .meta и GUID — та же, что у веб-пака.
    project.pack_assets()?;
    if project.assets_dir().is_dir() {
        let size = copy_dir(&project.assets_dir(), &out.join("assets"))?;
        files.push(("assets/".to_string(), size));
        // Окно игры берёт значок из `assets/icon.png`: положим туда значок из настроек.
        if icon_env.is_some() {
            if let Some(png) = project.icon_png()? {
                std::fs::write(out.join("assets").join("icon.png"), png)?;
            }
        }
    }
    let mut notes = vec![format!("run: {}", out.join(&files[0].0).display())];
    notes.push(match exe_icon_sizes(&out.join(&files[0].0)) {
        Ok(Some(sizes)) => format!("exe icon: {} images ({})", sizes.len(), sizes.join(", ")),
        Ok(None) => "the exe has no icon: add `engine_build::windows_icon();` to games/<name>/build.rs                      (and `engine-build` to [build-dependencies]); see docs/api/export.md, section Icon"
            .to_string(),
        Err(e) => format!("cannot check the exe icon: {e}"),
    });
    Ok(Report {
        notes,
        out_dir: out,
        files,
    })
}

/// Размеры образов значка в готовом exe (`None` — значка нет).
fn exe_icon_sizes(exe: &std::path::Path) -> Result<Option<Vec<String>>> {
    let bytes = std::fs::read(exe)?;
    let icon = engine_build::read_exe_icon(&bytes).map_err(|e| Error(e.to_string()))?;
    Ok(icon.map(|images| images.iter().map(|i| format!("{0}x{0}", i.width)).collect()))
}
