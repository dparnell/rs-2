//! Print jobs: `<spool dir>\<queue>\nnnnn.SPL` (the data) and
//! `nnnnn.SHD` (the shadow: the job's description), as IBM's spooler
//! keeps them. The shadow is rs2's own format: "RS2SHD1\0", the numbers,
//! then length-prefixed strings and the DRIVDATA.
//!
//! Every process works on the files directly under one named mutex
//! (`\SEM32\PMSPL`); a small named shared memory block keeps the next job
//! id and the job the spooler should abort. The spooler waits on the
//! event semaphore `\SEM32\PMSPL\WAKE`, which it owns: a job that becomes
//! ready posts it, and when it cannot be opened no spooler is running.

use alloc::vec::Vec;

use crate::ini;
use crate::util::{cat, dec, delete, file_size, find, mkdir, read_file, upper, write_file};

unsafe extern "C" {
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
}

const MUTEX: &[u8] = b"\\SEM32\\PMSPL\0";
pub const WAKE: &[u8] = b"\\SEM32\\PMSPL\\WAKE\0";
const SHMEM: &[u8] = b"\\SHAREMEM\\PMSPL\0";
const MAGIC: &[u8; 8] = b"RS2SHD1\0";

// Job status (PRJINFO fsStatus).
pub const PRJ_QS_QUEUED: u16 = 0;
pub const PRJ_QS_PAUSED: u16 = 1;
pub const PRJ_QS_SPOOLING: u16 = 2;
pub const PRJ_QS_PRINTING: u16 = 3;
pub const PRJ_ERROR: u16 = 0x10;
pub const PRJ_DELETED: u16 = 0x8000;

#[repr(C)]
struct Shared {
    magic: u32,
    next_id: u32,
    /// The job being printed that should stop (0 none).
    abort: u32,
}

static mut HMTX: u32 = 0;
static mut SHARED: *mut Shared = core::ptr::null_mut();

fn shared() -> &'static mut Shared {
    unsafe {
        if SHARED.is_null() {
            let mut p: *mut u8 = core::ptr::null_mut();
            let mut rc = os2::DosGetNamedSharedMem(&mut p, SHMEM.as_ptr(), os2::PAG_READ | os2::PAG_WRITE);
            if rc != 0 {
                rc = os2::DosAllocSharedMem(&mut p, SHMEM.as_ptr(), 4096, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT);
                if rc != 0 {
                    os2::DosGetNamedSharedMem(&mut p, SHMEM.as_ptr(), os2::PAG_READ | os2::PAG_WRITE);
                }
            }
            SHARED = p as *mut Shared;
            let s = &mut *SHARED;
            if s.magic != 0x4c50_5352 {
                s.magic = 0x4c50_5352;
                s.next_id = 0;
                s.abort = 0;
            }
        }
        &mut *SHARED
    }
}

/// Run `f` with the spooler's lock held (the configuration set up first).
pub fn locked<R>(f: impl FnOnce() -> R) -> R {
    let m = unsafe {
        if HMTX == 0 {
            let mut h = 0;
            if DosOpenMutexSem(MUTEX.as_ptr(), &mut h) == 0 || os2::DosCreateMutexSem(MUTEX.as_ptr(), &mut h, 0, 0) == 0 || DosOpenMutexSem(MUTEX.as_ptr(), &mut h) == 0 {
                HMTX = h;
            }
        }
        HMTX
    };
    let rc = unsafe { os2::DosRequestMutexSem(m, os2::SEM_INDEFINITE_WAIT) };
    ini::ensure_defaults();
    let r = f();
    if rc == 0 || rc == 105 {
        unsafe { os2::DosReleaseMutexSem(m) };
    }
    r
}

/// Wake the spooler; false if none is running.
pub fn wake() -> bool {
    let mut h = 0;
    if unsafe { DosOpenEventSem(WAKE.as_ptr(), &mut h) } != 0 {
        return false;
    }
    unsafe {
        os2::DosPostEventSem(h);
        os2::DosCloseEventSem(h);
    }
    true
}

pub fn set_abort(id: u32) {
    shared().abort = id;
}

pub fn abort_requested(id: u32) -> bool {
    shared().abort == id && id != 0
}

#[derive(Clone, Default)]
pub struct Job {
    pub id: u32,
    pub queue: Vec<u8>,
    pub priority: u16,
    /// Submission order (positions follow it).
    pub seq: u32,
    pub status: u16,
    pub held: bool,
    pub submitted: u32,
    pub size: u32,
    pub pages: u32,
    pub user: Vec<u8>,
    pub notify: Vec<u8>,
    pub datatype: Vec<u8>,
    pub comment: Vec<u8>,
    pub document: Vec<u8>,
    pub qproc: Vec<u8>,
    pub qproc_params: Vec<u8>,
    pub spl_params: Vec<u8>,
    pub net_params: Vec<u8>,
    pub driver: Vec<u8>,
    pub logaddr: Vec<u8>,
    /// The printer printing it (while printing).
    pub printer: Vec<u8>,
    pub status_text: Vec<u8>,
    pub token: Vec<u8>,
    pub drivdata: Vec<u8>,
}

pub fn queue_dir(q: &[u8]) -> Vec<u8> {
    cat(&[&ini::spool_dir(), b"\\", &upper(q)])
}

fn base(q: &[u8], id: u32) -> Vec<u8> {
    cat(&[&queue_dir(q), b"\\", &dec(id, 5)])
}

impl Job {
    pub fn spl_path(&self) -> Vec<u8> {
        cat(&[&base(&self.queue, self.id), b".SPL"])
    }

    fn shd_path(&self) -> Vec<u8> {
        cat(&[&base(&self.queue, self.id), b".SHD"])
    }

    pub fn qstatus(&self) -> u16 {
        self.status & 3
    }

    /// PRJINFO fsStatus: held jobs that are not printing report PAUSED.
    pub fn fs_status(&self) -> u16 {
        let s = if self.held && self.qstatus() == PRJ_QS_QUEUED { PRJ_QS_PAUSED } else { self.qstatus() };
        (self.status & !3) | s
    }

    pub fn save(&self) -> bool {
        let mut d = Vec::new();
        d.extend_from_slice(MAGIC);
        for v in [self.id, self.priority as u32, self.seq, self.status as u32, self.held as u32, self.submitted, self.size, self.pages] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        for s in [&self.queue, &self.user, &self.notify, &self.datatype, &self.comment, &self.document, &self.qproc, &self.qproc_params, &self.spl_params, &self.net_params, &self.driver, &self.logaddr, &self.printer, &self.status_text, &self.token, &self.drivdata] {
            d.extend_from_slice(&(s.len() as u32).to_le_bytes());
            d.extend_from_slice(s);
        }
        mkdir(&ini::spool_dir());
        mkdir(&queue_dir(&self.queue));
        write_file(&self.shd_path(), &d)
    }

    fn parse(d: &[u8]) -> Option<Job> {
        if d.len() < 8 + 32 || &d[..8] != MAGIC {
            return None;
        }
        let mut at = 8;
        let mut num = || {
            let v = u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?);
            at += 4;
            Some(v)
        };
        let mut j = Job { id: num()?, priority: num()? as u16, seq: num()?, status: num()? as u16, held: num()? != 0, submitted: num()?, size: num()?, pages: num()?, ..Job::default() };
        let mut s = || {
            let n = u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize;
            let v = d.get(at + 4..at + 4 + n)?.to_vec();
            at += 4 + n;
            Some(v)
        };
        j.queue = s()?;
        j.user = s()?;
        j.notify = s()?;
        j.datatype = s()?;
        j.comment = s()?;
        j.document = s()?;
        j.qproc = s()?;
        j.qproc_params = s()?;
        j.spl_params = s()?;
        j.net_params = s()?;
        j.driver = s()?;
        j.logaddr = s()?;
        j.printer = s()?;
        j.status_text = s()?;
        j.token = s()?;
        j.drivdata = s()?;
        Some(j)
    }

    /// Delete the job's files.
    pub fn destroy(&self) {
        delete(&self.spl_path());
        delete(&self.shd_path());
    }
}

/// The jobs of a queue, in print order (higher priority first, then
/// submission).
pub fn queue_jobs(q: &[u8]) -> Vec<Job> {
    let dir = queue_dir(q);
    let mut v: Vec<Job> = find(&cat(&[&dir, b"\\*.SHD"]), false)
        .iter()
        .filter_map(|n| read_file(&cat(&[&dir, b"\\", n])))
        .filter_map(|d| Job::parse(&d))
        .collect();
    v.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.seq.cmp(&b.seq)));
    v
}

pub fn all_jobs() -> Vec<Job> {
    let mut v = Vec::new();
    for q in ini::queues() {
        v.extend(queue_jobs(&q.name));
    }
    v
}

pub fn find_job(id: u32) -> Option<Job> {
    all_jobs().into_iter().find(|j| j.id == id)
}

/// A job of the queue `q` (any queue when `q` is empty).
pub fn find_in(q: &[u8], id: u32) -> Option<Job> {
    if q.is_empty() {
        return find_job(id);
    }
    queue_jobs(q).into_iter().find(|j| j.id == id)
}

/// The next free job id (1..=9999, as five-digit names allow 99999 but
/// PRJINFO keeps ids in a USHORT).
pub fn next_id() -> u32 {
    let used: Vec<u32> = all_jobs().iter().map(|j| j.id).collect();
    let s = shared();
    let mut id = s.next_id;
    for _ in 0..10000 {
        id = if id >= 9999 { 1 } else { id + 1 };
        if !used.contains(&id) {
            s.next_id = id;
            return id;
        }
    }
    0
}

pub fn next_seq() -> u32 {
    all_jobs().iter().map(|j| j.seq).max().unwrap_or(0) + 1
}

/// The spool file's size now.
pub fn spl_size(j: &Job) -> u32 {
    file_size(&j.spl_path()).unwrap_or(0)
}

/// Position (1-based) of a job in its queue.
pub fn position(j: &Job) -> u16 {
    queue_jobs(&j.queue).iter().position(|x| x.id == j.id).map_or(0, |p| p as u16 + 1)
}

/// Move a job to `pos` (1-based) in its queue by renumbering.
pub fn set_position(j: &Job, pos: u16) {
    let mut jobs = queue_jobs(&j.queue);
    let Some(at) = jobs.iter().position(|x| x.id == j.id) else { return };
    let me = jobs.remove(at);
    let to = (pos.max(1) as usize - 1).min(jobs.len());
    jobs.insert(to, me);
    let base = jobs.iter().map(|x| x.seq).min().unwrap_or(1);
    for (i, x) in jobs.iter_mut().enumerate() {
        x.seq = base + i as u32;
        // Positions only order jobs of one priority; moving a job gives it
        // its neighbours' priority.
        x.save();
    }
}

/// Jobs left PRINTING by a spooler that stopped are queued again.
pub fn recover() {
    for mut j in all_jobs() {
        if j.qstatus() == PRJ_QS_PRINTING {
            j.status &= !3;
            j.printer.clear();
            j.save();
        }
    }
}
