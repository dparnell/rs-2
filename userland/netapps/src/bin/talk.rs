//! TALK: talk to another user (the 4.3BSD "ntalk" protocol).
//!
//! `TALK user[@host] [tty]`: TALK asks the other host's talk daemon
//! (TALKD, UDP 518) whether the other user invited us (LOOK_UP); if so it
//! connects to them. Otherwise it leaves an invitation with this host's
//! daemon and has the other daemon announce the call, again every 30
//! seconds, until the other user answers (`talk us@this-host`). The
//! screen is split: what you type above, what they type below. Each side
//! first sends its erase, kill and word-erase characters (Backspace,
//! Ctrl+U, Ctrl+W here). Ctrl+L redraws, Esc or Ctrl+C ends the
//! conversation. The local user name is %USER% (else "user").
//!
//! With standard input redirected, TALK sends that text and writes what
//! the other side types to standard output, ending a few seconds after
//! the other side stops (for scripts and tests).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::talk::*;
use netapps::*;
use os2::net::*;
use os2::vio::{call16, ptr_words};
use os2::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "VIOCALLS.10"]
    fn VIOWRTCELLSTR();
    #[link_name = "VIOCALLS.15"]
    fn VIOSETCURPOS();
}

const ERASE: u8 = 0x08;
const KILL: u8 = 0x15;
const WERASE: u8 = 0x17;

/// Send a control message to the daemon at `to` and wait for its answer
/// (sent again every 2 seconds, 5 times).
fn transact(ctl: &Sock, to: [u8; 4], port: u16, m: &CtlMsg) -> Option<Response> {
    let data = m.encode();
    let dest = SockAddrIn::new(to, port);
    let mut buf = [0u8; 256];
    for _ in 0..5 {
        let _ = ctl.send_to(&data, dest);
        let start = ms();
        while ms().wrapping_sub(start) < 2000 {
            if !ctl.readable(200) {
                continue;
            }
            let Ok((n, _)) = ctl.recv_from(&mut buf) else { break };
            if let Some(r) = Response::decode(&buf[..n])
                && r.typ == m.typ
            {
                return Some(r);
            }
        }
    }
    None
}

/// This host's address toward `peer`.
fn my_address(peer: [u8; 4]) -> [u8; 4] {
    if peer[0] == 127 {
        return [127, 0, 0, 1];
    }
    if let Ok(u) = Sock::udp() {
        let a = SockAddrIn::new(peer, 518);
        if unsafe { Rs2SockConnect(u.0, &a, 16) } == 0 {
            let l = u.local();
            if l.addr != [0; 4] {
                return l.addr;
            }
        }
    }
    lookup(&hostname()).unwrap_or([127, 0, 0, 1])
}

/// One half of the split screen.
struct Pane {
    top: usize,
    rows: usize,
    row: usize,
    col: usize,
}

struct Ui {
    cols: usize,
    rows: usize,
    cells: Vec<u16>,
    buf: *mut u8,
    me: Pane,
    them: Pane,
    batch: bool,
    out: Vec<u8>,
}

impl Ui {
    fn new(batch: bool) -> Ui {
        let (cols, rows) = os2::vio::vio_get_mode().ok().map_or((80, 25), |m| (m.col as usize, m.row as usize));
        let mut block: *mut u8 = core::ptr::null_mut();
        if !batch {
            unsafe { os2::DosAllocMem(&mut block as *mut *mut u8 as *mut _, 65536, 0x13) };
        }
        let half = (rows - 1) / 2;
        let mut u = Ui {
            cols,
            rows,
            cells: alloc::vec![0x0720; cols * rows],
            buf: block,
            me: Pane { top: 1, rows: half - 1, row: 0, col: 0 },
            them: Pane { top: half + 1, rows: rows - half - 1, row: 0, col: 0 },
            batch,
            out: Vec::new(),
        };
        let line = alloc::vec![0x0fc4u16; cols];
        let r = half;
        u.cells[r * cols..(r + 1) * cols].copy_from_slice(&line);
        u
    }

    fn status(&mut self, s: &str) {
        if self.batch {
            outln(&alloc::format!("[{s}]"));
            return;
        }
        for c in 0..self.cols {
            let ch = s.as_bytes().get(c).copied().unwrap_or(b' ');
            self.cells[c] = 0x7000 | ch as u16;
        }
        self.draw();
    }

    fn draw(&mut self) {
        if self.batch {
            return;
        }
        for r in 0..self.rows {
            let row = &self.cells[r * self.cols..(r + 1) * self.cols];
            let n = row.len() * 2;
            unsafe { core::ptr::copy_nonoverlapping(row.as_ptr() as *const u8, self.buf, n) };
            let p = ptr_words(self.buf);
            unsafe { call16(VIOWRTCELLSTR as *const () as usize, &[p[0], p[1], n as u16, r as u16, 0, 0]) };
        }
        let p = &self.me;
        unsafe { call16(VIOSETCURPOS as *const () as usize, &[(p.top + p.row) as u16, p.col as u16, 0]) };
    }

    /// A character typed by us (`mine`) or them, with the edit characters
    /// `edit` (erase, kill, word erase).
    fn put(&mut self, mine: bool, ch: u8, edit: [u8; 3]) {
        if self.batch {
            if !mine {
                self.out.push(ch);
            }
            return;
        }
        let cols = self.cols;
        let p = if mine { &mut self.me } else { &mut self.them };
        let at = |p: &Pane, c: usize| (p.top + p.row) * cols + c;
        if ch == edit[0] || ch == 0x7f {
            if p.col > 0 {
                p.col -= 1;
                let i = at(p, p.col);
                self.cells[i] = 0x0720;
            }
        } else if ch == edit[1] {
            for c in 0..cols {
                let i = at(p, c);
                self.cells[i] = 0x0720;
            }
            p.col = 0;
        } else if ch == edit[2] {
            while p.col > 0 && self.cells[at(p, p.col - 1)] as u8 == b' ' {
                p.col -= 1;
            }
            while p.col > 0 && self.cells[at(p, p.col - 1)] as u8 != b' ' {
                p.col -= 1;
                let i = at(p, p.col);
                self.cells[i] = 0x0720;
            }
        } else if ch == b'\r' || ch == b'\n' {
            p.col = cols;
        } else if ch == 0x0c {
        } else if ch >= 0x20 || ch == b'\t' {
            if p.col >= cols {
                p.col = cols;
            } else {
                let i = at(p, p.col);
                self.cells[i] = 0x0700 | if ch == b'\t' { b' ' } else { ch } as u16;
                p.col += 1;
                return;
            }
        }
        // A full or ended line: the next one (wrapping to the pane's top,
        // whose line is cleared).
        if p.col >= cols {
            p.row = (p.row + 1) % p.rows;
            p.col = 0;
            for c in 0..cols {
                let i = at(p, c);
                self.cells[i] = 0x0720;
            }
        }
    }
}

fn usage() -> u32 {
    eprint("Usage: talk user[@host] [tty]\r\n");
    1
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let Some(target) = a.first() else { return usage() };
    let (ruser, rhost) = match target.split_once(['@', '!', ':']) {
        Some((u, h)) if target.contains('!') => (String::from(h), String::from(u)),
        Some((u, h)) => (String::from(u), String::from(h)),
        None => (target.clone(), String::from("127.0.0.1")),
    };
    let tty = a.get(1).cloned().unwrap_or_default();
    let me = lpd::user();
    let batch = !stdin_is_console();
    let his = match lookup(&rhost) {
        Ok(h) => h,
        Err(e) => {
            eprint(&alloc::format!("talk: {e}\r\n"));
            return 2;
        }
    };
    let my = my_address(his);
    let daemon = service_port("ntalk", "udp", 518);
    let (Ok(listen), Ok(ctl)) = (Sock::tcp(), Sock::udp()) else { return 2 };
    if listen.bind([0; 4], 0).is_err() || listen.listen(5).is_err() || ctl.bind([0; 4], 0).is_err() {
        eprint("talk: cannot open sockets\r\n");
        return 2;
    }
    let mut ui = Ui::new(batch);
    let mut msg = CtlMsg {
        typ: LOOK_UP,
        id: 0,
        addr: Addr { ip: my, port: listen.local().port() },
        ctl_addr: Addr { ip: my, port: ctl.local().port() },
        pid: unsafe {
            let mut tib = core::ptr::null_mut();
            let mut pib: *mut Pib = core::ptr::null_mut();
            DosGetInfoBlocks(&mut tib, &mut pib);
            (*pib).pid
        },
        l_name: me.clone(),
        r_name: ruser.clone(),
        r_tty: tty,
    };
    ui.status("[Checking for invitation on caller's machine]");
    // Were we invited?
    let mut conn = None;
    if let Some(r) = transact(&ctl, his, daemon, &msg)
        && r.answer == SUCCESS
    {
        ui.status("[Waiting for your party to respond]");
        conn = Sock::connect_addr(r.addr.ip, r.addr.port).ok();
        // The invitation is answered: the inviter deletes it.
        msg.typ = DELETE;
        msg.id = r.id;
        let _ = transact(&ctl, his, daemon, &msg);
    }
    if conn.is_none() {
        // Invite them: leave an invitation here, announce it there.
        msg.typ = LEAVE_INVITE;
        let local = transact(&ctl, my, daemon, &msg);
        let Some(local) = local.filter(|r| r.answer == SUCCESS) else {
            ui.status("[No talk daemon on this machine: start TALKD]");
            return 3;
        };
        let mut remote_id = 0;
        let mut tries = 0;
        loop {
            msg.typ = ANNOUNCE;
            match transact(&ctl, his, daemon, &msg) {
                Some(r) if r.answer == SUCCESS => remote_id = r.id,
                Some(r) => {
                    ui.status(&alloc::format!("[{}]", answer_text(r.answer)));
                    break;
                }
                None => {
                    ui.status("[The other machine's talk daemon does not answer]");
                    break;
                }
            }
            ui.status(if tries == 0 { "[Waiting for your party to respond]" } else { "[Ringing your party again]" });
            if listen.readable(30_000) {
                conn = listen.accept().ok().map(|(c, _)| c);
                break;
            }
            tries += 1;
            if batch && tries >= 3 {
                break;
            }
            // Esc gives up.
            if !batch && key_in(false).is_some_and(|k| k.ch == 0x1b || k.ch == 3) {
                break;
            }
        }
        msg.typ = DELETE;
        msg.id = local.id;
        let _ = transact(&ctl, my, daemon, &msg);
        if remote_id != 0 {
            msg.id = remote_id;
            let _ = transact(&ctl, his, daemon, &msg);
        }
    }
    let Some(c) = conn else {
        ui.status("[No connection]");
        return 4;
    };
    // The edit characters.
    let _ = c.send(&[ERASE, KILL, WERASE]);
    let mut his_edit = [0u8; 3];
    let mut got = 0;
    let mut buf = [0u8; 1024];
    while got < 3 {
        match c.recv(&mut buf[..3 - got]) {
            Ok(n) if n > 0 => {
                his_edit[got..got + n].copy_from_slice(&buf[..n]);
                got += n;
            }
            _ => {
                ui.status("[Connection closed]");
                return 4;
            }
        }
    }
    ui.status("[Connection established]");
    unsafe { DosBeep(880, 80) };
    let my_edit = [ERASE, KILL, WERASE];
    if batch {
        let input = utils::read_all(0);
        let _ = c.send(&input);
        // What they say, until they stop for a few seconds or close.
        loop {
            if !c.readable(4000) {
                break;
            }
            match c.recv(&mut buf) {
                Ok(n) if n > 0 => {
                    for &b in &buf[..n] {
                        ui.put(false, b, his_edit);
                    }
                }
                _ => break,
            }
        }
        os2::write(&ui.out);
        if !ui.out.ends_with(b"\n") {
            outln("");
        }
        outln("[Connection closing. Exiting]");
        return 0;
    }
    loop {
        if c.readable(50) {
            match c.recv(&mut buf) {
                Ok(n) if n > 0 => {
                    for &b in &buf[..n] {
                        ui.put(false, b, his_edit);
                    }
                    ui.draw();
                }
                _ => {
                    ui.status("[Connection closing. Exiting]");
                    break;
                }
            }
        }
        while let Some(k) = key_in(false) {
            let ch = match (k.ch, k.scan) {
                (0x1b, _) | (3, _) => {
                    ui.status("[Connection closing. Exiting]");
                    return 0;
                }
                (0x0c, _) => {
                    ui.draw();
                    continue;
                }
                (0 | 0xe0, _) => continue,
                (ch, _) => ch,
            };
            let _ = c.send(&[if ch == b'\r' { b'\n' } else { ch }]);
            ui.put(true, ch, my_edit);
            ui.draw();
        }
    }
    0
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}
