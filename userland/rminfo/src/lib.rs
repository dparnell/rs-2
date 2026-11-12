//! RMINFO.DLL: the resource manager's calls for programs (IBM's DDK
//! rmcalls.h, ring 3), as IBM's RMINFO.DLL provides them and its
//! RMVIEW.EXE and PNP.DLL (the Hardware Manager) call them, studied with
//! Ghidra. They are _Optlink functions: the first three arguments come in
//! EAX, EDX and ECX with their stack slots reserved.
//!
//! The information calls go to the kernel's `RESMGR$` device (IOCtl
//! category 0x80; kernel/src/resmgr.rs): RMEnumNodes (function 2) and
//! RMGetNodeInfo (function 1). RMGetSnoopLevel and RMSetSnoopLevel keep
//! the hardware detection levels as IBM's do, in the first two words of
//! `\OS2\BOOT\PREVIOUS.DAT` on the boot drive (TARGETPATH's drive when it
//! is set); rs2 detects no hardware at boot, so RMSaveDetectedData has
//! nothing to save. The calls drivers use (creating drivers, adapters
//! and devices, allocating resources) are not for programs: they fail.

#![no_std]
#![no_main]

extern crate os2;

use os2::dos::*;

/// RMRC_ codes.
const RMRC_SUCCESS: u32 = 0;
const RMRC_NOT_INITIALIZED: u32 = 1;
const RMRC_BAD_PARM: u32 = 0x0e;
const RMRC_NO_DETECTED_DATA: u32 = 0x10;
const RMRC_BUFFER_TOO_SMALL: u32 = 0x11;
const RMRC_BAD_LEVEL: u32 = 0x18;
const RMRC_FILE_ERROR: u32 = 0x19;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// DosDevIOCtl on RESMGR$, its error as RMINFO reports it.
fn rm_ioctl(function: u32, params: *mut u8, cb_params: u32, data: *mut u8, cb_data: u32) -> u32 {
    let (mut h, mut action) = (0u32, 0u32);
    // Open existing, deny none.
    if unsafe { DosOpen(c"RESMGR$".as_ptr() as *const u8, &mut h, &mut action, 0, 0, 1, 0x42, core::ptr::null_mut()) } != 0 {
        return RMRC_NOT_INITIALIZED;
    }
    let (mut pl, mut dl) = (cb_params, cb_data);
    let rc = unsafe { DosDevIOCtl(h, 0x80, function, params, cb_params, &mut pl, data, cb_data, &mut dl) };
    unsafe { DosClose(h) };
    match rc {
        0 => RMRC_SUCCESS,
        rc if rc & 0xff == 0x0c => RMRC_BUFFER_TOO_SMALL,
        _ => RMRC_BAD_PARM,
    }
}

/// RMEnumNodes(pCommand, pEnumData, cbEnumData): the nodes of a view
/// (0 physical, 1 logical, 2 drivers, 3/4 previous/current detected).
#[unsafe(no_mangle)]
pub extern "C" fn RMEnumNodes(cmd: *mut u16, data: *mut u8, cb: u32) -> u32 {
    if cmd.is_null() || unsafe { *cmd } > 4 {
        return RMRC_NOT_INITIALIZED;
    }
    rm_ioctl(2, cmd as *mut u8, 2, data, cb)
}

/// RMGetNodeInfo(hNode, pNodeData, cbNodeData).
#[unsafe(no_mangle)]
pub extern "C" fn RMGetNodeInfo(handle: u32, data: *mut u8, cb: u32) -> u32 {
    if data.is_null() {
        return RMRC_BAD_PARM;
    }
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&handle.to_le_bytes());
    p[4..8].copy_from_slice(&(data as u32).to_le_bytes());
    rm_ioctl(1, p.as_mut_ptr(), 8, data, cb & 0xffff)
}

/// RMConvertID(pulID, pszID, usCommand): a compressed EISA ID to its
/// seven characters (1) or back (2).
#[unsafe(no_mangle)]
pub extern "C" fn RMConvertID(id: *mut u32, text: *mut u8, cmd: u32) -> u32 {
    if id.is_null() || text.is_null() {
        return RMRC_BAD_PARM;
    }
    match cmd & 0xffff {
        1 => {
            let b = unsafe { *id }.to_le_bytes();
            let hex = |n: u8| b"0123456789ABCDEF"[n as usize & 15];
            let s = [
                ((b[0] >> 2) & 0x1f) + 0x40,
                (((b[0] & 3) << 3) | (b[1] >> 5)) + 0x40,
                (b[1] & 0x1f) + 0x40,
                hex(b[2] >> 4),
                hex(b[2]),
                hex(b[3] >> 4),
                hex(b[3]),
                0,
            ];
            unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), text, 8) };
            RMRC_SUCCESS
        }
        2 => {
            let t = unsafe { core::slice::from_raw_parts(text, 7) };
            let letter = |c: u8| (c.to_ascii_uppercase().wrapping_sub(0x40)) & 0x1f;
            let digit = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            let (a, b, c) = (letter(t[0]), letter(t[1]), letter(t[2]));
            let v = [a << 2 | b >> 3, (b & 7) << 5 | c, digit(t[3]) << 4 | digit(t[4]), digit(t[5]) << 4 | digit(t[6])];
            unsafe { *id = u32::from_le_bytes(v) };
            RMRC_SUCCESS
        }
        _ => RMRC_BAD_PARM,
    }
}

/// `?:\OS2\BOOT\PREVIOUS.DAT` on the boot drive (TARGETPATH's drive
/// while installing).
fn data_file() -> [u8; 26] {
    let mut f = *b"?:\\OS2\\BOOT\\PREVIOUS.DAT\0\0";
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c"TARGETPATH".as_ptr() as *const u8, &mut v) } == 0 && !v.is_null() {
        f[0] = unsafe { *v };
    } else {
        let mut d = 0u32;
        unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
        f[0] = b'@' + d as u8;
    }
    f
}

/// The two levels the file holds (the default 3, medium, when it
/// does not exist yet).
fn read_levels() -> Option<[u16; 2]> {
    let f = data_file();
    let (mut h, mut action) = (0u32, 0u32);
    if unsafe { DosOpen(f.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut b = [0u8; 4];
    let mut got = 0;
    unsafe {
        DosRead(h, b.as_mut_ptr(), 4, &mut got);
        DosClose(h);
    }
    (got == 4).then(|| [u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]])])
}

fn write_levels(l: [u16; 2]) -> bool {
    let f = data_file();
    let (mut h, mut action) = (0u32, 0u32);
    // Open or create, write access, deny write.
    if unsafe { DosOpen(f.as_ptr(), &mut h, &mut action, 4, 0, 0x11, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut b = [0u8; 4];
    b[0..2].copy_from_slice(&l[0].to_le_bytes());
    b[2..4].copy_from_slice(&l[1].to_le_bytes());
    let mut n = 0;
    unsafe {
        DosWrite(h, b.as_ptr(), 4, &mut n);
        DosClose(h);
    }
    n == 4
}

/// RMGetSnoopLevel(pusLevel, usFlags): flag 1 the first level the file
/// keeps, 2 the second.
#[unsafe(no_mangle)]
pub extern "C" fn RMGetSnoopLevel(level: *mut u16, flags: u32) -> u32 {
    let flags = flags & 0xffff;
    if flags & !3 != 0 || flags == 3 || level.is_null() {
        return RMRC_BAD_LEVEL;
    }
    let Some(l) = read_levels() else { return RMRC_FILE_ERROR };
    if flags & 1 != 0 {
        unsafe { *level = l[0] };
    } else if flags & 2 != 0 {
        unsafe { *level = l[1] };
    }
    RMRC_SUCCESS
}

/// RMSetSnoopLevel(usLevel, usFlags): sets the levels the flags name.
#[unsafe(no_mangle)]
pub extern "C" fn RMSetSnoopLevel(level: u32, flags: u32) -> u32 {
    let (level, flags) = (level as u16, flags & 0xffff);
    if level > 4 {
        return RMRC_BAD_PARM;
    }
    if flags & !3 != 0 {
        return RMRC_BAD_LEVEL;
    }
    let mut l = read_levels().unwrap_or([3, 3]);
    if flags & 1 != 0 {
        l[0] = level;
    }
    if flags & 2 != 0 {
        l[1] = level;
    }
    if write_levels(l) { RMRC_SUCCESS } else { RMRC_FILE_ERROR }
}

/// RMSaveDetectedData(usFlags): the boot's detected hardware (none on
/// rs2).
#[unsafe(no_mangle)]
pub extern "C" fn RMSaveDetectedData(flags: u32) -> u32 {
    if flags & 0xffff != 0 {
        return RMRC_BAD_LEVEL;
    }
    RMRC_NO_DETECTED_DATA
}

/// The calls of device drivers.
#[unsafe(no_mangle)]
pub extern "C" fn RMNotHere() -> u32 {
    RMRC_NOT_INITIALIZED
}

/// _Optlink entries: the register arguments (up to three) become the C
/// ones, on a stack aligned for Rust (IBM's compilers keep only 4-byte
/// alignment; LLVM takes 16 for granted). The caller's reserved slots
/// are not touched: it may have reserved fewer than three.
macro_rules! optlink {
    ($($o:ident => $f:ident;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $o() {
            core::arch::naked_asm!(
                "push ebp",
                "mov ebp, esp",
                "and esp, -16",
                "sub esp, 16",
                "mov dword ptr [esp], eax",
                "mov dword ptr [esp + 4], edx",
                "mov dword ptr [esp + 8], ecx",
                "call {f}",
                "mov esp, ebp",
                "pop ebp",
                "ret",
                f = sym $f,
            );
        }
    )*};
}

optlink! {
    __o_RMEnumNodes => RMEnumNodes;
    __o_RMGetNodeInfo => RMGetNodeInfo;
    __o_RMConvertID => RMConvertID;
    __o_RMGetSnoopLevel => RMGetSnoopLevel;
    __o_RMSetSnoopLevel => RMSetSnoopLevel;
    __o_RMSaveDetectedData => RMSaveDetectedData;
}
