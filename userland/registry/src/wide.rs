//! The W (Unicode) registry calls at IBM's REGISTRY ordinals. As in IBM's,
//! they convert names to the process code page (UCONV) and call what the
//! A calls do; string data (REG_SZ, REG_EXPAND_SZ, REG_MULTI_SZ) are
//! converted both ways, so a string set by one kind of call reads the
//! same through the other. Lengths of names count characters, of data
//! bytes, as in Win32 (IBM's RegEnumKeyExW and RegQueryInfoKeyW count
//! name lengths in bytes; IBM's own programs use only the A calls, and
//! Open32's PMWINX forwards no W call). Limits as in IBM's: data of
//! RegSetValueExW at most 32728 bytes.

use alloc::vec::Vec;

use crate::{ERROR_BADKEY, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_SUCCESS, REG_SZ, is_string, key_of, set_out, zero_filetime};

unsafe extern "C" {
    #[link_name = "UCONV.1"]
    fn UniCreateUconvObject(name: *const u16, out: *mut u32) -> i32;
    #[link_name = "UCONV.2"]
    fn UniUconvToUcs(u: u32, inp: *mut *const u8, inb: *mut u32, out: *mut *mut u16, outc: *mut u32, subst: *mut u32) -> i32;
    #[link_name = "UCONV.3"]
    fn UniUconvFromUcs(u: u32, inp: *mut *const u16, inc: *mut u32, out: *mut *mut u8, outb: *mut u32, subst: *mut u32) -> i32;
}

/// The conversion object of the process code page, made on first use.
static mut CO: u32 = 0;

fn co() -> u32 {
    unsafe {
        let co = &mut *core::ptr::addr_of_mut!(CO);
        if *co == 0 && UniCreateUconvObject([0u16].as_ptr(), co) != 0 {
            *co = 0;
        }
        *co
    }
}

/// Unicode to the process code page (one byte a character: rs2's code
/// pages are single-byte; without UCONV, Latin-1).
pub fn to_cp(s: &[u16]) -> Vec<u8> {
    let co = co();
    if co == 0 {
        return s.iter().map(|&c| if c < 256 { c as u8 } else { b'?' }).collect();
    }
    let mut out = alloc::vec![0u8; s.len()];
    let (mut i, mut n, mut o, mut m, mut subst) = (s.as_ptr(), s.len() as u32, out.as_mut_ptr(), out.len() as u32, 0);
    unsafe { UniUconvFromUcs(co, &mut i, &mut n, &mut o, &mut m, &mut subst) };
    let len = out.len() - m as usize;
    out.truncate(len);
    out
}

/// The process code page to Unicode.
pub fn to_ucs(b: &[u8]) -> Vec<u16> {
    let co = co();
    if co == 0 {
        return b.iter().map(|&c| c as u16).collect();
    }
    let mut out = alloc::vec![0u16; b.len()];
    let (mut i, mut n, mut o, mut m, mut subst) = (b.as_ptr(), b.len() as u32, out.as_mut_ptr(), out.len() as u32, 0);
    unsafe { UniUconvToUcs(co, &mut i, &mut n, &mut o, &mut m, &mut subst) };
    let len = out.len() - m as usize;
    out.truncate(len);
    out
}

/// A NUL-terminated Unicode string (NULL: empty).
pub fn wstr(p: *const u16) -> Vec<u16> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { p.add(i).read_unaligned() }).take_while(|&c| c != 0).collect()
}

fn name(p: *const u16) -> Vec<u8> {
    to_cp(&wstr(p))
}

/// A value's data as the W calls see them.
fn data_out(ty: u32, d: Vec<u8>) -> Vec<u8> {
    if !is_string(ty) {
        return d;
    }
    to_ucs(&d).iter().flat_map(|c| c.to_le_bytes()).collect()
}

/// Copy a name (with its NUL) into a buffer of *pcch characters; *pcch
/// becomes the length without the NUL, or the size needed (with it).
fn copy_name(src: &[u8], buf: *mut u16, pcch: *mut u32) -> u32 {
    let s = to_ucs(src);
    let have = unsafe { *pcch } as usize;
    if have < s.len() + 1 {
        unsafe { *pcch = s.len() as u32 + 1 };
        return ERROR_MORE_DATA;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
        *pcch = s.len() as u32;
    }
    ERROR_SUCCESS
}

/// RegOpenKeyExW(hKey, lpSubKey, ulOptions, samDesired, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegOpenKeyExW(h: u32, sub: *const u16, _opt: u32, _sam: u32, out: *mut u32) -> u32 {
    crate::open_key(h, &name(sub), out)
}

/// RegOpenKeyW(hKey, lpSubKey, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegOpenKeyW(h: u32, sub: *const u16, out: *mut u32) -> u32 {
    crate::open_key(h, &name(sub), out)
}

/// RegCreateKeyExW(hKey, lpSubKey, Reserved, lpClass, dwOptions,
/// samDesired, lpSecurityAttributes, phkResult, lpdwDisposition).
#[unsafe(no_mangle)]
pub extern "C" fn RegCreateKeyExW(h: u32, sub: *const u16, _r: u32, _class: *const u16, _opt: u32, _sam: u32, _sec: u32, out: *mut u32, disp: *mut u32) -> u32 {
    crate::create_key(h, &name(sub), out, disp)
}

/// RegCreateKeyW(hKey, lpSubKey, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegCreateKeyW(h: u32, sub: *const u16, out: *mut u32) -> u32 {
    crate::create_key(h, &name(sub), out, core::ptr::null_mut())
}

/// RegDeleteKeyW(hKey, lpSubKey).
#[unsafe(no_mangle)]
pub extern "C" fn RegDeleteKeyW(h: u32, sub: *const u16) -> u32 {
    crate::delete_key(h, &name(sub))
}

/// RegDeleteValueW(hKey, lpValueName).
#[unsafe(no_mangle)]
pub extern "C" fn RegDeleteValueW(h: u32, value: *const u16) -> u32 {
    crate::delete_value(h, &name(value))
}

/// RegEnumKeyExW(hKey, dwIndex, lpName, lpcName, lpReserved, lpClass,
/// lpcClass, lpftLastWriteTime).
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumKeyExW(h: u32, i: u32, buf: *mut u16, pcch: *mut u32, _r: u32, class: *mut u16, pcclass: *mut u32, ft: *mut u32) -> u32 {
    if key_of(h).is_none() {
        return ERROR_BADKEY;
    }
    if buf.is_null() || pcch.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let n = match crate::enum_key(h, i) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let r = copy_name(&n, buf, pcch);
    if r != ERROR_SUCCESS {
        return r;
    }
    if !class.is_null() && !pcclass.is_null() && unsafe { *pcclass } > 0 {
        unsafe {
            *class = 0;
            *pcclass = 0;
        }
    }
    zero_filetime(ft);
    ERROR_SUCCESS
}

/// RegEnumKeyW(hKey, dwIndex, lpName, cchName).
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumKeyW(h: u32, i: u32, buf: *mut u16, cch: u32) -> u32 {
    let mut n = cch;
    RegEnumKeyExW(h, i, buf, &mut n, 0, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
}

/// RegEnumValueW(hKey, dwIndex, lpValueName, lpcValueName, lpReserved,
/// lpType, lpData, lpcbData).
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumValueW(h: u32, i: u32, buf: *mut u16, pcch: *mut u32, _r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32 {
    if key_of(h).is_none() {
        return ERROR_BADKEY;
    }
    if buf.is_null() || pcch.is_null() || (pcb.is_null() && !data.is_null()) {
        return ERROR_INVALID_PARAMETER;
    }
    let (n, t, d) = match crate::enum_value(h, i) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let r = copy_name(&n, buf, pcch);
    if r != ERROR_SUCCESS {
        return r;
    }
    set_out(ty, t);
    if !pcb.is_null() {
        return crate::copy_out(&data_out(t, d), false, data, pcb);
    }
    ERROR_SUCCESS
}

/// RegQueryValueExW(hKey, lpValueName, lpReserved, lpType, lpData,
/// lpcbData).
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryValueExW(h: u32, value: *const u16, _r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32 {
    if pcb.is_null() && !data.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let (t, d) = match crate::query_value(h, &name(value)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    set_out(ty, t);
    crate::copy_out(&data_out(t, d), false, data, pcb)
}

/// RegQueryValueW(hKey, lpSubKey, lpValue, lpcbValue): the unnamed value
/// of a subkey; *lpcbValue counts bytes.
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryValueW(h: u32, sub: *const u16, data: *mut u16, pcb: *mut u32) -> u32 {
    let mut k = 0;
    let r = crate::open_key(h, &name(sub), &mut k);
    if r != ERROR_SUCCESS {
        return r;
    }
    let r = RegQueryValueExW(k, [0u16].as_ptr(), 0, core::ptr::null_mut(), data as *mut u8, pcb);
    crate::RegCloseKey(k);
    r
}

/// RegSetValueExW(hKey, lpValueName, Reserved, dwType, lpData, cbData).
#[unsafe(no_mangle)]
pub extern "C" fn RegSetValueExW(h: u32, value: *const u16, r: u32, ty: u32, data: *const u8, cb: u32) -> u32 {
    if r != 0 || cb > 0x7fd8 || (is_string(ty) && data.is_null()) {
        return ERROR_INVALID_PARAMETER;
    }
    let raw = if data.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(data, cb as usize) } };
    let d = if is_string(ty) {
        let s: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        to_cp(&s)
    } else {
        raw.to_vec()
    };
    crate::set_value(h, &name(value), ty, d)
}

/// RegSetValueW(hKey, lpSubKey, dwType, lpData, cbData): the unnamed
/// value of a subkey (made if need be), a REG_SZ.
#[unsafe(no_mangle)]
pub extern "C" fn RegSetValueW(h: u32, sub: *const u16, ty: u32, data: *const u16, cb: u32) -> u32 {
    if ty != REG_SZ || data.is_null() || cb > 0x7fd8 {
        return ERROR_INVALID_PARAMETER;
    }
    let mut k = 0;
    let r = crate::create_key(h, &name(sub), &mut k, core::ptr::null_mut());
    if r != ERROR_SUCCESS {
        return r;
    }
    let r = crate::set_value(k, b"", REG_SZ, name(data));
    crate::RegCloseKey(k);
    r
}

/// RegQueryInfoKeyW(hKey, lpClass, lpcClass, lpReserved, lpcSubKeys,
/// lpcMaxSubKeyLen, lpcMaxClassLen, lpcValues, lpcMaxValueNameLen,
/// lpcMaxValueLen, lpcbSecurityDescriptor, lpftLastWriteTime): name
/// lengths in characters, data in bytes as RegQueryValueExW gives them.
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryInfoKeyW(
    h: u32,
    class: *mut u16,
    pcclass: *mut u32,
    _r: u32,
    subkeys: *mut u32,
    max_sub: *mut u32,
    max_class: *mut u32,
    nvalues: *mut u32,
    max_name: *mut u32,
    max_data: *mut u32,
    sec: *mut u32,
    ft: *mut u32,
) -> u32 {
    let (kids, vals) = match crate::key_info(h) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !class.is_null() && !pcclass.is_null() && unsafe { *pcclass } > 0 {
        unsafe { *class = 0 };
    }
    set_out(pcclass, 0);
    set_out(subkeys, kids.len() as u32);
    set_out(max_sub, kids.iter().map(|k| to_ucs(k).len()).max().unwrap_or(0) as u32);
    set_out(max_class, 0);
    set_out(nvalues, vals.len() as u32);
    set_out(max_name, vals.iter().map(|v| to_ucs(&v.0).len()).max().unwrap_or(0) as u32);
    set_out(max_data, vals.iter().map(|v| if is_string(v.1) { to_ucs(&v.2).len() * 2 } else { v.2.len() }).max().unwrap_or(0) as u32);
    set_out(sec, 0);
    zero_filetime(ft);
    ERROR_SUCCESS
}
