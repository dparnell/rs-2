//! What used to run under the kernel's last big lock, from several
//! processes at once: pop-ups (VioPopUp waits its turn), sessions started,
//! selected and stopped, keyboard and disk IOCtls, DosQuerySysState. The
//! checks are that every call gives a sensible result and nothing hangs
//! (the watchdog reports a stall).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::vio::{call16, ptr_words};
use os2::*;

use crate::api::*;
use crate::{check, ms};

unsafe extern "C" {
    #[link_name = "VIOCALLS.11"]
    fn VIOPOPUP();
    #[link_name = "VIOCALLS.1"]
    fn VIOENDPOPUP();
    #[link_name = "VIOCALLS.13"]
    fn VIOWRTCHARSTR();
}

fn entry(f: unsafe extern "C" fn()) -> usize {
    f as *const () as usize
}

#[repr(C, align(64))]
struct Small([u8; 32]);

/// Child: pop-ups, one after another (VP_WAIT: each waits for the others').
fn popups(n: u32) -> u32 {
    let mut wait = Small([0; 32]);
    wait.0[0] = 1; // VP_WAIT, opaque
    let mut text = Small([0; 32]);
    text.0[..10].copy_from_slice(b"STRESS POP");
    for _ in 0..n {
        let p = ptr_words(wait.0.as_ptr());
        let rc = unsafe { call16(entry(VIOPOPUP), &[p[0], p[1], 0]) };
        if rc != 0 {
            return 1;
        }
        let t = ptr_words(text.0.as_ptr());
        unsafe { call16(entry(VIOWRTCHARSTR), &[t[0], t[1], 10, 0, 0, 0]) };
        unsafe { DosSleep(5) };
        if unsafe { call16(entry(VIOENDPOPUP), &[0]) } != 0 {
            return 2;
        }
    }
    0
}

/// Child: sessions started (each running a short program), selected and
/// stopped.
fn sessions(n: u32) -> u32 {
    for i in 0..n {
        let sd = StartData {
            length: core::mem::size_of::<StartData>() as u16,
            related: 1,
            fgbg: (i & 1) as u16, // foreground every other time
            trace: 0,
            title: b"stress\0".as_ptr(),
            name: crate::SELF.as_ptr(),
            inputs: b"-child exit 0\0".as_ptr(),
            termq: null(),
            env: null(),
            inherit: 0,
            session_type: 1,
        };
        let (mut sid, mut pid) = (0, 0);
        let rc = unsafe { DosStartSession(&sd, &mut sid, &mut pid) };
        // 457: started in the background (this process is not in front).
        if rc != 0 && rc != 457 {
            return 10 + rc.min(200);
        }
        unsafe {
            DosSelectSession(sid);
            DosSleep(10);
            // It may have ended already (369/371).
            let rc = DosStopSession(0, sid);
            if rc != 0 && rc != 369 && rc != 371 {
                return 300;
            }
        }
    }
    0
}

/// Child: keyboard and disk IOCtls and system state queries.
fn ioctls(n: u32) -> u32 {
    let open = |name: &[u8], flags: u32| -> Option<u32> {
        let (mut h, mut act) = (0, 0);
        (unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, flags, null_mut()) } == 0).then_some(h)
    };
    let Some(kbd) = open(b"\\DEV\\KBD$\0", OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE) else { return 1 };
    let disk = open(b"C:\0", OPEN_FLAGS_DASD | OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE);
    let mut buf = alloc::vec![0u8; 64 * 1024];
    for _ in 0..n {
        let (mut m, mut cb) = (0xffu8, 1u32);
        if unsafe { DosDevIOCtl(kbd, 4, 0x71, null_mut(), 0, null_mut(), &mut m, 1, &mut cb) } != 0 {
            return 2;
        }
        // KBD_SETINTERIMFLAG to what it is (read and changed in place).
        let (mut i, mut cbi) = (0u8, 1u32);
        unsafe { DosDevIOCtl(kbd, 4, 0x72, null_mut(), 0, null_mut(), &mut i, 1, &mut cbi) };
        let mut cbp = 1u32;
        if unsafe { DosDevIOCtl(kbd, 4, 0x52, &mut i, 1, &mut cbp, null_mut(), 0, null_mut()) } != 0 {
            return 3;
        }
        if let Some(d) = disk {
            // DSK_GETDEVICEPARAMS.
            let mut cmd = 0u8;
            let mut cbp = 1u32;
            let mut bpb = [0u8; 64];
            let mut cbd = 64u32;
            if unsafe { DosDevIOCtl(d, 8, 0x63, &mut cmd, 1, &mut cbp, bpb.as_mut_ptr(), 64, &mut cbd) } != 0 {
                return 4;
            }
        }
        if unsafe { DosQuerySysState(QS_PROCESS | QS_SEMAPHORE | QS_MTE | QS_FILESYS, 0, 0, 0, buf.as_mut_ptr(), buf.len() as u32) } != 0 {
            return 5;
        }
    }
    unsafe { DosClose(kbd) };
    if let Some(d) = disk {
        unsafe { DosClose(d) };
    }
    0
}

/// Child modes (`STRESS -child corner-KIND n`).
pub fn child(kind: &str, n: u32) -> u32 {
    match kind {
        "corner-popup" => popups(n),
        "corner-session" => sessions(n),
        "corner-ioctl" => ioctls(n),
        _ => 255,
    }
}

pub fn run() {
    let t0 = ms();
    let mut pids = Vec::new();
    for _ in 0..2 {
        pids.push(("pop-ups", crate::start_child("corner-popup 40")));
        pids.push(("sessions", crate::start_child("corner-session 20")));
        pids.push(("IOCtls", crate::start_child("corner-ioctl 100")));
    }
    let mut ok = true;
    for (what, pid) in pids {
        let (rc, cause, code, _) = crate::wait_child(pid);
        if rc != 0 || cause != 0 || code != 0 {
            println!("  {what} child: rc {rc} cause {cause} code {code}");
            ok = false;
        }
    }
    check("corner: pop-ups, sessions and IOCtls from several processes at once", ok);
    println!("  corner section: {} ms", ms().wrapping_sub(t0));
}
