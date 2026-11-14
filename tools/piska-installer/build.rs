//! Значок движка в exe (проводник, панель задач, Alt-Tab). Подсистема «GUI» задана в
//! `main.rs` атрибутом `windows_subsystem` — у установщика окна консоли нет никогда.

fn main() {
    engine_build::windows_icon();
    // Вшитые скрипты: программа пересобирается, когда они меняются.
    for file in ["install.ps1", "install.sh", "packages.toml"] {
        println!("cargo:rerun-if-changed=../../install/{file}");
    }
}
