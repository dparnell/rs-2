//! DosQuerySysState: processes, threads, modules, semaphores and open files
//! as used by PSTAT, PSFILES, PSSEMS and friends.
//!
//! Layout as in the Toolkit's bsedos.h (QSPTRREC first; each process record
//! is followed by its arrays, then its thread records, then the next
//! process record; all pointers are flat addresses in the caller's buffer).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::fs::Handle;
use crate::task::thread::State;

const QS_PROCESS: u32 = 0x0001;
const QS_SEMAPHORE: u32 = 0x0002;
const QS_MTE: u32 = 0x0004;
const QS_FILESYS: u32 = 0x0008;
const QS_SHMEMORY: u32 = 0x0010;
const QS_THREAD: u32 = 0x0100;

const PREC_SIZE: usize = 60;
const TREC_SIZE: usize = 28;

struct Buf {
    b: Vec<u8>,
    base: u32,
}

impl Buf {
    fn at(&self) -> usize {
        self.b.len()
    }
    fn ptr(&self, off: usize) -> u32 {
        self.base + off as u32
    }
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn zeros(&mut self, n: usize) {
        self.b.resize(self.b.len() + n, 0);
    }
    fn str(&mut self, s: &str) {
        self.b.extend_from_slice(s.as_bytes());
        self.b.push(0);
    }
    fn align(&mut self) {
        while self.b.len() % 4 != 0 {
            self.b.push(0);
        }
    }
    fn put16(&mut self, off: usize, v: u16) {
        self.b[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(&mut self, off: usize, v: u32) {
        self.b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// An open handle in the system file table.
struct Sft {
    sfn: u16,
    name: String,
    size: u32,
    mode: u16,
    attr: u16,
}

fn handle_name(h: &Handle) -> (String, u32, u16, u16) {
    match h {
        Handle::File(f) => {
            let info = f.file.info();
            (f.path.display(), f.file.size() as u32, f.mode.load(core::sync::atomic::Ordering::Relaxed) as u16, info.attr as u16)
        }
        Handle::Con => (String::from("\\DEV\\CON"), 0, 0x42, 0),
        Handle::Nul => (String::from("\\DEV\\NUL"), 0, 0x42, 0),
        Handle::Kbd => (String::from("\\DEV\\KBD$"), 0, 0x40, 0),
        Handle::Screen => (String::from("\\DEV\\SCREEN$"), 0, 0x41, 0),
        Handle::Clock => (String::from("\\DEV\\CLOCK$"), 0, 0x40, 0),
        Handle::Pipe(_) => (String::from("\\PIPE"), 0, 0x42, 0),
        Handle::NPipe(_) => (String::from("\\PIPE\\"), 0, 0x42, 0),
        Handle::PhysDisk(n) => (alloc::format!("\\DEV\\DISK{n}"), 0, 0x42, 0),
        Handle::DosDev => (String::from("\\DEV\\DOS$"), 0, 0x42, 0),
        Handle::Drive(_) => (String::from("DASD"), 0, 0x8042, 0),
    }
}

/// DosQuerySysState(EntityList, EntityLevel, pid, tid, pDataBuf, cbBuf)
pub fn dos_query_sys_state(c: &mut Ctx) -> u32 {
    let (list, level, want_pid, buf, cb) = (c.arg(0), c.arg(1), c.arg(2), c.arg(4), c.arg(5));
    if buf == 0 {
        return err::INVALID_PARAMETER;
    }
    let mut procs: Vec<Arc<crate::task::Process>> = crate::task::process::all()
        .into_iter()
        .filter(|p| p.exit_info().is_none() && (want_pid == 0 || p.pid == want_pid))
        .collect();
    procs.sort_by_key(|p| p.pid);

    let mut o = Buf { b: Vec::new(), base: buf };
    o.zeros(32); // QSPTRREC
    let global = o.at();
    o.zeros(12);

    let sems32 = super::sem::snapshot32();
    let sems16 = super::sem::snapshot16();
    // Shared memory objects with the processes that have them mapped.
    let all_procs = crate::task::process::all();
    let shms: Vec<(Arc<crate::vm::SharedMem>, Vec<u32>)> = super::shmem::objects()
        .into_iter()
        .filter(|m| !m.device)
        .map(|m| {
            let users = all_procs.iter().filter(|p| p.space.lock().regions.contains_key(&m.base)).map(|p| p.pid).collect();
            (m, users)
        })
        .collect();

    // 16-bit system semaphores: QSS16HEADREC, then a QSS16REC each.
    let sem16 = o.at();
    o.u32(QS_SEMAPHORE);
    o.zeros(16);
    if list & QS_SEMAPHORE != 0 && !sems16.is_empty() {
        let first = o.ptr(o.at());
        o.put32(sem16 + 4, first);
        let n = sems16.len();
        for (i, (name, owner, flags, refs, cnt, _)) in sems16.iter().enumerate() {
            let rec = o.at();
            o.u32(0);
            o.u16(*owner);
            o.u8(*flags);
            o.u8(*refs);
            o.u8(*cnt);
            o.u8(0);
            o.u16(0);
            o.u16(i as u16 + 1);
            o.str(name);
            o.align();
            if i + 1 < n {
                let next = o.ptr(o.at());
                o.put32(rec, next);
            }
        }
    }

    // 32-bit semaphores: a QSS32REC each, linked, name after the record.
    let mut sem32_head = 0u32;
    if list & QS_SEMAPHORE != 0 {
        let mut prev: Option<usize> = None;
        for s in &sems32 {
            let rec = o.at();
            match prev {
                Some(pv) => {
                    let p = o.ptr(rec);
                    o.put32(pv, p);
                }
                None => sem32_head = o.ptr(rec),
            }
            prev = Some(rec);
            o.u32(0); // pNextRec
            o.u16(s.flags);
            o.u16(0);
            o.u32(0); // pName
            o.u32(0); // QSHUN: pMuxQ / pSemRec
            o.u16(s.count);
            o.u16(s.extra);
            o.u32(0); // blockid
            o.u16(s.handle as u16);
            o.u16(s.opens.len() as u16);
            for (pid, n) in &s.opens {
                o.u16(*pid as u16);
                o.u16(*n as u16);
            }
            if let Some(n) = &s.name {
                let at = o.ptr(o.at());
                o.put32(rec + 8, at);
                o.str(n);
            }
            o.align();
        }
    }

    // Shared memory: a QSMREC each (next, handle, selector, references,
    // name).
    let mut shm_head = 0u32;
    if list & QS_SHMEMORY != 0 {
        let mut prev: Option<usize> = None;
        for (i, (m, users)) in shms.iter().enumerate() {
            let rec = o.at();
            match prev {
                Some(pv) => {
                    let p = o.ptr(rec);
                    o.put32(pv, p);
                }
                None => shm_head = o.ptr(rec),
            }
            prev = Some(rec);
            o.u32(0);
            o.u16(i as u16 + 1);
            o.u16(crate::vm::tile_selector(m.base));
            o.u16(users.len() as u16);
            o.str(m.name.as_deref().unwrap_or(""));
            o.align();
        }
    }
    let empty = o.at();
    o.u32(0);

    let mut sfts: Vec<Sft> = Vec::new();
    let mut threads_total = 0u32;
    let proc_head = o.at();
    if list & QS_PROCESS != 0 {
        for p in &procs {
            let rec = o.at();
            o.zeros(PREC_SIZE);
            // Runtime-linked libraries.
            let libs: Vec<u16> = p.loaded.lock().iter().map(|(m, _)| m.hmte).collect();
            let lib_at = o.at();
            for h in &libs {
                o.u16(*h);
            }
            o.align();
            // Private 32-bit semaphores, 16-bit system semaphores open and
            // shared memory mapped by the process.
            let pid = p.pid;
            let private: Vec<u16> = sems32
                .iter()
                .filter(|s| s.flags & 1 == 0 && s.opens.iter().any(|o| o.0 == pid))
                .map(|s| s.handle as u16)
                .collect();
            let sem32_at = o.at();
            for h in &private {
                o.u16(*h);
            }
            o.align();
            let s16: Vec<u16> = sems16
                .iter()
                .enumerate()
                .filter(|(_, s)| s.5.contains(&pid))
                .map(|(i, _)| i as u16 + 1)
                .collect();
            let sem16_at = o.at();
            for h in &s16 {
                o.u16(*h);
            }
            o.align();
            let mine: Vec<u16> =
                shms.iter().enumerate().filter(|(_, (_, u))| u.contains(&pid)).map(|(i, _)| i as u16 + 1).collect();
            let shm_at = o.at();
            for h in &mine {
                o.u16(*h);
            }
            o.align();
            // Handle table: SFN per open handle, 0xFFFF if closed.
            let hs: Vec<Option<(String, u32, u16, u16)>> =
                p.handles.lock().iter().map(|h| h.as_ref().map(handle_name)).collect();
            let fs_at = o.at();
            for h in &hs {
                match h {
                    Some((name, size, mode, attr)) => {
                        let sfn = sfts.len() as u16 + 1;
                        sfts.push(Sft { sfn, name: name.clone(), size: *size, mode: *mode, attr: *attr });
                        o.u16(sfn);
                    }
                    None => o.u16(0xffff),
                }
            }
            o.align();
            let threads: Vec<_> =
                p.threads.lock().iter().filter(|t| t.state() != State::Dead).cloned().collect();
            let thr_at = o.at();
            for t in &threads {
                o.u32(QS_THREAD);
                o.u16(t.tid as u16);
                o.u16(t.id as u16);
                o.u32(0); // sleep id
                o.u32(t.priority());
                o.u32(0);
                o.u32(0);
                o.u8(match t.state() {
                    State::Ready => 1,
                    State::Blocked => 2,
                    State::Suspended => 3,
                    State::Running => 5,
                    State::Dead => 0,
                });
                o.u8(0);
                o.u16(0);
            }
            threads_total += threads.len() as u32;
            let ptype = if p.detached.load(core::sync::atomic::Ordering::Relaxed) {
                4
            } else {
                match p.exe.app_type & 7 {
                    3 => 3, // PM
                    2 => 2, // windowable VIO
                    _ => 0, // full screen
                }
            };
            o.put32(rec, QS_PROCESS);
            o.put32(rec + 4, o.ptr(thr_at));
            o.put16(rec + 8, p.pid as u16);
            o.put16(rec + 10, p.ppid as u16);
            o.put32(rec + 12, ptype);
            o.put32(rec + 16, 0); // status
            o.put32(rec + 20, 1); // screen group
            o.put16(rec + 24, p.exe.hmte);
            o.put16(rec + 26, threads.len() as u16);
            o.put32(rec + 28, private.len() as u32); // c32PSem
            o.put32(rec + 32, if private.is_empty() { 0 } else { o.ptr(sem32_at) }); // p32SemRec
            o.put16(rec + 36, s16.len() as u16); // c16Sem
            o.put16(rec + 38, libs.len() as u16);
            o.put16(rec + 40, mine.len() as u16); // cShrMem
            o.put16(rec + 42, hs.len() as u16);
            o.put32(rec + 44, if s16.is_empty() { o.ptr(empty) } else { o.ptr(sem16_at) });
            o.put32(rec + 48, o.ptr(lib_at));
            o.put32(rec + 52, if mine.is_empty() { o.ptr(empty) } else { o.ptr(shm_at) });
            o.put32(rec + 56, o.ptr(fs_at));
        }
        // End of the process records.
        o.u32(0);
        o.zeros(PREC_SIZE - 4);
    }

    // Modules.
    let mut lib_head = 0usize;
    if list & QS_MTE != 0 {
        let mods = crate::loader::all_modules();
        let mut prev: Option<usize> = None;
        for m in &mods {
            let rec = o.at();
            if let Some(pv) = prev {
                let p = o.ptr(rec);
                o.put32(pv, p);
            } else {
                lib_head = rec;
            }
            prev = Some(rec);
            o.zeros(24);
            for i in &m.imports {
                o.u16(i.hmte);
            }
            o.align();
            let name_at = o.at();
            o.str(if m.path.starts_with("<kernel>") { &m.name } else { &m.path });
            o.align();
            let obj_at = o.at();
            let with_objs = level & QS_MTE != 0;
            if with_objs {
                for ob in &m.objects {
                    o.u32(ob.base);
                    o.u32(ob.size);
                    o.u32(ob.prot);
                }
            }
            o.put16(rec + 4, m.hmte);
            o.put16(rec + 6, (!m.is16) as u16);
            o.put32(rec + 8, m.imports.len() as u32);
            o.put32(rec + 12, m.objects.len() as u32);
            o.put32(rec + 16, if with_objs && !m.objects.is_empty() { o.ptr(obj_at) } else { 0 });
            o.put32(rec + 20, o.ptr(name_at));
        }
    }

    // System file table.
    let fs_head = o.at();
    if list & QS_FILESYS != 0 {
        let n = sfts.len();
        for (i, s) in sfts.iter().enumerate() {
            let rec = o.at();
            o.u32(QS_FILESYS);
            o.zeros(12);
            o.str(&s.name);
            o.align();
            let sft = o.at();
            o.u16(s.sfn);
            o.u16(1);
            o.u16(0);
            o.u16(0);
            o.u16(s.mode);
            o.u16(0);
            o.u32(s.size);
            o.u16(0);
            o.u16(s.attr);
            o.u16(0);
            o.align();
            let next = if i + 1 < n { o.ptr(o.at()) } else { 0 };
            o.put32(rec + 4, next);
            o.put32(rec + 8, 1);
            o.put32(rec + 12, o.ptr(sft));
        }
        if n == 0 {
            o.u32(0); // QS_END
            o.zeros(12);
        }
    }

    o.put32(global, threads_total);
    o.put32(global + 8, sfts.len() as u32);
    o.put32(0, o.ptr(global));
    o.put32(4, if list & QS_PROCESS != 0 { o.ptr(proc_head) } else { 0 });
    o.put32(8, if list & QS_SEMAPHORE != 0 { o.ptr(sem16) } else { 0 });
    o.put32(12, sem32_head);
    o.put32(16, shm_head);
    o.put32(20, if list & QS_MTE != 0 && lib_head != 0 { o.ptr(lib_head) } else { 0 });
    o.put32(24, shm_head);
    o.put32(28, if list & QS_FILESYS != 0 { o.ptr(fs_head) } else { 0 });

    if o.b.len() > cb as usize {
        return err::BUFFER_OVERFLOW;
    }
    if c.write(buf, &o.b) { 0 } else { err::INVALID_ADDRESS }
}
