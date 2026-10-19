//! Profiles on disk, shared by every process.
//!
//! * One named mutex semaphore (`\SEM32\PMSHAPI\PRF`) serialises all
//!   profile access in the system; every Prf call holds it from start to
//!   end, inside a must-complete section, so a killed process finishes
//!   its update first.
//! * A small named shared memory object (`\SHAREMEM\PMSHAPI\PRF`) holds
//!   the current user and system profile names (so PrfReset is
//!   system-wide) and 256 generation counters, one per hash of a file's
//!   full path. A write bumps the counter of its file.
//! * Each process keeps the parsed profiles it uses. Before every use the
//!   cached copy is checked against the file's generation counter and its
//!   size and time stamp (which catches files replaced by other means,
//!   such as COPY) and re-read when either changed.
//! * Every write goes to disk at once, under the mutex, so the next
//!   reader in any process sees it and nothing is lost if a process dies.
//!   The new contents go to a temporary file next to the profile (the
//!   name with the extension `!!!`, as IBM's PMSHAPI names its
//!   temporary files) which is flushed, then the profile is deleted and
//!   the temporary file renamed (DosMove does not replace files). If the
//!   profile is missing but the temporary file is there (a crash between
//!   the two), the temporary file becomes the profile.

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use inifile::Profile;

pub const HINI_PROFILE: u32 = 0;
pub const HINI_USERPROFILE: u32 = 0xffff_ffff;
pub const HINI_SYSTEMPROFILE: u32 = 0xffff_fffe;

unsafe extern "C" {
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
    #[link_name = "DOSCALLS.380"]
    fn DosEnterMustComplete(pul: *mut u32) -> u32;
    #[link_name = "DOSCALLS.381"]
    fn DosExitMustComplete(pul: *mut u32) -> u32;
    #[link_name = "DOSCALLS.254"]
    fn DosResetBuffer(hf: u32) -> u32;
}

const MUTEX_NAME: &[u8] = b"\\SEM32\\PMSHAPI\\PRF\0";
const SHMEM_NAME: &[u8] = b"\\SHAREMEM\\PMSHAPI\\PRF\0";
const MAGIC: u32 = 0x3146_5250; // "PRF1"
const PATH_MAX: usize = 260;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_OPEN_FAILED: u32 = 110;
const ERROR_SEM_OWNER_DIED: u32 = 105;

/// The system-wide part (named shared memory).
#[repr(C)]
struct Shared {
    magic: u32,
    user: [u8; PATH_MAX],
    system: [u8; PATH_MAX],
    gens: [u32; 256],
}

/// A profile this process has used.
struct File {
    /// Full path, NUL-terminated, as resolved.
    path: Vec<u8>,
    /// The path upper-cased, without the NUL: the identity of the file.
    key: Vec<u8>,
    loaded: bool,
    generation: u32,
    stamp: Option<(u16, u16, u32)>,
    profile: Profile,
    /// Not a profile (or unreadable): queries see nothing, writes fail.
    foreign: bool,
}

struct State {
    shared: *mut Shared,
    files: Vec<File>,
    /// PrfOpenProfile handles: (hini, key of the file).
    handles: Vec<(u32, Vec<u8>)>,
    next: u32,
}

struct Global(UnsafeCell<State>);
unsafe impl Sync for Global {}

static STATE: Global = Global(UnsafeCell::new(State { shared: null_mut(), files: Vec::new(), handles: Vec::new(), next: 0x0001_0001 }));
/// Used when the shared memory cannot be had (then profiles are not
/// shared between processes).
static mut LOCAL: Shared = Shared { magic: 0, user: [0; PATH_MAX], system: [0; PATH_MAX], gens: [0; 256] };
static MUTEX: AtomicU32 = AtomicU32::new(0);

fn mutex() -> u32 {
    let h = MUTEX.load(Ordering::Acquire);
    if h != 0 {
        return h;
    }
    let mut h = 0;
    for _ in 0..100 {
        if unsafe { DosOpenMutexSem(MUTEX_NAME.as_ptr(), &mut h) } == 0 || unsafe { os2::DosCreateMutexSem(MUTEX_NAME.as_ptr(), &mut h, 0, 0) } == 0 {
            break;
        }
        unsafe { os2::DosSleep(1) };
    }
    // Two threads of this process may both have opened it: keep one.
    match MUTEX.compare_exchange(0, h, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => h,
        Err(other) => {
            unsafe { os2::DosCloseMutexSem(h) };
            other
        }
    }
}

/// Run `f` holding the profile lock.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let m = mutex();
    let rc = unsafe { os2::DosRequestMutexSem(m, os2::SEM_INDEFINITE_WAIT) };
    let locked = rc == 0 || rc == ERROR_SEM_OWNER_DIED;
    let mut nest = 0;
    unsafe { DosEnterMustComplete(&mut nest) };
    let s = unsafe { &mut *STATE.0.get() };
    s.init();
    let r = f(s);
    unsafe { DosExitMustComplete(&mut nest) };
    if locked {
        unsafe { os2::DosReleaseMutexSem(m) };
    }
    r
}

pub unsafe fn cbytes<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    Some(unsafe { core::slice::from_raw_parts(p, n) })
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

fn upper(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn fnv(b: &[u8]) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for &c in b {
        h = (h ^ c as u32).wrapping_mul(0x0100_0193);
    }
    (h ^ h >> 16) as usize & 255
}

/// The full path of `name` (NUL-terminated).
fn full_path(name: &[u8]) -> Option<Vec<u8>> {
    if name.is_empty() || name.len() >= PATH_MAX {
        return None;
    }
    let mut z = name.to_vec();
    z.push(0);
    let mut buf = [0u8; PATH_MAX];
    if unsafe { os2::DosQueryPathInfo(z.as_ptr(), os2::FIL_QUERYFULLNAME, buf.as_mut_ptr(), buf.len() as u32) } != 0 {
        return None;
    }
    let mut v = cstr(&buf).to_vec();
    v.push(0);
    Some(v)
}

fn stat(path: &[u8]) -> Option<(u16, u16, u32)> {
    let mut st = os2::FileStatus3::default();
    let rc = unsafe { os2::DosQueryPathInfo(path.as_ptr(), os2::FIL_STANDARD, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) };
    (rc == 0).then_some((st.fdate_write, st.ftime_write, st.cb_file))
}

/// The temporary file of a profile: the extension replaced by `!!!`.
fn tmp_name(path: &[u8]) -> Vec<u8> {
    let p = cstr(path);
    let base = p.iter().rposition(|&c| c == b'\\' || c == b'/' || c == b':').map_or(0, |i| i + 1);
    let end = p[base..].iter().rposition(|&c| c == b'.').map_or(p.len(), |i| base + i);
    let mut v = p[..end].to_vec();
    v.extend_from_slice(b".!!!\0");
    v
}

fn read_file(path: &[u8]) -> Result<Vec<u8>, u32> {
    let (mut hf, mut action) = (0u32, 0u32);
    let rc = unsafe {
        os2::DosOpen(
            path.as_ptr(),
            &mut hf,
            &mut action,
            0,
            0,
            os2::OPEN_ACTION_FAIL_IF_NEW | os2::OPEN_ACTION_OPEN_IF_EXISTS,
            os2::OPEN_ACCESS_READONLY | os2::OPEN_SHARE_DENYNONE | os2::OPEN_FLAGS_NOINHERIT | os2::OPEN_FLAGS_FAIL_ON_ERROR,
            null_mut(),
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    let mut st = os2::FileStatus3::default();
    let mut rc = unsafe { os2::DosQueryFileInfo(hf, os2::FIL_STANDARD, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) };
    let mut v = alloc::vec![0u8; st.cb_file as usize];
    let mut got = 0usize;
    while rc == 0 && got < v.len() {
        let mut n = 0;
        rc = unsafe { os2::DosRead(hf, v.as_mut_ptr().add(got), (v.len() - got) as u32, &mut n) };
        if n == 0 {
            break;
        }
        got += n as usize;
    }
    unsafe { os2::DosClose(hf) };
    v.truncate(got);
    if rc != 0 { Err(rc) } else { Ok(v) }
}

/// Replace the file at `path` with `data` (see the module documentation).
fn write_file(path: &[u8], data: &[u8]) -> Result<(), u32> {
    let tmp = tmp_name(path);
    let (mut hf, mut action) = (0u32, 0u32);
    let rc = unsafe {
        os2::DosOpen(
            tmp.as_ptr(),
            &mut hf,
            &mut action,
            data.len() as u32,
            os2::FILE_NORMAL,
            os2::OPEN_ACTION_CREATE_IF_NEW | os2::OPEN_ACTION_REPLACE_IF_EXISTS,
            os2::OPEN_ACCESS_READWRITE | os2::OPEN_SHARE_DENYREADWRITE | os2::OPEN_FLAGS_NOINHERIT | os2::OPEN_FLAGS_FAIL_ON_ERROR,
            null_mut(),
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    let mut n = 0;
    let mut rc = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n) };
    if rc == 0 && n as usize != data.len() {
        rc = 112; // ERROR_DISK_FULL
    }
    if rc == 0 {
        rc = unsafe { DosResetBuffer(hf) };
    }
    unsafe { os2::DosClose(hf) };
    if rc == 0 {
        rc = match unsafe { os2::DosDelete(path.as_ptr()) } {
            0 | ERROR_FILE_NOT_FOUND => unsafe { os2::DosMove(tmp.as_ptr(), path.as_ptr()) },
            e => e,
        };
    }
    if rc != 0 {
        unsafe { os2::DosDelete(tmp.as_ptr()) };
        return Err(rc);
    }
    Ok(())
}

fn boot_drive() -> u8 {
    let mut d = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) }; // QSV_BOOT_DRIVE
    b'A' + (d.clamp(1, 26) - 1) as u8
}

/// The profile named by an environment variable, or the default under
/// \OS2 of the boot drive.
fn env_profile(var: &[u8], default: &[u8]) -> Vec<u8> {
    let mut p: *const u8 = null();
    if unsafe { os2::DosScanEnv(var.as_ptr(), &mut p) } == 0 {
        if let Some(full) = unsafe { cbytes(p) }.map(|v| v.trim_ascii()).and_then(full_path) {
            return full;
        }
    }
    let mut v = alloc::vec![boot_drive(), b':'];
    v.extend_from_slice(default);
    v.push(0);
    v
}

fn set_name(dst: &mut [u8; PATH_MAX], v: &[u8]) {
    let v = cstr(v);
    let n = v.len().min(PATH_MAX - 1);
    dst[..n].copy_from_slice(&v[..n]);
    dst[n..].fill(0);
}

impl State {
    fn init(&mut self) {
        if self.shared.is_null() {
            let mut p: *mut u8 = null_mut();
            let mut rc = unsafe { os2::DosGetNamedSharedMem(&mut p, SHMEM_NAME.as_ptr(), os2::PAG_READ | os2::PAG_WRITE) };
            if rc != 0 {
                rc = unsafe { os2::DosAllocSharedMem(&mut p, SHMEM_NAME.as_ptr(), 4096, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT) };
            }
            self.shared = if rc == 0 && !p.is_null() { p as *mut Shared } else { core::ptr::addr_of_mut!(LOCAL) };
        }
        let sh = unsafe { &mut *self.shared };
        if sh.magic != MAGIC {
            set_name(&mut sh.user, &env_profile(b"USER_INI\0", b"\\OS2\\OS2.INI"));
            set_name(&mut sh.system, &env_profile(b"SYSTEM_INI\0", b"\\OS2\\OS2SYS.INI"));
            sh.magic = MAGIC;
        }
    }

    fn sh(&mut self) -> &mut Shared {
        unsafe { &mut *self.shared }
    }

    pub fn user_path(&mut self) -> Vec<u8> {
        cstr(&self.sh().user).to_vec()
    }

    pub fn system_path(&mut self) -> Vec<u8> {
        cstr(&self.sh().system).to_vec()
    }

    /// The index of the file with full path `path` (with or without the
    /// NUL), up to date.
    fn file(&mut self, path: &[u8]) -> usize {
        let path = cstr(path);
        let key = upper(path);
        let i = match self.files.iter().position(|f| f.key == key) {
            Some(i) => i,
            None => {
                let mut p = path.to_vec();
                p.push(0);
                self.files.push(File { path: p, key, loaded: false, generation: 0, stamp: None, profile: Profile::default(), foreign: false });
                self.files.len() - 1
            }
        };
        self.refresh(i);
        i
    }

    fn refresh(&mut self, i: usize) {
        let slot = fnv(&self.files[i].key);
        let generation = self.sh().gens[slot];
        let f = &mut self.files[i];
        let stamp = stat(&f.path);
        if f.loaded && f.generation == generation && f.stamp == stamp {
            return;
        }
        let mut r = read_file(&f.path);
        if matches!(r, Err(ERROR_FILE_NOT_FOUND | ERROR_OPEN_FAILED)) {
            // A crash between deleting the profile and renaming its
            // replacement: the replacement is complete.
            let tmp = tmp_name(&f.path);
            if unsafe { os2::DosMove(tmp.as_ptr(), f.path.as_ptr()) } == 0 {
                r = read_file(&f.path);
            }
        }
        match r {
            Ok(b) => {
                let p = inifile::parse(&b);
                f.profile = p.profile;
                f.foreign = p.foreign;
            }
            Err(ERROR_FILE_NOT_FOUND | ERROR_OPEN_FAILED) => {
                // A new profile: create the file.
                f.profile = Profile::default();
                f.foreign = false;
                let _ = write_file(&f.path, &f.profile.to_bytes());
            }
            Err(_) => {
                f.profile = Profile::default();
                f.foreign = true;
            }
        }
        f.generation = generation;
        f.stamp = stat(&f.path);
        f.loaded = true;
    }

    /// The files a handle reads from, in search order (None: bad handle).
    fn search(&mut self, hini: u32) -> Option<Vec<usize>> {
        Some(match hini {
            HINI_PROFILE => {
                let (u, s) = (self.user_path(), self.system_path());
                alloc::vec![self.file(&u), self.file(&s)]
            }
            HINI_USERPROFILE => {
                let u = self.user_path();
                alloc::vec![self.file(&u)]
            }
            HINI_SYSTEMPROFILE => {
                let s = self.system_path();
                alloc::vec![self.file(&s)]
            }
            h => {
                let key = self.handles.iter().find(|(x, _)| *x == h)?.1.clone();
                let path = self.files.iter().find(|f| f.key == key)?.path.clone();
                alloc::vec![self.file(&path)]
            }
        })
    }

    /// Save file `i` with new contents.
    fn save(&mut self, i: usize, p: Profile) -> bool {
        let bytes = p.to_bytes();
        let slot = fnv(&self.files[i].key);
        let ok = write_file(&self.files[i].path, &bytes).is_ok();
        let generation = {
            let sh = self.sh();
            sh.gens[slot] = sh.gens[slot].wrapping_add(1);
            sh.gens[slot]
        };
        let f = &mut self.files[i];
        if ok {
            f.profile = p;
            f.generation = generation;
            f.stamp = stat(&f.path);
        } else {
            f.loaded = false;
        }
        ok
    }
}

/// Run `f` on the profiles `hini` reads from, in search order (user, then
/// system for HINI_PROFILE). None for an invalid handle.
pub fn read<R>(hini: u32, f: impl FnOnce(&[&Profile]) -> R) -> Option<R> {
    with(|s| {
        let idx = s.search(hini)?;
        let ps: Vec<&Profile> = idx.iter().filter(|&&i| !s.files[i].foreign).map(|&i| &s.files[i].profile).collect();
        Some(f(&ps))
    })
}

/// Change the profile `hini` writes to (the user profile for
/// HINI_PROFILE): `f` edits a copy and returns None to give up; the copy
/// is written to disk if it differs. None if the handle is invalid, `f`
/// gave up or the file could not be written.
pub fn write<R>(hini: u32, f: impl FnOnce(&mut Profile) -> Option<R>) -> Option<R> {
    let hini = if hini == HINI_PROFILE { HINI_USERPROFILE } else { hini };
    with(|s| {
        let i = s.search(hini)?[0];
        if s.files[i].foreign {
            return None;
        }
        let mut p = s.files[i].profile.clone();
        let r = f(&mut p)?;
        if p != s.files[i].profile && !s.save(i, p) {
            return None;
        }
        Some(r)
    })
}

/// PrfOpenProfile: a handle for the profile file `name` (created if it
/// does not exist); 0 for the current user or system profile, a file that
/// is not a profile, or a name that cannot be resolved.
pub fn open(name: &[u8]) -> u32 {
    let Some(full) = full_path(name) else { return 0 };
    with(|s| {
        let key = upper(cstr(&full));
        let (u, sy) = (upper(&s.user_path()), upper(&s.system_path()));
        if key == u || key == sy {
            return 0;
        }
        let i = s.file(&full);
        if s.files[i].foreign || s.files[i].stamp.is_none() {
            if !s.handles.iter().any(|(_, k)| *k == key) {
                s.files.remove(i);
            }
            return 0;
        }
        let h = s.next;
        s.next = s.next.wrapping_add(1).max(0x0001_0001);
        s.handles.push((h, key));
        h
    })
}

/// PrfCloseProfile.
pub fn close(hini: u32) -> bool {
    with(|s| {
        let Some(i) = s.handles.iter().position(|(h, _)| *h == hini) else { return false };
        let (_, key) = s.handles.remove(i);
        if !s.handles.iter().any(|(_, k)| *k == key) {
            let (u, sy) = (upper(&s.user_path()), upper(&s.system_path()));
            if key != u && key != sy {
                s.files.retain(|f| f.key != key);
            }
        }
        true
    })
}

/// The current user and system profile names.
pub fn names() -> (Vec<u8>, Vec<u8>) {
    with(|s| (s.user_path(), s.system_path()))
}

/// PrfReset: make `user` the user profile (created if missing); `system`
/// must name the current system profile (None: unchanged).
pub fn reset(user: &[u8], system: Option<&[u8]>) -> bool {
    let Some(u) = full_path(user) else { return false };
    let sys = match system {
        Some(n) => match full_path(n) {
            Some(p) => Some(p),
            None => return false,
        },
        None => None,
    };
    with(|s| {
        if let Some(p) = sys {
            if upper(cstr(&p)) != upper(&s.system_path()) {
                return false;
            }
        }
        let i = s.file(&u);
        if s.files[i].foreign || s.files[i].stamp.is_none() {
            return false;
        }
        set_name(&mut s.sh().user, &u);
        true
    })
}
