//! Dynamic data exchange: WinDdeInitiate, WinDdeRespond, WinDdePostMsg.
//! The DDEINIT and DDESTRUCT blocks are giveable shared memory that is
//! given to the receiving process, as OS/2 does; the messages themselves
//! are ordinary sent or posted messages.

use alloc::vec::Vec;
use os2::{DosAllocSharedMem, DosFreeMem};

unsafe extern "C" {
    #[link_name = "DOSCALLS.303"]
    fn DosGiveSharedMem(pb: *mut u8, pid: u32, flags: u32) -> u32;
}

use crate::msg::{post, send};
use crate::state::{pid, st};

const WM_DDE_INITIATE: u32 = 0xa0;
const WM_DDE_INITIATEACK: u32 = 0xa9;
const DDEPM_NOFREE: u32 = 2;
/// PAG_COMMIT | PAG_WRITE | PAG_READ | OBJ_GIVEABLE.
const GIVEABLE: u32 = 0x213;

/// The process of a window's thread.
fn window_pid(h: u32) -> Option<u32> {
    let s = st();
    let q = s.queue_of(h);
    s.queues.get(&q).map(|q| q.pid)
}

fn cstr(p: *const u8) -> Vec<u8> {
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// A DDEINIT block (cb, pszAppName, pszTopic, offConvContext) with the
/// conversation context (CONVCONTEXT, its first ULONG is its size) and
/// the two strings after it.
fn init_data(app: *const u8, topic: *const u8, ctx: *const u32) -> Option<*mut u8> {
    if app.is_null() || topic.is_null() {
        return None;
    }
    let (a, t) = (cstr(app), cstr(topic));
    let cc = if ctx.is_null() { 0 } else { (unsafe { ctx.read_unaligned() }) as usize & 0xffff };
    let size = 16 + cc + a.len() + 1 + t.len() + 1;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { DosAllocSharedMem(&mut p, core::ptr::null(), size as u32, GIVEABLE) } != 0 {
        return None;
    }
    unsafe {
        let base = p as u32;
        let pa = base + 16 + cc as u32;
        let pt = pa + a.len() as u32 + 1;
        let hdr = [16u32, pa, pt, if cc == 0 { 0 } else { 16 }];
        for (i, v) in hdr.iter().enumerate() {
            (p as *mut u32).add(i).write_unaligned(*v);
        }
        if cc != 0 {
            core::ptr::copy_nonoverlapping(ctx as *const u8, p.add(16), cc);
        }
        core::ptr::copy_nonoverlapping(a.as_ptr(), pa as *mut u8, a.len());
        *((pa as *mut u8).add(a.len())) = 0;
        core::ptr::copy_nonoverlapping(t.as_ptr(), pt as *mut u8, t.len());
        *((pt as *mut u8).add(t.len())) = 0;
    }
    Some(p)
}

/// Let the process of window `h` use the block; true if it is the
/// caller's own (nothing to give).
fn give(p: *mut u8, h: u32) -> Option<bool> {
    let to = window_pid(h)?;
    if to == pid() {
        return Some(true);
    }
    // PAG_READ | PAG_WRITE.
    (unsafe { DosGiveSharedMem(p, to, 3) } == 0).then_some(false)
}

/// WinDdeInitiate(hwndClient, pszAppName, pszTopicName, pcctxt): sends
/// WM_DDE_INITIATE to every top-level window but the client, each with
/// its own DDEINIT.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeInitiate(client: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32 {
    let tops: Vec<u32> = st().desk.get(pmwin::HWND_DESKTOP).map_or(Vec::new(), |d| d.children.iter().rev().copied().collect());
    for h in tops {
        if h == client || !st().desk.exists(h) {
            continue;
        }
        let Some(p) = init_data(app, topic, ctx) else { break };
        match give(p, h) {
            Some(own) => {
                send(h, WM_DDE_INITIATE, client, p as u32);
                // The receiver frees its block; the caller's access to a
                // given one goes.
                if !own {
                    unsafe { DosFreeMem(p) };
                }
            }
            None => unsafe {
                DosFreeMem(p);
            },
        }
    }
    1
}

/// WinDdeRespond(hwndClient, hwndServer, pszAppName, pszTopicName,
/// pcctxt): sends WM_DDE_INITIATEACK to the client.
#[unsafe(no_mangle)]
pub extern "C" fn WinDdeRespond(client: u32, server: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32 {
    if !st().desk.exists(client) {
        return 0;
    }
    let Some(p) = init_data(app, topic, ctx) else { return 0 };
    let Some(own) = give(p, client) else {
        unsafe { DosFreeMem(p) };
        return 0;
    };
    let r = send(client, WM_DDE_INITIATEACK, server, p as u32);
    if !own {
        unsafe { DosFreeMem(p) };
    }
    r
}

/// WinDdePostMsg(hwndTo, hwndFrom, wm, pddest, flOptions): posts a DDE
/// message with its DDESTRUCT, which goes to the receiving process (the
/// caller keeps access with DDEPM_NOFREE).
#[unsafe(no_mangle)]
pub extern "C" fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32 {
    if !st().desk.exists(to) {
        return 0;
    }
    if !dde.is_null() {
        let Some(own) = give(dde, to) else { return 0 };
        if !own && fl & DDEPM_NOFREE == 0 {
            unsafe { DosFreeMem(dde) };
        }
    }
    // rs2's queues do not fill up, so DDEPM_RETRY never has to wait.
    post(to, wm, from, dde as u32) as u32
}
