//! AVIO: a VIO presentation space associated with a window's device
//! context, written with the VIO calls, through its buffer and with
//! VioWrtTTY, and drawn in the window.

use core::ptr::null;

use os2::vio::{call16, ptr_words};

use crate::pm::*;
use crate::{check, pump};

unsafe extern "C" {
    #[link_name = "PMWIN.794"]
    fn WinOpenWindowDC(h: u32) -> u32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const i32) -> i32;
    #[link_name = "VIOCALLS.55"]
    fn VIO16ASSOCIATE();
    #[link_name = "VIOCALLS.56"]
    fn VIO16CREATEPS();
    #[link_name = "VIOCALLS.58"]
    fn VIO16GETDEVICECELLSIZE();
    #[link_name = "VIOCALLS.59"]
    fn VIO16GETORG();
    #[link_name = "VIOCALLS.61"]
    fn VIO16DESTROYPS();
    #[link_name = "VIOCALLS.63"]
    fn VIO16SETORG();
    #[link_name = "VIOCALLS.65"]
    fn VIO16SETDEVICECELLSIZE();
    #[link_name = "VIOCALLS.66"]
    fn VIO16SHOWPS();
    #[link_name = "VIOCALLS.15"]
    fn VIO16SETCURPOS();
    #[link_name = "VIOCALLS.19"]
    fn VIO16WRTTTY();
    #[link_name = "VIOCALLS.24"]
    fn VIO16READCELLSTR();
    #[link_name = "VIOCALLS.26"]
    fn VIO16WRTNATTR();
    #[link_name = "VIOCALLS.31"]
    fn VIO16GETBUF();
    #[link_name = "VIOCALLS.43"]
    fn VIO16SHOWBUF();
    #[link_name = "VIOCALLS.48"]
    fn VIO16WRTCHARSTRATT();
    #[link_name = "VIOCALLS.52"]
    fn VIO16WRTNCELL();
}

const CLR_BLUE: i32 = 1;
const CLR_RED: i32 = 2;
const ROWS: u16 = 4;
const COLS: u16 = 20;

/// Memory the 16-bit calls see: never across a 64 KB tile.
#[repr(C, align(64))]
struct Buf([u8; 64]);

fn call(f: unsafe extern "C" fn(), words: &[u16]) -> u16 {
    unsafe { call16(f as *const () as usize, words) }
}

fn p(b: &Buf, at: usize) -> [u16; 2] {
    ptr_words(b.0[at..].as_ptr())
}

/// The colour at the middle of a cell of the window.
fn pel(h: u32, row: i32, col: i32) -> i32 {
    let hps = unsafe { WinGetPS(h) };
    let pt = [col * 8 + 4, ROWS as i32 * 16 - row * 16 - 8];
    let v = unsafe { GpiQueryPel(hps, pt.as_ptr()) };
    unsafe { WinReleasePS(hps) };
    v
}

pub fn avio_checks(hab: u32, client: u32) {
    let mut b = Buf([0; 64]);
    let h = {
        let w = p(&b, 0);
        let rc = call(VIO16CREATEPS, &[w[0], w[1], ROWS, COLS, 0, 1, 0]);
        let hvps = u16::from_le_bytes([b.0[0], b.0[1]]);
        check("VioCreatePS", rc == 0 && hvps != 0);
        hvps
    };
    let win = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, 220, 10, COLS as i32 * 8, ROWS as i32 * 16, client, 3, 80, null(), null()) };
    let hdc = unsafe { WinOpenWindowDC(win) };
    let rc = call(VIO16ASSOCIATE, &[(hdc >> 16) as u16, hdc as u16, h]);
    check("VioAssociate with the window's DC", hdc != 0 && rc == 0);
    unsafe { WinUpdateWindow(win) };

    // Every cell blank on bright red, shown.
    b.0[0..2].copy_from_slice(&[b' ', 0xc0]);
    let w = p(&b, 0);
    let rc = call(VIO16WRTNCELL, &[w[0], w[1], ROWS * COLS, 0, 0, h]);
    call(VIO16SHOWPS, &[ROWS, COLS, 0, h]);
    pump(hab);
    check("VioWrtNCell on the PS, drawn in the window", rc == 0 && pel(win, 0, 0) == CLR_RED && pel(win, 3, 19) == CLR_RED);

    // An attribute: bright blue background.
    b.0[0] = 0x90;
    let w = p(&b, 0);
    call(VIO16WRTNATTR, &[w[0], w[1], 1, 3, 19, h]);
    pump(hab);
    check("VioWrtNAttr redraws the cell", pel(win, 3, 19) == CLR_BLUE && pel(win, 3, 18) == CLR_RED);

    // Characters with an attribute, read back.
    b.0[0..2].copy_from_slice(b"AB");
    b.0[2] = 0x9f;
    let (s, a) = (p(&b, 0), p(&b, 2));
    call(VIO16WRTCHARSTRATT, &[s[0], s[1], 2, 1, 2, a[0], a[1], h]);
    b.0[8..10].copy_from_slice(&4u16.to_le_bytes());
    let (out, cb) = (p(&b, 16), p(&b, 8));
    let rc = call(VIO16READCELLSTR, &[out[0], out[1], cb[0], cb[1], 1, 2, h]);
    check("VioWrtCharStrAtt / VioReadCellStr on the PS", rc == 0 && b.0[16..20] == [b'A', 0x9f, b'B', 0x9f]);

    // Through the buffer: VioGetBuf, a store, VioShowBuf.
    let (pb, pc) = (p(&b, 24), p(&b, 28));
    let rc = call(VIO16GETBUF, &[pb[0], pb[1], pc[0], pc[1], h]);
    let far = u32::from_le_bytes([b.0[24], b.0[25], b.0[26], b.0[27]]);
    let len = u16::from_le_bytes([b.0[28], b.0[29]]);
    let flat = ((far >> 19) << 16) | (far & 0xffff);
    let off = (2 * COLS as u32) * 2;
    unsafe { core::ptr::write_volatile((flat + off) as *mut [u8; 2], [b' ', 0x90]) };
    call(VIO16SHOWBUF, &[off as u16, 2, h]);
    pump(hab);
    check("VioGetBuf / VioShowBuf on the PS", rc == 0 && len == ROWS * COLS * 2 && pel(win, 2, 0) == CLR_BLUE);

    // VioWrtTTY at the last line scrolls the PS up.
    call(VIO16SETCURPOS, &[3, 0, h]);
    b.0[0..3].copy_from_slice(b"x\r\n");
    let s = p(&b, 0);
    let rc = call(VIO16WRTTTY, &[s[0], s[1], 3, h]);
    b.0[8..10].copy_from_slice(&4u16.to_le_bytes());
    let (out, cb) = (p(&b, 16), p(&b, 8));
    call(VIO16READCELLSTR, &[out[0], out[1], cb[0], cb[1], 0, 2, h]);
    pump(hab);
    check("VioWrtTTY scrolls the PS", rc == 0 && b.0[16..20] == [b'A', 0x9f, b'B', 0x9f] && pel(win, 1, 0) == CLR_BLUE);

    // Origin and cell size.
    call(VIO16SETORG, &[1, 0, h]);
    let (r, c) = (p(&b, 32), p(&b, 34));
    let rc = call(VIO16GETORG, &[r[0], r[1], c[0], c[1], h]);
    pump(hab);
    check("VioSetOrg / VioGetOrg (the origin's row at the top)", rc == 0 && b.0[32..36] == [1, 0, 0, 0] && pel(win, 0, 0) == CLR_BLUE);
    let (ch, cw) = (p(&b, 40), p(&b, 42));
    let rc = call(VIO16GETDEVICECELLSIZE, &[ch[0], ch[1], cw[0], cw[1], h]);
    let bad = call(VIO16SETDEVICECELLSIZE, &[10, 8, h]);
    let good = call(VIO16SETDEVICECELLSIZE, &[8, 8, h]);
    check("VioGetDeviceCellSize / VioSetDeviceCellSize", rc == 0 && b.0[40..44] == [16, 0, 8, 0] && bad != 0 && good == 0);

    let rc = call(VIO16ASSOCIATE, &[0, 0, h]);
    let d = call(VIO16DESTROYPS, &[h]);
    let again = call(VIO16DESTROYPS, &[h]);
    check("VioAssociate(0) / VioDestroyPS", rc == 0 && d == 0 && again != 0);
    unsafe { WinDestroyWindow(win) };
}
