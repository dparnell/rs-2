//! DOSCALLS: the kernel API.

use alloc::string::String;
use alloc::vec::Vec;

use super::{err, Ctx, Handler};
use crate::fs::Handle;
use crate::loader;
use crate::task::{process, sched};
use crate::vm::{self, pag, SegKind};

/// Implementation for a gate entry, if there is one.
pub fn handler(module: u8, ordinal: u16) -> Option<Handler> {
    let h: Handler = match (module, ordinal) {
        // ---- 16-bit DOSCALLS ----
        (0, 5) => dos16_exit,
        (0, 8) => dos16_get_info_seg,
        (0, 32) => dos16_sleep,
        (0, 33) => dos16_get_date_time,
        (0, 34) => dos16_alloc_seg,
        (0, 38) => dos16_realloc_seg,
        (0, 39) => dos16_free_seg,
        (0, 47) => dos16_get_mod_handle,
        (0, 48) => dos16_get_mod_name,
        (0, 49) => dos16_get_machine_mode,
        (0, 50) => dos_beep,
        (0, 59) => dos_close,
        (0, 77) => dos16_qhand_type,
        (0, 85) => ok,  // DosSetMaxFH
        (0, 89) => dos16_set_vec,
        (0, 91) => dos16_get_env,
        (0, 92) => dos16_get_version,
        (0, 94) => dos16_get_pid,
        (0, 120) => ok, // DosError
        (0, 127) => dos16_mem_avail,
        (0, 137) => dos16_read,
        (0, 138) => dos16_write,
        (0, 152) => dos16_scan_env,
        (0, 215) => dos16_page_size,
        // ---- 32-bit DOSCALLS ----
        (0, 209) => ok, // DosSetMaxFH
        (0, 212) => ok, // DosError
        (0, 224) => dos_query_htype,
        (0, 227) => dos_scan_env,
        (0, 229) => dos_sleep,
        (0, 230) => dos_get_date_time,
        (0, 234) => dos_exit,
        (0, 257) => dos_close,
        (0, 281) => dos_read,
        (0, 282) => dos_write,
        (0, 286) => dos_beep,
        (0, 291) => dos_query_cp,
        (0, 299) => dos_alloc_mem,
        (0, 304) => dos_free_mem,
        (0, 305) => dos_set_mem,
        (0, 306) => dos_query_mem,
        (0, 312) => dos_get_info_blocks,
        (0, 319) => dos_query_module_handle,
        (0, 320) => dos_query_module_name,
        (0, 348) => dos_query_sys_info,
        (0, 354) => dos_set_exception_handler,
        (0, 355) => dos_unset_exception_handler,
        (0, 382) => dos_set_rel_max_fh,
        (0, 425) => dos_flat_to_sel,
        (0, 426) => dos_sel_to_flat,
        _ => return None,
    };
    Some(h)
}

fn ok(_c: &mut Ctx) -> u32 {
    err::NO_ERROR
}

// ---- Handles ----

fn handle(c: &Ctx, hf: u32) -> Option<Handle> {
    c.process.handles.lock().get(hf as usize).cloned().flatten()
}

/// Write to a handle; returns (error, bytes written).
fn write_handle(c: &Ctx, hf: u32, buf: u32, len: u32) -> (u32, u32) {
    let Some(h) = handle(c, hf) else { return (err::INVALID_HANDLE, 0) };
    let Some(data) = c.read_vec(buf, len) else { return (err::INVALID_ADDRESS, 0) };
    match h {
        Handle::Con => {
            crate::console::write(&data);
            (0, len)
        }
        Handle::Nul => (0, len),
        Handle::File(_) => (err::ACCESS_DENIED, 0),
    }
}

fn read_handle(c: &Ctx, hf: u32, buf: u32, len: u32) -> (u32, u32) {
    let Some(h) = handle(c, hf) else { return (err::INVALID_HANDLE, 0) };
    match h {
        Handle::Con => {
            let line = crate::kbd::read_line(len as usize);
            let n = line.len().min(len as usize);
            if !c.write(buf, &line[..n]) {
                return (err::INVALID_ADDRESS, 0);
            }
            (0, n as u32)
        }
        Handle::Nul => (0, 0),
        Handle::File(f) => {
            let mut f = f.lock();
            let pos = f.pos as usize;
            let avail = f.data.len().saturating_sub(pos);
            let n = avail.min(len as usize);
            let chunk: Vec<u8> = f.data[pos..pos + n].to_vec();
            if !c.write(buf, &chunk) {
                return (err::INVALID_ADDRESS, 0);
            }
            f.pos += n as u64;
            (0, n as u32)
        }
    }
}

fn dos16_write(c: &mut Ctx) -> u32 {
    let (e, n) = write_handle(c, c.arg(0), c.ptr(1), c.arg(2));
    c.write_u16(c.ptr(3), n as u16);
    e
}

fn dos_write(c: &mut Ctx) -> u32 {
    let (e, n) = write_handle(c, c.arg(0), c.arg(1), c.arg(2));
    c.write_u32(c.arg(3), n);
    e
}

fn dos16_read(c: &mut Ctx) -> u32 {
    let (e, n) = read_handle(c, c.arg(0), c.ptr(1), c.arg(2));
    c.write_u16(c.ptr(3), n as u16);
    e
}

fn dos_read(c: &mut Ctx) -> u32 {
    let (e, n) = read_handle(c, c.arg(0), c.arg(1), c.arg(2));
    c.write_u32(c.arg(3), n);
    e
}

fn dos_close(c: &mut Ctx) -> u32 {
    let hf = c.arg(0) as usize;
    let mut hs = c.process.handles.lock();
    match hs.get_mut(hf) {
        Some(slot @ Some(_)) => {
            *slot = None;
            0
        }
        _ => err::INVALID_HANDLE,
    }
}

/// (type, device attribute) for DosQHandType / DosQueryHType.
fn htype(c: &Ctx, hf: u32) -> Option<(u32, u32)> {
    Some(match handle(c, hf)? {
        // HANDTYPE_DEVICE; attributes: char device, CON in/out.
        Handle::Con => (1, 0x8083),
        Handle::Nul => (1, 0x8084),
        Handle::File(_) => (0, 0),
    })
}

fn dos16_qhand_type(c: &mut Ctx) -> u32 {
    let Some((t, a)) = htype(c, c.arg(0)) else { return err::INVALID_HANDLE };
    c.write_u16(c.ptr(1), t as u16);
    c.write_u16(c.ptr(2), a as u16);
    0
}

fn dos_query_htype(c: &mut Ctx) -> u32 {
    let Some((t, a)) = htype(c, c.arg(0)) else { return err::INVALID_HANDLE };
    c.write_u32(c.arg(1), t);
    c.write_u32(c.arg(2), a);
    0
}

fn dos_set_rel_max_fh(c: &mut Ctx) -> u32 {
    // Report the current maximum.
    let n = c.process.handles.lock().len().max(20) as u32;
    c.write_u32(c.arg(1), n);
    0
}

// ---- Process ----

fn dos16_exit(c: &mut Ctx) -> u32 {
    exit(c, c.arg(0), c.arg(1) & 0xffff)
}

fn dos_exit(c: &mut Ctx) -> u32 {
    exit(c, c.arg(0), c.arg(1))
}

fn exit(c: &mut Ctx, action: u32, code: u32) -> u32 {
    let p = c.process.clone();
    // EXIT_THREAD (0) on the last thread ends the process too.
    if action == 0 && p.threads.lock().iter().filter(|t| t.state() != crate::task::thread::State::Dead).count() > 1 {
        sched::exit_current();
    }
    p.exit(0, code)
}

fn dos16_get_pid(c: &mut Ctx) -> u32 {
    // PIDINFO: pid, tid, pidParent.
    let a = c.ptr(0);
    c.write_u16(a, c.process.pid as u16);
    c.write_u16(a + 2, c.thread.tid as u16);
    c.write_u16(a + 4, c.process.ppid as u16);
    0
}

fn dos16_get_machine_mode(c: &mut Ctx) -> u32 {
    c.write_u8(c.ptr(0), 1); // MODE_PROTECTED
    0
}

/// OS/2 Warp 4.52 reports version 20.45.
pub const VERSION_MAJOR: u32 = 20;
pub const VERSION_MINOR: u32 = 45;

fn dos16_get_version(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), (VERSION_MAJOR << 8 | VERSION_MINOR) as u16);
    0
}

fn dos16_set_vec(c: &mut Ctx) -> u32 {
    // Exception vectors for 16-bit programs (divide, overflow, bound,
    // invalid opcode, NPX). Recorded but not yet delivered.
    let prev = c.ptr(2);
    if prev != 0 {
        c.write_u32(prev, 0);
    }
    0
}

fn dos16_get_env(c: &mut Ctx) -> u32 {
    let p = &c.process;
    c.write_u16(c.ptr(0), vm::tile_selector(p.env));
    c.write_u16(c.ptr(1), (p.cmdline - p.env) as u16);
    0
}

/// Find `name` in the environment; returns the address of its value.
fn scan_env(c: &Ctx, name: &str) -> Option<u32> {
    let mut a = c.process.env;
    loop {
        let s = c.read_str(a, 4096)?;
        if s.is_empty() {
            return None;
        }
        if let Some((k, _)) = s.split_once('=') {
            if k.eq_ignore_ascii_case(name) {
                return Some(a + k.len() as u32 + 1);
            }
        }
        a += s.len() as u32 + 1;
    }
}

fn dos_scan_env(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.arg(0), 256) else { return err::INVALID_PARAMETER };
    match scan_env(c, &name) {
        Some(v) => {
            c.write_u32(c.arg(1), v);
            0
        }
        None => err::ENVVAR_NOT_FOUND,
    }
}

fn dos16_scan_env(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.ptr(0), 256) else { return err::INVALID_PARAMETER };
    match scan_env(c, &name) {
        Some(v) => {
            let far = (vm::tile_selector(v) as u32) << 16 | (v & 0xffff);
            c.write_u32(c.ptr(1), far);
            0
        }
        None => err::ENVVAR_NOT_FOUND,
    }
}

fn dos_get_info_blocks(c: &mut Ctx) -> u32 {
    let ptib = c.arg(0);
    let ppib = c.arg(1);
    if ptib != 0 {
        c.write_u32(ptib, c.thread.tib());
    }
    if ppib != 0 {
        c.write_u32(ppib, c.process.info + process::info::PIB);
    }
    0
}

fn dos16_get_info_seg(c: &mut Ctx) -> u32 {
    c.write_u16(c.ptr(0), crate::infoseg::global_selector());
    c.write_u16(c.ptr(1), vm::tile_selector(c.process.info + process::info::LINFOSEG));
    0
}

// ---- Time ----

fn dos16_sleep(c: &mut Ctx) -> u32 {
    sched::sleep_ms(c.arg(0) as u64);
    0
}

fn dos_sleep(c: &mut Ctx) -> u32 {
    sched::sleep_ms(c.arg(0) as u64);
    0
}

fn write_datetime(c: &Ctx, a: u32) -> u32 {
    let t = crate::rtc::now();
    let hundredths = ((crate::irq::uptime_ms() / 10) % 100) as u8;
    // DATETIME: hours, minutes, seconds, hundredths, day, month, year,
    // timezone (minutes from UTC, -1 = undefined), weekday.
    let mut b = [0u8; 11];
    b[0] = t.hour;
    b[1] = t.minute;
    b[2] = t.second;
    b[3] = hundredths;
    b[4] = t.day;
    b[5] = t.month;
    b[6..8].copy_from_slice(&t.year.to_le_bytes());
    b[8..10].copy_from_slice(&(-1i16).to_le_bytes());
    b[10] = t.weekday;
    if c.write(a, &b) { 0 } else { err::INVALID_ADDRESS }
}

fn dos16_get_date_time(c: &mut Ctx) -> u32 {
    write_datetime(c, c.ptr(0))
}

fn dos_get_date_time(c: &mut Ctx) -> u32 {
    write_datetime(c, c.arg(0))
}

fn dos_beep(_c: &mut Ctx) -> u32 {
    0
}

// ---- 16-bit memory ----

fn dos16_alloc_seg(c: &mut Ctx) -> u32 {
    let size = match c.arg(0) {
        0 => 0x10000,
        n => n,
    };
    let r = c.space().alloc(size, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None);
    match r {
        Ok(base) => {
            c.space().set_seg_limit(base, size - 1);
            c.write_u16(c.ptr(1), vm::tile_selector(base));
            0
        }
        Err(e) => e.code(),
    }
}

fn dos16_realloc_seg(c: &mut Ctx) -> u32 {
    let size = match c.arg(0) {
        0 => 0x10000,
        n => n,
    };
    let base = vm::tiled_flat(c.arg(1) as u16, 0);
    let mut sp = c.space();
    let Some(r) = sp.regions.get(&base) else { return err::INVALID_ACCESS };
    let cur = r.size;
    if size > cur {
        if sp.resize(base, size).is_err() {
            return err::NOT_ENOUGH_MEMORY;
        }
    }
    sp.set_seg_limit(base, size - 1);
    0
}

fn dos16_free_seg(c: &mut Ctx) -> u32 {
    let base = vm::tiled_flat(c.arg(0) as u16, 0);
    match c.space().free(base) {
        Ok(()) => 0,
        Err(_) => err::INVALID_ACCESS,
    }
}

fn dos16_mem_avail(c: &mut Ctx) -> u32 {
    let free = crate::mm::phys::free_count() * 4096;
    c.write_u32(c.ptr(0), free.min(u32::MAX as u64) as u32);
    0
}

fn dos16_page_size(c: &mut Ctx) -> u32 {
    c.write_u32(c.ptr(0), 4096);
    0
}

// ---- 32-bit memory ----

fn dos_alloc_mem(c: &mut Ctx) -> u32 {
    let (ppb, cb, flags) = (c.arg(0), c.arg(1), c.arg(2));
    if flags & !(pag::READ | pag::WRITE | pag::EXECUTE | pag::GUARD | pag::COMMIT | pag::OBJ_TILE | pag::OBJ_ANY) != 0 {
        return err::INVALID_PARAMETER;
    }
    if flags & (pag::READ | pag::WRITE | pag::EXECUTE) == 0 {
        return err::INVALID_PARAMETER;
    }
    let r = c.space().alloc(cb, flags, SegKind::Data, None);
    match r {
        Ok(base) => {
            c.write_u32(ppb, base);
            0
        }
        Err(e) => e.code(),
    }
}

fn dos_free_mem(c: &mut Ctx) -> u32 {
    match c.space().free(c.arg(0)) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn dos_set_mem(c: &mut Ctx) -> u32 {
    match c.space().set_mem(c.arg(0), c.arg(1), c.arg(2)) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn dos_query_mem(c: &mut Ctx) -> u32 {
    let (addr, pcb, pflag) = (c.arg(0), c.arg(1), c.arg(2));
    let Some(want) = c.read_u32(pcb) else { return err::INVALID_PARAMETER };
    let res = c.space().query(addr, want);
    match res {
        Some((size, flags)) => {
            c.write_u32(pcb, size);
            c.write_u32(pflag, flags);
            0
        }
        None => err::INVALID_ADDRESS,
    }
}

fn dos_flat_to_sel(c: &mut Ctx) -> u32 {
    // Register-based in real OS/2 (EAX in, EAX out); our gate passes the
    // value on the stack for _System callers.
    let a = c.f.rax as u32;
    (vm::tile_selector(a) as u32) << 16 | (a & 0xffff)
}

fn dos_sel_to_flat(c: &mut Ctx) -> u32 {
    let v = c.f.rax as u32;
    vm::tiled_flat((v >> 16) as u16, v as u16)
}

// ---- Exceptions ----

fn dos_set_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.arg(0);
    let tib = c.thread.tib();
    let Some(prev) = c.read_u32(tib) else { return err::INVALID_ADDRESS };
    if !c.write_u32(rec, prev) {
        return err::INVALID_ADDRESS;
    }
    c.write_u32(tib, rec);
    0
}

fn dos_unset_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.arg(0);
    let tib = c.thread.tib();
    let Some(prev) = c.read_u32(rec) else { return err::INVALID_ADDRESS };
    c.write_u32(tib, prev);
    0
}

// ---- Modules ----

fn module_name_for(c: &Ctx, hmod: u32) -> Option<String> {
    if hmod == 0 {
        return Some(c.process.exe.path.clone());
    }
    loader::by_handle(hmod as u16).map(|m| m.path.clone())
}

fn dos_query_module_name(c: &mut Ctx) -> u32 {
    let Some(name) = module_name_for(c, c.arg(0)) else { return err::INVALID_HANDLE };
    if name.len() + 1 > c.arg(1) as usize {
        return err::BUFFER_OVERFLOW;
    }
    c.write_str(c.arg(2), &name);
    0
}

fn dos16_get_mod_name(c: &mut Ctx) -> u32 {
    let Some(name) = module_name_for(c, c.arg(0)) else { return err::INVALID_HANDLE };
    if name.len() + 1 > c.arg(1) as usize {
        return err::BUFFER_OVERFLOW;
    }
    c.write_str(c.ptr(2), &name);
    0
}

fn find_module_handle(name: &str) -> Option<u16> {
    loader::find_loaded(&loader::module_name(name)).map(|m| m.hmte)
}

fn dos_query_module_handle(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.arg(0), 260) else { return err::INVALID_PARAMETER };
    match find_module_handle(&name) {
        Some(h) => {
            c.write_u32(c.arg(1), h as u32);
            0
        }
        None => err::MOD_NOT_FOUND,
    }
}

fn dos16_get_mod_handle(c: &mut Ctx) -> u32 {
    let Some(name) = c.read_str(c.ptr(0), 260) else { return err::INVALID_PARAMETER };
    match find_module_handle(&name) {
        Some(h) => {
            c.write_u16(c.ptr(1), h);
            0
        }
        None => err::MOD_NOT_FOUND,
    }
}

// ---- System information ----

fn dos_query_cp(c: &mut Ctx) -> u32 {
    let (cb, buf, pcb) = (c.arg(0), c.arg(1), c.arg(2));
    let cps = [850u32, 850, 437];
    let n = (cb as usize / 4).min(cps.len());
    for (i, cp) in cps.iter().take(n).enumerate() {
        c.write_u32(buf + 4 * i as u32, *cp);
    }
    c.write_u32(pcb, (n * 4) as u32);
    if n < cps.len() { err::BUFFER_OVERFLOW } else { 0 }
}

fn sys_info_value(i: u32) -> u32 {
    let total = crate::mm::phys::total_frames() * 4096;
    let free = crate::mm::phys::free_count() * 4096;
    match i {
        1 => 260,              // QSV_MAX_PATH_LENGTH
        2 => 16,               // QSV_MAX_TEXT_SESSIONS
        3 => 16,               // QSV_MAX_PM_SESSIONS
        4 => 0,                // QSV_MAX_VDM_SESSIONS
        5 => 3,                // QSV_BOOT_DRIVE (C:)
        6 => 1,                // QSV_DYN_PRI_VARIATION
        7 => 3,                // QSV_MAX_WAIT
        8 => 32,               // QSV_MIN_SLICE
        9 => 248,              // QSV_MAX_SLICE
        10 => 4096,            // QSV_PAGE_SIZE
        11 => VERSION_MAJOR,   // QSV_VERSION_MAJOR
        12 => VERSION_MINOR,   // QSV_VERSION_MINOR
        13 => 0,               // QSV_VERSION_REVISION
        14 => crate::irq::uptime_ms() as u32, // QSV_MS_COUNT
        15 => crate::rtc::unix_time(&crate::rtc::now()), // QSV_TIME_LOW
        16 => 0,               // QSV_TIME_HIGH
        17 => total.min(u32::MAX as u64) as u32, // QSV_TOTPHYSMEM
        18 => 0,               // QSV_TOTRESMEM
        19 => free.min(u32::MAX as u64) as u32, // QSV_TOTAVAILMEM
        20 => 0x1c00_0000,     // QSV_MAXPRMEM
        21 => 0x0c00_0000,     // QSV_MAXSHMEM
        22 => 31,              // QSV_TIMER_INTERVAL (tenths of ms)
        23 => 255,             // QSV_MAX_COMP_LENGTH
        24 => 1,               // QSV_FOREGROUND_FS_SESSION
        25 => 1,               // QSV_FOREGROUND_PROCESS
        26 => 1,               // QSV_NUMPROCESSORS
        27 => 0,               // QSV_MAXHPRMEM
        28 => 0,               // QSV_MAXHSHMEM
        29 => 1000,            // QSV_MAXPROCESSES
        30 => 512,             // QSV_VIRTUALADDRESSLIMIT (MB)
        _ => 0,
    }
}

fn dos_query_sys_info(c: &mut Ctx) -> u32 {
    let (first, last, buf, cb) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if first == 0 || last < first || last > 30 {
        return err::INVALID_PARAMETER;
    }
    if (last - first + 1) * 4 > cb {
        return err::BUFFER_OVERFLOW;
    }
    for (n, i) in (first..=last).enumerate() {
        c.write_u32(buf + 4 * n as u32, sys_info_value(i));
    }
    0
}
