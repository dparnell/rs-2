//! TALKD: the talk daemon (UDP port 518, the 4.3BSD "ntalk" protocol).
//!
//! `TALKD [-p port]` keeps the invitations TALK clients leave
//! (LEAVE_INVITE), finds them for the other party (LOOK_UP), deletes
//! them (DELETE) and announces a call (ANNOUNCE): it shows who is calling
//! and how to answer on its own screen, with a beep. Invitations expire
//! after a minute. OS/2 has one user at the console, so every name is
//! "here". Only one TALKD runs at a time.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::talk::*;
use netapps::*;
use os2::net::*;
use os2::*;
os2::entry!(main);

struct Entry {
    msg: CtlMsg,
    /// The invitation's address as seen from the sender.
    time: u32,
}

const MAX_LIFE: u32 = 60_000;

unsafe extern "C" {
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}

fn announce(m: &CtlMsg, from: [u8; 4]) {
    let host = gethost(from);
    let (h, mi) = clock();
    outln("");
    outln(&alloc::format!("Talk request from {}@{} at {h}:{mi:02}", m.l_name, host));
    outln(&alloc::format!("talk: answer with:  talk {}@{}", m.l_name, host));
    unsafe {
        DosBeep(880, 100);
        DosBeep(660, 100);
    }
}

/// A host's name, or its address.
fn gethost(a: [u8; 4]) -> String {
    ip(a)
}

fn clock() -> (u8, u8) {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    (dt.hours, dt.minutes)
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("ntalk", "udp", 518);
    let mut i = 0;
    while i < a.len() {
        if a[i].eq_ignore_ascii_case("-p") {
            i += 1;
            port = a.get(i).and_then(|v| v.parse().ok()).unwrap_or(port);
        } else {
            eprint("Usage: talkd [-p port]\r\n");
            return 1;
        }
        i += 1;
    }
    let s = match Sock::udp() {
        Ok(s) => s,
        Err(_) => return 2,
    };
    if let Err(e) = s.bind([0; 4], port) {
        eprint(&alloc::format!("TALKD: port {port}: {} (is talkd already running?)\r\n", strerror(e)));
        return 2;
    }
    outln(&alloc::format!("TALKD: listening on port {port}"));
    let mut table: Vec<Entry> = Vec::new();
    let mut next_id = 1u32;
    let mut buf = [0u8; 512];
    loop {
        let Ok((n, from)) = s.recv_from(&mut buf) else {
            unsafe { DosSleep(100) };
            continue;
        };
        let now = ms();
        table.retain(|e| now.wrapping_sub(e.time) < MAX_LIFE);
        let Some((m, vers, f1, f2)) = CtlMsg::decode(&buf[..n]) else { continue };
        let mut r = Response { typ: m.typ, answer: SUCCESS, id: 0, addr: Addr::default() };
        if vers != VERSION {
            r.answer = BADVERSION;
        } else if f1 != 2 && m.typ != DELETE {
            r.answer = BADADDR;
        } else if f2 != 2 {
            r.answer = BADCTLADDR;
        } else {
            match m.typ {
                LEAVE_INVITE | ANNOUNCE => {
                    // The same request again (a retransmission or
                    // re-announcement) keeps its id.
                    let same = table.iter_mut().find(|e| e.msg.typ == m.typ && e.msg.l_name == m.l_name && e.msg.r_name == m.r_name && e.msg.pid == m.pid && e.msg.ctl_addr == m.ctl_addr);
                    match same {
                        Some(e) => {
                            e.time = now;
                            r.id = e.msg.id;
                            if m.typ == ANNOUNCE {
                                announce(&m, from.addr);
                            }
                        }
                        None => {
                            let mut e = Entry { msg: m.clone(), time: now };
                            e.msg.id = next_id;
                            // A missing address: the sender's.
                            if e.msg.addr.ip == [0; 4] {
                                e.msg.addr.ip = from.addr;
                            }
                            next_id = next_id.wrapping_add(1).max(1);
                            r.id = e.msg.id;
                            if m.typ == ANNOUNCE {
                                announce(&m, from.addr);
                            }
                            table.push(e);
                        }
                    }
                }
                LOOK_UP => match table.iter().find(|e| e.msg.typ == LEAVE_INVITE && e.msg.l_name.eq_ignore_ascii_case(&m.r_name) && e.msg.r_name.eq_ignore_ascii_case(&m.l_name)) {
                    Some(e) => {
                        r.id = e.msg.id;
                        r.addr = e.msg.addr;
                    }
                    None => r.answer = NOT_HERE,
                },
                DELETE => match table.iter().position(|e| e.msg.id == m.id) {
                    Some(i) => {
                        table.remove(i);
                    }
                    None => r.answer = NOT_HERE,
                },
                _ => r.answer = UNKNOWN_REQUEST,
            }
        }
        let _ = s.send_to(&r.encode(), from);
    }
}
