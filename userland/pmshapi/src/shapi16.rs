//! The 16-bit entry points (OS/2 1.x API, `_pascal far`): the profile
//! calls (Prf16*, and the older Win16*Profile* calls on HINI_PROFILE) and
//! the switch list, for 16-bit programs. Pointers are 16:16, the
//! WinQueryProfile* sizes 16-bit, and SWCNTRL has 16-bit process and
//! session IDs, byte-sized flags and a 61-byte title.

use thunk16::{flat, Pascal};

use crate::switch;

thunk16::entries16_pascal! {
    dispatch = dispatch16;
    WIN16QUERYPROFILEINT = 2, 14;
    WIN16QUERYPROFILESTRING = 3, 22;
    WIN16WRITEPROFILESTRING = 4, 16;
    WIN16QUERYPROFILESIZE = 5, 16;
    WIN16QUERYPROFILEDATA = 6, 20;
    WIN16WRITEPROFILEDATA = 7, 18;
    PRF16QUERYPROFILEINT = 32, 14;
    PRF16QUERYPROFILESTRING = 33, 24;
    PRF16WRITEPROFILESTRING = 34, 16;
    PRF16QUERYPROFILESIZE = 35, 16;
    PRF16QUERYPROFILEDATA = 36, 20;
    PRF16WRITEPROFILEDATA = 37, 20;
    PRF16OPENPROFILE = 38, 8;
    PRF16CLOSEPROFILE = 39, 4;
    PRF16RESET = 42, 8;
    PRF16QUERYPROFILE = 43, 8;
    WIN16ADDSWITCHENTRY = 48, 4;
    WIN16CHANGESWITCHENTRY = 49, 8;
    WIN16QUERYSWITCHENTRY = 56, 8;
    WIN16QUERYSWITCHHANDLE = 57, 6;
    WIN16REMOVESWITCHENTRY = 68, 4;
    WIN16SWITCHTOPROGRAM = 69, 4;
}

/// HINI_PROFILE: the user profile, then the system one.
const HINI_PROFILE: u32 = 0;

extern "C" fn dispatch16(idx: u32, args: *const u8) -> u32 {
    let size = match idx {
        2 | 32 => 14,
        3 => 22,
        4 | 5 | 34 | 35 => 16,
        6 | 36 | 37 => 20,
        7 => 18,
        33 => 24,
        38 | 42 | 43 | 49 | 56 => 8,
        57 => 6,
        _ => 4,
    };
    let mut a = Pascal::new(args, size);
    match idx {
        // WinQueryProfileInt(hab, app, key, sDefault)
        2 => {
            let (_hab, app, key, def) = (a.l(), a.p(), a.p(), a.i());
            crate::PrfQueryProfileInt(HINI_PROFILE, app, key, def) as u32
        }
        // WinQueryProfileString(hab, app, key, default, buf, cchBuf)
        3 => {
            let (_hab, app, key, def, buf, cch) = (a.l(), a.p(), a.p(), a.p(), a.p(), a.w() as u32);
            crate::PrfQueryProfileString(HINI_PROFILE, app, key, def, buf, cch)
        }
        4 => {
            let (_hab, app, key, s) = (a.l(), a.p(), a.p(), a.p());
            crate::PrfWriteProfileString(HINI_PROFILE, app, key, s)
        }
        // WinQueryProfileSize(hab, app, key, pusSize)
        5 => {
            let (_hab, app, key, ps) = (a.l(), a.p(), a.p(), a.p());
            let mut n = 0u32;
            let r = crate::PrfQueryProfileSize(HINI_PROFILE, app, key, &mut n);
            put16(ps, n);
            r
        }
        // WinQueryProfileData(hab, app, key, buf, pusLength)
        6 => {
            let (_hab, app, key, buf, pl) = (a.l(), a.p(), a.p(), a.p(), a.p());
            let mut n = get16(pl);
            let r = crate::PrfQueryProfileData(HINI_PROFILE, app, key, buf, &mut n);
            put16(pl, n);
            r
        }
        7 => {
            let (_hab, app, key, data, n) = (a.l(), a.p(), a.p(), a.p(), a.w() as u32);
            crate::PrfWriteProfileData(HINI_PROFILE, app, key, data, n)
        }
        32 => {
            let (hini, app, key, def) = (a.l(), a.p(), a.p(), a.i());
            crate::PrfQueryProfileInt(hini, app, key, def) as u32
        }
        33 => {
            let (hini, app, key, def, buf, cch) = (a.l(), a.p(), a.p(), a.p(), a.p(), a.l());
            crate::PrfQueryProfileString(hini, app, key, def, buf, cch)
        }
        34 => {
            let (hini, app, key, s) = (a.l(), a.p(), a.p(), a.p());
            crate::PrfWriteProfileString(hini, app, key, s)
        }
        35 => {
            let (hini, app, key, ps) = (a.l(), a.p(), a.p(), a.p() as *mut u32);
            crate::PrfQueryProfileSize(hini, app, key, ps)
        }
        36 => {
            let (hini, app, key, buf, pl) = (a.l(), a.p(), a.p(), a.p(), a.p() as *mut u32);
            crate::PrfQueryProfileData(hini, app, key, buf, pl)
        }
        37 => {
            let (hini, app, key, data, n) = (a.l(), a.p(), a.p(), a.p(), a.l());
            crate::PrfWriteProfileData(hini, app, key, data, n)
        }
        38 => {
            let (hab, name) = (a.l(), a.p());
            crate::PrfOpenProfile(hab, name)
        }
        39 => crate::PrfCloseProfile(a.l()),
        // PRFPROFILE: cchUserName, pszUserName, cchSysName, pszSysName
        42 | 43 => {
            let (hab, p) = (a.l(), a.p() as *mut u32);
            if p.is_null() {
                return 0;
            }
            let mut v = [0u32; 4];
            for (k, s) in v.iter_mut().enumerate() {
                *s = unsafe { p.add(k).read_unaligned() };
            }
            v[1] = flat(v[1]) as u32;
            v[3] = flat(v[3]) as u32;
            if idx == 42 {
                return crate::PrfReset(hab, v.as_ptr());
            }
            let r = crate::PrfQueryProfile(hab, v.as_mut_ptr());
            unsafe {
                p.write_unaligned(v[0]);
                p.add(2).write_unaligned(v[2]);
            }
            r
        }
        48 => {
            let p = a.p();
            let s = swcntrl_in(p);
            switch::WinAddSwitchEntry(s.as_ptr() as u32)
        }
        49 => {
            let (h, p) = (a.l(), a.p());
            let s = swcntrl_in(p);
            switch::WinChangeSwitchEntry(h, s.as_ptr() as u32)
        }
        56 => {
            let (h, p) = (a.l(), a.p());
            let s = [0u8; 96];
            let r = switch::WinQuerySwitchEntry(h, s.as_ptr() as u32);
            if r == 0 && !p.is_null() {
                swcntrl_out(&s, p);
            }
            r
        }
        57 => {
            let (hwnd, pid) = (a.l(), a.w() as u32);
            switch::WinQuerySwitchHandle(hwnd, pid)
        }
        68 => switch::WinRemoveSwitchEntry(a.l()),
        69 => switch::WinSwitchToProgram(a.l()),
        _ => 0,
    }
}

fn get16(p: *const u8) -> u32 {
    if p.is_null() { 0 } else { unsafe { (p as *const u16).read_unaligned() as u32 } }
}

fn put16(p: *mut u8, v: u32) {
    if !p.is_null() {
        unsafe { (p as *mut u16).write_unaligned(v.min(0xffff) as u16) };
    }
}

/// SWCNTRL, 16-bit (80 bytes: hwnd, hwndIcon, hprog, idProcess and
/// idSession 16-bit, uchVisibility and fbJump bytes, szSwtitle[61]) to
/// 32-bit (96 bytes: 32-bit fields, szSwtitle[64], bProgType).
fn swcntrl_in(p: *const u8) -> [u8; 96] {
    let mut s = [0u8; 96];
    if p.is_null() {
        return s;
    }
    let b = unsafe { core::slice::from_raw_parts(p, 80) };
    s[..12].copy_from_slice(&b[..12]);
    s[12..14].copy_from_slice(&b[12..14]);
    s[16..18].copy_from_slice(&b[14..16]);
    s[20] = b[16];
    s[24] = b[17];
    s[28..28 + 61].copy_from_slice(&b[18..79]);
    s[28 + 60] = 0;
    s
}

fn swcntrl_out(s: &[u8; 96], p: *mut u8) {
    let b = unsafe { core::slice::from_raw_parts_mut(p, 80) };
    b[..12].copy_from_slice(&s[..12]);
    b[12..14].copy_from_slice(&s[12..14]);
    b[14..16].copy_from_slice(&s[16..18]);
    b[16] = s[20];
    b[17] = s[24];
    b[18..79].copy_from_slice(&s[28..89]);
    b[78] = 0;
    b[79] = 0;
}
