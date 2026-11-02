//! PSTAT: show processes, threads, system semaphores, shared memory and
//! run-time linked libraries. PSTAT [/C] [/S] [/L] [/M] [/P:pid] [/A]
//!
//! From DosQuerySysState (records as in the Toolkit's bsedos.h): QSPTRREC,
//! process records (60 bytes) each followed by its arrays and its thread
//! records (28 bytes), linked module, 16-bit semaphore and shared memory
//! records. The tables are laid out as IBM's PSTAT lays them out. Without
//! options everything is shown, a screen at a time on the console.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::APIRET;
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.368"]
    fn DosQuerySysState(list: u32, level: u32, pid: u32, tid: u32, buf: *mut u8, cb: u32) -> APIRET;
}

const QS_PROCESS: u32 = 0x0001;
const QS_SEMAPHORE: u32 = 0x0002;
const QS_MTE: u32 = 0x0004;
const QS_SHMEMORY: u32 = 0x0010;

struct Snap {
    buf: Vec<u8>,
    base: usize,
}

impl Snap {
    fn u8(&self, a: u32) -> u8 {
        self.buf.get(a as usize - self.base).copied().unwrap_or(0)
    }
    fn u16(&self, a: u32) -> u16 {
        u16::from_le_bytes([self.u8(a), self.u8(a + 1)])
    }
    fn u32(&self, a: u32) -> u32 {
        u32::from_le_bytes([self.u8(a), self.u8(a + 1), self.u8(a + 2), self.u8(a + 3)])
    }
    fn str(&self, a: u32) -> String {
        let mut s = String::new();
        let mut p = a;
        while a != 0 && self.u8(p) != 0 && s.len() < 260 {
            s.push(self.u8(p) as char);
            p += 1;
        }
        s
    }
    fn ok(&self, a: u32) -> bool {
        a as usize >= self.base && (a as usize) < self.base + self.buf.len()
    }
}

struct Thread {
    tid: u16,
    sleep: u32,
    prio: u32,
    state: u8,
}

struct Proc {
    pid: u16,
    ppid: u16,
    sgid: u32,
    hmte: u16,
    threads: Vec<Thread>,
    libs: Vec<u16>,
}

fn snapshot(list: u32) -> Option<Snap> {
    let mut size = 64 * 1024;
    loop {
        let mut buf = alloc::vec![0u8; size];
        let rc = unsafe { DosQuerySysState(list, 0, 0, 0, buf.as_mut_ptr(), size as u32) };
        if rc == 0 {
            let base = buf.as_ptr() as usize;
            return Some(Snap { buf, base });
        }
        if rc != 111 || size >= 4 * 1024 * 1024 {
            error(rc);
            return None;
        }
        size *= 2;
    }
}

fn procs(s: &Snap) -> Vec<Proc> {
    let mut v = Vec::new();
    let mut p = s.u32(s.base as u32 + 4);
    while p != 0 && s.ok(p) && s.u32(p) == QS_PROCESS {
        let n_thr = s.u16(p + 26) as u32;
        let thr = s.u32(p + 4);
        let threads = (0..n_thr)
            .map(|i| {
                let t = thr + i * 28;
                Thread { tid: s.u16(t + 4), sleep: s.u32(t + 8), prio: s.u32(t + 12), state: s.u8(t + 24) }
            })
            .collect();
        let arr = |count: u32, at: u32| -> Vec<u16> { if at == 0 { Vec::new() } else { (0..count).map(|i| s.u16(at + 2 * i)).collect() } };
        v.push(Proc {
            pid: s.u16(p + 8),
            ppid: s.u16(p + 10),
            sgid: s.u32(p + 20),
            hmte: s.u16(p + 24),
            threads,
            libs: arr(s.u16(p + 38) as u32, s.u32(p + 48)),
        });
        // The next process record follows the thread records.
        p = thr + n_thr * 28;
    }
    v
}

/// Module handle -> (name, imported module handles).
fn modules(s: &Snap) -> Vec<(u16, String, Vec<u16>)> {
    let mut v = Vec::new();
    let mut m = s.u32(s.base as u32 + 20);
    while m != 0 && s.ok(m) && v.len() < 4096 {
        let n = s.u32(m + 8);
        let imports = (0..n.min(512)).map(|i| s.u16(m + 24 + 2 * i)).collect();
        v.push((s.u16(m + 4), s.str(s.u32(m + 20)), imports));
        m = s.u32(m);
    }
    v
}

fn state(st: u8) -> &'static str {
    match st {
        1 => "Ready",
        2 => "Block",
        3 => "Frozen",
        4 => "Crit",
        5 => "Running",
        _ => "Ready",
    }
}

fn mod_name(mods: &[(u16, String, Vec<u16>)], h: u16) -> String {
    mods.iter().find(|m| m.0 == h).map_or_else(|| String::from("(kernel)"), |m| m.1.clone())
}

fn base(name: &str) -> &str {
    let b = name.rsplit(['\\', '/']).next().unwrap_or(name);
    b.split('.').next().unwrap_or(b)
}

fn processes(ps: &[Proc], mods: &[(u16, String, Vec<u16>)]) {
    outln("\r\n                    Process and Thread Information\r\n");
    outln("          Parent");
    outln("Process   Process   Session   Process   Thread");
    outln("  ID        ID        ID       Name       ID    Priority   Block ID   State\r\n");
    for p in ps {
        let name = mod_name(mods, p.hmte);
        for (i, t) in p.threads.iter().enumerate() {
            let head = if i == 0 {
                alloc::format!(" {:04X}      {:04X}       {:02X}      {name}", p.pid, p.ppid, p.sgid)
            } else {
                alloc::format!("{:38}", "")
            };
            outln(&alloc::format!("{head}    {:02X}      {:04X}     {:08X}  {:>6}", t.tid, t.prio, t.sleep, state(t.state)));
        }
    }
}

fn libraries(ps: &[Proc], mods: &[(u16, String, Vec<u16>)]) {
    outln("\r\n                       Run-Time Link Libraries\r\n");
    outln(" Process   Process   Session");
    outln("  Name       ID        ID      Library List\r\n");
    for p in ps {
        let name = mod_name(mods, p.hmte);
        // The program's own imports first, then the libraries loaded at
        // run time, each once.
        let mut seen: Vec<u16> = Vec::new();
        let own = mods.iter().find(|m| m.0 == p.hmte).map(|m| m.2.clone()).unwrap_or_default();
        for h in own.iter().chain(p.libs.iter()) {
            if *h != p.hmte && !seen.contains(h) {
                seen.push(*h);
            }
        }
        let names: Vec<String> = seen.iter().map(|h| String::from(base(&mod_name(mods, *h)))).collect();
        let first = names.first().cloned().unwrap_or_default();
        outln(&alloc::format!(" {name}   {:04X}       {:02X}      {first}", p.pid, p.sgid));
        for n in names.iter().skip(1) {
            outln(&alloc::format!("{:31}{n}", ""));
        }
    }
}

fn semaphores(s: &Snap, ps: &[Proc], mods: &[(u16, String, Vec<u16>)]) {
    outln("\r\n                    System Semaphore Information\r\n");
    outln("   Owning Process             No. of     No. of          Semaphore");
    outln("(Name  ID  Session)  Index  References  Requests  Flag     Name\r\n");
    // 32-bit shared semaphores with names (QSS32REC: next, flags, name,
    // ..., handle at +24, number of openers at +26).
    let mut r = s.u32(s.base as u32 + 12);
    let mut n = 0;
    while r != 0 && s.ok(r) && n < 4096 {
        let name = s.str(s.u32(r + 8));
        if !name.is_empty() {
            outln(&alloc::format!("{:<22}{:04X}      {:02X}         {:02X}     {:04X}   {}", "", s.u16(r + 24), s.u16(r + 26), 0, s.u16(r + 4), name));
        }
        r = s.u32(r);
        n += 1;
    }
    let head = s.u32(s.base as u32 + 8);
    if head == 0 {
        return;
    }
    let mut r = s.u32(head + 4);
    let mut n = 0;
    while r != 0 && s.ok(r) && n < 4096 {
        let owner = s.u16(r + 4);
        let flags = s.u8(r + 6);
        let refs = s.u8(r + 7);
        let reqs = s.u8(r + 8);
        let index = s.u16(r + 12);
        let name = s.str(r + 14);
        let who = ps.iter().find(|p| p.pid == owner && owner != 0).map(|p| alloc::format!("{} {:04X} {:02X}", base(&mod_name(mods, p.hmte)), p.pid, p.sgid));
        outln(&alloc::format!("{:<22}{:04X}      {:02X}         {:02X}     {:04X}   \\SEM\\{}", who.unwrap_or_default(), index, refs, reqs, flags, name.trim_start_matches("\\SEM\\").trim_start_matches('\\')));
        r = s.u32(r);
        n += 1;
    }
}

fn shared_memory(s: &Snap) {
    outln("\r\n          Named Shared Memory Information\r\n");
    outln("                            No. of");
    outln("  Handle     Selector     References     Shared Memory Name\r\n");
    let mut r = s.u32(s.base as u32 + 16);
    let mut n = 0;
    while r != 0 && s.ok(r) && n < 4096 {
        outln(&alloc::format!("   {:04X}       {:04X}          {:04X}         {}", s.u16(r + 4), s.u16(r + 6), s.u16(r + 8), s.str(r + 10)));
        r = s.u32(r);
        n += 1;
    }
}

fn usage() {
    outln("\r\nShows processes, threads, system semaphores, shared memory and dynamic link");
    outln("libraries.");
    outln("Syntax:");
    outln("    PSTAT [/C] [/S] [/L] [/M] [/P:pid] [/A]");
    outln("where:");
    outln(" /C     - processes and their threads");
    outln(" /S     - system semaphores");
    outln(" /L     - run-time link libraries of each process");
    outln(" /M     - named shared memory");
    outln(" /P:pid - one process (hexadecimal process ID)");
    outln(" /A     - all of the above\r\n");
}

fn main(st: &os2::Startup) -> u32 {
    let (mut c, mut sem, mut lib, mut mem) = (false, false, false, false);
    let mut only: Option<u16> = None;
    let a = args(st);
    for x in &a {
        let u = x.to_ascii_uppercase();
        match u.as_str() {
            "/C" => c = true,
            "/S" => sem = true,
            "/L" => lib = true,
            "/M" => mem = true,
            "/A" => (c, sem, lib, mem) = (true, true, true, true),
            "/F" => lib = true,
            _ if u.starts_with("/P:") => match u16::from_str_radix(&u[3..], 16) {
                Ok(p) => {
                    only = Some(p);
                    c = true;
                }
                Err(_) => {
                    usage();
                    return 1;
                }
            },
            _ => {
                usage();
                return 1;
            }
        }
    }
    if a.is_empty() {
        (c, sem, lib, mem) = (true, true, true, true);
    }
    let Some(s) = snapshot(QS_PROCESS | QS_SEMAPHORE | QS_MTE | QS_SHMEMORY) else { return 1 };
    let mut ps = procs(&s);
    if let Some(p) = only {
        ps.retain(|x| x.pid == p);
    }
    let mods = modules(&s);
    if c {
        processes(&ps, &mods);
    }
    if sem {
        semaphores(&s, &ps, &mods);
    }
    if mem {
        shared_memory(&s);
    }
    if lib {
        libraries(&ps, &mods);
    }
    0
}
