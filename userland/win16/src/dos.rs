//! The interrupts of Windows programs, reflected here by the kernel
//! (api/win16.rs): INT 21h (DOS, as Windows' KERNEL serves it to
//! protected-mode programs, on OS/2 files) and WIN16's own INT E0h
//! (InitTask); and their protection faults, of which one is fixed up:
//! loading an invalid selector into ES or DS (compilers load far
//! pointers early, before they know the pointer is one) loads 0.

use crate::kernel;
use crate::pm::trace;
use thunk16::flat;

/// INTREGS indexes (kernel api/win16.rs).
#[allow(dead_code)]
pub mod reg {
    pub const AX: usize = 0;
    pub const BX: usize = 1;
    pub const CX: usize = 2;
    pub const DX: usize = 3;
    pub const SI: usize = 4;
    pub const DI: usize = 5;
    pub const BP: usize = 6;
    pub const DS: usize = 7;
    pub const ES: usize = 8;
    pub const FLAGS: usize = 9;
    pub const IP: usize = 10;
    pub const CS: usize = 11;
    pub const INTNO: usize = 12;
    pub const SP: usize = 13;
    pub const ERROR: usize = 14;
}
use reg::*;

const CF: u16 = 1;

pub fn interrupt(p: *mut u16) {
    if p.is_null() {
        return;
    }
    let mut r: Regs = core::array::from_fn(|i| unsafe { p.add(i).read_unaligned() });
    match r[INTNO] {
        0xe0 => kernel::init_task(&mut r),
        0xe1 => kernel::fpmath(&mut r),
        0xe2 => kernel::catch(&mut r),
        0xe3 => kernel::throw(&mut r),
        0xe4 => kernel::undefined_dynalink(&r),
        0x21 => int21(&mut r),
        0x25 | 0x26 => abs_disk(&mut r),
        // A protection fault: handled when it is fixed (the number 0).
        0x10d => {
            if segment_load(&mut r) {
                r[INTNO] = 0;
            }
        }
        n => {
            trace(format_args!("WIN16: INT {n:02X}h AX={:04X} at {:04X}:{:04X} not supported", r[AX], r[CS], r[IP].wrapping_sub(2)));
            r[FLAGS] |= CF;
        }
    }
    for (i, v) in r.iter().enumerate() {
        unsafe { p.add(i).write_unaligned(*v) };
    }
}

/// INTREGS.
pub type Regs = [u16; 15];

/// INT 25h and 26h: absolute disk read and write of drive AL, CX
/// sectors from DX to DS:BX (CX FFFFh: a packet at DS:BX with the start
/// sector, the count and the buffer), on OS/2's DASD access to the
/// drive. They return with the flags still on the stack, as DOS's do.
fn abs_disk(r: &mut Regs) {
    let write = r[INTNO] == 0x26;
    let (start, count, buf) = if r[CX] == 0xffff {
        let p = ptr(r[DS], r[BX]);
        let rd = |o: usize| unsafe { (p.add(o) as *const u32).read_unaligned() };
        (rd(0), rd(4) & 0xffff, flat(unsafe { (p.add(6) as *const u32).read_unaligned() }))
    } else {
        (r[DX] as u32, r[CX] as u32, ptr(r[DS], r[BX]))
    };
    let drive = [b'A' + (r[AX] as u8 & 0x1f), b':', 0];
    let (mut hf, mut action) = (0u32, 0u32);
    let mode = 0x8000 | 0x40 | if write { 2 } else { 0 };
    let mut e = unsafe { os2::DosOpen(drive.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, mode, core::ptr::null_mut()) };
    if e == 0 {
        let mut pos = 0u32;
        e = unsafe { os2::DosSetFilePtr(hf, (start * 512) as i32, 0, &mut pos) };
        if e == 0 {
            let mut n = 0u32;
            e = if write {
                unsafe { os2::DosWrite(hf, buf, count * 512, &mut n) }
            } else {
                unsafe { os2::DosRead(hf, buf, count * 512, &mut n) }
            };
        }
        unsafe { os2::DosClose(hf) };
    }
    if e == 0 {
        ok(r);
    } else {
        // Drive not ready.
        r[AX] = 0x0802;
        r[FLAGS] |= CF;
    }
    // INTREGS end just below the interrupted SP, so the word at SP - 2
    // is their last (the error code): it becomes the pushed flags.
    r[SP] = r[SP].wrapping_sub(2);
    r[ERROR] = r[FLAGS];
}

/// The disk transfer address (0: the PSP's, at 80h).
static DTA: crate::Global<u32> = crate::Global::new(0);

fn dta() -> u32 {
    match *DTA.get() {
        0 => (kernel::TASK.get().psp as u32) << 16 | 0x80,
        d => d,
    }
}

/// The DTA of a search: 21 reserved bytes (here "W16" and the OS/2
/// search handle), the attributes, time, date, size and name (8.3).
fn find_first(r: &mut Regs) {
    let spec = path(r[DS], r[DX]);
    let d = flat(dta());
    // A volume label search finds none.
    if r[CX] & 0x08 != 0 && r[CX] & 0x10 == 0 {
        return fail(r, 18);
    }
    let mut hdir = 0xffff_ffffu32;
    let mut buf = [0u8; 300];
    let mut count = 1u32;
    let e = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut hdir, r[CX] as u32 & 0x16, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    if e != 0 || count == 0 {
        return fail(r, if e == 18 || e == 0 { 18 } else { e });
    }
    unsafe {
        core::ptr::copy_nonoverlapping(b"W16".as_ptr(), d, 3);
        (d.add(3) as *mut u32).write_unaligned(hdir);
    }
    put_found(d, &buf);
}

fn find_next(r: &mut Regs) {
    let d = flat(dta());
    if unsafe { core::slice::from_raw_parts(d, 3) } != b"W16" {
        return fail(r, 18);
    }
    let hdir = unsafe { (d.add(3) as *const u32).read_unaligned() };
    let mut buf = [0u8; 300];
    let mut count = 1u32;
    let e = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    if e != 0 || count == 0 {
        unsafe { os2::DosFindClose(hdir) };
        unsafe { *d = 0 };
        return fail(r, 18);
    }
    put_found(d, &buf);
}

/// A FILEFINDBUF3 into the DTA's found-file fields.
fn put_found(d: *mut u8, b: &[u8]) {
    let w = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let len = (b[28] as usize).min(12);
    unsafe {
        *d.add(21) = b[24];
        (d.add(22) as *mut u16).write_unaligned(w(14));
        (d.add(24) as *mut u16).write_unaligned(w(12));
        (d.add(26) as *mut u32).write_unaligned(u32::from_le_bytes([b[16], b[17], b[18], b[19]]));
        for i in 0..13 {
            *d.add(30 + i) = if i < len { b[29 + i].to_ascii_uppercase() } else { 0 };
        }
    }
}

/// A protection fault loading ES or DS with a selector that is not
/// valid (LES, LDS, MOV sreg, POP sreg): load 0 and go on.
fn segment_load(r: &mut Regs) -> bool {
    let code = flat((r[CS] as u32) << 16 | r[IP] as u32) as *const u8;
    let at = |i: usize| unsafe { *code.add(i) };
    // The stack segment: the program's (where this handler was called).
    let ss = (thunk16::sp16() >> 16) as u16;
    let mut i = 0;
    let mut seg = None;
    loop {
        match at(i) {
            0x26 => seg = Some(r[ES]),
            0x2e => seg = Some(r[CS]),
            0x36 => seg = Some(ss),
            0x3e => seg = Some(r[DS]),
            0x66 | 0x67 => return false,
            _ => break,
        }
        i += 1;
    }
    let op = at(i);
    let bad = r[ERROR] & !3;
    let fixed = match op {
        // POP ES, POP DS
        0x07 | 0x1f => {
            let sel = unsafe { (flat((ss as u32) << 16 | r[SP] as u32) as *const u16).read_unaligned() };
            if sel & !3 != bad {
                return false;
            }
            r[if op == 0x07 { ES } else { DS }] = 0;
            r[SP] = r[SP].wrapping_add(2);
            i + 1
        }
        // MOV ES/DS, r/m16 and LES/LDS r16, m16:16
        0x8e | 0xc4 | 0xc5 => {
            let modrm = at(i + 1);
            let (md, reg, rm) = (modrm >> 6, (modrm >> 3) & 7, modrm & 7);
            let sreg = match op {
                0xc4 => ES,
                0xc5 => DS,
                _ if reg == 0 => ES,
                _ if reg == 3 => DS,
                _ => return false,
            };
            // The 16-bit effective address.
            let (mut ea, mut def) = match rm {
                0 => (r[BX].wrapping_add(r[SI]), r[DS]),
                1 => (r[BX].wrapping_add(r[DI]), r[DS]),
                2 => (r[BP].wrapping_add(r[SI]), ss),
                3 => (r[BP].wrapping_add(r[DI]), ss),
                4 => (r[SI], r[DS]),
                5 => (r[DI], r[DS]),
                6 => (r[BP], ss),
                _ => (r[BX], r[DS]),
            };
            let mut len = 2;
            match md {
                0 if rm == 6 => {
                    ea = at(i + 2) as u16 | (at(i + 3) as u16) << 8;
                    def = r[DS];
                    len = 4;
                }
                1 => {
                    ea = ea.wrapping_add(at(i + 2) as i8 as u16);
                    len = 3;
                }
                2 => {
                    ea = ea.wrapping_add(at(i + 2) as u16 | (at(i + 3) as u16) << 8);
                    len = 4;
                }
                _ => {}
            }
            const GPR: [usize; 8] = [AX, CX, DX, BX, SP, BP, SI, DI];
            if md == 3 {
                // MOV sreg, r16
                if op != 0x8e || r[GPR[rm as usize]] & !3 != bad {
                    return false;
                }
            } else {
                let m = flat((seg.unwrap_or(def) as u32) << 16 | ea as u32) as *const u16;
                if m.is_null() {
                    return false;
                }
                let (off, sel) = unsafe { (m.read_unaligned(), m.add(1).read_unaligned()) };
                let loaded = if op == 0x8e { off } else { sel };
                if loaded & !3 != bad {
                    return false;
                }
                if op != 0x8e {
                    r[GPR[reg as usize]] = off;
                }
            }
            r[sreg] = 0;
            i + len
        }
        _ => return false,
    };
    if crate::tracing() {
        trace(format_args!("W16 {:04X}:{:04X}: invalid selector {bad:#06x} loaded as 0", r[CS], r[IP]));
    }
    r[IP] = r[IP].wrapping_add(fixed as u16);
    true
}

fn hi(v: u16) -> u8 {
    (v >> 8) as u8
}

fn ok(r: &mut Regs) {
    r[FLAGS] &= !CF;
}

fn fail(r: &mut Regs, e: u32) {
    r[AX] = dos_error(e) as u16;
    r[FLAGS] |= CF;
}

/// The DOS error code of an OS/2 one: OS/2's codes extend DOS's, but
/// opening a file that does not exist fails with ERROR_OPEN_FAILED.
pub fn dos_error(e: u32) -> u32 {
    match e {
        110 => 2,
        e if e > 0x58 => 0x1f, // general failure
        e => e,
    }
}

/// DS:DX (or another register pair) as a flat pointer.
fn ptr(seg: u16, off: u16) -> *mut u8 {
    flat((seg as u32) << 16 | off as u32)
}

/// A path argument, NUL-terminated for OS/2.
fn path(seg: u16, off: u16) -> alloc::vec::Vec<u8> {
    let mut v = unsafe { crate::cstr(ptr(seg, off)) }.to_vec();
    v.push(0);
    v
}

fn int21(r: &mut Regs) {
    let ah = hi(r[AX]);
    let al = r[AX] as u8;
    if crate::tracing() {
        trace(format_args!("W16 INT 21h AX={:04X} BX={:04X} CX={:04X} DX={:04X}", r[AX], r[BX], r[CX], r[DX]));
    }
    ok(r);
    match ah {
        // Select disk: the number of drives in AL.
        0x0e => {
            unsafe { os2::DosSetDefaultDisk((r[DX] as u32 & 0xff) + 1) };
            r[AX] = (r[AX] & 0xff00) | 26;
        }
        // Current disk.
        0x19 => {
            let (mut d, mut map) = (0, 0);
            unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
            r[AX] = (r[AX] & 0xff00) | (d as u16).saturating_sub(1);
        }
        // Set interrupt vector: kept by Windows for the task; nothing
        // here delivers hardware interrupts to Windows programs.
        0x25 => {}
        // Get date: CX year, DH month, DL day, AL weekday.
        0x2a => {
            let mut dt = os2::DateTime::default();
            unsafe { os2::DosGetDateTime(&mut dt) };
            r[CX] = dt.year;
            r[DX] = (dt.month as u16) << 8 | dt.day as u16;
            r[AX] = (r[AX] & 0xff00) | dt.weekday as u16;
        }
        // Get time: CH hours, CL minutes, DH seconds, DL hundredths.
        0x2c => {
            let mut dt = os2::DateTime::default();
            unsafe { os2::DosGetDateTime(&mut dt) };
            r[CX] = (dt.hours as u16) << 8 | dt.minutes as u16;
            r[DX] = (dt.seconds as u16) << 8 | dt.hundredths as u16;
        }
        // Set the disk transfer address (find first/next write there).
        0x1a => *DTA.get() = (r[DS] as u32) << 16 | r[DX] as u32,
        // Get it: ES:BX.
        0x2f => {
            let d = dta();
            r[ES] = (d >> 16) as u16;
            r[BX] = d as u16;
        }
        // Free disk space of drive DL (0 the current one): AX sectors per
        // cluster, BX free clusters, CX bytes per sector, DX clusters.
        0x36 => {
            let mut a = [0u32; 5];
            match unsafe { os2::DosQueryFSInfo(r[DX] as u32 & 0xff, 1, a.as_mut_ptr() as *mut u8, 18) } {
                0 => {
                    let (spc, total, free, bps) = (a[1], a[2], a[3], a[4] & 0xffff);
                    r[AX] = spc as u16;
                    r[BX] = free.min(0xffff) as u16;
                    r[CX] = bps as u16;
                    r[DX] = total.min(0xffff) as u16;
                }
                _ => r[AX] = 0xffff,
            }
        }
        // Make and remove a directory.
        0x39 | 0x3a => {
            let p = path(r[DS], r[DX]);
            let e = if ah == 0x39 { unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) } } else { unsafe { os2::DosDeleteDir(p.as_ptr()) } };
            if e != 0 {
                fail(r, e);
            }
        }
        // Get (AL 0) and set (AL 1) a file's attributes: CX.
        0x43 => {
            let p = path(r[DS], r[DX]);
            let mut fs = [0u8; 24];
            match unsafe { os2::DosQueryPathInfo(p.as_ptr(), 1, fs.as_mut_ptr(), 24) } {
                0 if al == 0 => r[CX] = u16::from_le_bytes([fs[20], fs[21]]),
                0 if al == 1 => {
                    fs[20..24].copy_from_slice(&(r[CX] as u32 & 0x27).to_le_bytes());
                    let e = unsafe { os2::DosSetPathInfo(p.as_ptr(), 1, fs.as_ptr(), 24, 0) };
                    if e != 0 {
                        fail(r, e);
                    }
                }
                0 => fail(r, 1),
                e => fail(r, e),
            }
        }
        // Duplicate a handle.
        0x45 => {
            let mut new = 0xffff_ffffu32;
            match unsafe { os2::DosDupHandle(r[BX] as u32, &mut new) } {
                0 => r[AX] = new as u16,
                e => fail(r, e),
            }
        }
        // Find the first file matching DS:DX with attributes CX, and the
        // next: the DTA gets it.
        0x4e => find_first(r),
        0x4f => find_next(r),
        // Rename DS:DX to ES:DI.
        0x56 => {
            let (from, to) = (path(r[DS], r[DX]), path(r[ES], r[DI]));
            let e = unsafe { os2::DosMove(from.as_ptr(), to.as_ptr()) };
            if e != 0 {
                fail(r, e);
            }
        }
        // A file's date and time: get (AL 0) in DX and CX, set (AL 1).
        0x57 if al <= 1 => {
            let mut fs = [0u8; 24];
            match unsafe { os2::DosQueryFileInfo(r[BX] as u32, 1, fs.as_mut_ptr(), 24) } {
                0 if al == 0 => {
                    r[DX] = u16::from_le_bytes([fs[8], fs[9]]);
                    r[CX] = u16::from_le_bytes([fs[10], fs[11]]);
                }
                0 => {
                    fs[8..10].copy_from_slice(&r[DX].to_le_bytes());
                    fs[10..12].copy_from_slice(&r[CX].to_le_bytes());
                    let e = unsafe { os2::DosSetFileInfo(r[BX] as u32, 1, fs.as_ptr(), 24) };
                    if e != 0 {
                        fail(r, e);
                    }
                }
                e => fail(r, e),
            }
        }
        // Long file names (Windows 95): not here.
        0x71 => fail(r, 0x7100),
        // DOS version: 5.0.
        0x30 => {
            r[AX] = 0x0005;
            r[BX] = 0;
            r[CX] = 0;
        }
        // Ctrl-Break checking: off.
        0x33 => {
            if al == 0 {
                r[DX] &= 0xff00;
            }
        }
        // Get interrupt vector: none.
        0x35 => {
            r[ES] = 0;
            r[BX] = 0;
        }
        // Change directory.
        0x3b => {
            let p = path(r[DS], r[DX]);
            match unsafe { os2::DosSetCurrentDir(p.as_ptr()) } {
                0 => {}
                e => fail(r, e),
            }
        }
        // Create (CX attributes) and open (AL mode) a file.
        0x3c | 0x3d => {
            let p = path(r[DS], r[DX]);
            // DOS's compatibility sharing mode is OS/2's deny none.
            let share = match al as u32 & 0x70 {
                0 => 0x40,
                s => s,
            };
            let (open_flags, mode) = if ah == 0x3c { (0x12, 0x42) } else { (0x01, (al as u32 & 0x07) | share) };
            let (mut hf, mut action) = (0u32, 0u32);
            let attr = if ah == 0x3c { r[CX] as u32 & 0x27 } else { 0 };
            match unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut action, 0, attr, open_flags, mode, core::ptr::null_mut()) } {
                0 => r[AX] = hf as u16,
                e => fail(r, e),
            }
        }
        // Close.
        0x3e => match unsafe { os2::DosClose(r[BX] as u32) } {
            0 => {}
            e => fail(r, e),
        },
        // Read and write: BX handle, CX bytes, DS:DX buffer.
        0x3f | 0x40 => {
            let buf = ptr(r[DS], r[DX]);
            let mut n = 0u32;
            let e = if ah == 0x3f {
                unsafe { os2::DosRead(r[BX] as u32, buf, r[CX] as u32, &mut n) }
            } else {
                unsafe { os2::DosWrite(r[BX] as u32, buf, r[CX] as u32, &mut n) }
            };
            match e {
                0 => r[AX] = n as u16,
                e => fail(r, e),
            }
        }
        // Delete.
        0x41 => {
            let p = path(r[DS], r[DX]);
            match unsafe { os2::DosDelete(p.as_ptr()) } {
                0 => {}
                e => fail(r, e),
            }
        }
        // Seek: AL method, CX:DX offset; DX:AX the new position.
        0x42 => {
            let off = ((r[CX] as u32) << 16 | r[DX] as u32) as i32;
            let mut pos = 0u32;
            match unsafe { os2::DosSetFilePtr(r[BX] as u32, off, al as u32, &mut pos) } {
                0 => {
                    r[AX] = pos as u16;
                    r[DX] = (pos >> 16) as u16;
                }
                e => fail(r, e),
            }
        }
        // IOCTL 00h: device information (bit 7 a character device; bits
        // 0 and 1 the console's input and output).
        0x44 if al == 0 => {
            let (mut ty, mut attr) = (0u32, 0u32);
            match unsafe { os2::DosQueryHType(r[BX] as u32, &mut ty, &mut attr) } {
                0 => r[DX] = if ty & 0xff == 1 { 0x80 | if r[BX] < 3 { 0x03 } else { 0 } } else { 0 },
                e => fail(r, e),
            }
        }
        // Current directory of drive DL (0 the current one) to DS:SI.
        0x47 => {
            let mut cb = 64u32;
            let mut buf = [0u8; 64];
            match unsafe { os2::DosQueryCurrentDir(r[DX] as u32 & 0xff, buf.as_mut_ptr(), &mut cb) } {
                0 => {
                    let d = ptr(r[DS], r[SI]);
                    let s = unsafe { crate::cstr(buf.as_ptr()) };
                    unsafe {
                        core::ptr::copy_nonoverlapping(s.as_ptr(), d, s.len());
                        *d.add(s.len()) = 0;
                    }
                }
                e => fail(r, e),
            }
        }
        // DOS memory: Windows programs allocate with GlobalAlloc.
        0x48 => {
            r[BX] = 0;
            fail(r, 8);
        }
        0x49 | 0x4a => fail(r, 9),
        // Exit.
        0x4c => unsafe { os2::DosExit(1, al as u32) },
        // The PSP.
        0x51 | 0x62 => r[BX] = kernel::TASK.get().psp,
        _ => {
            trace(format_args!("WIN16: INT 21h AX={:04X} not supported", r[AX]));
            fail(r, 1);
        }
    }
}
