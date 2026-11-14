//! STRESS: Windows 3.1's stress-testing library (STRESS.DLL), which takes
//! away global memory, USER and GDI heap space, file handles and disk
//! space so that a program can be tried with little of them left (Open
//! Watcom's Heap Walker allocates "all but n KB" with it).
//!
//! Also the memory figures those resources are measured by:
//!
//! - **the global heap.** On Windows every task allocates from one global
//!   heap; GetFreeSpace says what is left of it and GlobalAlloc fails
//!   beyond that. Here each task allocates from its own process, and the
//!   global heap is a budget of `GLOBAL_HEAP` bytes that the tasks'
//!   GlobalAlloc blocks share: each task publishes what its blocks take in
//!   the TOOLHELP task registry (toolhelp.rs), so a block STRESS takes in
//!   one task is missing from GetFreeSpace and GlobalAlloc in every other,
//!   as on Windows. It is never more than the system's free memory.
//! - **USER's and GDI's local heaps** (GetHeapSpaces, SystemHeapInfo,
//!   resource meters). WIN16 keeps windows and GDI objects in PM, not in
//!   64 KB heaps, so these heaps are 90% free but for what STRESS's
//!   AllocUserMem/AllocGDIMem took of them (in every task).
//!
//! AllocMem and AllocUserMem/AllocGDIMem take what they leave in blocks of
//! decreasing size, as Windows' STRESS does: 512 KB global blocks (2000 at
//! most) and 512-byte local blocks (500 at most), halving the size when a
//! block would leave less than asked; they return the number of blocks
//! taken.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::Global;

/// What the global heap holds, all tasks' blocks together.
pub const GLOBAL_HEAP: u32 = 64 << 20;
/// USER's and GDI's heaps: their size, and what is in use without STRESS.
const LOCAL_HEAP: u16 = 0xffff;
const LOCAL_FREE: u16 = 0xe666;
/// What a local block costs beyond its size (its handle and arena).
const LOCAL_OVERHEAD: u16 = 8;

const GMEM_MOVEABLE: u16 = 0x0002;

struct State {
    /// AllocMem's global blocks.
    mem: Vec<u16>,
    /// AllocUserMem's and AllocGDIMem's blocks (their sizes).
    user: Vec<u16>,
    gdi: Vec<u16>,
    /// AllocFileHandles's handles.
    files: Vec<u32>,
    /// The drive AllocDiskSpace filled for EDS_WIN, EDS_CUR, EDS_TEMP
    /// (its letter, 0: none).
    disk: [u8; 3],
}

static ST: Global<State> = Global::new(State { mem: Vec::new(), user: Vec::new(), gdi: Vec::new(), files: Vec::new(), disk: [0; 3] });

fn local_used(v: &[u16]) -> u32 {
    v.iter().map(|&n| (n + LOCAL_OVERHEAD) as u32).sum()
}

/// This task's part of the shared heaps: its global blocks' bytes, and
/// the USER and GDI heap bytes STRESS took (toolhelp.rs publishes it).
pub fn own_usage() -> [u32; 3] {
    let s = ST.get();
    [crate::kernel::global_used(), local_used(&s.user), local_used(&s.gdi)]
}

/// A task's global blocks or STRESS's local blocks changed.
pub fn global_changed() {
    crate::toolhelp::publish_usage(own_usage());
}

/// GetFreeSpace: the global heap's free bytes.
pub fn free_space() -> u32 {
    let used = crate::kernel::global_used().saturating_add(crate::toolhelp::others_usage()[0]);
    let mut avail = 0u32;
    unsafe { os2::DosQuerySysInfo(19, 19, &mut avail, 4) }; // QSV_TOTAVAILMEM
    let free = GLOBAL_HEAP.saturating_sub(used);
    if avail != 0 { free.min(avail) } else { free }
}

/// USER's (1) or GDI's (2) heap: its free bytes, all tasks' STRESS
/// blocks taken.
fn local_free(which: usize) -> u16 {
    let s = ST.get();
    let own = local_used(if which == 1 { &s.user } else { &s.gdi });
    let taken = own.saturating_add(crate::toolhelp::others_usage()[which]);
    (LOCAL_FREE as u32).saturating_sub(taken) as u16
}

/// Which shared heap a module handle's local heap is: USER 1, GDI 2.
fn heap_of(h: u16) -> Option<usize> {
    let sys = crate::module::SYSTEM;
    if h | 7 == sys[1].1 | 7 {
        Some(1)
    } else if h | 7 == sys[2].1 | 7 {
        Some(2)
    } else {
        None
    }
}

/// GetHeapSpaces(hmodule): (free, size) of a module's local heap.
pub fn heap_spaces(h: u16) -> (u16, u16) {
    match heap_of(h) {
        Some(w) => (local_free(w), LOCAL_HEAP),
        None => (LOCAL_FREE, LOCAL_HEAP),
    }
}

/// SystemHeapInfo's wUserFreePercent and wGDIFreePercent.
pub fn free_percent(which: usize) -> u16 {
    (local_free(which) as u32 * 100 / LOCAL_HEAP as u32) as u16
}

/// USER's or GDI's heap as TOOLHELP's LocalFirst/LocalNext walk it
/// (`h` its module handle, which SystemHeapInfo gives as the heap's
/// segment): what the system uses, STRESS's blocks, the free rest, as
/// (offset, size, LF_ flags, LT_ type).
pub fn system_heap(h: u16) -> Option<Vec<(u16, u16, u16, u16)>> {
    let which = heap_of(h)?;
    let s = ST.get();
    let own = if which == 1 { &s.user } else { &s.gdi };
    let others = crate::toolhelp::others_usage()[which];
    let mut v = Vec::new();
    let mut at = 0x10u32;
    let sys = (LOCAL_HEAP - LOCAL_FREE) as u32 + others;
    v.push((at as u16, sys.min(0xffef) as u16, 1, 0));
    at += sys;
    for &n in own.iter() {
        if at + n as u32 + LOCAL_OVERHEAD as u32 > 0xffff {
            break;
        }
        // USER's are UserSeeUserDo blocks (LT_USER_USERSEEUSERDOALLOC).
        v.push(((at + LOCAL_OVERHEAD as u32) as u16, n, 4, if which == 1 { 21 } else { 0 }));
        at += n as u32 + LOCAL_OVERHEAD as u32;
    }
    if at < 0xffff {
        v.push((at as u16, (0xffff - at) as u16, 2, 0xff));
    }
    Some(v)
}

/// AllocMem(dwLeft): global blocks until `left` bytes are free (those
/// taken before stay unless fewer than `left` are free now).
fn alloc_mem(left: u32) -> u32 {
    let mut free = free_space();
    if free < left {
        free_all_mem();
        free = free_space();
    }
    let mut chunk: u32 = 0x8_0000;
    while ST.get().mem.len() < 2000 && chunk > 0 && free > left {
        if free - left >= chunk {
            let h = crate::kernel::global_alloc(GMEM_MOVEABLE, chunk);
            if h != 0 {
                ST.get().mem.push(h);
                free -= chunk;
                continue;
            }
        }
        chunk >>= 1;
    }
    ST.get().mem.len() as u32
}

fn free_all_mem() {
    let blocks = core::mem::take(&mut ST.get().mem);
    for h in blocks {
        crate::kernel::global_free(h);
    }
}

/// AllocUserMem(uLeft) (1), AllocGDIMem(uLeft) (2): heap blocks until
/// `left` bytes are free.
fn alloc_local(which: usize, left: u16) -> u32 {
    let mut free = local_free(which);
    if free < left {
        free_all_local(which);
        free = local_free(which);
    }
    let s = ST.get();
    let v = if which == 1 { &mut s.user } else { &mut s.gdi };
    let mut chunk: u16 = 0x200;
    while v.len() < 500 && chunk > 0 && free > left {
        if free - left >= chunk + LOCAL_OVERHEAD {
            v.push(chunk);
            free -= chunk + LOCAL_OVERHEAD;
        } else {
            chunk >>= 1;
        }
    }
    let n = v.len() as u32;
    global_changed();
    n
}

fn free_all_local(which: usize) {
    let s = ST.get();
    if which == 1 { s.user.clear() } else { s.gdi.clear() }
    global_changed();
}

/// The task's file handle table (the PSP's job file table): 20 entries,
/// as many as SetHandleCount asks for (up to 255). Windows' file calls
/// give no handle beyond it (`jft_check`).
static JFT: Global<u16> = Global::new(20);

/// SetHandleCount(n): the table grows to `n` entries (it never shrinks);
/// its size now.
pub fn set_handle_count(n: u16) -> u16 {
    let j = JFT.get();
    *j = (*j).max(n.min(255));
    *j
}

/// A file a Windows program opened (OpenFile, _lopen, _lcreat, INT 21h)
/// got handle `hf`: beyond the task's handle table it is closed again and
/// the open fails with ERROR_TOO_MANY_OPEN_FILES (4), as on Windows
/// (AllocFileHandles takes the table's free entries).
pub fn jft_check(hf: u32) -> bool {
    if hf < *JFT.get() as u32 {
        return true;
    }
    unsafe { os2::DosClose(hf) };
    false
}

fn is_open(h: u32) -> bool {
    let mut st = 0u32;
    unsafe { os2::DosQueryFHState(h, &mut st) == 0 }
}

/// Open the null device (a file handle taken), if the handle is in the
/// task's table.
fn open_nul() -> Option<u32> {
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYNONE | OPEN_ACCESS_READONLY.
    let rc = unsafe { os2::DosOpen(b"NUL\0".as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) };
    (rc == 0 && jft_check(hf)).then_some(hf)
}

/// GetFreeFileHandles: the free entries of the task's handle table.
fn free_file_handles() -> u32 {
    (0..*JFT.get() as u32).filter(|&h| !is_open(h)).count() as u32
}

/// AllocFileHandles(left): take the handle table's entries until `left`
/// are free; the number left free (fewer than asked if there were not so
/// many), -1 for a negative `left`.
fn alloc_file_handles(left: i16) -> u32 {
    if left < 0 {
        return 0xffff;
    }
    unalloc_file_handles();
    while free_file_handles() > left as u32 {
        match open_nul() {
            Some(h) => ST.get().files.push(h),
            None => break,
        }
    }
    free_file_handles()
}

fn unalloc_file_handles() {
    for h in core::mem::take(&mut ST.get().files) {
        unsafe { os2::DosClose(h) };
    }
}

/// The drive (letter) of EDS_WIN (1: the Windows directory's), EDS_CUR
/// (2: the current one), EDS_TEMP (3: GetTempDrive's).
fn eds_drive(kind: u16) -> Option<u8> {
    match kind {
        1 => Some(crate::profile::SYSTEM_DIR[0]),
        2 => {
            let (mut d, mut map) = (0u32, 0u32);
            unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
            Some(b'A' + (d as u8).saturating_sub(1))
        }
        // GetTempDrive (kernel.rs) is C:.
        3 => Some(b'C'),
        _ => None,
    }
}

/// A drive's free bytes.
fn disk_free(drive: u8) -> Option<u64> {
    // FSALLOCATE: idFileSystem, cSectorUnit, cUnit, cUnitAvail, cbSector.
    let mut b = [0u8; 18];
    if unsafe { os2::DosQueryFSInfo((drive - b'A' + 1) as u32, 1, b.as_mut_ptr(), 18) } != 0 {
        return None;
    }
    let d = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    let sector = u16::from_le_bytes([b[16], b[17]]) as u64;
    Some(d(4) * d(12) * sector)
}

fn eat_file(drive: u8) -> [u8; 14] {
    let mut n = *b"?:\\STRESS.EAT\0";
    n[0] = drive;
    n
}

/// AllocDiskSpace(lLeft, uDrive): a file STRESS.EAT in the drive's root
/// directory that leaves `left` bytes free: 1 if made, 0 if fewer than
/// that were free or the file could not be made, -1 for a bad argument.
fn alloc_disk_space(left: i32, kind: u16) -> u32 {
    if left < 0 {
        return 0xffff;
    }
    let Some(drive) = eds_drive(kind) else { return 0xffff };
    let name = eat_file(drive);
    let Some(mut free) = disk_free(drive) else { return 0 };
    if free <= left as u64 {
        return 0;
    }
    // A STRESS.EAT already there gives its space back first.
    if unsafe { os2::DosDelete(name.as_ptr()) } == 0 {
        free = disk_free(drive).unwrap_or(free);
    }
    let (mut hf, mut action) = (0u32, 0u32);
    // Create or replace, write-only deny-write.
    if unsafe { os2::DosOpen(name.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return 0;
    }
    let size = (free - left as u64).min(u32::MAX as u64 - 1) as u32;
    let ok = unsafe { os2::DosSetFileSize(hf, size) } == 0;
    unsafe { os2::DosClose(hf) };
    if !ok {
        unsafe { os2::DosDelete(name.as_ptr()) };
        return 0;
    }
    ST.get().disk[kind as usize - 1] = drive;
    1
}

/// UnAllocDiskSpace(uDrive): the STRESS.EAT AllocDiskSpace made there
/// goes.
fn unalloc_disk_space(kind: u16) {
    let Some(slot) = ST.get().disk.get_mut((kind as usize).wrapping_sub(1)) else { return };
    if *slot != 0 {
        let name = eat_file(*slot);
        unsafe { os2::DosDelete(name.as_ptr()) };
        *slot = 0;
    }
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // AllocMem(dwLeft), FreeAllMem
        7902 => alloc_mem(a.l()),
        7903 => {
            free_all_mem();
            0
        }
        // AllocFileHandles(left), UnAllocFileHandles, GetFreeFileHandles
        7906 => alloc_file_handles(a.w() as i16),
        7907 => {
            unalloc_file_handles();
            0
        }
        7908 => free_file_handles(),
        // AllocDiskSpace(lLeft, uDrive), UnAllocDiskSpace(uDrive)
        7910 => {
            let (left, kind) = (a.l(), a.w());
            alloc_disk_space(left as i32, kind)
        }
        7911 => {
            unalloc_disk_space(a.w());
            0
        }
        // AllocUserMem(uLeft), FreeAllUserMem, AllocGDIMem(uLeft),
        // FreeAllGDIMem
        7912 => alloc_local(1, a.w()),
        7913 => {
            free_all_local(1);
            0
        }
        7914 => alloc_local(2, a.w()),
        7915 => {
            free_all_local(2);
            0
        }
        _ => return None,
    })
}
