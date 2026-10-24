//! VDM.EXE: a DOS session (docs/DOS.md).
//!
//! The kernel starts it as the leader of a DOS session with the DOS
//! program and its arguments (`VDM.EXE C:\GAMES\X.EXE /args`). It runs
//! rs2's DOS machine (crate `dos`, on the `x86emu` CPU) with the files
//! of the OS/2 drives, shows the machine's screen through the session
//! (text cells, or a graphics picture the kernel scales), and feeds it
//! the session's raw keyboard and mouse (RS2API 400-405).
//!
//! DOS settings come in the environment as OS/2 passes them
//! (`DOS_HIGH=1`, `EMS_MEMORY_LIMIT=2048`, ...); the other variables
//! form the DOS environment.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ::dos::video::{Kind, Vga};
use ::dos::{Config, Create, DateTime, DirEntry, Event, Host, Machine, OpenMode};
use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "RS2API.4"]
    fn Rs2DebugTrace(text: *const u8) -> u32;
    #[link_name = "RS2API.13"]
    fn Rs2VioQueryFont(sid: u32, cy: u32, glyphs: *mut u8, cb: u32) -> u32;
    #[link_name = "RS2API.400"]
    fn Rs2VdmRegister(fl: u32) -> u32;
    #[link_name = "RS2API.401"]
    fn Rs2VdmReadInput(ev: *mut VdmEvent, max: u32, count: *mut u32, ms: u32) -> u32;
    #[link_name = "RS2API.402"]
    fn Rs2VdmSetText(t: *const VdmText, cells: *const u16) -> u32;
    #[link_name = "RS2API.403"]
    fn Rs2VdmSetPixels(cx: u32, cy: u32, y0: u32, rows: u32, data: *const u8, pal: *const u32) -> u32;
    #[link_name = "RS2API.301"]
    fn Rs2AudioOpen(rate: u32, channels: u32, bits: u32, h: *mut u32) -> u32;
    #[link_name = "RS2API.302"]
    fn Rs2AudioWrite(h: u32, data: *const u8, cb: u32) -> u32;
    #[link_name = "RS2API.303"]
    fn Rs2AudioQueryPos(h: u32, bytes: *mut u32) -> u32;
    #[link_name = "RS2API.306"]
    fn Rs2AudioClose(h: u32) -> u32;
}

// ---- The PC speaker ----

/// The speaker's frequency (0 = silent), set by the machine's thread.
static SPEAKER_HZ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// 0 not started, 1 playing, 2 no audio.
static SPEAKER_STATE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
const RATE: u32 = 22050;

/// The speaker thread: a square wave at the current frequency, kept
/// about 60 ms ahead of what has been played.
unsafe extern "C" fn speaker_thread(h: u32) {
    use core::sync::atomic::Ordering;
    let mut phase = 0u32;
    let mut written = 0u32;
    let mut buf = [0x80u8; 441];
    loop {
        let mut played = 0;
        unsafe { Rs2AudioQueryPos(h, &mut played) };
        if written.wrapping_sub(played) > RATE * 60 / 1000 {
            unsafe { DosSleep(10) };
            continue;
        }
        let hz = SPEAKER_HZ.load(Ordering::Relaxed);
        for b in buf.iter_mut() {
            *b = if hz == 0 {
                0x80
            } else {
                phase = phase.wrapping_add(hz * 2);
                if (phase / RATE) % 2 == 0 { 0xa0 } else { 0x60 }
            };
        }
        if unsafe { Rs2AudioWrite(h, buf.as_ptr(), buf.len() as u32) } != 0 {
            return;
        }
        written = written.wrapping_add(buf.len() as u32);
    }
}

fn speaker(hz: Option<u32>) {
    use core::sync::atomic::Ordering;
    SPEAKER_HZ.store(hz.unwrap_or(0), Ordering::Relaxed);
    if hz.is_some() && SPEAKER_STATE.load(Ordering::Relaxed) == 0 {
        let mut h = 0;
        if unsafe { Rs2AudioOpen(RATE, 1, 8, &mut h) } != 0 {
            SPEAKER_STATE.store(2, Ordering::Relaxed);
            return;
        }
        let mut tid = 0;
        unsafe { DosCreateThread(&mut tid, speaker_thread, h, 0, 16384) };
        SPEAKER_STATE.store(1, Ordering::Relaxed);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VdmEvent {
    kind: u16,
    flags: u16,
    a: i32,
    b: i32,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct VdmText {
    cols: u16,
    rows: u16,
    cur_row: u16,
    cur_col: u16,
    cur_start: u8,
    cur_end: u8,
    flags: u8,
    cell_h: u8,
    colours: [u32; 16],
}

const OPEN_FLAGS_NOINHERIT: u32 = 0x0080;
const OPEN_FLAGS_FAIL_ON_ERROR: u32 = 0x2000;

fn cz(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = os2::text_bytes(s).into_owned();
    v.push(0);
    v
}

fn trace(s: &str) {
    let c = cz(s);
    unsafe { Rs2DebugTrace(c.as_ptr()) };
}

struct Os2Host {
    /// The Sound Blaster's stream: (handle, rate); handle 0 = none,
    /// u32::MAX = no audio hardware.
    sb_stream: (u32, u32),
    /// Input read while waiting, for the next poll.
    pending: VecDeque<Event>,
    tmr_freq: u64,
    /// What the session shows now.
    last_text: Option<(VdmText, Vec<u16>)>,
    last_pix: Option<(usize, usize, Vec<u8>, [u32; 256])>,
    pix: Vec<u8>,
    drives: u32,
}

fn convert(e: &VdmEvent) -> Option<Event> {
    match e.kind {
        1 => Some(Event::Key { scan: e.a as u8, ch: e.flags as u8 }),
        2 => Some(Event::Mouse { dx: e.a, dy: e.b, buttons: e.flags as u8 }),
        3 => Some(Event::MouseAbs { x: e.a as u32, y: e.b as u32, buttons: e.flags as u8 }),
        _ => None,
    }
}

impl Os2Host {
    fn read_input(&mut self, ms: u32) {
        let mut buf = [VdmEvent::default(); 64];
        let mut n = 0;
        unsafe { Rs2VdmReadInput(buf.as_mut_ptr(), 64, &mut n, ms) };
        for e in &buf[..n as usize] {
            if let Some(ev) = convert(e) {
                self.pending.push_back(ev);
            }
        }
    }

    fn file_status(&mut self, h: u32) -> Result<FileStatus3, u16> {
        let mut st = FileStatus3::default();
        let rc = unsafe { DosQueryFileInfo(h, 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
        if rc != 0 { Err(rc as u16) } else { Ok(st) }
    }

    fn path_status(&mut self, path: &str) -> Result<FileStatus3, u16> {
        let p = cz(path);
        let mut st = FileStatus3::default();
        let rc = unsafe { DosQueryPathInfo(p.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
        if rc != 0 { Err(rc as u16) } else { Ok(st) }
    }

    fn show_text(&mut self, v: &Vga, ram: &[u8], cols: u16, rows: u16, base: u32) {
        let start = (v.text_start() * 2) as usize;
        let n = cols as usize * rows as usize;
        let mut cells = Vec::with_capacity(n);
        for i in 0..n {
            let a = base as usize + start + i * 2;
            let lo = ram.get(a).copied().unwrap_or(0x20);
            let hi = ram.get(a + 1).copied().unwrap_or(0x07);
            cells.push(lo as u16 | (hi as u16) << 8);
        }
        let (pos, shown, cs, ce) = v.cursor();
        let off = pos.wrapping_sub(v.text_start());
        let (cr, cc) = ((off / cols as u32) as u16, (off % cols as u32) as u16);
        let cell_h = (v.crtc[9] & 0x1f) + 1;
        let mut flags = 0;
        if shown && cs <= ce.max(cs) && cr < rows {
            flags |= 1;
        }
        if v.blink() {
            flags |= 2;
        }
        let t = VdmText { cols, rows, cur_row: cr, cur_col: cc, cur_start: cs, cur_end: ce, flags, cell_h, colours: v.text_palette() };
        if let Some((lt, lc)) = &self.last_text {
            if *lt == t && *lc == cells {
                return;
            }
        }
        unsafe { Rs2VdmSetText(&t, cells.as_ptr()) };
        self.last_text = Some((t, cells));
        self.last_pix = None;
    }

    fn show_pixels(&mut self, v: &Vga, ram: &[u8]) {
        let mut px = core::mem::take(&mut self.pix);
        let (w, h) = v.render(ram, &mut px);
        if w == 0 || h == 0 {
            self.pix = px;
            return;
        }
        let pal = v.palette();
        // The rows that changed since the last picture.
        let (y0, y1) = match &self.last_pix {
            Some((lw, lh, old, lpal)) if *lw == w && *lh == h && *lpal == pal => {
                let first = (0..h).find(|&y| px[y * w..y * w + w] != old[y * w..y * w + w]);
                match first {
                    None => {
                        self.pix = px;
                        return;
                    }
                    Some(f) => {
                        let last = (f..h).rev().find(|&y| px[y * w..y * w + w] != old[y * w..y * w + w]).unwrap_or(f);
                        (f, last + 1)
                    }
                }
            }
            _ => (0, h),
        };
        unsafe { Rs2VdmSetPixels(w as u32, h as u32, y0 as u32, (y1 - y0) as u32, px[y0 * w..].as_ptr(), pal.as_ptr()) };
        match &mut self.last_pix {
            Some((lw, lh, old, lpal)) if *lw == w && *lh == h => {
                old[y0 * w..y1 * w].copy_from_slice(&px[y0 * w..y1 * w]);
                *lpal = pal;
            }
            _ => self.last_pix = Some((w, h, px.clone(), pal)),
        }
        self.last_text = None;
        self.pix = px;
    }
}

impl Host for Os2Host {
    fn open(&mut self, path: &str, mode: OpenMode) -> Result<u32, u16> {
        let p = cz(path);
        let (mut h, mut action) = (0, 0);
        let share = match mode.sharing() {
            0 => OPEN_SHARE_DENYNONE,
            s => (s as u32) << 4,
        };
        let m = (mode.access() as u32).min(2) | share | OPEN_FLAGS_NOINHERIT | OPEN_FLAGS_FAIL_ON_ERROR;
        let rc = unsafe { DosOpen(p.as_ptr(), &mut h, &mut action, 0, 0, OPEN_ACTION_FAIL_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, m, core::ptr::null_mut()) };
        if rc != 0 {
            return Err(rc as u16);
        }
        Ok(h)
    }
    fn create(&mut self, path: &str, attr: u8, how: Create) -> Result<u32, u16> {
        let p = cz(path);
        let (mut h, mut action) = (0, 0);
        let act = match how {
            Create::Truncate => OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            Create::New => OPEN_ACTION_CREATE_IF_NEW,
        };
        let m = OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE | OPEN_FLAGS_NOINHERIT;
        let rc = unsafe { DosOpen(p.as_ptr(), &mut h, &mut action, 0, (attr & 0x27) as u32, act, m, core::ptr::null_mut()) };
        if rc != 0 {
            return Err(if rc == 110 { 80 } else { rc as u16 });
        }
        Ok(h)
    }
    fn close(&mut self, h: u32) {
        unsafe { DosClose(h) };
    }
    fn read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u16> {
        let mut n = 0;
        let rc = unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 { Err(rc as u16) } else { Ok(n as usize) }
    }
    fn write(&mut self, h: u32, buf: &[u8]) -> Result<usize, u16> {
        let mut n = 0;
        let rc = unsafe { DosWrite(h, buf.as_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 { Err(rc as u16) } else { Ok(n as usize) }
    }
    fn seek(&mut self, h: u32, off: i64, whence: u8) -> Result<u64, u16> {
        let mut pos = 0;
        let rc = unsafe { DosSetFilePtr(h, off as i32, whence as u32, &mut pos) };
        if rc != 0 { Err(rc as u16) } else { Ok(pos as u64) }
    }
    fn set_size(&mut self, h: u32, size: u64) -> Result<(), u16> {
        let rc = unsafe { DosSetFileSize(h, size as u32) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn file_time(&mut self, h: u32) -> Result<(u16, u16), u16> {
        let st = self.file_status(h)?;
        Ok((st.fdate_write, st.ftime_write))
    }
    fn set_file_time(&mut self, h: u32, date: u16, time: u16) -> Result<(), u16> {
        let mut st = self.file_status(h)?;
        st.fdate_write = date;
        st.ftime_write = time;
        let rc = unsafe { DosSetFileInfo(h, 1, &st as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn delete(&mut self, path: &str) -> Result<(), u16> {
        let p = cz(path);
        let rc = unsafe { DosDelete(p.as_ptr()) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), u16> {
        let (a, b) = (cz(from), cz(to));
        let rc = unsafe { DosMove(a.as_ptr(), b.as_ptr()) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn mkdir(&mut self, path: &str) -> Result<(), u16> {
        let p = cz(path);
        let rc = unsafe { DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn rmdir(&mut self, path: &str) -> Result<(), u16> {
        let p = cz(path);
        let rc = unsafe { DosDeleteDir(p.as_ptr()) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn get_attr(&mut self, path: &str) -> Result<u8, u16> {
        if path.len() <= 3 {
            return Ok(0x10);
        }
        Ok(self.path_status(path)?.attr as u8)
    }
    fn set_attr(&mut self, path: &str, attr: u8) -> Result<(), u16> {
        let mut st = self.path_status(path)?;
        st.attr = (st.attr & 0x10) | (attr as u32 & 0x27);
        let p = cz(path);
        let rc = unsafe { DosSetPathInfo(p.as_ptr(), 1, &st as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32, 0) };
        if rc != 0 { Err(rc as u16) } else { Ok(()) }
    }
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, u16> {
        let spec = if path.ends_with('\\') { alloc::format!("{path}*") } else { alloc::format!("{path}\\*") };
        let p = cz(&spec);
        let mut hdir = HDIR_CREATE;
        let mut buf = vec![0u8; 8192];
        let mut count = 64;
        let mut out = Vec::new();
        let mut rc = unsafe { DosFindFirst(p.as_ptr(), &mut hdir, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
        if rc == 18 {
            return Ok(out);
        }
        if rc != 0 {
            return Err(if rc == 2 { 3 } else { rc as u16 });
        }
        while rc == 0 {
            let mut at = 0usize;
            for _ in 0..count {
                let rd16 = |o: usize| u16::from_le_bytes([buf[at + o], buf[at + o + 1]]);
                let rd32 = |o: usize| u32::from_le_bytes([buf[at + o], buf[at + o + 1], buf[at + o + 2], buf[at + o + 3]]);
                let next = rd32(0) as usize;
                let n = buf[at + 28] as usize;
                let name: String = buf[at + 29..at + 29 + n].iter().map(|&b| b as char).collect();
                out.push(DirEntry { name, attr: rd32(24) as u8, size: rd32(16), date: rd16(12), time: rd16(14) });
                if next == 0 {
                    break;
                }
                at += next;
            }
            count = 64;
            rc = unsafe { DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
        }
        unsafe { DosFindClose(hdir) };
        Ok(out)
    }
    fn drive_exists(&mut self, d: u8) -> bool {
        self.drives & (1 << d) != 0
    }
    fn disk_space(&mut self, d: u8) -> Result<(u16, u16, u32, u32), u16> {
        let mut a = FsAllocate::default();
        let rc = unsafe { DosQueryFSInfo(d as u32 + 1, 1, &mut a as *mut _ as *mut u8, core::mem::size_of::<FsAllocate>() as u32) };
        if rc != 0 {
            return Err(rc as u16);
        }
        // DOS reports at most 2 GB: scale clusters to fit 16 bits of
        // sectors per cluster.
        let (mut spc, mut free, mut total) = (a.sectors_per_unit, a.units_avail, a.units);
        while spc < 64 && total > 0xffff {
            spc *= 2;
            free /= 2;
            total /= 2;
        }
        Ok((spc.min(0xffff) as u16, a.bytes_per_sector, free, total))
    }
    fn volume_label(&mut self, d: u8) -> Option<String> {
        let mut b = [0u8; 64];
        let rc = unsafe { DosQueryFSInfo(d as u32 + 1, 2, b.as_mut_ptr(), b.len() as u32) };
        if rc != 0 || b[4] == 0 {
            return None;
        }
        Some(b[5..5 + (b[4] as usize).min(11)].iter().map(|&c| c as char).collect())
    }
    fn initial_dir(&mut self, d: u8) -> String {
        let mut buf = [0u8; 260];
        let mut n = buf.len() as u32;
        if unsafe { DosQueryCurrentDir(d as u32 + 1, buf.as_mut_ptr(), &mut n) } != 0 {
            return String::new();
        }
        buf.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
    }
    fn now(&mut self) -> DateTime {
        let mut t = os2::DateTime::default();
        unsafe { DosGetDateTime(&mut t) };
        DateTime { year: t.year, month: t.month, day: t.day, hour: t.hours, minute: t.minutes, second: t.seconds, hundredths: t.hundredths, weekday: t.weekday }
    }
    fn micros(&mut self) -> u64 {
        let mut t = 0u64;
        unsafe { DosTmrQueryTime(&mut t) };
        (t as u128 * 1_000_000 / self.tmr_freq.max(1) as u128) as u64
    }
    fn poll(&mut self, out: &mut Vec<Event>) {
        if self.pending.is_empty() {
            self.read_input(0);
        }
        out.extend(self.pending.drain(..));
    }
    fn wait(&mut self, us: u64) {
        let ms = (us / 1000).clamp(1, 50) as u32;
        self.read_input(ms);
    }
    fn present(&mut self, v: &Vga, ram: &[u8]) {
        match v.kind() {
            Kind::Text { cols, rows, base } => self.show_text(v, ram, cols, rows, base),
            Kind::Graphics { .. } => self.show_pixels(v, ram),
        }
    }
    fn log(&mut self, msg: &str) {
        trace(msg);
    }
    fn speaker(&mut self, hz: Option<u32>) {
        speaker(hz);
    }
    fn pcm(&mut self, rate: u32, data: &[u8]) {
        let (h, r) = self.sb_stream;
        if h == u32::MAX {
            return;
        }
        if h == 0 || r != rate {
            if h != 0 {
                unsafe { Rs2AudioClose(h) };
            }
            let mut nh = 0;
            if unsafe { Rs2AudioOpen(rate.clamp(4000, 96000), 1, 8, &mut nh) } != 0 {
                self.sb_stream = (u32::MAX, 0);
                return;
            }
            self.sb_stream = (nh, rate);
        }
        unsafe { Rs2AudioWrite(self.sb_stream.0, data.as_ptr(), data.len() as u32) };
    }
    fn font(&mut self, height: u8) -> Option<Vec<u8>> {
        let mut f = vec![0u8; 256 * height as usize];
        let rc = unsafe { Rs2VioQueryFont(0, height as u32, f.as_mut_ptr(), f.len() as u32) };
        (rc == 0).then_some(f)
    }
}

/// The value of `KEY=value` settings, as numbers.
fn num(v: &str) -> Option<u32> {
    v.trim().parse().ok()
}

fn main(s: &os2::Startup) -> u32 {
    unsafe { Rs2VdmRegister(0) };
    let args: String = s.args().iter().map(|&b| b as char).collect();
    let args = args.trim();
    let (prog, tail) = match args.find(' ') {
        Some(i) => (&args[..i], &args[i..]),
        None => (args, ""),
    };
    let prog = if prog.is_empty() { "C:\\OS2\\MDOS\\COMMAND.COM" } else { prog };
    // A batch file runs under COMMAND.COM.
    let batch_tail;
    let (prog, tail) = if prog.to_ascii_uppercase().ends_with(".BAT") {
        batch_tail = alloc::format!(" /C {prog}{tail}");
        ("C:\\OS2\\MDOS\\COMMAND.COM", batch_tail.as_str())
    } else {
        (prog, tail)
    };
    // DOS settings and the environment.
    let mut cfg = Config::default();
    let mut env: Vec<String> = Vec::new();
    let mut os2_path = String::new();
    unsafe {
        let mut p = s.env;
        while *p != 0 {
            let start = p;
            while *p != 0 {
                p = p.add(1);
            }
            let bytes = core::slice::from_raw_parts(start, p.offset_from(start) as usize);
            p = p.add(1);
            let var: String = bytes.iter().map(|&b| b as char).collect();
            let Some((k, v)) = var.split_once('=') else { continue };
            let key = k.to_ascii_uppercase();
            let on = |v: &str| matches!(v.trim().to_ascii_uppercase().as_str(), "1" | "ON" | "YES");
            match key.as_str() {
                "DOS_HIGH" => cfg.high = on(v),
                "DOS_UMB" => cfg.umb = on(v),
                "EMS_MEMORY_LIMIT" => cfg.ems_kb = num(v).unwrap_or(cfg.ems_kb),
                "XMS_MEMORY_LIMIT" => cfg.xms_kb = num(v).unwrap_or(cfg.xms_kb),
                "DPMI_MEMORY_LIMIT" => cfg.dpmi_kb = num(v).map_or(cfg.dpmi_kb, |m| m * 1024),
                "HW_NOFPU" => cfg.fpu = !on(v),
                "VDM_TRACE" => cfg.trace = on(v),
                "PATH" => os2_path = String::from(v),
                "COMSPEC" | "BEGINLIBPATH" | "ENDLIBPATH" | "LIBPATH" | "DPATH" | "KEYS" | "SYSTEM_INI" | "OS2_SHELL" | "RUNWORKPLACE" | "AUTOSTART" | "USER_INI" | "PROMPT" => {}
                k if k.starts_with("DOS_") || k.starts_with("EMS_") || k.starts_with("XMS_") || k.starts_with("DPMI_") || k.starts_with("VIDEO_") || k.starts_with("HW_") || k.starts_with("IDLE_") || k.starts_with("KBD_") || k.starts_with("MOUSE_") => {}
                _ => env.push(var.clone()),
            }
        }
    }
    let mut dos_env = alloc::vec![String::from("COMSPEC=C:\\OS2\\MDOS\\COMMAND.COM"), alloc::format!("PATH=C:\\OS2\\MDOS;{}", os2_path), String::from("PROMPT=$P$G")];
    dos_env.extend(env);
    cfg.env = dos_env;
    let mut freq = 0u32;
    unsafe { DosTmrQueryFreq(&mut freq) };
    let (mut cur, mut drives) = (0, 0);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut drives) };
    let mut host = Os2Host { sb_stream: (0, 0), pending: VecDeque::new(), tmr_freq: freq as u64, last_text: None, last_pix: None, pix: Vec::new(), drives };
    let mut m = Machine::new(&mut host, cfg);
    if cur >= 1 && cur <= 26 {
        m.dos.drive = (cur - 1) as u8;
    }
    if let Err(e) = m.start(&mut host, prog, tail) {
        let msg = alloc::format!("Cannot run {prog} in a DOS session: DOS error {e}.\r\n");
        os2::print(&msg);
        trace(&msg);
        unsafe { DosSleep(3000) };
        return 1;
    }
    let t0 = host.micros();
    let code = m.run(&mut host);
    let us = host.micros().saturating_sub(t0).max(1);
    trace(&alloc::format!("vdm: {prog} ended with {code}: {} instructions in {} ms ({} MIPS)", m.cpu.icount, us / 1000, m.cpu.icount / us));
    code as u32
}
