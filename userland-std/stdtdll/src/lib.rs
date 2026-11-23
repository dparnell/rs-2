//! A 64-bit DLL in Rust with std, its interface C's: STDTST64's `dll`
//! section loads it. It has a std, a heap and a C library of its own.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use rs2libc as _;

thread_local! {
    static COUNT: Cell<u32> = const { Cell::new(100) };
    static WORDS: RefCell<Vec<String>> = RefCell::new(vec!["first".into()]);
}

static TOTAL: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// Data a program finds by name.
#[unsafe(no_mangle)]
pub static STDTDLL_DATA: [u32; 4] = [1, 2, 3, 0x64];

/// The calling thread's counter (from 100), one higher each time.
#[unsafe(no_mangle)]
pub extern "C" fn dll_count() -> u32 {
    COUNT.set(COUNT.get() + 1);
    WORDS.with_borrow_mut(|w| w.push(format!("word {}", w.len())));
    if WORDS.with_borrow(|w| w.len()) as u32 + 99 != COUNT.get() {
        return 0;
    }
    COUNT.get()
}

/// Calls `f` in `n` threads of the DLL's; the sum of the results.
#[unsafe(no_mangle)]
pub extern "C" fn dll_threads(n: u32, f: extern "C" fn(u32) -> u32) -> u32 {
    struct D;
    impl Drop for D {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
    thread_local!(static GONE: D = const { D });
    let hs: Vec<_> = (0..n)
        .map(|i| {
            std::thread::spawn(move || {
                GONE.with(|_| ());
                let mine = dll_count();
                let r = f(i);
                *TOTAL.lock().unwrap().get_or_insert_default().entry(format!("{}", i % 2)).or_default() += r;
                if mine == 101 { r } else { 1_000_000 }
            })
        })
        .collect();
    hs.into_iter().map(|h| h.join().unwrap()).sum()
}

/// How many of [`dll_threads`]'s thread-locals were dropped.
#[unsafe(no_mangle)]
pub extern "C" fn dll_dropped() -> u32 {
    DROPPED.load(Ordering::SeqCst)
}

/// 1: a panic caught inside, its values dropped.
#[unsafe(no_mangle)]
pub extern "C" fn dll_panic() -> u32 {
    std::panic::set_hook(Box::new(|_| {}));
    let before = DROPPED.load(Ordering::SeqCst);
    let r = std::panic::catch_unwind(|| {
        struct D;
        impl Drop for D {
            fn drop(&mut self) {
                DROPPED.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _d = D;
        let v: Vec<u32> = Vec::new();
        v[std::hint::black_box(2)]
    });
    let ok = r.is_err() && DROPPED.swap(before, Ordering::SeqCst) == before.wrapping_sub(1);
    ok as u32
}

/// 1: the arguments, the environment and a file are the process's.
#[unsafe(no_mangle)]
pub extern "C" fn dll_env() -> u32 {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_uppercase()).unwrap_or_default();
    (std::env::args().next().is_some_and(|a| a.to_uppercase().contains("STDTST64"))
        && exe.ends_with("STDTST64.EXE")
        && std::env::var("PATH").is_ok()
        && std::fs::read(&exe).is_ok_and(|b| b.starts_with(b"\x7fELF"))) as u32
}
