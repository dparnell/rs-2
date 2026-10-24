//! PMBIDI: a layout object's transformation (text without bidirectional
//! characters stays as it is), the language information calls (PMMERGE's,
//! through PMBIDI's entries too), a presentation space's bidirectional
//! attributes.

use crate::check;

unsafe extern "C" {
    #[link_name = "PMBIDI.306"]
    fn layout_object_create(locale: *const u8, plo: *mut u32) -> i32;
    #[link_name = "PMBIDI.307"]
    fn layout_object_destroy(lo: u32) -> i32;
    #[link_name = "PMBIDI.308"]
    fn layout_object_transform(lo: u32, inp: *const u8, pin: *mut u32, out: *mut u8, pout: *mut u32, i2o: *mut u32, o2i: *mut u32, lvl: *mut u8) -> i32;
    #[link_name = "PMBIDI.310"]
    fn layout_object_setvalues(lo: u32, values: *const u32, index: *mut i32) -> i32;
    #[link_name = "PMBIDI.311"]
    fn layout_object_getvalues(lo: u32, values: *const u32, index: *mut i32) -> i32;
    #[link_name = "PMBIDI.20"]
    fn Win32SetLangInfo(h: u32, effect: u32, data: u32, mask: u32, fl: u32) -> u32;
    #[link_name = "PMMERGE.3501"]
    fn WinQueryLangInfo(h: u32, effect: u32, fl: u32) -> u32;
    #[link_name = "PMMERGE.3502"]
    fn WinSetKbdLayer(h: u32, layer: u32, fl: u32) -> u32;
    #[link_name = "PMBIDI.23"]
    fn Win32QueryKbdLayer(h: u32) -> u32;
    #[link_name = "PMBIDI.50"]
    fn Gpi32SetBidiAttr(hps: u32, attr: u32) -> u32;
    #[link_name = "PMBIDI.51"]
    fn Gpi32QueryBidiAttr(hps: u32) -> u32;
}

pub fn bidi_checks(client: u32) {
    unsafe {
        let mut lo = 0;
        let ok = layout_object_create(b"\0".as_ptr(), &mut lo) == 0 && lo != 0;
        let vals: [u32; 4] = [0x0101, 7, 0, 0];
        let set = layout_object_setvalues(lo, vals.as_ptr(), core::ptr::null_mut()) == 0;
        let mut got = 0u32;
        let q: [u32; 4] = [0x0101, &mut got as *mut u32 as u32, 0, 0];
        let get = layout_object_getvalues(lo, q.as_ptr(), core::ptr::null_mut()) == 0 && got == 7;
        check("layout_object_create, setvalues, getvalues", ok && set && get);
        let text = b"abc def";
        let mut n = text.len() as u32;
        let mut need = 0u32;
        layout_object_transform(lo, text.as_ptr(), &mut n, core::ptr::null_mut(), &mut need, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        let mut out = [0u8; 16];
        let mut m = out.len() as u32;
        let mut i2o = [0u32; 16];
        let rc = layout_object_transform(lo, text.as_ptr(), &mut n, out.as_mut_ptr(), &mut m, i2o.as_mut_ptr(), core::ptr::null_mut(), core::ptr::null_mut());
        check("layout_object_transform: the size needed, the text and its map", need == 7 && rc == 0 && m == 7 && &out[..7] == text && i2o[6] == 6);
        check("layout_object_destroy", layout_object_destroy(lo) == 0);
        // LI_BD_WND_ATTR (0x20) kept for the window.
        let ok = Win32SetLangInfo(client, 0x20, 0x31, 0xff, 1) != 0 && WinQueryLangInfo(client, 0x20, 1) == 0x31;
        check("WinSetLangInfo/WinQueryLangInfo (PMMERGE and PMBIDI)", ok);
        let ok = WinSetKbdLayer(client, 1, 0) != 0 && Win32QueryKbdLayer(client) == 1;
        check("WinSetKbdLayer/WinQueryKbdLayer", ok);
        let hps = crate::pm::WinGetPS(client);
        let ok = Gpi32SetBidiAttr(hps, 0x1234) == 0 && Gpi32QueryBidiAttr(hps) == 0x1234;
        crate::pm::WinReleasePS(hps);
        check("Gpi32SetBidiAttr/Gpi32QueryBidiAttr", ok);
    }
}
