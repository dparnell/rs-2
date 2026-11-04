//! Window properties (WinSetProperty, WinQueryProperty, WinRemoveProperty:
//! ordinals 5452, 5450 and 5451, in later Toolkits' pmwin.h): a pointer
//! kept for a window under a name (or an atom, 0xFFFFnnnn). Firefox keeps
//! its window objects this way.

use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy)]
struct Prop {
    hwnd: u32,
    key: [u8; 64],
    len: u8,
    value: u32,
}

const EMPTY: Prop = Prop { hwnd: 0, key: [0; 64], len: 0, value: 0 };
static mut PROPS: [Prop; 512] = [EMPTY; 512];
static LOCK: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut [Prop; 512]) -> R) -> R {
    while LOCK.swap(true, Ordering::Acquire) {
        core::hint::spin_loop();
    }
    let r = f(unsafe { &mut *core::ptr::addr_of_mut!(PROPS) });
    LOCK.store(false, Ordering::Release);
    r
}

/// The property's key: the name's bytes, or "#" and the atom.
fn key(name: *const u8) -> Option<([u8; 64], u8)> {
    let mut k = [0u8; 64];
    let p = name as u32;
    if p >> 16 == 0xffff {
        k[0] = b'#';
        k[1] = p as u8;
        k[2] = (p >> 8) as u8;
        return Some((k, 3));
    }
    if name.is_null() {
        return None;
    }
    let mut n = 0;
    while n < 63 {
        let c = unsafe { *name.add(n) };
        if c == 0 {
            break;
        }
        k[n] = c;
        n += 1;
    }
    Some((k, n as u8))
}

fn find(t: &[Prop; 512], hwnd: u32, k: &([u8; 64], u8)) -> Option<usize> {
    t.iter().position(|p| p.hwnd == hwnd && p.len == k.1 && p.key == k.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetProperty(hwnd: u32, name: *const u8, value: u32, _fl: u32) -> u32 {
    let Some(k) = key(name) else { return 0 };
    if hwnd == 0 {
        return 0;
    }
    with(|t| {
        let i = match find(t, hwnd, &k) {
            Some(i) => i,
            None => match t.iter().position(|p| p.hwnd == 0) {
                Some(i) => i,
                None => return 0,
            },
        };
        t[i] = Prop { hwnd, key: k.0, len: k.1, value };
        1
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryProperty(hwnd: u32, name: *const u8) -> u32 {
    let Some(k) = key(name) else { return 0 };
    with(|t| find(t, hwnd, &k).map_or(0, |i| t[i].value))
}

#[unsafe(no_mangle)]
pub extern "C" fn WinRemoveProperty(hwnd: u32, name: *const u8) -> u32 {
    let Some(k) = key(name) else { return 0 };
    with(|t| match find(t, hwnd, &k) {
        Some(i) => {
            let v = t[i].value;
            t[i] = EMPTY;
            v
        }
        None => 0,
    })
}
