//! Программы с окном собраны без консоли: подсистема PE «GUI» (2) и в отладочной, и в
//! выпускной сборке (`engine_build::windows_app`, отказ — `PISKA_CONSOLE=1`); сама
//! программа тестов остаётся консольной (3).

/// Подсистема PE: u16 по смещению 68 необязательного заголовка.
fn subsystem(path: &str) -> u16 {
    let exe = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let pe = u32::from_le_bytes(exe[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&exe[pe..pe + 4], b"PE\0\0", "{path} is a PE file");
    let at = pe + 24 + 68;
    u16::from_le_bytes([exe[at], exe[at + 1]])
}

fn checked() -> bool {
    cfg!(all(windows, target_env = "msvc")) && !std::env::var("PISKA_CONSOLE").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[test]
fn the_windowed_programs_have_no_console() {
    if !checked() {
        return;
    }
    assert_eq!(subsystem(env!("CARGO_BIN_EXE_piska-hub")), 2, "piska-hub");
    let me = std::env::current_exe().unwrap();
    assert_eq!(subsystem(&me.display().to_string()), 3, "the test program keeps its console");
}
