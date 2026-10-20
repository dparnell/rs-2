//! PMWP.DLL: the Workplace Shell's entry points that programs use
//! without the Workplace Shell itself: the file icon calls, which PMWIN
//! implements (rs2-private ordinals 9004-9006; these entries jump there),
//! and WinCreateObject for program objects (rs2's shell keeps them in the
//! program list).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;

use alloc::vec::Vec;

unsafe extern "C" {
    #[link_name = "PMSHAPI.109"]
    fn PrfAddProgram(hini: u32, details: *const u32, group: u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
}

const HINI_USERPROFILE: u32 = 0xffff_ffff;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

macro_rules! forward {
    ($($name:ident = $ord:literal;)*) => {$(
        #[allow(non_snake_case)]
        mod $name {
            unsafe extern "C" {
                #[link_name = concat!("PMWIN.", stringify!($ord))]
                pub fn target();
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!("jmp {}", sym $name::target);
        }
    )*};
}

forward! {
    WinLoadFileIcon = 9004;
    WinFreeFileIcon = 9005;
    WinSetFileIcon = 9006;
}

/// A NUL-terminated string's bytes.
fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// The value of KEY in a setup string (`KEY=value;KEY=value`).
fn setup_value(setup: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    setup.split(|&b| b == b';').find_map(|kv| {
        let eq = kv.iter().position(|&b| b == b'=')?;
        kv[..eq].trim_ascii().eq_ignore_ascii_case(key).then(|| kv[eq + 1..].to_vec())
    })
}

/// The handle of the group whose programs the shell shows on the desktop
/// (RS2_Desktop DesktopGroup in OS2.INI, hex).
fn desktop_group() -> Option<u32> {
    let mut buf = [0u8; 16];
    let mut n = buf.len() as u32;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, b"RS2_Desktop\0".as_ptr(), b"DesktopGroup\0".as_ptr(), buf.as_mut_ptr(), &mut n) } == 0 {
        return None;
    }
    let t = &buf[..(n as usize).min(buf.len())];
    let t = &t[..t.iter().position(|&b| b == 0).unwrap_or(t.len())];
    u32::from_str_radix(core::str::from_utf8(t).ok()?, 16).ok()
}

/// WinCreateObject(pszClassName, pszTitle, pszSetupString, pszLocation,
/// ulFlags): a new object. rs2 has no Workplace Shell: WPProgram objects
/// on the desktop (<WP_DESKTOP>) become programs of the shell's desktop
/// group (EXENAME, PARAMETERS, STARTUPDIR, ICONFILE and PROGTYPE of the
/// setup string), the handle is the program handle. Other classes and
/// locations fail (NULLHANDLE).
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateObject(class: *const u8, title: *const u8, setup: *const u8, location: *const u8, _flags: u32) -> u32 {
    let (class, setup, location) = (cstr(class), cstr(setup), cstr(location));
    if !class.eq_ignore_ascii_case(b"WPProgram") || !location.eq_ignore_ascii_case(b"<WP_DESKTOP>") {
        return 0;
    }
    let Some(group) = desktop_group() else { return 0 };
    // '^' breaks a title into lines.
    let mut t: Vec<u8> = cstr(title).into_iter().map(|b| if b == b'^' { b' ' } else { b }).collect();
    t.push(0);
    let z = |k: &[u8]| setup_value(&setup, k).map(|mut v| {
        v.push(0);
        v
    });
    let (exe, params, dir, icon) = (z(b"EXENAME"), z(b"PARAMETERS"), z(b"STARTUPDIR"), z(b"ICONFILE"));
    let progc = match setup_value(&setup, b"PROGTYPE").as_deref().map(|v| v.to_ascii_uppercase()) {
        Some(v) if v == b"FULLSCREEN" => 1,
        Some(v) if v == b"WINDOWABLEVIO" => 2,
        Some(v) if v == b"PM" => 3,
        _ => 0,
    };
    let ptr = |v: &Option<Vec<u8>>| v.as_ref().map_or(0, |v| v.as_ptr() as u32);
    // PROGDETAILS: Length, progt (progc, fbVisible), the strings, an
    // environment and the initial SWP.
    let mut d = [0u32; 18];
    d[0] = 72;
    d[1] = progc;
    d[3] = t.as_ptr() as u32;
    d[4] = ptr(&exe);
    d[5] = ptr(&params);
    d[6] = ptr(&dir);
    d[7] = ptr(&icon);
    unsafe { PrfAddProgram(HINI_USERPROFILE, d.as_ptr(), group) }
}
