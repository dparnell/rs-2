//! The DOS kernel: INT 20h-2Fh. State that programs look at directly
//! (PSPs, MCBs, the environment, the DTA, the list of lists, the current
//! directory structures, the InDOS flag) lives in guest memory in DOS's
//! layout; the rest (open files, searches) is kept here.

mod con;
mod fcb;
mod file;
mod mem;
mod proc;
mod country;

pub use file::{fcb_form, is_short as file_is_short, matches, Name};
pub use proc::pack_dt;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use x86emu::flags::CF;
use x86emu::{DS, ES};

use crate::{Cb, Host, Machine};

pub use file::Device;

/// DOS's own data segment (list of lists, SDA, CDS, SFT mirror).
pub const DOSSEG: u16 = 0x0070;
pub const SYSVARS: u16 = 0x0026;
pub const DEVICES: u16 = 0x0100;
pub const SDA: u16 = 0x0300;
pub const CDS: u16 = 0x0400;
pub const CDS_SIZE: u16 = 0x58;
pub const DPB: u16 = 0x0d00;
pub const DPB_SIZE: u16 = 0x21;
pub const SFT: u16 = 0x1060;
pub const SFT_SIZE: u16 = 0x3b;
pub const SFT_COUNT: u16 = 40;
pub const COUNTRY: u16 = 0x1a00;
pub const UPCASE: u16 = 0x1a40;
pub const FUCASE: u16 = 0x1ac2;
pub const FCHARS: u16 = 0x1b44;
pub const COLLATE: u16 = 0x1b6c;
pub const DBCS: u16 = 0x1c6e;
/// The DOS kernel's own PSP (owner of the first environment).
pub const SYS_PSP: u16 = 0x0060;
/// First MCB of conventional memory.
pub const FIRST_MCB: u16 = 0x0200;

/// DOS error codes.
pub mod err {
    pub const INVALID_FUNCTION: u16 = 1;
    pub const FILE_NOT_FOUND: u16 = 2;
    pub const PATH_NOT_FOUND: u16 = 3;
    pub const TOO_MANY_FILES: u16 = 4;
    pub const ACCESS_DENIED: u16 = 5;
    pub const INVALID_HANDLE: u16 = 6;
    pub const MCB_DESTROYED: u16 = 7;
    pub const NO_MEMORY: u16 = 8;
    pub const INVALID_BLOCK: u16 = 9;
    pub const BAD_ENVIRONMENT: u16 = 10;
    pub const BAD_FORMAT: u16 = 11;
    pub const INVALID_ACCESS: u16 = 12;
    pub const INVALID_DRIVE: u16 = 15;
    pub const CURRENT_DIRECTORY: u16 = 16;
    pub const NOT_SAME_DEVICE: u16 = 17;
    pub const NO_MORE_FILES: u16 = 18;
    pub const SHARING_VIOLATION: u16 = 32;
    pub const FILE_EXISTS: u16 = 80;
}

/// One open file or device (a system file table entry).
#[derive(Clone, Debug)]
pub struct Sft {
    pub refs: u16,
    pub dev: Option<Device>,
    pub host: u32,
    pub path: String,
    /// Open mode (with bit 7: not inherited).
    pub mode: u8,
    /// Device information word (IOCTL 00h).
    pub info: u16,
    pub owner: u16,
}

/// Search state of find first/next (the DTA keeps its id).
pub struct Search {
    pub entries: Vec<crate::DirEntry>,
    pub pos: usize,
}

/// A parent waiting for its child (exec 4B00h).
#[derive(Clone)]
pub struct ExecFrame {
    pub child: u16,
    pub regs: [u32; 8],
    pub segs: [u16; 6],
    pub flags: u32,
}

/// Buffered keyboard input in progress (0Ah, cooked CON reads).
#[derive(Clone, Default)]
pub struct LineEdit {
    pub buf: Vec<u8>,
    pub max: usize,
    /// The previous line (F3 repeats it).
    pub template: Vec<u8>,
    pub start_col: u8,
}

pub struct Dos {
    pub psp: u16,
    pub drive: u8,
    pub cwd: [String; 26],
    pub sft: Vec<Option<Sft>>,
    pub searches: BTreeMap<u32, Search>,
    pub next_search: u32,
    pub verify: bool,
    pub break_on: bool,
    pub break_pending: bool,
    pub strategy: u16,
    pub umb_link: bool,
    pub umb_start: u16,
    /// Extended error: code, class, action, locus.
    pub last_error: (u16, u8, u8, u8),
    /// AL exit code, AH termination type (4Dh).
    pub return_code: u16,
    pub exec_stack: Vec<ExecFrame>,
    pub line: Option<LineEdit>,
    pub template: Vec<u8>,
    /// Characters of a cooked console line not yet returned.
    pub con_ahead: Vec<u8>,
    pub wait_until: u64,
    pub root_psp: u16,
    pub lastdrive: u8,
    pub switchar: u8,
    /// Read-ahead of a character from INT 21h 06h/07h/08h's two-byte keys.
    pub pending_scan: Option<u8>,
    /// Drives that exist.
    pub present: [bool; 26],
    pub codepage: u16,
    /// FCB find first/next: drive, entries, position.
    pub fcb_search: Option<(u8, Vec<crate::DirEntry>, usize)>,
}

impl Dos {
    pub fn new() -> Dos {
        Dos {
            psp: SYS_PSP,
            drive: 2,
            cwd: Default::default(),
            sft: Vec::new(),
            searches: BTreeMap::new(),
            next_search: 1,
            verify: false,
            break_on: false,
            break_pending: false,
            strategy: 0,
            umb_link: false,
            umb_start: 0xffff,
            last_error: (0, 0, 0, 0),
            return_code: 0,
            exec_stack: Vec::new(),
            line: None,
            template: Vec::new(),
            con_ahead: Vec::new(),
            wait_until: 0,
            root_psp: 0,
            lastdrive: 26,
            switchar: b'/',
            pending_scan: None,
            present: [false; 26],
            codepage: 437,
            fcb_search: None,
        }
    }
}

impl Default for Dos {
    fn default() -> Self {
        Self::new()
    }
}

fn put(m: &mut Machine, seg: u16, off: u16, data: &[u8]) {
    let a = Machine::lin(seg, off);
    for (i, b) in data.iter().enumerate() {
        m.hw.ram[a as usize + i] = *b;
    }
}

impl Machine {
    pub(crate) fn dos_init<H: Host>(&mut self, host: &mut H) {
        let ds = DOSSEG;
        // Devices: NUL (in the list of lists) -> CON -> AUX -> PRN ->
        // CLOCK$ -> COM1 -> LPT1 -> LPT2 -> LPT3 -> EMMXXXX0 (if EMS).
        let names: [&[u8; 8]; 8] = [b"CON     ", b"AUX     ", b"PRN     ", b"CLOCK$  ", b"COM1    ", b"LPT1    ", b"LPT2    ", b"LPT3    "];
        let attrs = [0x8013u16, 0x8000, 0xa0c0, 0x8008, 0x8000, 0xa0c0, 0xa0c0, 0xa0c0];
        let retf = DEVICES + 0x100;
        put(self, ds, retf, &[0xcb]);
        for (i, n) in names.iter().enumerate() {
            let off = DEVICES + i as u16 * 18;
            let next = if i + 1 < names.len() { DEVICES + (i as u16 + 1) * 18 } else { 0xffff };
            let mut h = [0u8; 18];
            h[0..2].copy_from_slice(&next.to_le_bytes());
            h[2..4].copy_from_slice(&if next == 0xffff { 0xffffu16 } else { ds }.to_le_bytes());
            h[4..6].copy_from_slice(&attrs[i].to_le_bytes());
            h[6..8].copy_from_slice(&retf.to_le_bytes());
            h[8..10].copy_from_slice(&retf.to_le_bytes());
            h[10..18].copy_from_slice(&n[..]);
            put(self, ds, off, &h);
        }
        // The list of lists.
        let sv = SYSVARS;
        let w16 = |m: &mut Machine, off: u16, v: u16| put(m, ds, sv.wrapping_add(off), &v.to_le_bytes());
        w16(self, 0xfffe, FIRST_MCB);
        w16(self, 0x00, DPB);
        w16(self, 0x02, ds);
        w16(self, 0x04, SFT);
        w16(self, 0x06, ds);
        w16(self, 0x08, DEVICES + 3 * 18);
        w16(self, 0x0a, ds);
        w16(self, 0x0c, DEVICES);
        w16(self, 0x0e, ds);
        w16(self, 0x10, 512);
        w16(self, 0x16, CDS);
        w16(self, 0x18, ds);
        put(self, ds, sv + 0x20, &[26, self.dos.lastdrive]);
        // The NUL device header.
        let mut nul = [0u8; 18];
        nul[0..2].copy_from_slice(&DEVICES.to_le_bytes());
        nul[2..4].copy_from_slice(&ds.to_le_bytes());
        nul[4..6].copy_from_slice(&0x8004u16.to_le_bytes());
        nul[6..8].copy_from_slice(&retf.to_le_bytes());
        nul[8..10].copy_from_slice(&retf.to_le_bytes());
        nul[10..18].copy_from_slice(b"NUL     ");
        put(self, ds, sv + 0x22, &nul);
        w16(self, 0x3f, 20);
        put(self, ds, sv + 0x43, &[3, 1]);
        w16(self, 0x45, 0);
        // The SFT header (entries follow, filled as files open).
        put(self, ds, SFT, &[0xff, 0xff, 0xff, 0xff]);
        put(self, ds, SFT + 4, &SFT_COUNT.to_le_bytes());
        // Drives and their current directories.
        for d in 0..26u8 {
            self.dos.present[d as usize] = host.drive_exists(d);
            if self.dos.present[d as usize] {
                let cwd = host.initial_dir(d).to_ascii_uppercase();
                self.dos.cwd[d as usize] = cwd;
            }
            self.write_cds(host, d);
        }
        self.country_init();
        // Conventional memory: one free block up to 9FFFh.
        let hole = 0x9fff;
        self.mcb_write(FIRST_MCB, if self.cfg.umb { b'M' } else { b'Z' }, 0, hole - FIRST_MCB - 1);
        if self.cfg.umb {
            // The video and ROM hole, then the upper memory blocks.
            let umb = 0xc800u16;
            let end = if self.cfg.ems_kb > 0 { 0xe000u16 } else { 0xf000 };
            self.mcb_write(hole, b'M', 8, umb - hole - 1);
            self.mcb_name(hole, b"SC");
            self.mcb_write(umb, b'Z', 0, end - umb - 1);
            self.dos.umb_start = umb;
            self.hw.wr16(Machine::lin(ds, sv + 0x66), hole);
            self.hw.wr16(Machine::lin(ds, sv + 0x68), umb);
            // The conventional chain ends at the hole until linked.
            self.set_umb_link(false);
        }
        // DOS's interrupt vectors.
        for v in [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x29, 0x2e, 0x2f] {
            let stub = crate::bios::STUBS + v as u16 * 8;
            self.set_vector(v, crate::bios::ROM, stub);
        }
        self.set_vector(0x28, crate::bios::ROM, crate::bios::IRET_CODE);
        // The system PSP owns DOS's own allocations.
        self.dos.psp = SYS_PSP;
        self.init_psp(SYS_PSP, 0xa000, SYS_PSP, 0);
        self.hw.wr16(Machine::lin(ds, SDA + 0x10), SYS_PSP);
    }

    /// Write CDS entry `d` from the current directory.
    pub(crate) fn write_cds<H: Host>(&mut self, host: &mut H, d: u8) {
        let _ = host;
        let a = Machine::lin(DOSSEG, CDS + d as u16 * CDS_SIZE);
        for i in 0..CDS_SIZE as u32 {
            self.hw.ram[(a + i) as usize] = 0;
        }
        let path = alloc::format!("{}:\\{}", (b'A' + d) as char, self.dos.cwd[d as usize]);
        for (i, b) in path.bytes().take(66).enumerate() {
            self.hw.ram[a as usize + i] = b;
        }
        let present = self.dos.present[d as usize];
        let flags: u16 = if present { 0x4000 } else { 0 };
        self.hw.wr16(a + 0x43, flags);
        self.hw.wr16(a + 0x45, DPB + d as u16 * DPB_SIZE);
        self.hw.wr16(a + 0x47, DOSSEG);
        self.hw.wr16(a + 0x49, 0xffff);
        self.hw.wr16(a + 0x4b, 0xffff);
        self.hw.wr16(a + 0x4f, 2);
        // A DPB for the drive.
        let p = Machine::lin(DOSSEG, DPB + d as u16 * DPB_SIZE);
        self.hw.wr(p, d);
        self.hw.wr(p + 1, d);
        self.hw.wr16(p + 2, 512);
        self.hw.wr(p + 4, 7);
        self.hw.wr16(p + 0x0d, 0xfff0);
        self.hw.wr(p + 0x16, 0xf8);
        let next = if d < 25 { DPB + (d as u16 + 1) * DPB_SIZE } else { 0xffff };
        self.hw.wr16(p + 0x19, next);
        self.hw.wr16(p + 0x1b, if d < 25 { DOSSEG } else { 0xffff });
    }

    /// The InDOS flag's linear address.
    pub fn indos_addr() -> u32 {
        Machine::lin(DOSSEG, SDA + 1)
    }

    /// The current PSP, also in the SDA.
    pub fn set_psp(&mut self, psp: u16) {
        self.dos.psp = psp;
        self.hw.wr16(Machine::lin(DOSSEG, SDA + 0x10), psp);
    }

    pub fn dta(&mut self) -> (u16, u16) {
        let a = Machine::lin(DOSSEG, SDA + 0x0c);
        (self.hw.rd16(a + 2), self.hw.rd16(a))
    }

    pub fn set_dta(&mut self, seg: u16, off: u16) {
        let a = Machine::lin(DOSSEG, SDA + 0x0c);
        self.hw.wr16(a, off);
        self.hw.wr16(a + 2, seg);
    }

    /// Record the extended error for 59h and return it.
    pub fn dos_error(&mut self, code: u16) -> u16 {
        let (class, action, locus) = match code {
            2 | 3 | 15 | 18 => (8, 3, 2),
            4 | 8 => (1, 4, 1),
            5 | 12 | 32 | 33 => (3, 3, 2),
            6 | 9 => (7, 4, 1),
            80 => (12, 3, 2),
            _ => (13, 4, 1),
        };
        self.dos.last_error = (code, class, action, locus);
        code
    }

    /// AX = error, CF set (and the extended error recorded).
    pub fn fail(&mut self, code: u16) {
        let c = self.dos_error(code);
        self.ret_cf(Err(c));
    }

    pub fn ok(&mut self) {
        self.ret_cf(Ok(()));
    }

    /// DS:DX as a linear address.
    pub fn ds_dx(&self) -> u32 {
        Machine::lin(self.cpu.seg[DS].sel, self.cpu.dx())
    }

    pub fn es_di(&self) -> u32 {
        Machine::lin(self.cpu.seg[ES].sel, self.cpu.di())
    }

    pub(crate) fn dos_interrupt<H: Host>(&mut self, host: &mut H, v: u8) -> Cb {
        match v {
            0x20 => {
                self.dos_exit(host, 0, 0);
                Cb::Done
            }
            0x21 => self.int21(host),
            0x22 => Cb::Done,
            0x23 => {
                // Ctrl+C with no handler of the program's: end it.
                self.dos_exit(host, 0, 1);
                Cb::Done
            }
            0x24 => {
                // Critical error: fail the call.
                self.cpu.set_al(3);
                Cb::Done
            }
            0x25 | 0x26 => {
                // Absolute disk read/write: not allowed.
                self.cpu.set_ax(0x0207);
                self.set_ret_flag(CF, true);
                Cb::Done
            }
            0x27 => {
                // TSR: DX bytes of the PSP in CS.
                let paras = (self.cpu.dx() as u32).div_ceil(16) as u16;
                self.dos_tsr(host, paras.max(6), 0);
                Cb::Done
            }
            0x29 => {
                let c = self.cpu.al();
                self.tty_out(c);
                Cb::Done
            }
            0x2e => {
                // Run a command line (DS:SI) through the shell: not
                // supported without a resident COMMAND.COM.
                self.cpu.set_ax(0xffff);
                Cb::Done
            }
            0x2f => {
                self.int2f(host);
                Cb::Done
            }
            _ => Cb::Done,
        }
    }

    fn int2f<H: Host>(&mut self, host: &mut H) {
        let ax = self.cpu.ax();
        match ax {
            0x1680 => {
                // Idle: give up the slice.
                self.cpu.set_al(0);
                self.idle_poll();
                self.waiting = true;
            }
            0x1687 => self.dpmi_detect(host),
            0x1686 => self.cpu.set_ax(if self.cpu.pm() { 0 } else { 1 }),
            0x4300 => self.cpu.set_al(0x80),
            0x4310 => {
                self.cpu.seg[ES] = x86emu::Seg::real(crate::bios::ROM);
                self.cpu.set_bx(crate::bios::XMS_CODE);
            }
            0x4010 => {
                // OS/2: the version (20, minor) in BX.
                self.cpu.set_ax(0);
                let (maj, min) = self.cfg.version;
                self.cpu.set_bx((min as u16) << 8 | maj as u16);
            }
            0x1600 => self.cpu.set_al(0),
            0x1a00 => self.cpu.set_al(0),
            0x4a01 | 0x4a02 => {
                // HMA queries (DOS 5): none free.
                self.cpu.set_bx(0);
                self.cpu.seg[ES] = x86emu::Seg::real(0xffff);
                self.cpu.set_di(0xffff);
            }
            0xb700 => self.cpu.set_al(0),
            0x1100 | 0x0800 | 0x1000 | 0x0600 | 0x0100 | 0xb800 | 0x1a01 => self.cpu.set_al(0),
            0xae00 | 0xae01 => self.cpu.set_al(0),
            0x1213 => {
                let c = self.cpu.al();
                let u = self.upcase(c);
                self.cpu.set_al(u);
            }
            _ => {
                if self.cpu.ah() >= 0xc0 {
                    // Multiplex ids for applications: not installed.
                    self.cpu.set_al(0);
                }
            }
        }
    }

    /// Country upper-casing of a character.
    pub fn upcase(&mut self, c: u8) -> u8 {
        if c.is_ascii_lowercase() {
            return c - 32;
        }
        if c >= 0x80 {
            return self.hw.rd(Machine::lin(DOSSEG, UPCASE + 2 + (c - 0x80) as u16));
        }
        c
    }

    fn int21<H: Host>(&mut self, host: &mut H) -> Cb {
        let ah = self.cpu.ah();
        // Ctrl+C on console functions (and on all with BREAK=ON).
        if self.dos.break_pending && (matches!(ah, 0x01..=0x05 | 0x08..=0x0c) || self.dos.break_on) {
            self.dos.break_pending = false;
            self.dos.line = None;
            self.dos.con_ahead.clear();
            self.tty_out(b'^');
            self.tty_out(b'C');
            self.tty_out(b'\r');
            self.tty_out(b'\n');
            return self.ctrl_c(host);
        }
        self.hw.wr(Machine::indos_addr(), 1);
        let r = self.int21_fn(host, ah);
        self.hw.wr(Machine::indos_addr(), 0);
        r
    }

    /// Ctrl+C: through the program's INT 23h if it has one, else end it.
    pub(crate) fn ctrl_c<H: Host>(&mut self, host: &mut H) -> Cb {
        if self.shell_break() {
            return Cb::Done;
        }
        let (seg, off) = self.vector(0x23);
        if seg == crate::bios::ROM {
            self.dos_exit(host, 0, 1);
            return Cb::Done;
        }
        // Return to the INT 21h instruction (so the call is repeated if
        // the handler IRETs) and take INT 23h from there.
        let ip = self.ret_ip().wrapping_sub(2);
        let cs = self.ret_cs();
        let fl = self.ret_flags();
        self.cpu.add_sp_pub(6);
        self.cpu.set_seg_real(x86emu::CS, cs);
        self.cpu.eip = ip as u32;
        self.cpu.flags = (self.cpu.flags & !0xffff) | fl as u32;
        let _ = (seg, off);
        self.chain_int = Some(0x23);
        Cb::Done
    }

    fn int21_fn<H: Host>(&mut self, host: &mut H, ah: u8) -> Cb {
        match ah {
            0x00 => self.dos_exit(host, 0, 0),
            0x01 | 0x03..=0x0c => return self.con_fn(host, ah),
            0x02 => {
                let c = self.cpu.dl();
                self.con_write_std(host, &[c]);
                self.cpu.set_al(c);
            }
            0x0d => {}
            0x0e => {
                let d = self.cpu.dl();
                if d < 26 && self.drive_valid(host, d) {
                    self.dos.drive = d;
                }
                self.cpu.set_al(self.dos.lastdrive);
            }
            0x0f..=0x17 | 0x21..=0x24 | 0x27..=0x29 => return self.fcb_fn(host, ah),
            0x18 | 0x1d | 0x1e | 0x20 => self.cpu.set_al(0),
            0x19 => self.cpu.set_al(self.dos.drive),
            0x1a => {
                let (s, o) = (self.cpu.seg[DS].sel, self.cpu.dx());
                self.set_dta(s, o);
            }
            0x1b | 0x1c => {
                let d = if ah == 0x1b { self.dos.drive } else if self.cpu.dl() == 0 { self.dos.drive } else { self.cpu.dl() - 1 };
                match host.disk_space(d) {
                    Ok((spc, bps, _free, total)) => {
                        self.cpu.set_al(spc as u8);
                        self.cpu.set_cx(bps);
                        self.cpu.set_dx(total.min(0xfff0) as u16);
                        self.cpu.seg[DS] = x86emu::Seg::real(DOSSEG);
                        let id = DPB + d as u16 * DPB_SIZE + 0x16;
                        self.cpu.set_bx(id);
                    }
                    Err(_) => self.cpu.set_al(0xff),
                }
            }
            0x1f | 0x32 => {
                let d = if ah == 0x1f || self.cpu.dl() == 0 { self.dos.drive } else { self.cpu.dl() - 1 };
                if d < 26 && self.drive_valid(host, d) {
                    self.cpu.seg[DS] = x86emu::Seg::real(DOSSEG);
                    self.cpu.set_bx(DPB + d as u16 * DPB_SIZE);
                    self.cpu.set_al(0);
                } else {
                    self.cpu.set_al(0xff);
                }
            }
            0x25 => {
                let (al, ds, dx) = (self.cpu.al(), self.cpu.seg[DS].sel, self.cpu.dx());
                self.set_vector(al, ds, dx);
            }
            0x26 => {
                let seg = self.cpu.dx();
                let psp = self.dos.psp;
                let end = self.hw.rd16(Machine::lin(psp, 2));
                self.copy_psp(psp, seg, end);
            }
            0x2a => {
                let n = host.now();
                self.cpu.set_cx(n.year);
                self.cpu.set_dh(n.month);
                self.cpu.set_dl(n.day);
                self.cpu.set_al(n.weekday);
            }
            0x2b | 0x2d => self.cpu.set_al(0),
            0x2c => {
                let n = host.now();
                self.cpu.set_ch(n.hour);
                self.cpu.set_cl(n.minute);
                self.cpu.set_dh(n.second);
                self.cpu.set_dl(n.hundredths);
            }
            0x2e => self.dos.verify = self.cpu.al() != 0,
            0x2f => {
                let (s, o) = self.dta();
                self.cpu.seg[ES] = x86emu::Seg::real(s);
                self.cpu.set_bx(o);
            }
            0x30 => {
                let (maj, min) = self.cfg.version;
                // OS/2's VDM: major 20 (or 10 for OS/2 1.x).
                let (maj, min) = if self.cpu.al() == 1 { (maj, min) } else { (maj, min) };
                self.cpu.set_al(maj);
                self.cpu.set_ah(min);
                // OEM number (BH): IBM; BL:CX serial 0. AL=01h: BH = flags
                // (DOS in HMA).
                if self.cpu.al() == 1 {
                    self.cpu.set_bh(if self.cfg.high { 0x10 } else { 0 });
                } else {
                    self.cpu.set_bh(0x00);
                }
                self.cpu.set_bl(0);
                self.cpu.set_cx(0);
                self.cpu.set_al(maj);
            }
            0x31 => {
                let (dx, al) = (self.cpu.dx(), self.cpu.al());
                self.dos_tsr(host, dx.max(6), al);
            }
            0x33 => match self.cpu.al() {
                0 => self.cpu.set_dl(self.dos.break_on as u8),
                1 => self.dos.break_on = self.cpu.dl() & 1 != 0,
                2 => {
                    let old = self.dos.break_on as u8;
                    self.dos.break_on = self.cpu.dl() & 1 != 0;
                    self.cpu.set_dl(old);
                }
                5 => self.cpu.set_dl(3),
                6 => {
                    let (maj, min) = self.cfg.version;
                    self.cpu.set_bl(maj);
                    self.cpu.set_bh(min);
                    self.cpu.set_dl(0);
                    self.cpu.set_dh(if self.cfg.high { 0x10 } else { 0 });
                }
                _ => self.cpu.set_al(0xff),
            },
            0x34 => {
                self.cpu.seg[ES] = x86emu::Seg::real(DOSSEG);
                self.cpu.set_bx(SDA + 1);
            }
            0x35 => {
                let (s, o) = self.vector(self.cpu.al());
                self.cpu.seg[ES] = x86emu::Seg::real(s);
                self.cpu.set_bx(o);
            }
            0x36 => {
                let d = if self.cpu.dl() == 0 { self.dos.drive } else { self.cpu.dl() - 1 };
                match host.disk_space(d) {
                    Ok((spc, bps, free, total)) => {
                        self.cpu.set_ax(spc);
                        self.cpu.set_bx(free.min(0xffff) as u16);
                        self.cpu.set_cx(bps);
                        self.cpu.set_dx(total.min(0xffff) as u16);
                    }
                    Err(_) => self.cpu.set_ax(0xffff),
                }
            }
            0x37 => match self.cpu.al() {
                0 => {
                    let s = self.dos.switchar;
                    self.cpu.set_dl(s);
                    self.cpu.set_al(0);
                }
                1 => {
                    self.dos.switchar = self.cpu.dl();
                    self.cpu.set_al(0);
                }
                _ => self.cpu.set_al(0xff),
            },
            0x38 => self.country_fn(host),
            0x39..=0x3b | 0x3c..=0x46 | 0x47 | 0x4e | 0x4f | 0x56 | 0x57 | 0x5a | 0x5b | 0x5c | 0x60 | 0x67 | 0x68 | 0x6a | 0x6c => {
                return self.file_fn(host, ah);
            }
            0x48..=0x4a | 0x58 => self.mem_fn(host, ah),
            0x4b => self.exec_fn(host),
            0x4c => {
                let al = self.cpu.al();
                self.dos_exit(host, al, 0);
            }
            0x4d => {
                let rc = self.dos.return_code;
                self.cpu.set_ax(rc);
                self.dos.return_code = 0;
                self.set_ret_flag(CF, false);
            }
            0x50 => {
                let b = self.cpu.bx();
                self.set_psp(b);
            }
            0x51 | 0x62 => {
                let p = self.dos.psp;
                self.cpu.set_bx(p);
            }
            0x52 => {
                self.cpu.seg[ES] = x86emu::Seg::real(DOSSEG);
                self.cpu.set_bx(SYSVARS);
            }
            0x53 => {}
            0x54 => self.cpu.set_al(self.dos.verify as u8),
            0x55 => {
                let (dx, si) = (self.cpu.dx(), self.cpu.si());
                let psp = self.dos.psp;
                self.copy_psp(psp, dx, si);
                self.hw.wr16(Machine::lin(dx, 0x16), psp);
                self.inherit_handles(psp, dx);
                self.set_psp(dx);
            }
            0x59 => {
                let (code, class, action, locus) = self.dos.last_error;
                self.cpu.set_ax(code);
                self.cpu.set_bh(class);
                self.cpu.set_bl(action);
                self.cpu.set_ch(locus);
            }
            0x5d => match self.cpu.al() {
                0x06 => {
                    self.cpu.seg[DS] = x86emu::Seg::real(DOSSEG);
                    self.cpu.set_si(SDA);
                    self.cpu.set_cx(0x80);
                    self.cpu.set_dx(0x1a);
                    self.ok();
                }
                0x0a => self.ok(),
                _ => self.fail(err::INVALID_FUNCTION),
            },
            0x5e | 0x5f => self.fail(err::INVALID_FUNCTION),
            0x63 => {
                if self.cpu.al() == 0 {
                    self.cpu.seg[DS] = x86emu::Seg::real(DOSSEG);
                    self.cpu.set_si(DBCS + 2);
                    self.cpu.set_al(0);
                } else {
                    self.cpu.set_al(0xff);
                }
            }
            0x65 | 0x66 => self.country_fn(host),
            0x69 => self.fail(err::INVALID_FUNCTION),
            0x6b => self.cpu.set_al(0),
            0x71 | 0x73 => {
                // Long file names, FAT32: not supported (AX=7100h).
                self.cpu.set_ax(0x7100);
                self.set_ret_flag(CF, true);
            }
            _ => {
                self.cpu.set_al(0);
                self.set_ret_flag(CF, true);
                self.cpu.set_ax(err::INVALID_FUNCTION);
            }
        }
        Cb::Done
    }
}

// Exposed for the CPU's stack adjustment from callbacks.
trait AddSp {
    fn add_sp_pub(&mut self, n: u32);
}

impl AddSp for x86emu::Cpu {
    fn add_sp_pub(&mut self, n: u32) {
        let sp = self.stack_ptr().wrapping_add(n);
        if self.seg[x86emu::SS].big() {
            self.regs[x86emu::ESP] = sp;
        } else {
            self.regs[x86emu::ESP] = (self.regs[x86emu::ESP] & 0xffff_0000) | (sp & 0xffff);
        }
    }
}

