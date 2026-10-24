//! System hooks for PMXTEST: they count what they see per process in a
//! named shared memory block (\SHAREMEM\PMXHOOK), so PMXTEST can tell
//! that PMWIN loaded this DLL into other processes and called its hooks
//! there; the journal playback hook plays back three keys.

#![no_std]
#![no_main]

extern crate os2;

use os2::*;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// The counts: per process (pid, input messages, sent messages), the
/// journal: records, last recorded message, played keys.
#[repr(C)]
pub struct Counts {
    pub procs: [[u32; 3]; 16],
    pub records: u32,
    pub last_record: u32,
    pub played: u32,
    pub playback_calls: u32,
}

const NAME: &[u8] = b"\\SHAREMEM\\PMXHOOK\0";

fn counts() -> Option<&'static mut Counts> {
    let mut p: *mut u8 = core::ptr::null_mut();
    unsafe {
        if DosGetNamedSharedMem(&mut p, NAME.as_ptr(), PAG_READ | PAG_WRITE) != 0
            && DosAllocSharedMem(&mut p, NAME.as_ptr(), core::mem::size_of::<Counts>() as u32, PAG_READ | PAG_WRITE | PAG_COMMIT) != 0
        {
            return None;
        }
        (p as *mut Counts).as_mut()
    }
}

fn pid() -> u32 {
    let (mut t, mut p) = (core::ptr::null_mut(), core::ptr::null_mut());
    unsafe {
        DosGetInfoBlocks(&mut t, &mut p);
        (*p).pid
    }
}

fn slot(c: &mut Counts) -> Option<&mut [u32; 3]> {
    let me = pid();
    let i = c.procs.iter().position(|e| e[0] == me).or_else(|| c.procs.iter().position(|e| e[0] == 0))?;
    c.procs[i][0] = me;
    Some(&mut c.procs[i])
}

/// The counts block (PMXTEST reads it).
#[unsafe(no_mangle)]
pub extern "C" fn PmxCounts() -> *mut Counts {
    counts().map_or(core::ptr::null_mut(), |c| c as *mut Counts)
}

/// HK_INPUT: counts, takes nothing.
#[unsafe(no_mangle)]
pub extern "C" fn PmxInputHook(_hab: u32, _pq: *mut u32, _fs: u32) -> u32 {
    if let Some(e) = counts().and_then(slot) {
        e[1] += 1;
    }
    0
}

/// HK_SENDMSG: counts.
#[unsafe(no_mangle)]
pub extern "C" fn PmxSendMsgHook(_hab: u32, _psmh: *mut u32, _inter: u32) -> u32 {
    if let Some(e) = counts().and_then(slot) {
        e[2] += 1;
    }
    0
}

/// HK_JOURNALRECORD: counts and keeps the message number.
#[unsafe(no_mangle)]
pub extern "C" fn PmxRecordHook(_hab: u32, pq: *const u32) -> u32 {
    if let Some(c) = counts() {
        c.records += 1;
        if !pq.is_null() {
            c.last_record = unsafe { *pq.add(1) };
        }
    }
    0
}

const KEYS: &[u8] = b"xyz";

/// HK_JOURNALPLAYBACK: WM_CHAR for x, y and z (KC_CHAR), then nothing
/// more (it asks to be called again later until it is released).
#[unsafe(no_mangle)]
pub extern "C" fn PmxPlaybackHook(_hab: u32, skip: u32, pq: *mut u32) -> u32 {
    let Some(c) = counts() else { return 100 };
    c.playback_calls += 1;
    if skip != 0 {
        if (c.played as usize) < KEYS.len() {
            c.played += 1;
        }
        return 0;
    }
    let Some(&ch) = KEYS.get(c.played as usize) else { return 200 };
    if !pq.is_null() {
        unsafe {
            // QMSG: hwnd, msg, mp1 (fs | repeat << 16 | scan << 24), mp2
            // (char | vkey << 16), time, ptl.
            *pq = 0;
            *pq.add(1) = 0x7a;
            *pq.add(2) = 1 | 1 << 16;
            *pq.add(3) = ch as u32;
        }
    }
    0
}
