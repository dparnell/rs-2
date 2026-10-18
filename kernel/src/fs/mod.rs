//! Virtual file system: drive letters, open files, devices.
//!
//! File systems implement [`FileSystem`] (the role of an OS/2 IFS); paths
//! handed to them are relative to the volume root with `\` separators.

pub mod block;
pub mod fatfs;
pub mod hpfsfs;
pub mod iso9660;
pub mod path;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub use path::FullPath;

use crate::api::err;
use crate::sync::SpinLock;

/// File attributes (FILE_*).
pub mod attr {
    pub const NORMAL: u32 = 0x00;
    pub const READONLY: u32 = 0x01;
    pub const HIDDEN: u32 = 0x02;
    pub const SYSTEM: u32 = 0x04;
    pub const DIRECTORY: u32 = 0x10;
    pub const ARCHIVED: u32 = 0x20;
}

/// Packed FDATE/FTIME pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub date: u16,
    pub time: u16,
}

#[derive(Clone, Debug, Default)]
pub struct FileInfo {
    pub name: String,
    pub attr: u32,
    pub size: u64,
    pub alloc: u64,
    pub created: Stamp,
    pub accessed: Stamp,
    pub modified: Stamp,
    /// Size of the extended attributes (FEA2LIST), 0 if none.
    pub ea_size: u32,
}

impl FileInfo {
    pub fn is_dir(&self) -> bool {
        self.attr & attr::DIRECTORY != 0
    }
}

/// Fields to change with set_info; None leaves a field alone.
#[derive(Clone, Debug, Default)]
pub struct SetInfo {
    pub attr: Option<u32>,
    pub created: Option<Stamp>,
    pub accessed: Option<Stamp>,
    pub modified: Option<Stamp>,
}

#[derive(Clone, Debug, Default)]
pub struct VolumeInfo {
    pub serial: u32,
    pub label: String,
    pub sectors_per_unit: u32,
    pub units: u32,
    pub free_units: u32,
    pub bytes_per_sector: u32,
}

pub trait FileSystem: Send + Sync {
    /// File system name as reported by DosQueryFSAttach ("FAT", "HPFS").
    fn name(&self) -> &'static str;
    fn stat(&self, path: &str) -> Result<FileInfo, u32>;
    fn list(&self, dir: &str) -> Result<Vec<FileInfo>, u32>;
    fn open(&self, path: &str) -> Result<Arc<dyn FsFile>, u32>;
    /// Create a new, empty file; fails if it exists.
    fn create(&self, path: &str, attr: u32) -> Result<Arc<dyn FsFile>, u32>;
    fn delete(&self, path: &str) -> Result<(), u32>;
    fn mkdir(&self, path: &str) -> Result<(), u32>;
    fn rmdir(&self, path: &str) -> Result<(), u32>;
    fn rename(&self, from: &str, to: &str) -> Result<(), u32>;
    fn set_info(&self, path: &str, info: &SetInfo) -> Result<(), u32>;
    fn volume(&self) -> Result<VolumeInfo, u32>;
    fn set_label(&self, _label: &str) -> Result<(), u32> {
        Err(err::NOT_SUPPORTED)
    }
    /// Maximum length of a path component (8.3 file systems: 12).
    fn max_component(&self) -> usize {
        255
    }
    /// The volume's block device, for DASD access and remounting.
    fn device(&self) -> Option<Arc<dyn block::BlockDev>> {
        None
    }
}

pub trait FsFile: Send + Sync {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, u32>;
    fn write(&self, off: u64, data: &[u8]) -> Result<usize, u32>;
    fn size(&self) -> u64;
    fn set_size(&self, size: u64) -> Result<(), u32>;
    fn info(&self) -> FileInfo;
    fn set_info(&self, info: &SetInfo) -> Result<(), u32>;
    fn flush(&self) -> Result<(), u32> {
        Ok(())
    }
}

static DRIVES: SpinLock<[Option<Arc<dyn FileSystem>>; 26]> = SpinLock::new([const { None }; 26]);

pub fn mount(drive: u8, fs: Arc<dyn FileSystem>) {
    crate::println!("drive {}: {} file system", (b'A' + drive) as char, fs.name());
    DRIVES.lock()[drive as usize] = Some(fs);
}

pub fn drive(d: u8) -> Option<Arc<dyn FileSystem>> {
    if locked_by_other(d) {
        return None;
    }
    DRIVES.lock().get(d as usize).cloned().flatten()
}

/// Bit mask of valid drives (bit 0 = A:).
pub fn drive_map() -> u32 {
    DRIVES.lock().iter().enumerate().filter(|(_, d)| d.is_some()).fold(0, |m, (i, _)| m | 1 << i)
}

/// The file system for a path, or ERROR_INVALID_DRIVE.
pub fn fs_of(p: &FullPath) -> Result<Arc<dyn FileSystem>, u32> {
    if locked_by_other(p.drive) {
        return Err(err::DRIVE_LOCKED);
    }
    drive(p.drive).ok_or(err::INVALID_DRIVE)
}

// ---- Volume locking and remounting (DASD access) ----

/// Process holding DSK_LOCKDRIVE on each drive (0 = not locked).
static LOCKS: SpinLock<[u32; 26]> = SpinLock::new([0; 26]);

fn current_pid() -> u32 {
    crate::task::sched::current_opt().and_then(|t| t.process()).map_or(0, |p| p.pid)
}

/// True if another process has locked drive `d`.
pub fn locked_by_other(d: u8) -> bool {
    let owner = LOCKS.lock().get(d as usize).copied().unwrap_or(0);
    owner != 0 && owner != current_pid()
}

/// DSK_LOCKDRIVE: fails while files are open on the drive.
pub fn lock_drive(d: u8) -> Result<(), u32> {
    let prefix = alloc::format!("{}:", (b'A' + d) as char);
    if SHARES.lock().keys().any(|k| k.starts_with(&prefix)) {
        return Err(err::DRIVE_LOCKED);
    }
    let pid = current_pid();
    let mut l = LOCKS.lock();
    if l[d as usize] != 0 && l[d as usize] != pid {
        return Err(err::DRIVE_LOCKED);
    }
    l[d as usize] = pid;
    Ok(())
}

pub fn unlock_drive(d: u8) {
    LOCKS.lock()[d as usize] = 0;
}

/// Recognise the file system on `dev`: the boot sector names HPFS
/// volumes, otherwise FAT is tried first. A volume nothing recognises is
/// still mounted so that FORMAT can open it.
pub fn probe_volume(dev: Arc<dyn block::BlockDev>) -> Arc<dyn FileSystem> {
    let mut bs = alloc::vec![0u8; dev.sector_size().max(512)];
    let hpfs_boot = dev.read(0, &mut bs).is_ok() && &bs[54..58] == b"HPFS";
    if !hpfs_boot {
        if let Ok(fs) = fatfs::FatFs::mount(dev.clone()) {
            return fs;
        }
    }
    if hpfsfs::probe(&dev) {
        if let Ok(fs) = hpfsfs::HpfsFs::mount(dev.clone()) {
            return fs;
        }
    }
    if hpfs_boot {
        if let Ok(fs) = fatfs::FatFs::mount(dev.clone()) {
            return fs;
        }
    }
    Arc::new(Unformatted(dev))
}

/// Mount drive `d` again after its volume was rewritten (FORMAT, CHKDSK /F).
pub fn remount(d: u8) -> Result<(), u32> {
    let dev = DRIVES.lock()[d as usize].as_ref().and_then(|f| f.device()).ok_or(err::INVALID_DRIVE)?;
    let fs = probe_volume(dev);
    crate::println!("drive {}: remounted ({})", (b'A' + d) as char, fs.name());
    DRIVES.lock()[d as usize] = Some(fs);
    Ok(())
}

/// A volume without a recognised file system.
struct Unformatted(Arc<dyn block::BlockDev>);

impl FileSystem for Unformatted {
    fn name(&self) -> &'static str {
        "UNKNOWN"
    }
    fn stat(&self, _: &str) -> Result<FileInfo, u32> {
        Err(err::NOT_READY)
    }
    fn list(&self, _: &str) -> Result<Vec<FileInfo>, u32> {
        Err(err::NOT_READY)
    }
    fn open(&self, _: &str) -> Result<Arc<dyn FsFile>, u32> {
        Err(err::NOT_READY)
    }
    fn create(&self, _: &str, _: u32) -> Result<Arc<dyn FsFile>, u32> {
        Err(err::NOT_READY)
    }
    fn delete(&self, _: &str) -> Result<(), u32> {
        Err(err::NOT_READY)
    }
    fn mkdir(&self, _: &str) -> Result<(), u32> {
        Err(err::NOT_READY)
    }
    fn rmdir(&self, _: &str) -> Result<(), u32> {
        Err(err::NOT_READY)
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), u32> {
        Err(err::NOT_READY)
    }
    fn set_info(&self, _: &str, _: &SetInfo) -> Result<(), u32> {
        Err(err::NOT_READY)
    }
    fn volume(&self) -> Result<VolumeInfo, u32> {
        Err(err::NOT_READY)
    }
    fn device(&self) -> Option<Arc<dyn block::BlockDev>> {
        Some(self.0.clone())
    }
}

/// A drive opened with OPEN_FLAGS_DASD: the volume as a sequence of
/// sectors. Closing the last handle releases a lock taken through it and
/// remounts the volume if it was written.
pub struct Dasd {
    pub drive: u8,
    pub dev: Arc<dyn block::BlockDev>,
    pub pos: SpinLock<u64>,
    pub locked: core::sync::atomic::AtomicBool,
    pub written: core::sync::atomic::AtomicBool,
}

impl Dasd {
    pub fn new(drive: u8, dev: Arc<dyn block::BlockDev>) -> Arc<Dasd> {
        Arc::new(Dasd {
            drive,
            dev,
            pos: SpinLock::new(0),
            locked: core::sync::atomic::AtomicBool::new(false),
            written: core::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Remount if the volume was written since the last remount.
    pub fn remount_if_written(&self) {
        if self.written.swap(false, Ordering::Relaxed) {
            let _ = remount(self.drive);
        }
    }
}

impl Drop for Dasd {
    fn drop(&mut self) {
        if self.locked.load(Ordering::Relaxed) {
            unlock_drive(self.drive);
        }
        self.remount_if_written();
    }
}

/// Current time as an FDATE/FTIME pair.
pub fn now() -> Stamp {
    let t = crate::rtc::now();
    let f = fat::Timestamp::new(t.year, t.month, t.day, t.hour, t.minute, t.second);
    Stamp { date: f.date, time: f.time }
}

// ---- Devices ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Con,
    Nul,
    Kbd,
    Screen,
    Clock,
    DosDev,
}

/// Recognise device names (`CON`, `NUL`, `\DEV\KBD$`, `CON.TXT`, ...).
pub fn device_name(path: &str) -> Option<Device> {
    let base = path.rsplit(['\\', '/', ':']).next().unwrap_or(path);
    let base = base.split('.').next().unwrap_or(base).to_ascii_uppercase();
    let dev = match base.as_str() {
        "CON" => Device::Con,
        "NUL" => Device::Nul,
        "KBD$" => Device::Kbd,
        "SCREEN$" => Device::Screen,
        "CLOCK$" => Device::Clock,
        "DOS$" => Device::DosDev,
        _ => return None,
    };
    // `\DEV\X` or a bare name (any directory).
    Some(dev)
}

// ---- Open files ----

/// Open mode bits.
pub mod mode {
    pub const ACCESS_MASK: u32 = 0x0007;
    pub const READONLY: u32 = 0x0000;
    pub const WRITEONLY: u32 = 0x0001;
    pub const READWRITE: u32 = 0x0002;
    pub const SHARE_MASK: u32 = 0x0070;
    pub const DENYREADWRITE: u32 = 0x0010;
    pub const DENYWRITE: u32 = 0x0020;
    pub const DENYREAD: u32 = 0x0030;
    pub const DENYNONE: u32 = 0x0040;
    pub const NOINHERIT: u32 = 0x0080;
    pub const NO_LOCALITY: u32 = 0x0000;
    pub const FAILONERROR: u32 = 0x2000;
    pub const WRITE_THROUGH: u32 = 0x4000;
    pub const DASD: u32 = 0x8000;
}

pub struct OpenFile {
    pub path: FullPath,
    pub file: Arc<dyn FsFile>,
    pub pos: SpinLock<u64>,
    /// Open mode (access, sharing, flags). Mutable by DosSetFHState.
    pub mode: AtomicU32,
    share_key: String,
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let _ = self.file.flush();
        let mut s = SHARES.lock();
        if let Some(v) = s.get_mut(&self.share_key) {
            let m = self.mode.load(Ordering::Relaxed);
            if let Some(i) = v.iter().position(|&x| x == (m & mode::ACCESS_MASK, m & mode::SHARE_MASK)) {
                v.remove(i);
            }
            if v.is_empty() {
                s.remove(&self.share_key);
            }
        }
    }
}

/// Sharing state of open files: key = upper-case full path.
static SHARES: SpinLock<BTreeMap<String, Vec<(u32, u32)>>> = SpinLock::new(BTreeMap::new());

fn allows(share: u32, access: u32) -> bool {
    let reads = access != mode::WRITEONLY;
    let writes = access != mode::READONLY;
    match share {
        mode::DENYREADWRITE => false,
        mode::DENYWRITE => !writes,
        mode::DENYREAD => !reads,
        _ => true,
    }
}

impl OpenFile {
    /// Register a new open, enforcing OS/2 sharing rules.
    pub fn new(path: FullPath, file: Arc<dyn FsFile>, open_mode: u32) -> Result<Arc<OpenFile>, u32> {
        let key = path.display().to_ascii_uppercase();
        let access = open_mode & mode::ACCESS_MASK;
        let share = open_mode & mode::SHARE_MASK;
        {
            let mut s = SHARES.lock();
            let v = s.entry(key.clone()).or_default();
            for &(oa, os) in v.iter() {
                if !allows(os, access) || !allows(share, oa) {
                    if v.is_empty() {
                        s.remove(&key);
                    }
                    return Err(err::SHARING_VIOLATION);
                }
            }
            v.push((access, share));
        }
        Ok(Arc::new(OpenFile { path, file, pos: SpinLock::new(0), mode: AtomicU32::new(open_mode), share_key: key }))
    }

    pub fn access(&self) -> u32 {
        self.mode.load(Ordering::Relaxed) & mode::ACCESS_MASK
    }
}

/// An entry in a process handle table.
#[derive(Clone)]
pub enum Handle {
    Con,
    Nul,
    Kbd,
    Screen,
    Clock,
    File(Arc<OpenFile>),
    Pipe(Arc<crate::pipe::PipeEnd>),
    /// One end of a named pipe instance.
    NPipe(Arc<crate::npipe::End>),
    /// DosPhysicalDisk IOCtl handle for disk n (0-based).
    PhysDisk(u32),
    /// The DOS$ device (reboot IOCtl).
    DosDev,
    /// A logical drive opened for DASD access.
    Drive(Arc<Dasd>),
}

impl Handle {
    pub fn device(d: Device) -> Handle {
        match d {
            Device::Con => Handle::Con,
            Device::Nul => Handle::Nul,
            Device::Kbd => Handle::Kbd,
            Device::Screen => Handle::Screen,
            Device::Clock => Handle::Clock,
            Device::DosDev => Handle::DosDev,
        }
    }
}

// ---- Helpers for the loader ----

/// Directories searched for DLLs (CONFIG.SYS LIBPATH).
static LIBPATH: SpinLock<Vec<String>> = SpinLock::new(Vec::new());

pub fn set_libpath(v: &str) {
    *LIBPATH.lock() = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
}

pub fn libpath() -> Vec<String> {
    LIBPATH.lock().clone()
}

/// Read a whole file given a fully qualified path.
pub fn read_file(path: &str) -> Option<Arc<[u8]>> {
    let empty: [String; 26] = Default::default();
    let p = path::canonical(path, 2, &empty).ok()?;
    let fs = fs_of(&p).ok()?;
    let f = fs.open(&p.rel()).ok()?;
    let mut v = alloc::vec![0u8; f.size() as usize];
    let n = f.read(0, &mut v).ok()?;
    v.truncate(n);
    Some(Arc::from(v))
}

pub fn stat(path: &FullPath) -> Result<FileInfo, u32> {
    fs_of(path)?.stat(&path.rel())
}

/// Locate a DLL for the loader: a path, or `NAME.DLL` along the LIBPATH.
pub fn find_module(name: &str) -> Option<(String, Arc<[u8]>)> {
    let has_path = name.contains(['\\', '/', ':']);
    if has_path {
        return read_file(name).map(|d| (name.to_string(), d));
    }
    let file = if name.contains('.') { name.to_string() } else { alloc::format!("{name}.DLL") };
    // BeginLIBPATH, LIBPATH, EndLIBPATH of the loading process.
    let (begin, end) = crate::task::sched::current_opt()
        .and_then(|t| t.process())
        .map(|p| {
            let x = p.ext_libpath.lock();
            (x.0.clone(), x.1.clone())
        })
        .unwrap_or_default();
    let mut dirs: Vec<String> = begin.split(';').filter(|s| !s.is_empty()).map(String::from).collect();
    dirs.extend(libpath());
    dirs.extend(end.split(';').filter(|s| !s.is_empty()).map(String::from));
    let cwd = crate::task::sched::current_opt().and_then(|t| t.process()).map(|p| {
        let c = p.cwd.lock();
        alloc::format!("{}:\\{}", (b'A' + c.0) as char, c.1[c.0 as usize])
    });
    for dir in dirs {
        // "." is the loading process's current directory.
        let dir = if dir == "." { cwd.clone().unwrap_or_else(|| String::from("C:\\")) } else { dir };
        let full = alloc::format!("{}\\{}", dir.trim_end_matches('\\'), file);
        if let Some(d) = read_file(&full) {
            return Some((full.to_ascii_uppercase(), d));
        }
    }
    None
}

/// Mount the boot volumes handed over by the loader: a module named
/// `X.IMG` becomes drive X:.
pub fn init(info: &bootinfo::BootInfo) {
    set_libpath("C:\\OS2\\DLL;C:\\OS2;C:\\;C:\\OS2\\APPS\\DLL;C:\\RS2");
    // A boot volume image (C.IMG) from the loader is C: (live media and
    // development); otherwise an installed system's boot partition becomes
    // C: when the storage drivers find it.
    for m in info.modules() {
        let name = m.name().to_ascii_uppercase();
        let Some(letter) = name.strip_suffix(".IMG") else { continue };
        if letter.len() != 1 {
            continue;
        }
        let d = letter.as_bytes()[0];
        if !d.is_ascii_uppercase() {
            continue;
        }
        let disk = block::RamDisk::from_module(m);
        match fatfs::FatFs::mount(disk) {
            Ok(fs) => mount(d - b'A', fs),
            Err(e) => crate::println!("{name}: cannot mount ({e})"),
        }
    }
}

/// True if the loader supplied a C: boot volume image.
pub fn has_boot_image(info: &bootinfo::BootInfo) -> bool {
    info.modules().iter().any(|m| m.name().eq_ignore_ascii_case("C.IMG"))
}
