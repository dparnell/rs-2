//! VIO, KBD, device monitor and character device checks.
//!
//! The 16-bit VIOCALLS/KBDCALLS/MONCALLS entries are called through
//! `os2::vio::call16`; their buffers live in one 64 KB block so that no
//! 16:16 pointer crosses a segment boundary.

use core::ptr::null_mut;

use os2::vio::{call16, ptr_words};
use os2::*;

use crate::check;

unsafe extern "C" {
    #[link_name = "VIOCALLS.1"]
    fn VIOENDPOPUP();
    #[link_name = "VIOCALLS.2"]
    fn VIOGETPHYSBUF();
    #[link_name = "VIOCALLS.3"]
    fn VIOGETANSI();
    #[link_name = "VIOCALLS.5"]
    fn VIOSETANSI();
    #[link_name = "VIOCALLS.6"]
    fn VIODEREGISTER();
    #[link_name = "VIOCALLS.9"]
    fn VIOGETCURPOS();
    #[link_name = "VIOCALLS.15"]
    fn VIOSETCURPOS();
    #[link_name = "VIOCALLS.45"]
    fn VIOREGISTER();
    #[link_name = "VIOCALLS.7"]
    fn VIOSCROLLUP();
    #[link_name = "VIOCALLS.11"]
    fn VIOPOPUP();
    #[link_name = "VIOCALLS.12"]
    fn VIOSCROLLRT();
    #[link_name = "VIOCALLS.13"]
    fn VIOWRTCHARSTR();
    #[link_name = "VIOCALLS.18"]
    fn VIOSCRUNLOCK();
    #[link_name = "VIOCALLS.19"]
    fn VIOWRTTTY();
    #[link_name = "VIOCALLS.21"]
    fn VIOGETMODE();
    #[link_name = "VIOCALLS.22"]
    fn VIOSETMODE();
    #[link_name = "VIOCALLS.23"]
    fn VIOSCRLOCK();
    #[link_name = "VIOCALLS.24"]
    fn VIOREADCELLSTR();
    #[link_name = "VIOCALLS.27"]
    fn VIOGETCURTYPE();
    #[link_name = "VIOCALLS.29"]
    fn VIOGETFONT();
    #[link_name = "VIOCALLS.30"]
    fn VIOREADCHARSTR();
    #[link_name = "VIOCALLS.32"]
    fn VIOSETCURTYPE();
    #[link_name = "VIOCALLS.46"]
    fn VIOGETCONFIG();
    #[link_name = "VIOCALLS.47"]
    fn VIOSCROLLDN();
    #[link_name = "VIOCALLS.49"]
    fn VIOGETSTATE();
    #[link_name = "VIOCALLS.51"]
    fn VIOSETSTATE();
    #[link_name = "VIOCALLS.52"]
    fn VIOWRTNCELL();
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
    #[link_name = "KBDCALLS.10"]
    fn KBDGETSTATUS();
    #[link_name = "KBDCALLS.11"]
    fn KBDSETSTATUS();
    #[link_name = "KBDCALLS.12"]
    fn KBDGETFOCUS();
    #[link_name = "KBDCALLS.14"]
    fn KBDXLATE();
    #[link_name = "KBDCALLS.17"]
    fn KBDCLOSE();
    #[link_name = "KBDCALLS.18"]
    fn KBDFREEFOCUS();
    #[link_name = "KBDCALLS.22"]
    fn KBDPEEK();
    #[link_name = "KBDCALLS.23"]
    fn KBDOPEN();
    #[link_name = "MONCALLS.1"]
    fn DOSMONWRITE();
    #[link_name = "MONCALLS.2"]
    fn DOSMONREAD();
    #[link_name = "MONCALLS.3"]
    fn DOSMONCLOSE();
    #[link_name = "MONCALLS.4"]
    fn DOSMONOPEN();
    #[link_name = "MONCALLS.5"]
    fn DOSMONREG();
}

/// A 64 KB block for 16-bit buffers, carved up by offset.
struct Buf(*mut u8);

impl Buf {
    fn at<T>(&self, off: usize) -> *mut T {
        unsafe { self.0.add(off) as *mut T }
    }
    fn bytes(&self, off: usize, n: usize) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.0.add(off), n) }
    }
    /// Far pointer words of offset `off`.
    fn p(&self, off: usize) -> [u16; 2] {
        ptr_words(self.at::<u8>(off))
    }
}

fn f(e: unsafe extern "C" fn()) -> usize {
    e as *const () as usize
}

fn call(e: unsafe extern "C" fn(), words: &[u16]) -> u16 {
    unsafe { call16(f(e), words) }
}

fn w16(b: &Buf, off: usize) -> u16 {
    unsafe { core::ptr::read_volatile(b.at::<u16>(off)) }
}

fn set16(b: &Buf, off: usize, v: u16) {
    unsafe { core::ptr::write_volatile(b.at::<u16>(off), v) }
}

fn set32(b: &Buf, off: usize, v: u32) {
    unsafe { core::ptr::write_volatile(b.at::<u32>(off), v) }
}

fn set_mode(b: &Buf, cols: u16, rows: u16) -> u16 {
    let m = b.bytes(0, 12);
    m.fill(0);
    set16(b, 0, 8);
    m[2] = 1;
    m[3] = 4;
    set16(b, 4, cols);
    set16(b, 6, rows);
    let p = b.p(0);
    call(VIOSETMODE, &[p[0], p[1], 0])
}

/// Rows of the current mode and the font height (VioGetFont type 0).
fn mode_rows(b: &Buf) -> (u16, u16) {
    set16(b, 0, 34);
    let p = b.p(0);
    let rc = call(VIOGETMODE, &[p[0], p[1], 0]);
    let rows = if rc == 0 { w16(b, 6) } else { 0 };
    // VIOFONTINFO at 64, font data at 0x1000.
    set16(b, 64, 14);
    set16(b, 66, 0);
    let q = b.p(0x1000);
    set16(b, 72, q[1]);
    set16(b, 74, q[0]);
    set16(b, 76, 4096);
    let p = b.p(64);
    let rc = call(VIOGETFONT, &[p[0], p[1], 0]);
    (rows, if rc == 0 { w16(b, 70) } else { 0 })
}

fn read_cells(b: &Buf, row: u16, col: u16, n: u16) -> &'static [u8] {
    set16(b, 0x100, n * 2);
    let (p, q) = (b.p(0x200), b.p(0x100));
    let rc = call(VIOREADCELLSTR, &[p[0], p[1], q[0], q[1], row, col, 0]);
    if rc != 0 {
        return &[];
    }
    b.bytes(0x200, w16(b, 0x100) as usize)
}

fn key_in(b: &Buf, wait: bool) -> (u8, u8, u8) {
    let p = b.p(0x300);
    let rc = call(KBDCHARIN, &[p[0], p[1], if wait { 0 } else { 1 }, 0]);
    let k = b.bytes(0x300, 10);
    if rc != 0 { (0, 0, 0) } else { (k[0], k[1], k[2]) }
}

/// The program runs in a PM window (a windowable VIO session): the
/// calls that need the physical screen fail there, as on OS/2.
fn windowed() -> bool {
    let mut tib = null_mut();
    let mut pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) == 0 && !pib.is_null() && (*pib).ultype == 2 }
}

/// ERROR_VIO_EXTENDED_SG: not in a VIO window.
const ERROR_VIO_EXTENDED_SG: u16 = 494;

pub fn run() {
    let mut mem = null_mut();
    if unsafe { DosAllocMem(&mut mem, 0x10000, PAG_READ | PAG_WRITE | PAG_COMMIT) } != 0 {
        check("console test memory", false);
        return;
    }
    let b = Buf(mem);
    modes(&b);
    cells(&b);
    state(&b);
    popup(&b);
    physbuf(&b);
    keyboard(&b);
    monitors(&b);
    subsystem(&b);
    devices(&b);
    unsafe { DosFreeMem(mem) };
}

fn modes(b: &Buf) {
    check("VioSetMode 80x50", set_mode(b, 80, 50) == 0);
    // The largest font that fits is used, which depends on the screen
    // height (8x14 at 768 pixels, 8x16 at 1024).
    let fits = |(r, f): (u16, u16), rows: u16| r == rows && matches!(f, 8 | 14 | 16);
    check("50 rows with a built-in font", fits(mode_rows(b), 50));
    check("VioSetMode 80x60", set_mode(b, 80, 60) == 0);
    check("60 rows with a built-in font", fits(mode_rows(b), 60));
    check("VioSetMode 80x43", set_mode(b, 80, 43) == 0);
    check("43 rows use the 8x16 font", mode_rows(b) == (43, 16));
    check("VioSetMode 81 columns fails", set_mode(b, 81, 25) == 355);
    check("VioSetMode 80x25", set_mode(b, 80, 25) == 0);
    check("25 rows use the 8x16 font", mode_rows(b) == (25, 16));
    // VioGetFont of the built-in 8x14 font.
    set16(b, 64, 14);
    set16(b, 66, 1);
    set16(b, 68, 8);
    set16(b, 70, 14);
    let q = b.p(0x1000);
    set16(b, 72, q[1]);
    set16(b, 74, q[0]);
    set16(b, 76, 4096);
    let p = b.p(64);
    let rc = call(VIOGETFONT, &[p[0], p[1], 0]);
    check("VioGetFont ROM 8x14", rc == 0 && w16(b, 76) == 256 * 14);
    // VioGetConfig: a VGA.
    set16(b, 0, 32);
    let p = b.p(0);
    let rc = call(VIOGETCONFIG, &[0, p[0], p[1], 0]);
    check("VioGetConfig VGA", rc == 0 && w16(b, 2) == 3);
}

fn cells(b: &Buf) {
    // 4 cells at row 20, column 70, then scroll right by one with a fill.
    let src = b.bytes(0x400, 8);
    src.copy_from_slice(b"A\x1fB\x1fC\x1fD\x1f");
    let p = b.p(0x400);
    set16(b, 0x410, 0x4e20); // fill: blank on red, yellow
    let rc = call(VIOWRTNCELL, &[p[0], p[1], 4, 20, 70, 0]);
    let x = b.p(0x420);
    b.bytes(0x420, 4).copy_from_slice(b"ABCD");
    let rc2 = call(VIOWRTCHARSTR, &[x[0], x[1], 4, 20, 70, 0]);
    check("VioWrtNCell + VioWrtCharStr", rc == 0 && rc2 == 0 && read_cells(b, 20, 70, 1) == b"A\x1f");
    let fill = b.p(0x410);
    let rc = call(VIOSCROLLRT, &[20, 70, 20, 73, 1, fill[0], fill[1], 0]);
    let got = read_cells(b, 20, 70, 4);
    check("VioScrollRt with fill cell", rc == 0 && got.len() == 8 && got[0] == b' ' && got[1] == 0x4e && got[2] == b'A');
    let rc = call(VIOSCROLLUP, &[20, 70, 21, 73, 0xffff, fill[0], fill[1], 0]);
    check("VioScrollUp -1 clears", rc == 0 && read_cells(b, 20, 72, 1) == b" \x4e");
    let rc = call(VIOSCROLLDN, &[0, 0, 0xffff, 0xffff, 0, fill[0], fill[1], 0]);
    check("VioScrollDn 0 lines", rc == 0);
    // VioReadCharStr past the screen end fails.
    set16(b, 0x100, 4);
    let (p, q) = (b.p(0x200), b.p(0x100));
    check("VioReadCharStr bad row", call(VIOREADCHARSTR, &[p[0], p[1], q[0], q[1], 200, 0, 0]) == 358);
}

fn state(b: &Buf) {
    // Cursor: hide and show again.
    let p = b.p(0x500);
    call(VIOGETCURTYPE, &[p[0], p[1], 0]);
    let start = w16(b, 0x500);
    set16(b, 0x506, 0xffff);
    let rc = call(VIOSETCURTYPE, &[p[0], p[1], 0]);
    call(VIOGETCURTYPE, &[p[0], p[1], 0]);
    check("VioSetCurType hidden", rc == 0 && w16(b, 0x506) == 0xffff);
    set16(b, 0x500, 0xffce); // -50: lower half
    set16(b, 0x502, 0xff9c); // -100
    set16(b, 0x504, 1);
    set16(b, 0x506, 0);
    check("VioSetCurType percent", call(VIOSETCURTYPE, &[p[0], p[1], 0]) == 0);
    set16(b, 0x500, start);
    set16(b, 0x502, start + 1);
    call(VIOSETCURTYPE, &[p[0], p[1], 0]);
    // Background intensity.
    let st = b.p(0x600);
    set16(b, 0x600, 6);
    set16(b, 0x602, 2);
    set16(b, 0x604, 1);
    let rc = call(VIOSETSTATE, &[st[0], st[1], 0]);
    set16(b, 0x604, 9);
    call(VIOGETSTATE, &[st[0], st[1], 0]);
    check("VioSetState intensity", rc == 0 && w16(b, 0x604) == 1);
    set16(b, 0x604, 0);
    call(VIOSETSTATE, &[st[0], st[1], 0]);
    // Palette registers.
    set16(b, 0x600, 10);
    set16(b, 0x602, 0);
    set16(b, 0x604, 6);
    let rc = call(VIOGETSTATE, &[st[0], st[1], 0]);
    check("VioGetState palette (brown = 0x14)", rc == 0 && w16(b, 0x606) == 0x14 && w16(b, 0x608) == 7);
    // DAC registers through VIOCOLORREG.
    set16(b, 0x600, 12);
    set16(b, 0x602, 3);
    set16(b, 0x604, 0x14);
    set16(b, 0x606, 1);
    let q = b.p(0x700);
    set16(b, 0x608, q[1]);
    set16(b, 0x60a, q[0]);
    let rc = call(VIOGETSTATE, &[st[0], st[1], 0]);
    check("VioGetState colour register", rc == 0 && b.bytes(0x700, 3) == [0x2a, 0x15, 0]);
    // Border colour.
    set16(b, 0x600, 6);
    set16(b, 0x602, 1);
    set16(b, 0x604, 0);
    check("VioSetState overscan", call(VIOSETSTATE, &[st[0], st[1], 0]) == 0);
    set16(b, 0x602, 7);
    check("VioGetState bad type", call(VIOGETSTATE, &[st[0], st[1], 0]) == 421);
    // ANSI off and on.
    let a = b.p(0x620);
    call(VIOSETANSI, &[0, 0]);
    call(VIOGETANSI, &[a[0], a[1], 0]);
    let off = w16(b, 0x620);
    call(VIOSETANSI, &[1, 0]);
    call(VIOGETANSI, &[a[0], a[1], 0]);
    check("VioSetAnsi off/on", off == 0 && w16(b, 0x620) == 1);
    // Screen lock.
    let l = b.p(0x630);
    if windowed() {
        check("VioScrLock in a window", call(VIOSCRLOCK, &[1, l[0], l[1], 0]) == ERROR_VIO_EXTENDED_SG);
        check("VioScrUnLock in a window", call(VIOSCRUNLOCK, &[0]) == ERROR_VIO_EXTENDED_SG);
        return;
    }
    let rc = call(VIOSCRLOCK, &[1, l[0], l[1], 0]);
    let got = b.bytes(0x630, 1)[0];
    check("VioScrLock", rc == 0 && got == 0 && call(VIOSCRUNLOCK, &[0]) == 0);
    check("VioScrUnLock not locked", call(VIOSCRUNLOCK, &[0]) == 367);
}

fn popup(b: &Buf) {
    let before = read_cells(b, 0, 0, 5).to_vec();
    let p = b.p(0x800);
    set16(b, 0x800, 0); // VP_NOWAIT, opaque
    let rc = call(VIOPOPUP, &[p[0], p[1], 0]);
    let x = b.p(0x810);
    b.bytes(0x810, 5).copy_from_slice(b"POPUP");
    call(VIOWRTCHARSTR, &[x[0], x[1], 5, 0, 0, 0]);
    let inside = read_cells(b, 0, 0, 5).to_vec();
    check("VioPopUp", rc == 0 && inside.iter().step_by(2).copied().eq(b"POPUP".iter().copied()));
    check("second VioPopUp fails", call(VIOPOPUP, &[p[0], p[1], 0]) == 406);
    let rc = call(VIOENDPOPUP, &[0]);
    check("VioEndPopUp restores the screen", rc == 0 && read_cells(b, 0, 0, 5) == &before[..]);
    check("VioEndPopUp without pop-up", call(VIOENDPOPUP, &[0]) == 405);
}

fn physbuf(b: &Buf) {
    // VIOPHYSBUF { pBuf = B8000, cb = 4000, asel[1] }
    set32(b, 0x900, 0xb8000);
    set32(b, 0x904, 4000);
    let p = b.p(0x900);
    let rc = call(VIOGETPHYSBUF, &[p[0], p[1], 0]);
    if windowed() {
        check("VioGetPhysBuf in a window", rc == ERROR_VIO_EXTENDED_SG);
        return;
    }
    let sel = w16(b, 0x908);
    if rc != 0 || sel == 0 {
        check("VioGetPhysBuf", false);
        return;
    }
    let flat = (((sel >> 3) as u32) << 16) as *mut u16;
    // Row 23, column 0: what the screen shows, then a new cell.
    let at = unsafe { flat.add(23 * 80) };
    let old = unsafe { core::ptr::read_volatile(at) };
    let shown = read_cells(b, 23, 0, 1);
    let same = shown.len() == 2 && u16::from_le_bytes([shown[0], shown[1]]) == old;
    unsafe { core::ptr::write_volatile(at, 0x1f5a) };
    unsafe { DosSleep(100) };
    let now = read_cells(b, 23, 0, 1).to_vec();
    check("VioGetPhysBuf shows the screen", same);
    check("physical buffer writes reach the screen", now == [0x5a, 0x1f]);
    unsafe { core::ptr::write_volatile(at, old) };
}

fn keyboard(b: &Buf) {
    // KBDINFO at 0xa00.
    let p = b.p(0xa00);
    set16(b, 0xa00, 10);
    let rc = call(KBDGETSTATUS, &[p[0], p[1], 0]);
    let mask = w16(b, 0xa02);
    check("KbdGetStatus ASCII echo", rc == 0 && mask & 0x0009 == 0x0009);
    set16(b, 0xa02, 0x0004 | 0x0002);
    let rc = call(KBDSETSTATUS, &[p[0], p[1], 0]);
    set16(b, 0xa02, 0);
    call(KBDGETSTATUS, &[p[0], p[1], 0]);
    check("KbdSetStatus binary", rc == 0 && w16(b, 0xa02) & 0x0006 == 0x0006);
    set16(b, 0xa02, 0x0004 | 0x0001);
    check("KbdSetStatus binary with echo fails", call(KBDSETSTATUS, &[p[0], p[1], 0]) == 377);
    set16(b, 0xa02, 0x0008 | 0x0001);
    call(KBDSETSTATUS, &[p[0], p[1], 0]);
    // No key waiting.
    let k = b.p(0xa20);
    let rc = call(KBDPEEK, &[k[0], k[1], 0]);
    check("KbdPeek nothing waiting", rc == 0 && b.bytes(0xa20, 3)[2] == 0);
    // ANSI device status report: the reply is typed into the buffer.
    let t = b.p(0xa40);
    b.bytes(0xa40, 4).copy_from_slice(b"\x1b[6n");
    call(VIOWRTTTY, &[t[0], t[1], 4, 0]);
    let (c1, _, _) = key_in(b, false);
    let (c2, _, _) = key_in(b, false);
    let mut rest = 0;
    while key_in(b, false).2 != 0 {
        rest += 1;
    }
    check("ANSI ESC[6n reports the cursor", c1 == 0x1b && c2 == b'[' && rest == 7);
    // Logical keyboards and focus.
    let h = b.p(0xa60);
    let rc = call(KBDOPEN, &[h[0], h[1]]);
    let hk = w16(b, 0xa60);
    let k = b.p(0xa20);
    let nofocus = call(KBDCHARIN, &[k[0], k[1], 1, hk]);
    let rc2 = call(KBDGETFOCUS, &[1, hk]);
    let focus = call(KBDCHARIN, &[k[0], k[1], 1, hk]);
    let rc3 = call(KBDFREEFOCUS, &[hk]);
    let rc4 = call(KBDCLOSE, &[hk]);
    check("KbdOpen", rc == 0 && hk != 0);
    check("KbdCharIn needs the focus", nofocus == 445 && rc2 == 0 && focus == 0);
    check("KbdFreeFocus, KbdClose", rc3 == 0 && rc4 == 0 && call(KBDCLOSE, &[hk]) == 439);
    // KbdXlate: scan code 0x1E is 'a', shifted 'A'.
    let x = b.bytes(0xa80, 18);
    x.fill(0);
    x[1] = 0x1e;
    let xp = b.p(0xa80);
    let rc = call(KBDXLATE, &[xp[0], xp[1], 0]);
    let lower = b.bytes(0xa80, 1)[0];
    let x = b.bytes(0xa80, 18);
    x.fill(0);
    x[1] = 0x1e;
    x[4] = 0x02; // left shift
    call(KBDXLATE, &[xp[0], xp[1], 0]);
    check("KbdXlate", rc == 0 && lower == b'a' && b.bytes(0xa80, 1)[0] == b'A' && w16(b, 0xa8c) == 1);
}

fn monitors(b: &Buf) {
    let h = b.p(0xb00);
    let rc = call(DOSMONOPEN, &{
        let n = b.p(0xb10);
        b.bytes(0xb10, 5).copy_from_slice(b"KBD$\0");
        [n[0], n[1], h[0], h[1]]
    });
    let hm = w16(b, 0xb00);
    check("DosMonOpen KBD$", rc == 0 && hm != 0);
    let bad = {
        let n = b.p(0xb10);
        b.bytes(0xb10, 6).copy_from_slice(b"FOO$$\0");
        call(DOSMONOPEN, &[n[0], n[1], h[0], h[1]])
    };
    check("DosMonOpen bad device", bad == 380);
    set16(b, 0xc00, 128);
    set16(b, 0xd00, 128);
    let (i, o) = (b.p(0xc00), b.p(0xd00));
    let rc = call(DOSMONREG, &[hm, i[0], i[1], o[0], o[1], 0, 0xffff]);
    check("DosMonReg", rc == 0);
    set16(b, 0xb20, 64);
    let (d, cb) = (b.p(0xb40), b.p(0xb20));
    let rc = call(DOSMONREAD, &[i[0], i[1], 1, d[0], d[1], cb[0], cb[1]]);
    check("DosMonRead no wait, empty", rc == 383);
    // A packet written by the monitor reaches the keyboard queue.
    let pk = b.bytes(0xb40, 14);
    pk.fill(0);
    pk[1] = 0x10;
    pk[2] = b'q';
    pk[3] = 0x10;
    pk[4] = 0x40;
    let rc = call(DOSMONWRITE, &[o[0], o[1], d[0], d[1], 14]);
    let (ch, scan, _) = key_in(b, false);
    check("DosMonWrite to the session", rc == 0 && ch == b'q' && scan == 0x10);
    check("DosMonClose", call(DOSMONCLOSE, &[hm]) == 0 && call(DOSMONCLOSE, &[hm]) == 381);
}

/// VioRegister with TESTDLL's 16-bit VIOREPLACE: it answers VioGetCurPos
/// itself and passes VioSetCurPos on to the base VIO.
fn subsystem(b: &Buf) {
    let (m, e) = (b.p(0xe00), b.p(0xe20));
    b.bytes(0xe00, 8).copy_from_slice(b"TESTDLL\0");
    b.bytes(0xe20, 11).copy_from_slice(b"VIOREPLACE\0");
    let mask: u32 = 1 | 1 << 5;
    let rc = call(VIOREGISTER, &[m[0], m[1], e[0], e[1], (mask >> 16) as u16, mask as u16, 0, 0]);
    check("VioRegister", rc == 0);
    let (r, c) = (b.p(0xe40), b.p(0xe42));
    set16(b, 0xe40, 0);
    set16(b, 0xe42, 0);
    let rc = call(VIOGETCURPOS, &[r[0], r[1], c[0], c[1], 0]);
    check("replacement VioGetCurPos", rc == 0 && w16(b, 0xe40) == 7 && w16(b, 0xe42) == 9);
    let rc = call(VIOSETCURPOS, &[3, 4, 0]);
    let rc2 = call(VIODEREGISTER, &[]);
    call(VIOGETCURPOS, &[r[0], r[1], c[0], c[1], 0]);
    check("passed on to the base VIO, VioDeRegister", rc == 0 && rc2 == 0 && w16(b, 0xe40) == 3 && w16(b, 0xe42) == 4);
}

fn open(name: &[u8]) -> Result<u32, u32> {
    let (mut h, mut act) = (0, 0);
    let rc = unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, null_mut()) };
    if rc == 0 { Ok(h) } else { Err(rc) }
}

fn devices(_b: &Buf) {
    // CLOCK$: days since 1980, minutes, hours, hundredths, seconds.
    match open(b"\\DEV\\CLOCK$\0") {
        Ok(h) => {
            let mut rec = [0u8; 6];
            let mut n = 0;
            let rc = unsafe { DosRead(h, rec.as_mut_ptr(), 6, &mut n) };
            let days = u16::from_le_bytes([rec[0], rec[1]]);
            check("CLOCK$ read", rc == 0 && n == 6 && days > 16000 && rec[2] < 60 && rec[3] < 24);
            unsafe { DosClose(h) };
        }
        Err(_) => check("CLOCK$ open", false),
    }
    // LPT1 is a sink; NUL too.
    match open(b"LPT1\0") {
        Ok(h) => {
            let mut n = 0;
            let rc = unsafe { DosWrite(h, b"x\r\n".as_ptr(), 3, &mut n) };
            let (mut st, mut cb) = (0u8, 1u32);
            let rc2 = unsafe { DosDevIOCtl(h, 5, 0x66, null_mut(), 0, null_mut(), &mut st, 1, &mut cb) };
            check("LPT1 write and status", rc == 0 && n == 3 && rc2 == 0 && st == 0x90);
            unsafe { DosClose(h) };
        }
        Err(_) => check("LPT1 open", false),
    }
    // KBD$: input mode and shift state.
    match open(b"\\DEV\\KBD$\0") {
        Ok(h) => {
            let (mut m, mut cb) = (0xffu8, 1u32);
            let rc = unsafe { DosDevIOCtl(h, 4, 0x71, null_mut(), 0, null_mut(), &mut m, 1, &mut cb) };
            let mut ss = [0u8; 3];
            let mut cb2 = 3;
            let rc2 = unsafe { DosDevIOCtl(h, 4, 0x73, null_mut(), 0, null_mut(), ss.as_mut_ptr(), 3, &mut cb2) };
            let mut rate = [250u16, 30];
            let mut cbp = 4;
            let rc3 = unsafe { DosDevIOCtl(h, 4, 0x54, rate.as_mut_ptr() as *mut u8, 4, &mut cbp, null_mut(), 0, null_mut()) };
            check("KBD$ IOCtls (input mode, shift state, typematic)", rc == 0 && m == 0 && rc2 == 0 && rc3 == 0);
            unsafe { DosClose(h) };
        }
        Err(_) => check("KBD$ open", false),
    }
    // COM2 when the machine has one (QEMU's second -serial).
    match open(b"COM2\0") {
        Ok(h) => {
            let mut baud = 9600u16;
            let mut cbp = 2;
            let rc = unsafe { DosDevIOCtl(h, 1, 0x41, &mut baud as *mut u16 as *mut u8, 2, &mut cbp, null_mut(), 0, null_mut()) };
            let mut lc = [8u8, 0, 0];
            let mut cbp = 3;
            let rc2 = unsafe { DosDevIOCtl(h, 1, 0x42, lc.as_mut_ptr(), 3, &mut cbp, null_mut(), 0, null_mut()) };
            let (mut got, mut cbd) = (0u16, 2u32);
            let rc3 = unsafe { DosDevIOCtl(h, 1, 0x61, null_mut(), 0, null_mut(), &mut got as *mut u16 as *mut u8, 2, &mut cbd) };
            check("COM2 baud rate and line control", rc == 0 && rc2 == 0 && rc3 == 0 && got == 9600);
            let mut n = 0;
            let msg = b"rs2 COM2 test\r\n";
            let rc = unsafe { DosWrite(h, msg.as_ptr(), msg.len() as u32, &mut n) };
            check("COM2 write", rc == 0 && n == msg.len() as u32);
            // Read what the other end sent within half a second.
            let mut dcb = [0u8; 11];
            let mut cbd = 11;
            unsafe { DosDevIOCtl(h, 1, 0x73, null_mut(), 0, null_mut(), dcb.as_mut_ptr(), 11, &mut cbd) };
            dcb[2] = 50;
            dcb[3] = 0;
            dcb[6] = (dcb[6] & !0x06) | 0x04; // wait for something
            let mut cbp = 11;
            unsafe { DosDevIOCtl(h, 1, 0x53, dcb.as_mut_ptr(), 11, &mut cbp, null_mut(), 0, null_mut()) };
            let mut buf = [0u8; 64];
            let mut n = 0;
            unsafe { DosRead(h, buf.as_mut_ptr(), 64, &mut n) };
            println!("COM2 received {} bytes: {:?}", n, core::str::from_utf8(&buf[..n as usize]).unwrap_or("?"));
            unsafe { DosClose(h) };
        }
        Err(rc) => println!("COM2 not present ({rc})"),
    }
    check("DosError disable/enable", unsafe { DosError(0) } == 0 && unsafe { DosError(1) } == 0 && unsafe { DosError(4) } == 87);
}

/// `apitest keys`: a keyboard monitor sees the next keystrokes (make and
/// break), passes them on, and the program reads them with KbdCharIn.
pub fn keys() {
    let mut mem = null_mut();
    if unsafe { DosAllocMem(&mut mem, 0x10000, PAG_READ | PAG_WRITE | PAG_COMMIT) } != 0 {
        return;
    }
    let b = Buf(mem);
    let (n, h) = (b.p(0x10), b.p(0));
    b.bytes(0x10, 5).copy_from_slice(b"KBD$\0");
    let rc = call(DOSMONOPEN, &[n[0], n[1], h[0], h[1]]);
    let hm = w16(&b, 0);
    set16(&b, 0x100, 128);
    set16(&b, 0x200, 128);
    let (i, o) = (b.p(0x100), b.p(0x200));
    let rc2 = call(DOSMONREG, &[hm, i[0], i[1], o[0], o[1], 1, 0xffff]);
    println!("monitor open {rc} reg {rc2}: press a key");
    // A make and its break.
    for _ in 0..2 {
        set16(&b, 0x300, 64);
        let (d, cb) = (b.p(0x400), b.p(0x300));
        let rc = call(DOSMONREAD, &[i[0], i[1], 0, d[0], d[1], cb[0], cb[1]]);
        let len = w16(&b, 0x300);
        let p = b.bytes(0x400, len as usize);
        if rc != 0 || len < 14 {
            println!("DosMonRead {rc}");
            break;
        }
        println!(
            "packet: raw {:02x} char {:02x} scan {:02x} status {:02x} shift {:04x} ddflags {:04x}",
            p[1],
            p[2],
            p[3],
            p[4],
            u16::from_le_bytes([p[6], p[7]]),
            u16::from_le_bytes([p[12], p[13]])
        );
        call(DOSMONWRITE, &[o[0], o[1], d[0], d[1], len]);
    }
    let (ch, scan, st) = key_in(&b, true);
    println!("KbdCharIn after the monitor: char {ch:02x} scan {scan:02x} status {st:02x}");
    call(DOSMONCLOSE, &[hm]);
    unsafe { DosFreeMem(mem) };
}
