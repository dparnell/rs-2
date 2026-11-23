//! STDTRUST.EXE: a program on STDTRLIB.DLL, a DLL with a Rust interface.
//! The two share one std (it is in the DLL): String, Vec, trait objects,
//! panics and thread-locals go from one to the other.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;

use stdtrlib::*;

thread_local! {
    static MINE: Cell<u64> = const { Cell::new(0x1234_5678_9abc) };
    static LIST: RefCell<Vec<u32>> = RefCell::new(vec![1, 2, 3]);
}

struct Square(f64);
impl Shape for Square {
    fn area(&self) -> f64 {
        self.0 * self.0
    }
}

static DROPS: AtomicU32 = AtomicU32::new(0);
struct D;
impl Drop for D {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

fn main() {
    let mut failed = 0;
    let mut check = |name: &str, ok: bool| {
        println!("{} {name}", if ok { "PASS" } else { "FAIL" });
        failed += !ok as i32;
    };
    check("the program's thread-locals", MINE.get() == 0x1234_5678_9abc && LIST.with_borrow(|l| l == &[1, 2, 3]));
    check("the DLL's thread-locals from both", DEPTH.get() == 7 && {
        DEPTH.set(8);
        depth() == 8
    });
    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Square(3.0)), Box::new(Square(4.0))];
    check("trait objects", total(&shapes) == 25.0);
    note(("a", 1));
    log(String::from("b"));
    check("Strings allocated here and freed there", logged() == ["(\"a\", 1)", "b"]);

    // Unwinding through frames of both modules.
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(|| {
        let _d = D;
        then_panic(|| note(1), "from the DLL".into());
    });
    let msg = r.err().and_then(|e| e.downcast::<String>().ok());
    check("a panic in the DLL caught here", msg.is_some_and(|m| *m == "from the DLL") && DROPS.load(Ordering::SeqCst) == 1);
    let r = std::panic::catch_unwind(|| {
        call(&|| {
            let _d = D;
            let v: Vec<u32> = Vec::new();
            std::hint::black_box(v[std::hint::black_box(1)]);
        })
    });
    check("a panic here through the DLL's frames", r.is_err() && DROPS.load(Ordering::SeqCst) == 2);

    let (tx, rx) = mpsc::channel();
    let hs: Vec<_> = (0..8u32)
        .map(|i| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                MINE.set(MINE.get() + i as u64);
                DEPTH.set(DEPTH.get() + i);
                LIST.with_borrow_mut(|l| l.push(i));
                std::thread::yield_now();
                tx.send(MINE.get() == 0x1234_5678_9abc + i as u64 && depth() == 7 + i && LIST.with_borrow(|l| l.len() == 4)).unwrap();
            })
        })
        .collect();
    drop(tx);
    let ok: Vec<bool> = rx.iter().collect();
    let joined = hs.into_iter().all(|h| h.join().is_ok());
    check("threads with thread-locals of both", ok.len() == 8 && ok.iter().all(|&b| b) && joined);
    let exe = std::env::current_exe();
    check("arguments", std::env::args().nth(1).as_deref() == Some("one two"));
    check("current_exe", exe.is_ok_and(|p| p.to_string_lossy().to_uppercase().ends_with("STDTRUST.EXE")));
    println!("stdtrust: {failed} failure(s)");
    std::process::exit(failed);
}
