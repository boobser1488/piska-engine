//! Значок в настоящем exe: читаем ресурсы собственного исполняемого файла `piska-export`
//! (его `build.rs` вызывает `engine_build::windows_icon()`) тем же способом, каким их
//! читает проводник, — по таблице ресурсов PE.

#[cfg(windows)]
#[test]
fn piska_export_exe_carries_the_engine_icon() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_piska-export")).unwrap();
    let images = engine_build::read_exe_icon(&exe)
        .unwrap()
        .expect("piska-export.exe has no icon resource");
    let sizes: Vec<u32> = images.iter().map(|i| i.width).collect();
    assert_eq!(sizes, engine_build::ICO_SIZES.to_vec());
    // Байты в exe те же, что в `assets/branding/icon.ico`.
    let source = engine_build::parse_ico(engine_build::ENGINE_ICO).unwrap();
    assert_eq!(images, source);
    assert!(images.iter().all(|i| i.is_png()));
}
