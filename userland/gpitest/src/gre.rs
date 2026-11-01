//! The graphics engine called directly (PMGRE.DLL's Gre32EntryN, as
//! COMETRUN does): bit-block transfers between DCs in device
//! coordinates, screen bits saved and restored, the DC queries and
//! handles, and IBM's dispatcher errors.

use crate::{check, free, mem, pel, pt, rect, PointL, RectL, PU_PELS, ROP_ONE, ROP_SRCCOPY, ROP_ZERO};

unsafe extern "C" {
    #[link_name = "PMGRE.62"]
    fn Gre32Entry2(a: u32, f: u32) -> u32;
    #[link_name = "PMGRE.64"]
    fn Gre32Entry4(a: u32, b: u32, c: u32, f: u32) -> u32;
    #[link_name = "PMGRE.65"]
    fn Gre32Entry5(a: u32, b: u32, c: u32, d: u32, f: u32) -> u32;
    #[link_name = "PMGRE.66"]
    fn Gre32Entry6(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32) -> u32;
    #[link_name = "PMGRE.69"]
    fn Gre32Entry9(a: u32, b: u32, c: u32, d: u32, e: u32, g: u32, h: u32, i: u32, f: u32) -> u32;
    #[link_name = "PMGRE.37"]
    fn GetDriverInfo32(h: u32, ty: u32, hdc: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
}

const NGRE_QUERY_ENGINE_VERSION: u32 = 0x020a;
const NGRE_BITBLT: u32 = 0x6026;
const NGRE_GET_PEL: u32 = 0x6027;
const NGRE_SAVE_SCREEN_BITS: u32 = 0x402c;
const NGRE_RESTORE_SCREEN_BITS: u32 = 0x402d;
const NGRE_GET_DC_ORIGIN: u32 = 0x40ba;
const NGRE_QUERY_DEVICE_CAPS: u32 = 0x40d1;
const NGRE_GET_HANDLE: u32 = 0x4203;
const NGRE_SET_HANDLE: u32 = 0x4204;
const NGRE_SET_DC_OWNER: u32 = 0x420b;
const RSB_RESTORE: u32 = 1;
const RSB_FREE: u32 = 2;
const OD_DIRECT: i32 = 5;

fn blt(dst: u32, src: u32, p: &[PointL], rop: i32) -> u32 {
    unsafe { Gre32Entry9(dst, src, p.len() as u32, p.as_ptr() as u32, rop as u32, 0, 0, 0, NGRE_BITBLT) }
}

pub fn gre_checks(hab: u32) {
    unsafe {
        check("GreQueryEngineVersion", Gre32Entry2(0, NGRE_QUERY_ENGINE_VERSION) == 0x23f);

        // Blits between memory DCs, in device coordinates (top right
        // exclusive), the COM_ flags left 0 as the Toolkit's macros do.
        let a = mem(hab, 16, 16, PU_PELS);
        let b = mem(hab, 16, 16, PU_PELS);
        let all = [pt(0, 0), pt(16, 16)];
        check("GreBitblt without a source (ROP_ZERO)", blt(a.hdc, 0, &all, ROP_ZERO) == 1 && pel(&a, 15, 15) == 0);
        check("GreBitblt ROP_ONE, top right exclusive", blt(a.hdc, 0, &[pt(2, 2), pt(6, 6)], ROP_ONE) == 1 && pel(&a, 5, 5) == 0xffffff && pel(&a, 6, 6) == 0 && pel(&a, 1, 1) == 0);
        blt(b.hdc, 0, &all, ROP_ZERO);
        check("GreBitblt from another DC (ROP_SRCCOPY)", blt(b.hdc, a.hdc, &[pt(8, 8), pt(12, 12), pt(2, 2)], ROP_SRCCOPY) == 1 && pel(&b, 8, 8) == 0xffffff && pel(&b, 11, 11) == 0xffffff && pel(&b, 12, 12) == 0);
        // A PS of its own (`hdc | 1`) is the DC.
        check("GreBitblt to hdc | 1", blt(b.hdc | 1, 0, &[pt(0, 0), pt(1, 1)], ROP_ONE) == 1 && pel(&b, 0, 0) == 0xffffff);

        // Screen bits: saved, overwritten, put back and forgotten.
        let r = rect(2, 2, 6, 6);
        let hsb = Gre32Entry4(a.hdc, &r as *const RectL as u32, 0, NGRE_SAVE_SCREEN_BITS);
        check("GreSaveScreenBits", hsb != 0);
        blt(a.hdc, 0, &all, ROP_ZERO);
        let ok = Gre32Entry6(a.hdc, hsb, &r as *const RectL as u32, RSB_RESTORE, 0, NGRE_RESTORE_SCREEN_BITS);
        check("GreRestoreScreenBits RSB_RESTORE", ok == 1 && pel(&a, 2, 2) == 0xffffff && pel(&a, 5, 5) == 0xffffff && pel(&a, 6, 6) == 0);
        // Again, only inside a smaller rectangle, then freed.
        blt(a.hdc, 0, &all, ROP_ZERO);
        let part = rect(4, 4, 6, 6);
        let ok = Gre32Entry6(a.hdc, hsb, &part as *const RectL as u32, RSB_RESTORE | RSB_FREE, 0, NGRE_RESTORE_SCREEN_BITS);
        check("GreRestoreScreenBits inside a rectangle", ok == 1 && pel(&a, 4, 4) == 0xffffff && pel(&a, 3, 3) == 0);
        check("GreRestoreScreenBits after RSB_FREE fails", Gre32Entry6(a.hdc, hsb, &r as *const RectL as u32, RSB_RESTORE, 0, NGRE_RESTORE_SCREEN_BITS) == 0);

        // The DC's queries and handles.
        let mut o = pt(-1, -1);
        check("GreGetDCOrigin of a memory DC", Gre32Entry4(a.hdc, &mut o as *mut PointL as u32, 0, NGRE_GET_DC_ORIGIN) == 1 && o == pt(0, 0));
        let mut caps = [0i32; 2];
        check("GreQueryDeviceCaps (CAPS_HEIGHT, CAPS_WIDTH)", Gre32Entry6(a.hdc, 4, caps.as_mut_ptr() as u32, 2, 0, NGRE_QUERY_DEVICE_CAPS) == 1 && caps[0] > 0 && caps[1] > 0);
        check("GreSetHandle/GreGetHandle", Gre32Entry5(a.hdc, 0x1234, 2, 0, NGRE_SET_HANDLE) == 1 && Gre32Entry4(a.hdc, 2, 0, NGRE_GET_HANDLE) == 0x1234 && Gre32Entry4(b.hdc, 2, 0, NGRE_GET_HANDLE) == 0);
        check("GreSetDCOwner", Gre32Entry4(a.hdc, u32::MAX, 0, NGRE_SET_DC_OWNER) == 1);
        check("GetDriverInfo32: no driver data", GetDriverInfo32(a.hdc, 0, a.hdc) == u32::MAX);

        // IBM's dispatcher: a bad DC is PMERR_INV_HDC with the function's
        // default value (-1 for GreGetPel, 0 for most); a function no
        // device has is PMERR_DEV_FUNC_NOT_INSTALLED.
        WinGetLastError(hab);
        let v = Gre32Entry4(0x7777_0000, &o as *const PointL as u32, 0, NGRE_GET_DC_ORIGIN);
        check("bad DC: 0 and PMERR_INV_HDC", v == 0 && WinGetLastError(hab) & 0xffff == 0x207c);
        check("bad DC: GreGetPel's default -1", Gre32Entry4(0x7777_0000, &o as *const PointL as u32, 0, NGRE_GET_PEL) == u32::MAX);
        WinGetLastError(hab);
        check("unknown function: 0 and PMERR_DEV_FUNC_NOT_INSTALLED", Gre32Entry4(a.hdc, 0, 0, 0x40ff) == 0 && WinGetLastError(hab) & 0xffff == 0x2023);
        free(a);
        free(b);

        // The display opened directly (COMETRUN's DC): screen bits save
        // and restore there too.
        let names: [*const u8; 2] = [b"DISPLAY\0".as_ptr(), b"DISPLAY\0".as_ptr()];
        let hdc = DevOpenDC(hab, OD_DIRECT, b"*\0".as_ptr(), 2, names.as_ptr() as *const u32, 0);
        check("DevOpenDC OD_DIRECT DISPLAY", hdc != 0);
        let r = rect(0, 0, 8, 8);
        let hsb = Gre32Entry4(hdc, &r as *const RectL as u32, 0, NGRE_SAVE_SCREEN_BITS);
        check("GreSaveScreenBits on the screen", hsb != 0 && Gre32Entry6(hdc, hsb, &r as *const RectL as u32, RSB_RESTORE | RSB_FREE, 0, NGRE_RESTORE_SCREEN_BITS) == 1);
        DevCloseDC(hdc);
    }
}
