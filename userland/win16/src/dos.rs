//! The interrupts of Windows programs, reflected here by the kernel
//! (api/win16.rs): INT 21h (DOS, as Windows' KERNEL serves it to
//! protected-mode programs, on OS/2 files) and WIN16's own INT E0h
//! (InitTask).

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
}
use reg::*;

const CF: u16 = 1;

pub fn interrupt(p: *mut u16) {
    if p.is_null() {
        return;
    }
    let mut r: [u16; 14] = core::array::from_fn(|i| unsafe { p.add(i).read_unaligned() });
    match r[INTNO] {
        0xe0 => kernel::init_task(&mut r),
        0x21 => int21(&mut r),
        n => {
            trace(format_args!("WIN16: INT {n:02X}h AX={:04X} at {:04X}:{:04X} not supported", r[AX], r[CS], r[IP].wrapping_sub(2)));
            r[FLAGS] |= CF;
        }
    }
    for (i, v) in r.iter().enumerate() {
        unsafe { p.add(i).write_unaligned(*v) };
    }
}

fn hi(v: u16) -> u8 {
    (v >> 8) as u8
}

fn ok(r: &mut [u16; 14]) {
    r[FLAGS] &= !CF;
}

fn fail(r: &mut [u16; 14], e: u32) {
    r[AX] = e as u16;
    r[FLAGS] |= CF;
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

fn int21(r: &mut [u16; 14]) {
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
