// The profiling build used for docs/PROFILING.md. NOT part of the app: to use it, copy it over
// src/bin/main.rs in a scratch checkout, `cargo add pprof@0.15 --features flamegraph`, build
// with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release`, and drive it with
// `PROF_CTL=1` / `PROF_CTL=allocs bench/profile.sh`. pprof-rs sampling plus an allocation
// counter around mimalloc, controlled by files in the $PROF_CTL directory.
use inertia_rust_starter_kit::app::App;
use loco_rs::cli;
use migration::Migrator;
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
static ON: AtomicBool = AtomicBool::new(false);

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ON.load(Relaxed) { ALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); }
        unsafe { mimalloc::MiMalloc.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { mimalloc::MiMalloc.dealloc(p, l) } }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if ON.load(Relaxed) { ALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); }
        unsafe { mimalloc::MiMalloc.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if ON.load(Relaxed) { ALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(n as u64, Relaxed); }
        unsafe { mimalloc::MiMalloc.realloc(p, l, n) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn control() {
    let Ok(dir) = std::env::var("PROF_CTL") else { return };
    ON.store(std::env::var("PROF_COUNT").is_ok_and(|v| !v.is_empty()), Relaxed);
    std::thread::spawn(move || {
        let dir = std::path::PathBuf::from(dir);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            // counters: write current totals on request
            if std::fs::remove_file(dir.join("count")).is_ok() {
                let _ = std::fs::write(dir.join("count.out"), format!("{} {}", ALLOCS.load(Relaxed), BYTES.load(Relaxed)));
            }
            let Ok(name) = std::fs::read_to_string(dir.join("start")) else { continue };
            let _ = std::fs::remove_file(dir.join("start"));
            let name = name.trim().to_owned();
            let guard = pprof::ProfilerGuardBuilder::default().frequency(1999)
                .blocklist(&["libc", "libgcc", "pthread", "vdso"]).build().unwrap();
            while std::fs::remove_file(dir.join("stop")).is_err() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let report = guard.report().build().unwrap();
            let mut folded = String::new();
            for (frames, count) in &report.data {
                let mut stack: Vec<String> = Vec::new();
                for f in frames.frames.iter().rev() {
                    for s in f.iter().rev() { stack.push(s.name().replace(";", ":")); }
                }
                folded.push_str(&format!("{};{} {}\n", frames.thread_name, stack.join(";"), count));
            }
            let _ = std::fs::write(dir.join(format!("{name}.folded")), folded);
            let _ = std::fs::write(dir.join("done"), "");
        }
    });
}

#[allow(clippy::result_large_err)]
#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    control();
    cli::main::<App, Migrator>().await
}
