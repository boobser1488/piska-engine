//! Потоковость: проект с ассетом в 200 МБ упаковывается, проверяется и распаковывается, а
//! память процесса остаётся в нескольких мегабайтах. Свой аллокатор считает пик занятой
//! памяти; тест один в файле, поэтому чужие потоки счёт не искажают.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine_project::*;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: всё делегируется системному аллокатору, счётчики — атомарные.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let now = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = System.realloc(ptr, layout, new_size);
        if !new.is_null() {
            if new_size >= layout.size() {
                let now = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + (new_size - layout.size());
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn peak_above_start(start: usize) -> usize {
    PEAK.load(Ordering::Relaxed).saturating_sub(start)
}

const SIZE_MB: usize = 200;

fn crc_of(path: &std::path::Path) -> (u64, u32) {
    let mut file = fs::File::open(path).unwrap();
    let mut crc = flate2::Crc::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        crc.update(&buffer[..n]);
        total += n as u64;
    }
    (total, crc.sum())
}

#[test]
fn a_200_mb_asset_is_streamed_with_a_small_memory_footprint() {
    let root = std::env::temp_dir().join(format!("engine-project-stream-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let project = root.join("p");
    fs::create_dir_all(project.join("assets")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"big\"\n").unwrap();
    fs::write(project.join("assets/icon.png"), [1, 2, 3]).unwrap();
    // Почти пустой файл: в каждом мегабайте 64 КБ случайных байт, остальное нули.
    {
        let mut file = fs::File::create(project.join("assets/huge.bin")).unwrap();
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut block = vec![0u8; 1 << 20];
        for _ in 0..SIZE_MB {
            for byte in block[..64 * 1024].iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            file.write_all(&block).unwrap();
        }
    }
    let original = crc_of(&project.join("assets/huge.bin"));
    assert_eq!(original.0, (SIZE_MB as u64) << 20);

    let archive = root.join("big.pks");
    let control = Control::default();
    let start = LIVE.load(Ordering::Relaxed);
    reset_peak();
    let summary = pack(&project, &archive, &PackOptions::default(), &control, &mut |_| {}).unwrap();
    let pack_peak = peak_above_start(start);
    let small = fs::metadata(project.join("Cargo.toml")).unwrap().len() + 3;
    assert_eq!(summary.bytes, original.0 + small);
    assert!(summary.archive_size < summary.bytes / 4, "{} -> {}", summary.bytes, summary.archive_size);

    let start = LIVE.load(Ordering::Relaxed);
    reset_peak();
    verify(&archive, &Limits::default(), &control, &mut |_| {}).unwrap();
    let verify_peak = peak_above_start(start);

    let restored = root.join("restored");
    let start = LIVE.load(Ordering::Relaxed);
    reset_peak();
    unpack(&archive, &restored, &UnpackOptions::default(), &control, &mut |_| {}).unwrap();
    let unpack_peak = peak_above_start(start);
    assert_eq!(crc_of(&restored.join("assets/huge.bin")), original, "the 200 MB file comes back byte for byte");

    let mib = |n: usize| n as f64 / (1024.0 * 1024.0);
    eprintln!(
        "{SIZE_MB} MB asset: peak memory above baseline: pack {:.2} MiB, verify {:.2} MiB, unpack {:.2} MiB; archive {:.1} MiB",
        mib(pack_peak),
        mib(verify_peak),
        mib(unpack_peak),
        summary.archive_size as f64 / (1024.0 * 1024.0)
    );
    for (what, peak) in [("pack", pack_peak), ("verify", verify_peak), ("unpack", unpack_peak)] {
        assert!(peak < 16 * 1024 * 1024, "{what} used {:.1} MiB at its peak; the file must be streamed, not loaded", mib(peak));
    }
    fs::remove_dir_all(root).unwrap();
}
