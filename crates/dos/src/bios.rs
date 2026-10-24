//! The BIOS: ROM layout and interrupt vectors, the BIOS data area, video
//! (INT 10h), keyboard (INT 9, INT 16h), timer (INT 8, INT 1Ah), disk
//! (INT 13h: no drives), system services (INT 15h).

use x86emu::flags::{CF, IF, ZF};
use x86emu::{DS, ES};

use crate::{Cb, Host, Machine};

/// The ROM segment and where its pieces are.
pub const ROM: u16 = 0xf000;
/// Callback stubs, 8 bytes per vector.
pub const STUBS: u16 = 0xe000;
/// Special code (INT 8, system callbacks).
pub const SPECIAL: u16 = 0xe800;
pub const CONFIG_TABLE: u16 = 0xe6f5;
/// The video BIOS's static functionality table (INT 10h AH=1Bh).
pub const VIDEO_STATIC: u16 = 0xe700;
/// VESA tables: OEM string (+0), mode list (+20h), vendor (+30h), the
/// window function (+40h: INT 10h AX=4F05h then RETF).
pub const VBE_TABLES: u16 = 0xe710;
/// The video ROM segment (fonts).
pub const VROM: u16 = 0xc000;

/// System callbacks (beyond the 256 interrupt services).
pub const CB_TERMINATED: u16 = 0x100;
pub const CB_MOUSE_RET: u16 = 0x104;

/// Offsets of special code in the ROM segment.
pub const INT8_CODE: u16 = SPECIAL;
pub const TERM_CODE: u16 = SPECIAL + 0x20;
pub const MOUSE_RET_CODE: u16 = SPECIAL + 0x30;
pub const XMS_CODE: u16 = SPECIAL + 0x40;
pub const SHELL_CODE: u16 = SPECIAL + 0x50;
pub const IRET_CODE: u16 = SPECIAL + 0x60;
pub const DPMI_CODE: u16 = SPECIAL + 0x100;

const BDA: u32 = 0x400;

/// Scan code to (normal, shift, ctrl, alt) BIOS key codes (scan << 8 |
/// ASCII) for the US layout, set 1 codes 01h-58h.
const KEYS: [[u16; 4]; 0x59] = {
    let mut t = [[0u16; 4]; 0x59];
    let normal = b"\x00\x1b1234567890-=\x08\tqwertyuiop[]\r\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
    let shifted = b"\x00\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";
    let mut i = 1;
    while i < 0x3a {
        let s = (i as u16) << 8;
        let n = normal[i];
        let sh = shifted[i];
        let ctrl: u16 = if n >= b'a' && n <= b'z' {
            s | (n - b'a' + 1) as u16
        } else {
            match n {
                b'[' => s | 0x1b,
                b']' => s | 0x1d,
                b'\\' => s | 0x1c,
                b'-' => s | 0x1f,
                b'6' => s | 0x1e,
                b'2' => 0x0300,
                b'\r' => s | 0x0a,
                b'\x08' => s | 0x7f,
                0x1b => s | 0x1b,
                b'\t' => 0x9400,
                b' ' => s | 0x20,
                _ => 0,
            }
        };
        let alt: u16 = if (n >= b'a' && n <= b'z') || (n >= b'0' && n <= b'9') || n == b'-' || n == b'=' {
            if i >= 2 && i <= 0x0d { ((i as u16) + 0x76) << 8 } else { s }
        } else {
            match i {
                0x01 => 0x0100,
                0x0e => 0x0e00,
                0x0f => 0xa500,
                0x1c => 0x1c00,
                0x1a | 0x1b | 0x27 | 0x28 | 0x29 | 0x2b | 0x33 | 0x34 | 0x35 => s,
                0x37 => 0x3700,
                0x39 => 0x3920,
                _ => 0,
            }
        };
        t[i] = [s | n as u16, s | sh as u16, ctrl, alt];
        i += 1;
    }
    // F1-F10
    let mut f = 0;
    while f < 10 {
        let sc = 0x3b + f as u16;
        t[0x3b + f] = [sc << 8, (0x54 + f as u16) << 8, (0x5e + f as u16) << 8, (0x68 + f as u16) << 8];
        f += 1;
    }
    // Keypad: Home Up PgUp - Left 5 Right + End Down PgDn Ins Del.
    let pad: [(u8, u16, u16, u16); 13] = [
        (0x47, 0x4737, 0x7700, 0x9700),
        (0x48, 0x4838, 0x8d00, 0x9800),
        (0x49, 0x4939, 0x8400, 0x9900),
        (0x4a, 0x4a2d, 0x8e00, 0x4a00),
        (0x4b, 0x4b34, 0x7300, 0x9b00),
        (0x4c, 0x4c35, 0x8f00, 0x0000),
        (0x4d, 0x4d36, 0x7400, 0x9d00),
        (0x4e, 0x4e2b, 0x9000, 0x4e00),
        (0x4f, 0x4f31, 0x7500, 0x9f00),
        (0x50, 0x5032, 0x9100, 0xa000),
        (0x51, 0x5133, 0x7600, 0xa100),
        (0x52, 0x5230, 0x9200, 0xa200),
        (0x53, 0x532e, 0x9300, 0xa300),
    ];
    let mut k = 0;
    while k < 13 {
        let (sc, sh, ctrl, alt) = pad[k];
        let normal = if sc == 0x4a || sc == 0x4e { sh } else { (sc as u16) << 8 };
        t[sc as usize] = [normal, sh, ctrl, alt];
        k += 1;
    }
    t[0x57] = [0x8500, 0x8700, 0x8900, 0x8b00];
    t[0x58] = [0x8600, 0x8800, 0x8a00, 0x8c00];
    t
};

impl Machine {
    pub(crate) fn bios_init<H: Host>(&mut self, host: &mut H) {
        let rom = (ROM as u32) << 4;
        // Every vector gets its callback stub.
        for v in 0..256u32 {
            let s = rom + STUBS as u32 + v * 8;
            self.hw.ram[s as usize..s as usize + 5].copy_from_slice(&[0xfe, 0x38, v as u8, 0x00, 0xcf]);
            let ivt = (v * 4) as usize;
            self.hw.ram[ivt..ivt + 2].copy_from_slice(&(STUBS + v as u16 * 8).to_le_bytes());
            self.hw.ram[ivt + 2..ivt + 4].copy_from_slice(&ROM.to_le_bytes());
        }
        let put = |m: &mut Machine, off: u16, code: &[u8]| {
            let a = rom as usize + off as usize;
            m.hw.ram[a..a + code.len()].copy_from_slice(code);
        };
        // INT 8: the tick, INT 1Ch, EOI.
        put(self, INT8_CODE, &[0xfe, 0x38, 0x08, 0x00, 0xcd, 0x1c, 0x50, 0xb0, 0x20, 0xe6, 0x20, 0x58, 0xcf]);
        self.set_vector(0x08, ROM, INT8_CODE);
        // Program termination (the PSP's return), mouse handler return,
        // XMS entry, a bare IRET.
        put(self, TERM_CODE, &[0xfe, 0x38, CB_TERMINATED as u8, (CB_TERMINATED >> 8) as u8]);
        put(self, MOUSE_RET_CODE, &[0xfe, 0x38, CB_MOUSE_RET as u8, (CB_MOUSE_RET >> 8) as u8]);
        put(self, XMS_CODE, &[0xeb, 0x03, 0x90, 0x90, 0x90, 0xfe, 0x38, crate::xms::CB_XMS as u8, (crate::xms::CB_XMS >> 8) as u8, 0xcb]);
        put(self, IRET_CODE, &[0xcf]);
        // Vectors that are data or do nothing.
        for v in [0x1cu8, 0x1b, 0x23, 0x28, 0x2a, 0x05] {
            self.set_vector(v, ROM, IRET_CODE);
        }
        self.set_vector(0x1d, VROM, 0);
        self.set_vector(0x1e, ROM, 0xefc7);
        // Floppy parameter table.
        put(self, 0xefc7, &[0xdf, 0x02, 0x25, 0x02, 0x12, 0x1b, 0xff, 0x6c, 0xf6, 0x0f, 0x08]);
        // The configuration table (INT 15h C0h), model AT.
        put(self, CONFIG_TABLE, &[0x08, 0x00, 0xfc, 0x00, 0x01, 0x74, 0x40, 0x00, 0x00, 0x00]);
        // Modes 0-7, 0Dh-13h; 200/350/400 lines; 8 font blocks, 2 active;
        // every palette, font and cursor function; state save.
        put(self, VIDEO_STATIC, &[0xff, 0xe0, 0x0f, 0, 0, 0, 0, 0x07, 0x08, 0x02, 0xff, 0x0e, 0, 0, 0x3f, 0]);
        put(self, VBE_TABLES, b"rs2 DOS session VGA\0");
        let mut list = alloc::vec::Vec::new();
        for (m, _, _) in crate::video::VBE_MODES {
            list.extend_from_slice(&m.to_le_bytes());
        }
        list.extend_from_slice(&0xffffu16.to_le_bytes());
        put(self, VBE_TABLES + 0x20, &list);
        put(self, VBE_TABLES + 0x30, b"rs2\0");
        // mov ax,4F05h / int 10h / retf
        put(self, VBE_TABLES + 0x40, &[0xb8, 0x05, 0x4f, 0xcd, 0x10, 0xcb]);
        put(self, 0xfff0, &[0xea, 0x5b, 0xe0, 0x00, 0xf0]);
        put(self, 0xfff5, b"01/01/92");
        put(self, 0xfffe, &[0xfc, 0x00]);
        // Fonts in the video ROM.
        let vrom = (VROM as u32) << 4;
        self.hw.ram[vrom as usize..vrom as usize + 3].copy_from_slice(&[0x55, 0xaa, 0x40]);
        let mut at = 0x1000u16;
        for (i, h) in [8u8, 14, 16].into_iter().enumerate() {
            if let Some(f) = host.font(h) {
                let n = (256 * h as usize).min(f.len());
                let a = vrom as usize + at as usize;
                self.hw.ram[a..a + n].copy_from_slice(&f[..n]);
            }
            self.fonts[i] = (VROM, at);
            at += 256 * h as u16;
        }
        self.set_vector(0x1f, VROM, self.fonts[0].1 + 128 * 8);
        self.set_vector(0x43, VROM, self.fonts[2].1);
        // The BIOS data area.
        let equip: u16 = 0x0020 | if self.cfg.fpu { 2 } else { 0 } | 0x0400 * 0;
        self.hw.wr16(BDA + 0x10, equip);
        self.hw.wr16(BDA + 0x13, 640);
        self.hw.wr16(BDA + 0x1a, 0x1e);
        self.hw.wr16(BDA + 0x1c, 0x1e);
        self.hw.wr16(BDA + 0x80, 0x1e);
        self.hw.wr16(BDA + 0x82, 0x3e);
        self.hw.wr(BDA + 0x96, 0x10);
        self.hw.wr16(BDA + 0x72, 0x1234);
        if self.cfg.fpu {
            self.hw.cmos.ram[0x14] |= 2;
        }
        self.hw.cmos.ram[0x17] = 0;
        self.hw.cmos.ram[0x18] = 0;
        self.set_mode(3, false);
        // Set the tick count to the time of day.
        let n = self.hw.now;
        let secs = n.hour as u64 * 3600 + n.minute as u64 * 60 + n.second as u64;
        let ticks = (secs * 1_000_000 + n.hundredths as u64 * 10_000) * 1193182 / 65536 / 1_000_000;
        self.hw.wr32(BDA + 0x6c, ticks as u32);
        self.hw.a20 = false;
    }

    pub fn set_vector(&mut self, v: u8, seg: u16, off: u16) {
        let a = v as u32 * 4;
        self.hw.wr16(a, off);
        self.hw.wr16(a + 2, seg);
    }

    pub fn vector(&mut self, v: u8) -> (u16, u16) {
        let a = v as u32 * 4;
        (self.hw.rd16(a + 2), self.hw.rd16(a))
    }

    // ---- Timer ----

    pub(crate) fn bios_timer(&mut self) {
        let mut t = self.hw.rd32(BDA + 0x6c).wrapping_add(1);
        if t >= 0x1800b0 {
            t = 0;
            let f = self.hw.rd(BDA + 0x70);
            self.hw.wr(BDA + 0x70, f.wrapping_add(1));
        }
        self.hw.wr32(BDA + 0x6c, t);
        let motor = self.hw.rd(BDA + 0x40);
        if motor > 0 {
            self.hw.wr(BDA + 0x40, motor - 1);
        }
    }

    pub(crate) fn int1a(&mut self) {
        let bcd = |v: u8| (v / 10) << 4 | (v % 10);
        let n = self.hw.now;
        match self.cpu.ah() {
            0x00 => {
                let t = self.hw.rd32(BDA + 0x6c);
                self.cpu.set_cx((t >> 16) as u16);
                self.cpu.set_dx(t as u16);
                let f = self.hw.rd(BDA + 0x70);
                self.cpu.set_al(f);
                self.hw.wr(BDA + 0x70, 0);
            }
            0x01 => {
                let t = (self.cpu.cx() as u32) << 16 | self.cpu.dx() as u32;
                self.hw.wr32(BDA + 0x6c, t);
            }
            0x02 => {
                self.cpu.set_ch(bcd(n.hour));
                self.cpu.set_cl(bcd(n.minute));
                self.cpu.set_dh(bcd(n.second));
                self.cpu.set_dl(0);
                self.set_ret_flag(CF, false);
            }
            0x04 => {
                self.cpu.set_ch(bcd((n.year / 100) as u8));
                self.cpu.set_cl(bcd((n.year % 100) as u8));
                self.cpu.set_dh(bcd(n.month));
                self.cpu.set_dl(bcd(n.day));
                self.set_ret_flag(CF, false);
            }
            0x03 | 0x05 => self.set_ret_flag(CF, false),
            _ => self.set_ret_flag(CF, true),
        }
    }

    // ---- Keyboard ----

    fn shift_flags(&mut self) -> u8 {
        self.hw.rd(BDA + 0x17)
    }

    /// IRQ 1: the scan code in port 60h into the BIOS buffer.
    pub(crate) fn bios_keyboard_irq(&mut self) {
        let sc = self.hw.kbc.data;
        let host_ch = self.hw.kbc.data_ch;
        self.hw.kbc.full = false;
        self.eoi(1);
        let mut st = self.hw.rd(BDA + 0x96);
        if sc == 0xe0 || sc == 0xe1 {
            self.hw.wr(BDA + 0x96, st | 2);
            return;
        }
        let ext = st & 2 != 0;
        st &= !2;
        self.hw.wr(BDA + 0x96, st);
        let up = sc & 0x80 != 0;
        let code = sc & 0x7f;
        let mut f = self.hw.rd(BDA + 0x17);
        let mut f2 = self.hw.rd(BDA + 0x18);
        let bit = |b: u8, on: bool, v: &mut u8| if on { *v |= b } else { *v &= !b };
        match code {
            0x2a => bit(0x02, !up, &mut f),
            0x36 => {
                if !ext {
                    bit(0x01, !up, &mut f)
                }
            }
            0x1d => {
                bit(0x04, !up, &mut f);
                if !ext {
                    bit(0x01, !up, &mut f2);
                }
            }
            0x38 => {
                bit(0x08, !up, &mut f);
                if !ext {
                    bit(0x02, !up, &mut f2);
                }
            }
            0x3a => {
                bit(0x40, !up, &mut f2);
                if !up && f2 & 0x40 != 0 {
                    // Toggle on press only (key repeat leaves it).
                }
                if !up {
                    f ^= 0x40;
                }
            }
            0x45 => {
                bit(0x20, !up, &mut f2);
                if !up {
                    f ^= 0x20;
                }
            }
            0x46 => {
                if !up && f & 0x04 != 0 {
                    // Ctrl+Break.
                    self.hw.wr(BDA + 0x71, 0x80);
                    self.buffer_key(0);
                    self.chain_int = Some(0x1b);
                    self.dos.break_pending = true;
                } else {
                    bit(0x10, !up, &mut f2);
                    if !up {
                        f ^= 0x10;
                    }
                }
            }
            0x52 if !up && f & 0x0c == 0 => {
                f ^= 0x80;
            }
            _ => {}
        }
        self.hw.wr(BDA + 0x17, f);
        self.hw.wr(BDA + 0x18, f2);
        if up || matches!(code, 0x2a | 0x36 | 0x1d | 0x38 | 0x3a | 0x45 | 0x46) {
            return;
        }
        // Ctrl+C sets the break flag too (DOS checks it).
        let key = self.translate_key(code, ext, f, host_ch);
        if key == 0 {
            return;
        }
        if key == 0x2e03 {
            self.dos.break_pending = true;
        }
        self.buffer_key(key);
    }

    /// The BIOS key code for scan `code` in shift state `f`.
    fn translate_key(&self, code: u8, ext: bool, f: u8, host_ch: u8) -> u16 {
        let shift = f & 3 != 0;
        let ctrl = f & 4 != 0;
        let alt = f & 8 != 0;
        if ext {
            // Grey keys: E0 xx for the cursor block, keypad Enter and /.
            return match code {
                0x1c => {
                    if ctrl {
                        0xe00a
                    } else {
                        0xe00d
                    }
                }
                0x35 => {
                    if alt {
                        0xa400
                    } else if ctrl {
                        0x9500
                    } else {
                        0xe02f
                    }
                }
                0x47..=0x53 => {
                    let t = KEYS[code as usize];
                    if alt {
                        t[3]
                    } else if ctrl {
                        t[2]
                    } else {
                        (code as u16) << 8 | 0xe0
                    }
                }
                _ => 0,
            };
        }
        let Some(t) = KEYS.get(code as usize) else { return 0 };
        if alt {
            return t[3];
        }
        if ctrl {
            return t[2];
        }
        let caps = f & 0x40 != 0;
        let num = f & 0x20 != 0;
        let mut k = if (0x47..=0x53).contains(&code) && code != 0x4a && code != 0x4e {
            if num != shift { t[1] } else { t[0] }
        } else {
            let letter = (t[0] as u8).is_ascii_lowercase();
            if shift != (caps && letter) { t[1] } else { t[0] }
        };
        // The host's layout knows the character better (national keys).
        if host_ch != 0 && (k as u8 != 0 || (0x02..=0x35).contains(&code)) {
            k = (k & 0xff00) | host_ch as u16;
        }
        k
    }

    /// Put a key code into the BIOS buffer (false if full).
    pub fn buffer_key(&mut self, k: u16) -> bool {
        let head = self.hw.rd16(BDA + 0x1a);
        let tail = self.hw.rd16(BDA + 0x1c);
        let start = self.hw.rd16(BDA + 0x80);
        let end = self.hw.rd16(BDA + 0x82);
        let mut next = tail + 2;
        if next >= end {
            next = start;
        }
        if next == head {
            return false;
        }
        self.hw.wr16(BDA + tail as u32, k);
        self.hw.wr16(BDA + 0x1c, next);
        true
    }

    /// The next key in the buffer, removed (`take`) or not.
    pub fn peek_key(&mut self, take: bool) -> Option<u16> {
        let head = self.hw.rd16(BDA + 0x1a);
        let tail = self.hw.rd16(BDA + 0x1c);
        if head == tail {
            return None;
        }
        let k = self.hw.rd16(BDA + head as u32);
        if take {
            let mut next = head + 2;
            if next >= self.hw.rd16(BDA + 0x82) {
                next = self.hw.rd16(BDA + 0x80);
            }
            self.hw.wr16(BDA + 0x1a, next);
        }
        Some(k)
    }

    /// Key codes the old functions (00h, 01h) do not return translated.
    fn compat_key(k: u16, extended: bool) -> Option<u16> {
        if extended {
            return Some(k);
        }
        let (sc, ch) = ((k >> 8) as u8, k as u8);
        if ch == 0xe0 && sc != 0 {
            return Some((sc as u16) << 8);
        }
        if sc == 0xe0 {
            return Some(0x1c00 | ch as u16);
        }
        if sc > 0x84 || (ch == 0 && matches!(sc, 0x01 | 0x0e | 0x1c | 0x37 | 0x39 | 0x4a | 0x4c | 0x4e | 0x94 | 0x95)) {
            return None;
        }
        Some(k)
    }

    pub(crate) fn int16(&mut self) -> Cb {
        let ah = self.cpu.ah();
        match ah {
            0x00 | 0x10 => loop {
                match self.peek_key(true) {
                    Some(k) => {
                        if let Some(k) = Self::compat_key(k, ah == 0x10) {
                            self.cpu.set_ax(k);
                            self.polls = 0;
                            return Cb::Done;
                        }
                    }
                    None => return Cb::Retry,
                }
            },
            0x01 | 0x11 => loop {
                match self.peek_key(false) {
                    Some(k) => match Self::compat_key(k, ah == 0x11) {
                        Some(k) => {
                            self.cpu.set_ax(k);
                            self.set_ret_flag(ZF, false);
                            return Cb::Done;
                        }
                        None => {
                            self.peek_key(true);
                        }
                    },
                    None => {
                        self.set_ret_flag(ZF, true);
                        self.idle_poll();
                        return Cb::Done;
                    }
                }
            },
            0x02 => {
                let f = self.shift_flags();
                self.cpu.set_al(f);
            }
            0x12 => {
                let f = self.shift_flags();
                let f2 = self.hw.rd(BDA + 0x18);
                let f3 = self.hw.rd(BDA + 0x96);
                let mut hi = f2 & 0x73;
                if f3 & 0x04 != 0 {
                    hi |= 0x04;
                }
                if f3 & 0x08 != 0 {
                    hi |= 0x08;
                }
                self.cpu.set_al(f);
                self.cpu.set_ah(hi | (f2 & 0x80));
            }
            0x05 => {
                let ok = self.buffer_key(self.cpu.cx());
                self.cpu.set_al(if ok { 0 } else { 1 });
            }
            0x09 => self.cpu.set_al(0x2c),
            0x0a => self.cpu.set_bx(0x41ab),
            _ => {}
        }
        Cb::Done
    }

    /// A program polling for keys without getting one: after many polls
    /// in a row, wait for input (it is idle at a prompt).
    pub fn idle_poll(&mut self) {
        self.polls += 1;
        if self.polls >= 200 {
            self.polls = 0;
            self.waiting = true;
        }
    }

    // ---- Disk, system ----

    pub(crate) fn int13(&mut self) {
        match self.cpu.ah() {
            0x00 => {
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            0x15 => {
                // Drive type: none.
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            _ => {
                self.cpu.set_ah(0x01);
                self.set_ret_flag(CF, true);
            }
        }
    }

    pub(crate) fn int15<H: Host>(&mut self, host: &mut H) -> Cb {
        let _ = host;
        match self.cpu.ah() {
            0x24 => {
                match self.cpu.al() {
                    0 => self.hw.a20 = false,
                    1 => self.hw.a20 = true,
                    2 => {
                        let a = self.hw.a20 as u8;
                        self.cpu.set_al(a);
                    }
                    _ => self.cpu.set_bx(3),
                }
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            0x4f => self.set_ret_flag(CF, true),
            0x86 => {
                // Wait CX:DX microseconds.
                let us = (self.cpu.cx() as u64) << 16 | self.cpu.dx() as u64;
                let until = self.hw.us_now + us;
                if self.dos.wait_until == 0 {
                    self.dos.wait_until = until;
                }
                if self.hw.us_now < self.dos.wait_until {
                    return Cb::Retry;
                }
                self.dos.wait_until = 0;
                self.set_ret_flag(CF, false);
            }
            0x87 => {
                // Block move: CX words between the descriptors at ES:SI.
                let gdt = Machine::lin(self.cpu.seg[ES].sel, self.cpu.si());
                let base = |m: &mut Machine, d: u32| {
                    let a = gdt + d;
                    m.hw.rd16(a + 2) as u32 | (m.hw.rd(a + 4) as u32) << 16 | (m.hw.rd(a + 7) as u32) << 24
                };
                let src = base(self, 0x10);
                let dst = base(self, 0x18);
                let n = self.cpu.cx() as u32 * 2;
                let a20 = self.hw.a20;
                self.hw.a20 = true;
                for i in 0..n {
                    if dst + i >= crate::hw::EXT_BASE {
                        self.hw.grow_ext(dst + i + 1);
                    }
                    let b = self.hw.rd(src + i);
                    self.hw.wr(dst + i, b);
                }
                self.hw.a20 = a20;
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            0x88 => {
                // Extended memory: all of it belongs to the XMS driver.
                self.cpu.set_ax(0);
                self.set_ret_flag(CF, false);
            }
            0x90 | 0x91 => {
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            0xc0 => {
                self.cpu.seg[ES] = x86emu::Seg::real(ROM);
                self.cpu.set_bx(CONFIG_TABLE);
                self.cpu.set_ah(0);
                self.set_ret_flag(CF, false);
            }
            0xc2 => {
                // PS/2 mouse BIOS: not available (INT 33h is).
                self.cpu.set_ah(0x86);
                self.set_ret_flag(CF, true);
            }
            0xe8 | 0xe9 => {
                self.cpu.set_ah(0x86);
                self.set_ret_flag(CF, true);
            }
            _ => {
                self.cpu.set_ah(0x86);
                self.set_ret_flag(CF, true);
            }
        }
        Cb::Done
    }

    // ---- Video (INT 10h) ----

    pub fn bda_mode(&mut self) -> u8 {
        self.hw.rd(BDA + 0x49)
    }
    pub fn bda_cols(&mut self) -> u16 {
        self.hw.rd16(BDA + 0x4a)
    }
    pub fn bda_rows(&mut self) -> u16 {
        self.hw.rd(BDA + 0x84) as u16 + 1
    }
    fn page(&mut self) -> u8 {
        self.hw.rd(BDA + 0x62)
    }
    fn text_mode(&mut self) -> bool {
        let m = self.bda_mode();
        m <= 3 || m == 7
    }
    fn text_base(&mut self) -> u32 {
        if self.bda_mode() == 7 { 0xb0000 } else { 0xb8000 }
    }

    pub fn cursor(&mut self, page: u8) -> (u8, u8) {
        let v = self.hw.rd16(BDA + 0x50 + (page as u32 & 7) * 2);
        ((v >> 8) as u8, v as u8)
    }

    pub fn set_cursor(&mut self, page: u8, row: u8, col: u8) {
        self.hw.wr16(BDA + 0x50 + (page as u32 & 7) * 2, (row as u16) << 8 | col as u16);
        if page == self.page() {
            let cols = self.bda_cols();
            let start = self.hw.rd16(BDA + 0x4e) / 2;
            let pos = start + row as u16 * cols + col as u16;
            self.hw.vga.crtc[0x0e] = (pos >> 8) as u8;
            self.hw.vga.crtc[0x0f] = pos as u8;
            self.hw.vga.dirty = true;
        }
    }

    /// INT 10h AH=00h.
    pub fn set_mode(&mut self, mode: u8, keep: bool) {
        self.hw.vga.vbe = None;
        let mode = mode & 0x7f;
        let m = match mode {
            0..=7 | 0x0d..=0x13 => mode,
            _ => 3,
        };
        self.hw.vga.set_mode_regs(m);
        self.hw.vga.load_dac(m);
        let (cols, rows, ch, page) = match m {
            0 | 1 => (40u16, 25u8, 16u8, 0x0800u16),
            2 | 3 | 7 => (80, 25, 16, 0x1000),
            4 | 5 | 0x0d | 0x13 => (40, 25, 8, 0x4000),
            6 | 0x0e => (80, 25, 8, 0x4000),
            0x0f | 0x10 => (80, 25, 14, 0x8000),
            _ => (80, 30, 16, 0xa000),
        };
        self.hw.wr(BDA + 0x49, m);
        self.hw.wr16(BDA + 0x4a, cols);
        self.hw.wr16(BDA + 0x4c, page);
        self.hw.wr16(BDA + 0x4e, 0);
        for p in 0..8 {
            self.hw.wr16(BDA + 0x50 + p * 2, 0);
        }
        self.hw.wr16(BDA + 0x60, 0x0607);
        self.hw.wr(BDA + 0x62, 0);
        self.hw.wr16(BDA + 0x63, if m == 7 { 0x3b4 } else { 0x3d4 });
        self.hw.wr(BDA + 0x65, self.hw.vga.cga_mode);
        self.hw.wr(BDA + 0x66, self.hw.vga.cga_color);
        self.hw.wr(BDA + 0x84, rows - 1);
        self.hw.wr16(BDA + 0x85, ch as u16);
        self.hw.wr(BDA + 0x87, if keep { 0xe0 } else { 0x60 });
        self.hw.wr(BDA + 0x88, 0x09);
        self.hw.wr(BDA + 0x89, 0x11);
        self.hw.wr(BDA + 0x8a, 0x08);
        // INT 43h: the graphics font for the mode's character height.
        let f = match ch {
            8 => self.fonts[0],
            14 => self.fonts[1],
            _ => self.fonts[2],
        };
        self.set_vector(0x43, f.0, f.1);
        if !keep {
            if m <= 3 || m == 7 {
                let base = if m == 7 { 0xb0000 } else { 0xb8000 };
                for i in 0..0x4000u32 {
                    self.hw.wr16(base + i * 2, 0x0720);
                }
            } else if m <= 6 {
                for i in 0..0x4000u32 {
                    self.hw.ram[0xb8000 + i as usize] = 0;
                }
            } else {
                self.hw.vga.planes.iter_mut().for_each(|b| *b = 0);
            }
        }
        // Text cursor: lines 13-14 of the 16-line cell.
        self.hw.vga.crtc[0x0a] = 0x0d;
        self.hw.vga.crtc[0x0b] = 0x0e;
        self.hw.vga.crtc[0x0e] = 0;
        self.hw.vga.crtc[0x0f] = 0;
        self.hw.vga.dirty = true;
    }

    /// Text rows with a font of `h` lines (INT 10h AX=1111h/1112h/1114h).
    fn set_text_font(&mut self, h: u8) {
        if !self.text_mode() {
            return;
        }
        let lines = self.hw.vga.scanlines();
        let rows = (lines / h as u32).max(1) as u8;
        self.hw.vga.crtc[9] = (self.hw.vga.crtc[9] & 0xe0) | (h - 1);
        self.hw.vga.crtc[0x0a] = h.saturating_sub(3);
        self.hw.vga.crtc[0x0b] = h.saturating_sub(2);
        self.hw.wr(BDA + 0x84, rows - 1);
        self.hw.wr16(BDA + 0x85, h as u16);
        let cols = self.bda_cols();
        self.hw.wr16(BDA + 0x4c, ((cols as u32 * rows as u32 * 2 + 0xff) & !0xff) as u16);
        self.hw.vga.dirty = true;
    }

    /// Write a character to the screen as the teletype does (CR, LF, BS,
    /// BEL; wraps and scrolls).
    pub fn tty_out(&mut self, ch: u8) {
        let page = self.page();
        let (mut row, mut col) = self.cursor(page);
        let cols = self.bda_cols() as u8;
        let rows = self.bda_rows() as u8;
        match ch {
            b'\r' => col = 0,
            b'\n' => row += 1,
            8 => col = col.saturating_sub(1),
            7 => self.beep_until = self.hw.us_now + 150_000,
            _ => {
                if self.text_mode() {
                    let a = self.cell_addr(page, row, col);
                    self.hw.wr(a, ch);
                } else {
                    let fg = 0x0f;
                    self.gfx_char(row, col, ch, fg, false);
                }
                col += 1;
                if col >= cols {
                    col = 0;
                    row += 1;
                }
            }
        }
        if row >= rows {
            let attr = if self.text_mode() {
                let a = self.cell_addr(page, rows - 1, 0);
                self.hw.rd(a + 1)
            } else {
                0
            };
            self.scroll(page, true, 1, attr, 0, 0, rows - 1, cols - 1);
            row = rows - 1;
        }
        self.set_cursor(page, row, col);
    }

    fn cell_addr(&mut self, page: u8, row: u8, col: u8) -> u32 {
        let base = self.text_base();
        let size = self.hw.rd16(BDA + 0x4c) as u32;
        let cols = self.bda_cols() as u32;
        base + page as u32 * size + (row as u32 * cols + col as u32) * 2
    }

    /// Scroll a window up or down by `n` lines (0 = clear it).
    #[allow(clippy::too_many_arguments)]
    fn scroll(&mut self, page: u8, up: bool, n: u8, attr: u8, top: u8, left: u8, bottom: u8, right: u8) {
        let cols = self.bda_cols() as u8;
        let rows = self.bda_rows() as u8;
        let right = right.min(cols - 1);
        let bottom = bottom.min(rows - 1);
        if top > bottom || left > right {
            return;
        }
        let height = bottom - top + 1;
        let n = if n == 0 || n > height { height } else { n };
        if self.text_mode() {
            for i in 0..height {
                let dst = if up { top + i } else { bottom - i };
                let keep = i + n < height;
                for c in left..=right {
                    let d = self.cell_addr(page, dst, c);
                    let v = if keep {
                        let src = if up { dst + n } else { dst - n };
                        let s = self.cell_addr(page, src, c);
                        self.hw.rd16(s)
                    } else {
                        (attr as u16) << 8 | 0x20
                    };
                    self.hw.wr16(d, v);
                }
            }
        } else {
            let ch = self.hw.rd16(BDA + 0x85) as u32;
            for i in 0..height {
                let dst = if up { top + i } else { bottom - i };
                let keep = i + n < height;
                let src = if up { dst.wrapping_add(n) } else { dst.wrapping_sub(n) };
                for line in 0..ch {
                    let dy = dst as u32 * ch + line;
                    let sy = src as u32 * ch + line;
                    for x in left as u32 * 8..(right as u32 + 1) * 8 {
                        let c = if keep { self.get_pixel(x, sy) } else { attr };
                        self.put_pixel(x, dy, c);
                    }
                }
            }
        }
        self.hw.vga.dirty = true;
    }

    /// Draw a character in a graphics mode.
    fn gfx_char(&mut self, row: u8, col: u8, ch: u8, color: u8, xor: bool) {
        let h = self.hw.rd16(BDA + 0x85) as u32;
        let (seg, off) = self.vector(0x43);
        let (seg, off) = if h == 8 && ch >= 128 && self.bda_mode() <= 6 {
            let (s, o) = self.vector(0x1f);
            (s, o.wrapping_add((ch as u16 - 128) * 8).wrapping_sub(ch as u16 * 8))
        } else {
            (seg, off)
        };
        let font = Machine::lin(seg, off) + ch as u32 * h;
        for y in 0..h {
            let bits = self.hw.rd(font + y);
            for x in 0..8u32 {
                let on = bits & (0x80 >> x) != 0;
                let px = col as u32 * 8 + x;
                let py = row as u32 * h + y;
                if xor {
                    if on {
                        let c = self.get_pixel(px, py);
                        self.put_pixel(px, py, c ^ color);
                    }
                } else {
                    self.put_pixel(px, py, if on { color } else { 0 });
                }
            }
        }
    }

    pub fn put_pixel(&mut self, x: u32, y: u32, c: u8) {
        let m = self.bda_mode();
        match m {
            4 | 5 => {
                let a = 0xb8000 + (y & 1) * 0x2000 + (y >> 1) * 80 + x / 4;
                let sh = 6 - (x & 3) * 2;
                let v = self.hw.ram[a as usize];
                self.hw.ram[a as usize] = (v & !(3 << sh)) | (c & 3) << sh;
            }
            6 => {
                let a = 0xb8000 + (y & 1) * 0x2000 + (y >> 1) * 80 + x / 8;
                let sh = 7 - (x & 7);
                let v = self.hw.ram[a as usize];
                self.hw.ram[a as usize] = (v & !(1 << sh)) | (c & 1) << sh;
            }
            0x13 => {
                let lin = y * 320 + x;
                if lin < 64000 {
                    self.hw.vga.planes[(lin & 3) as usize * crate::video::PLANE + (lin >> 2) as usize] = c;
                }
            }
            0x0d..=0x12 => {
                let bpr = if m == 0x0d { 40 } else { 80 };
                let o = (y * bpr + x / 8) as usize & 0xffff;
                let bit = 0x80 >> (x & 7);
                for p in 0..4 {
                    let b = &mut self.hw.vga.planes[p * crate::video::PLANE + o];
                    if c & (1 << p) != 0 {
                        *b |= bit;
                    } else {
                        *b &= !bit;
                    }
                }
            }
            _ => {}
        }
        self.hw.vga.dirty = true;
    }

    pub fn get_pixel(&mut self, x: u32, y: u32) -> u8 {
        let m = self.bda_mode();
        match m {
            4 | 5 => {
                let a = 0xb8000 + (y & 1) * 0x2000 + (y >> 1) * 80 + x / 4;
                (self.hw.ram[a as usize] >> (6 - (x & 3) * 2)) & 3
            }
            6 => {
                let a = 0xb8000 + (y & 1) * 0x2000 + (y >> 1) * 80 + x / 8;
                (self.hw.ram[a as usize] >> (7 - (x & 7))) & 1
            }
            0x13 => {
                let lin = y * 320 + x;
                if lin < 64000 { self.hw.vga.planes[(lin & 3) as usize * crate::video::PLANE + (lin >> 2) as usize] } else { 0 }
            }
            0x0d..=0x12 => {
                let bpr = if m == 0x0d { 40 } else { 80 };
                let o = (y * bpr + x / 8) as usize & 0xffff;
                let bit = 7 - (x & 7);
                (0..4).fold(0, |c, p| c | ((self.hw.vga.planes[p * crate::video::PLANE + o] >> bit) & 1) << p)
            }
            _ => 0,
        }
    }

    pub(crate) fn int10<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        let ah = self.cpu.ah();
        match ah {
            0x00 => {
                let al = self.cpu.al();
                self.set_mode(al, al & 0x80 != 0);
                let m = self.bda_mode();
                self.cpu.set_al(if m <= 7 && m != 6 { 0x30 } else { 0x20 });
            }
            0x01 => {
                let ch = self.cpu.ch();
                let cl = self.cpu.cl();
                self.hw.wr16(BDA + 0x60, (ch as u16) << 8 | cl as u16);
                // CGA-style 8-line shapes are emulated on the cell height.
                let h = self.hw.rd16(BDA + 0x85) as u8;
                let (mut s, mut e) = (ch & 0x1f, cl & 0x1f);
                if h > 8 && s < 8 && e < 8 && ch & 0x20 == 0 {
                    if e < s {
                        // Split: keep as is.
                    } else if s >= 4 || e >= 7 {
                        let d = h - 8;
                        if e >= 6 {
                            e += d;
                            if s >= 4 {
                                s += d;
                            }
                        }
                    }
                }
                self.hw.vga.crtc[0x0a] = (ch & 0x60) | s;
                self.hw.vga.crtc[0x0b] = e;
                self.hw.vga.dirty = true;
            }
            0x02 => {
                let (bh, dh, dl) = (self.cpu.bh(), self.cpu.dh(), self.cpu.dl());
                self.set_cursor(bh, dh, dl);
            }
            0x03 => {
                let (r, c) = self.cursor(self.cpu.bh());
                self.cpu.set_dh(r);
                self.cpu.set_dl(c);
                let shape = self.hw.rd16(BDA + 0x60);
                self.cpu.set_cx(shape);
            }
            0x04 => self.cpu.set_ah(0),
            0x05 => {
                let p = self.cpu.al() & 7;
                self.hw.wr(BDA + 0x62, p);
                let size = self.hw.rd16(BDA + 0x4c);
                let start = p as u16 * size;
                self.hw.wr16(BDA + 0x4e, start);
                let cells = if self.text_mode() { start / 2 } else { start };
                self.hw.vga.crtc[0x0c] = (cells >> 8) as u8;
                self.hw.vga.crtc[0x0d] = cells as u8;
                let (r, c) = self.cursor(p);
                self.set_cursor(p, r, c);
            }
            0x06 | 0x07 => {
                let page = self.page();
                let (al, bh, ch, cl, dh, dl) = (self.cpu.al(), self.cpu.bh(), self.cpu.ch(), self.cpu.cl(), self.cpu.dh(), self.cpu.dl());
                self.scroll(page, ah == 6, al, bh, ch, cl, dh, dl);
            }
            0x08 => {
                let p = self.cpu.bh();
                let (r, c) = self.cursor(p);
                if self.text_mode() {
                    let a = self.cell_addr(p, r, c);
                    let v = self.hw.rd16(a);
                    self.cpu.set_ax(v);
                } else {
                    self.cpu.set_ax(0);
                }
            }
            0x09 | 0x0a => {
                let p = self.cpu.bh();
                let (r, c) = self.cursor(p);
                let (ch, attr) = (self.cpu.al(), self.cpu.bl());
                let cols = self.bda_cols() as u32;
                let n = self.cpu.cx() as u32;
                if self.text_mode() {
                    let a = self.cell_addr(p, r, c);
                    for i in 0..n.min(cols * 60) {
                        self.hw.wr(a + i * 2, ch);
                        if ah == 9 {
                            self.hw.wr(a + i * 2 + 1, attr);
                        }
                    }
                } else {
                    for i in 0..n.min(cols * 60) {
                        let pos = c as u32 + i;
                        let (rr, cc) = (r as u32 + pos / cols, pos % cols);
                        self.gfx_char(rr as u8, cc as u8, ch, attr & 0x7f, attr & 0x80 != 0);
                    }
                }
            }
            0x0b => {
                // CGA palette: BH 0 background/border, 1 palette select.
                let mut v = self.hw.vga.cga_color;
                if self.cpu.bh() == 0 {
                    v = (v & 0xe0) | (self.cpu.bl() & 0x1f);
                } else {
                    v = (v & !0x20) | if self.cpu.bl() & 1 != 0 { 0x20 } else { 0 };
                }
                self.hw.vga.cga_color = v;
                self.hw.wr(BDA + 0x66, v);
                // The EGA palette registers follow.
                let pal1 = v & 0x20 != 0;
                let bright = if v & 0x10 != 0 { 0x10 } else { 0 };
                let colors: [u8; 3] = if pal1 { [0x03, 0x05, 0x07] } else { [0x02, 0x04, 0x06] };
                if self.bda_mode() == 4 || self.bda_mode() == 5 {
                    for (i, c) in colors.iter().enumerate() {
                        self.hw.vga.attr[i + 1] = c | bright;
                    }
                }
                let bg = v & 0x0f;
                self.hw.vga.attr[0] = (bg & 7) | if bg & 8 != 0 { 0x10 } else { 0 };
                self.hw.vga.dirty = true;
                self.hw.vga.palette_dirty = true;
            }
            0x0c => {
                let (x, y, c) = (self.cpu.cx() as u32, self.cpu.dx() as u32, self.cpu.al());
                if c & 0x80 != 0 && self.bda_mode() != 0x13 {
                    let old = self.get_pixel(x, y);
                    self.put_pixel(x, y, old ^ (c & 0x7f));
                } else {
                    self.put_pixel(x, y, c);
                }
            }
            0x0d => {
                let c = self.get_pixel(self.cpu.cx() as u32, self.cpu.dx() as u32);
                self.cpu.set_al(c);
            }
            0x0e => {
                let al = self.cpu.al();
                if !self.text_mode() && al >= 0x20 {
                    let p = self.page();
                    let (r, c) = self.cursor(p);
                    let color = self.cpu.bl();
                    self.gfx_char(r, c, al, color, false);
                    let cols = self.bda_cols() as u8;
                    let (mut r, mut c) = (r, c + 1);
                    if c >= cols {
                        c = 0;
                        r += 1;
                    }
                    let rows = self.bda_rows() as u8;
                    if r >= rows {
                        self.scroll(p, true, 1, 0, 0, 0, rows - 1, cols - 1);
                        r = rows - 1;
                    }
                    self.set_cursor(p, r, c);
                } else {
                    self.tty_out(al);
                }
            }
            0x0f => {
                let m = self.bda_mode() | (self.hw.rd(BDA + 0x87) & 0x80);
                let cols = self.bda_cols() as u8;
                let p = self.page();
                self.cpu.set_al(m);
                self.cpu.set_ah(cols);
                self.cpu.set_bh(p);
            }
            0x10 => self.int10_palette(),
            0x11 => self.int10_font(),
            0x12 => match self.cpu.bl() {
                0x10 => {
                    self.cpu.set_bx(0x0003);
                    self.cpu.set_cx(0x0009);
                }
                0x30 | 0x31 | 0x32 | 0x33 | 0x34 | 0x36 => self.cpu.set_al(0x12),
                _ => {}
            },
            0x13 => {
                // Write string ES:BP, CX chars, at DH,DL on page BH.
                let al = self.cpu.al();
                let p = self.cpu.bh();
                let (orow, ocol) = self.cursor(p);
                let (r, c) = (self.cpu.dh(), self.cpu.dl());
                self.set_cursor(p, r, c);
                let mut a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.bp());
                let mut attr = self.cpu.bl();
                for _ in 0..self.cpu.cx() {
                    let ch = self.hw.rd(a);
                    a += 1;
                    if al & 2 != 0 {
                        attr = self.hw.rd(a);
                        a += 1;
                    }
                    if matches!(ch, 7 | 8 | 10 | 13) || !self.text_mode() {
                        self.tty_out(ch);
                    } else {
                        let (r, c) = self.cursor(p);
                        let addr = self.cell_addr(p, r, c);
                        self.hw.wr(addr, ch);
                        self.hw.wr(addr + 1, attr);
                        self.tty_advance(p);
                    }
                }
                if al & 1 == 0 {
                    self.set_cursor(p, orow, ocol);
                }
            }
            0x1a => {
                if self.cpu.al() == 0 {
                    self.cpu.set_al(0x1a);
                    self.cpu.set_bx(0x0008);
                } else {
                    self.cpu.set_al(0x1a);
                }
            }
            0x1b => {
                // Functionality/state information.
                let a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.di());
                for i in 0..64 {
                    self.hw.wr(a + i, 0);
                }
                self.hw.wr16(a, VIDEO_STATIC);
                self.hw.wr16(a + 2, ROM);
                let m = self.bda_mode();
                let cols = self.bda_cols();
                self.hw.wr(a + 4, m);
                self.hw.wr16(a + 5, cols);
                let ps = self.hw.rd16(BDA + 0x4c);
                self.hw.wr16(a + 7, ps);
                for p in 0..8 {
                    let c = self.hw.rd16(BDA + 0x50 + p * 2);
                    self.hw.wr16(a + 0x0b + p * 2, c);
                }
                let shape = self.hw.rd16(BDA + 0x60);
                self.hw.wr16(a + 0x1b, shape);
                let pg = self.page();
                self.hw.wr(a + 0x1d, pg);
                self.hw.wr16(a + 0x1e, 0x3d4);
                let rows = self.bda_rows() as u8;
                self.hw.wr(a + 0x22, rows);
                let ch = self.hw.rd16(BDA + 0x85);
                self.hw.wr16(a + 0x23, ch);
                self.hw.wr(a + 0x25, 0x08);
                self.hw.wr16(a + 0x27, 256);
                self.hw.wr(a + 0x29, 8);
                self.hw.wr(a + 0x2a, 2);
                self.hw.wr(a + 0x31, 3);
                self.cpu.set_al(0x1b);
            }
            0x4f => self.vbe(),
            _ => {}
        }
    }

    /// INT 10h AH=4Fh: VESA BIOS extensions 2.0 (8-bit modes, a banked
    /// window at A000h and a linear framebuffer).
    fn vbe(&mut self) {
        use crate::video::{Vbe, LFB_BASE, VBE_MEM, VBE_MODES};
        let al = self.cpu.al();
        let es_di = Machine::lin(self.cpu.seg[ES].sel, self.cpu.di());
        let ok = |m: &mut Machine| m.cpu.set_ax(0x004f);
        match al {
            0x00 => {
                let vbe2 = self.hw.rd32(es_di) == u32::from_le_bytes(*b"VBE2");
                for i in 0..if vbe2 { 512 } else { 256 } {
                    self.hw.wr(es_di + i, 0);
                }
                self.hw.write_bytes(es_di, b"VESA");
                self.hw.wr16(es_di + 4, 0x0200);
                // OEM string and the mode list in ROM.
                self.hw.wr16(es_di + 6, VBE_TABLES);
                self.hw.wr16(es_di + 8, ROM);
                self.hw.wr32(es_di + 10, 0);
                self.hw.wr16(es_di + 14, VBE_TABLES + 0x20);
                self.hw.wr16(es_di + 16, ROM);
                self.hw.wr16(es_di + 18, (VBE_MEM / 65536) as u16);
                if vbe2 {
                    self.hw.wr16(es_di + 20, 0x0100);
                    for (o, t) in [(22u32, 0u16), (26, 0x30), (30, 0x30)] {
                        self.hw.wr16(es_di + o, VBE_TABLES + t);
                        self.hw.wr16(es_di + o + 2, ROM);
                    }
                }
                ok(self);
            }
            0x01 => {
                let cx = self.cpu.cx() & 0x1ff;
                let Some(&(_, w, h)) = VBE_MODES.iter().find(|m| m.0 == cx) else {
                    self.cpu.set_ax(0x014f);
                    return;
                };
                for i in 0..256 {
                    self.hw.wr(es_di + i, 0);
                }
                // Supported, colour graphics, linear framebuffer too.
                self.hw.wr16(es_di, 0x009b);
                self.hw.wr(es_di + 2, 7);
                self.hw.wr16(es_di + 4, 64);
                self.hw.wr16(es_di + 6, 64);
                self.hw.wr16(es_di + 8, 0xa000);
                self.hw.wr16(es_di + 0x0c, VBE_TABLES + 0x40);
                self.hw.wr16(es_di + 0x0e, ROM);
                self.hw.wr16(es_di + 0x10, w);
                self.hw.wr16(es_di + 0x12, w);
                self.hw.wr16(es_di + 0x14, h);
                self.hw.wr(es_di + 0x16, 8);
                self.hw.wr(es_di + 0x17, 16);
                self.hw.wr(es_di + 0x18, 1);
                self.hw.wr(es_di + 0x19, 8);
                self.hw.wr(es_di + 0x1a, 1);
                self.hw.wr(es_di + 0x1b, 4);
                let pages = (VBE_MEM / (w as usize * h as usize)).clamp(1, 255) - 1;
                self.hw.wr(es_di + 0x1d, pages as u8);
                self.hw.wr(es_di + 0x1e, 1);
                self.hw.wr32(es_di + 0x28, LFB_BASE);
                ok(self);
            }
            0x02 => {
                let bx = self.cpu.bx();
                let mode = bx & 0x1ff;
                if mode < 0x100 {
                    self.hw.vga.vbe = None;
                    self.set_mode(mode as u8, bx & 0x8000 != 0);
                    ok(self);
                    return;
                }
                let Some(&(_, w, h)) = VBE_MODES.iter().find(|m| m.0 == mode) else {
                    self.cpu.set_ax(0x014f);
                    return;
                };
                self.set_mode(0x13, true);
                if self.hw.vga.vbe_mem.len() < VBE_MEM {
                    self.hw.vga.vbe_mem.resize(VBE_MEM, 0);
                }
                if bx & 0x8000 == 0 {
                    self.hw.vga.vbe_mem.iter_mut().for_each(|b| *b = 0);
                }
                self.hw.vga.vbe = Some(Vbe { mode, width: w, height: h, pitch: w as u32, bank: 0, start: 0 });
                self.hw.wr(0x449, 0x13);
                self.hw.vga.dirty = true;
                ok(self);
            }
            0x03 => {
                let m = self.hw.vga.vbe.map_or(self.bda_mode() as u16, |b| b.mode);
                self.cpu.set_bx(m);
                ok(self);
            }
            0x04 => {
                // Save/restore state: nothing to save.
                self.cpu.set_bx(1);
                ok(self);
            }
            0x05 => {
                let Some(b) = self.hw.vga.vbe.as_mut() else {
                    self.cpu.set_ax(0x034f);
                    return;
                };
                if self.cpu.bh() == 0 {
                    b.bank = self.cpu.dx() as u32 % (VBE_MEM as u32 >> 16);
                } else {
                    let bank = b.bank as u16;
                    self.cpu.set_dx(bank);
                }
                ok(self);
            }
            0x06 => {
                let Some(b) = self.hw.vga.vbe.as_mut() else {
                    self.cpu.set_ax(0x034f);
                    return;
                };
                match self.cpu.bl() {
                    0 => b.pitch = (self.cpu.cx() as u32).max(b.width as u32),
                    2 => b.pitch = (self.cpu.cx() as u32).max(b.width as u32),
                    _ => {}
                }
                let (pitch, h) = (b.pitch, b.height);
                self.cpu.set_bx(pitch as u16);
                self.cpu.set_cx(pitch as u16);
                self.cpu.set_dx(((VBE_MEM as u32 / pitch) as u16).max(h));
                self.hw.vga.dirty = true;
                ok(self);
            }
            0x07 => {
                let Some(b) = self.hw.vga.vbe.as_mut() else {
                    self.cpu.set_ax(0x034f);
                    return;
                };
                match self.cpu.bl() & 0x7f {
                    0 => b.start = self.cpu.dx() as u32 * b.pitch + self.cpu.cx() as u32,
                    1 => {
                        let (x, y) = (b.start % b.pitch, b.start / b.pitch);
                        self.cpu.set_cx(x as u16);
                        self.cpu.set_dx(y as u16);
                    }
                    _ => {}
                }
                self.hw.vga.dirty = true;
                ok(self);
            }
            0x08 => {
                self.cpu.set_bh(6);
                ok(self);
            }
            0x09 => {
                let (start, n) = (self.cpu.dx() as usize, self.cpu.cx() as usize);
                for i in 0..n {
                    let a = es_di + i as u32 * 4;
                    let (b, g, r) = (self.hw.rd(a), self.hw.rd(a + 1), self.hw.rd(a + 2));
                    if self.cpu.bl() & 1 == 0 {
                        if let Some(e) = self.hw.vga.dac.get_mut(start + i) {
                            *e = [r & 0x3f, g & 0x3f, b & 0x3f];
                        }
                    } else if let Some(e) = self.hw.vga.dac.get(start + i).copied() {
                        self.hw.wr(a, e[2]);
                        self.hw.wr(a + 1, e[1]);
                        self.hw.wr(a + 2, e[0]);
                    }
                }
                self.hw.vga.palette_dirty = true;
                self.hw.vga.dirty = true;
                ok(self);
            }
            _ => self.cpu.set_ax(0x014f),
        }
    }

    fn tty_advance(&mut self, p: u8) {
        let (mut r, mut c) = self.cursor(p);
        let cols = self.bda_cols() as u8;
        let rows = self.bda_rows() as u8;
        c += 1;
        if c >= cols {
            c = 0;
            r += 1;
        }
        if r >= rows {
            self.scroll(p, true, 1, 0x07, 0, 0, rows - 1, cols - 1);
            r = rows - 1;
        }
        self.set_cursor(p, r, c);
    }

    fn int10_palette(&mut self) {
        let v = &mut self.hw.vga;
        v.palette_dirty = true;
        v.dirty = true;
        match self.cpu.al() {
            0x00 => {
                let i = self.cpu.bl() as usize;
                if i < 16 {
                    self.hw.vga.attr[i] = self.cpu.bh();
                }
            }
            0x01 => self.hw.vga.attr[0x11] = self.cpu.bh(),
            0x02 => {
                let a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.dx());
                for i in 0..16 {
                    let b = self.hw.rd(a + i);
                    self.hw.vga.attr[i as usize] = b;
                }
                let o = self.hw.rd(a + 16);
                self.hw.vga.attr[0x11] = o;
            }
            0x03 => {
                let on = self.cpu.bl() != 0;
                let m = &mut self.hw.vga.attr[0x10];
                *m = (*m & !8) | if on { 8 } else { 0 };
            }
            0x07 => {
                let i = (self.cpu.bl() & 0xf) as usize;
                let v = self.hw.vga.attr[i];
                self.cpu.set_bh(v);
            }
            0x08 => {
                let v = self.hw.vga.attr[0x11];
                self.cpu.set_bh(v);
            }
            0x09 => {
                let a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.dx());
                for i in 0..16 {
                    let v = self.hw.vga.attr[i as usize];
                    self.hw.wr(a + i, v);
                }
                let o = self.hw.vga.attr[0x11];
                self.hw.wr(a + 16, o);
            }
            0x10 => {
                let i = self.cpu.bl() as usize;
                self.hw.vga.dac[i] = [self.cpu.dh() & 0x3f, self.cpu.ch() & 0x3f, self.cpu.cl() & 0x3f];
            }
            0x12 => {
                let a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.dx());
                let start = self.cpu.bx() as usize;
                for i in 0..self.cpu.cx() as usize {
                    let at = a + i as u32 * 3;
                    let rgb = [self.hw.rd(at) & 0x3f, self.hw.rd(at + 1) & 0x3f, self.hw.rd(at + 2) & 0x3f];
                    if start + i < 256 {
                        self.hw.vga.dac[start + i] = rgb;
                    }
                }
            }
            0x13 => {
                let m = &mut self.hw.vga.attr[0x10];
                if self.cpu.bl() == 0 {
                    *m = (*m & !0x80) | if self.cpu.bh() & 1 != 0 { 0x80 } else { 0 };
                } else {
                    self.hw.vga.attr[0x14] = self.cpu.bh();
                }
            }
            0x15 => {
                let [r, g, b] = self.hw.vga.dac[self.cpu.bl() as usize];
                self.cpu.set_dh(r);
                self.cpu.set_ch(g);
                self.cpu.set_cl(b);
            }
            0x17 => {
                let a = Machine::lin(self.cpu.seg[ES].sel, self.cpu.dx());
                let start = self.cpu.bx() as usize;
                for i in 0..self.cpu.cx() as usize {
                    let c = self.hw.vga.dac[(start + i) & 0xff];
                    let at = a + i as u32 * 3;
                    self.hw.wr(at, c[0]);
                    self.hw.wr(at + 1, c[1]);
                    self.hw.wr(at + 2, c[2]);
                }
            }
            0x18 => self.hw.vga.pel_mask = self.cpu.bl(),
            0x19 => {
                let m = self.hw.vga.pel_mask;
                self.cpu.set_bl(m);
            }
            0x1a => {
                let m = self.hw.vga.attr[0x10];
                let s = self.hw.vga.attr[0x14];
                self.cpu.set_bl(m >> 7);
                self.cpu.set_bh(if m & 0x80 != 0 { s & 0xf } else { (s >> 2) & 3 });
            }
            0x1b => {
                let start = self.cpu.bx() as usize;
                for i in 0..self.cpu.cx() as usize {
                    let [r, g, b] = self.hw.vga.dac[(start + i) & 0xff];
                    let y = ((r as u32 * 30 + g as u32 * 59 + b as u32 * 11 + 50) / 100) as u8;
                    self.hw.vga.dac[(start + i) & 0xff] = [y, y, y];
                }
            }
            _ => {}
        }
    }

    fn int10_font(&mut self) {
        let al = self.cpu.al();
        match al {
            0x00 | 0x10 => {
                // User font ES:BP, CX characters from DX, BH bytes each,
                // BL block: in the font plane (kept as the ROM copy).
                let h = self.cpu.bh();
                if al == 0x10 && (8..=32).contains(&h) {
                    self.set_text_font(h);
                }
            }
            0x01 | 0x11 => self.set_text_font(14),
            0x02 | 0x12 => self.set_text_font(8),
            0x04 | 0x14 => self.set_text_font(16),
            0x03 => {}
            0x20 => {
                let (es, bp) = (self.cpu.seg[ES].sel, self.cpu.bp());
                self.set_vector(0x1f, es, bp);
            }
            0x21 | 0x22 | 0x23 | 0x24 => {
                let f = match al {
                    0x22 => self.fonts[1],
                    0x23 => self.fonts[0],
                    0x24 => self.fonts[2],
                    _ => (self.cpu.seg[ES].sel, self.cpu.bp()),
                };
                self.set_vector(0x43, f.0, f.1);
                let h = match al {
                    0x21 => self.cpu.cx() as u8,
                    0x22 => 14,
                    0x23 => 8,
                    _ => 16,
                };
                self.hw.wr16(BDA + 0x85, h as u16);
            }
            0x30 => {
                let (seg, off) = match self.cpu.bh() {
                    0 => self.vector(0x1f),
                    1 => self.vector(0x43),
                    2 | 5 => self.fonts[1],
                    3 => self.fonts[0],
                    4 => (self.fonts[0].0, self.fonts[0].1 + 128 * 8),
                    _ => self.fonts[2],
                };
                self.cpu.seg[ES] = x86emu::Seg::real(seg);
                self.cpu.regs[x86emu::EBP] = (self.cpu.regs[x86emu::EBP] & 0xffff_0000) | off as u32;
                let h = self.hw.rd16(BDA + 0x85);
                self.cpu.set_cx(h);
                let r = self.hw.rd(BDA + 0x84);
                self.cpu.set_dl(r);
            }
            _ => {}
        }
        let _ = DS;
        let _ = IF;
    }
}
