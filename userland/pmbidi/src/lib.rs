//! PMBIDI.DLL: OS/2's bidirectional (Arabic, Hebrew) language support, as
//! far as programs on the OS/2 CD need it: the text converter (TEXTCON1)
//! uses its layout objects, the WarpCenter its presentation space
//! attributes, the language viewer (BDVIEWER) the window and keyboard
//! layer calls (PMMERGE's; these entries forward there, see
//! exports.extra). IBM's PMBIDI passes the work on to BDCALL32, the
//! bidirectional library of the bidirectional editions; rs2's code pages
//! are not bidirectional, so a layout object's transformation leaves the
//! text in its order and shape (as IBM's does for text without
//! bidirectional characters), and the attributes are kept for whoever
//! asks for them.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

mod exports;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// errno values the layout calls return.
const EINVAL: i32 = 22;
const E2BIG: i32 = 7;

struct State {
    process: BTreeMap<u32, u32>,
    ps: BTreeMap<u32, u32>,
}

static mut STATE: Option<State> = None;

fn state() -> &'static mut State {
    let s = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    s.get_or_insert_with(|| State { process: BTreeMap::new(), ps: BTreeMap::new() })
}

/// PMBIDI_Initialize(): nothing to set up.
#[unsafe(no_mangle)]
pub extern "C" fn PMBIDI_Initialize() -> u32 {
    1
}

/// Win32SetProcessLangInfo(ulEffect, ulData): the process's bidirectional
/// attributes (LI_BD_PRC_ATTR and friends).
#[unsafe(no_mangle)]
pub extern "C" fn Win32SetProcessLangInfo(effect: u32, data: u32) -> u32 {
    state().process.insert(effect, data);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn Win32QueryProcessLangInfo(effect: u32) -> u32 {
    state().process.get(&effect).copied().unwrap_or(0)
}

/// Gpi32SetBidiAttr(hps, ulBidiAttr): the presentation space's
/// bidirectional attributes; returns the previous ones.
#[unsafe(no_mangle)]
pub extern "C" fn Gpi32SetBidiAttr(hps: u32, attr: u32) -> u32 {
    state().ps.insert(hps, attr).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Gpi32QueryBidiAttr(hps: u32) -> u32 {
    state().ps.get(&hps).copied().unwrap_or(0)
}

/// PMBIDI_Get_StdDlgLang_Setting(), PMBIDI_Get_HelpBidi_Setting(): the
/// standard dialogs and help in the system's language (not
/// bidirectional).
#[unsafe(no_mangle)]
pub extern "C" fn PMBIDI_Get_StdDlgLang_Setting() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn PMBIDI_Get_HelpBidi_Setting() -> u32 {
    0
}

// ---- Layout objects (X/Open's portable layout services) ----

/// A layout object: the values set on it (LayoutId -> value).
struct Layout {
    values: BTreeMap<u32, u32>,
}

/// layout_object_create(pszLocaleName, pLayoutObject).
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_create(_locale: *const u8, plo: *mut u32) -> i32 {
    if plo.is_null() {
        return EINVAL;
    }
    let lo = Box::into_raw(Box::new(Layout { values: BTreeMap::new() }));
    unsafe { plo.write_unaligned(lo as u32) };
    0
}

/// layout_object_destroy(LayoutObject).
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_destroy(lo: u32) -> i32 {
    if lo == 0 {
        return EINVAL;
    }
    drop(unsafe { Box::from_raw(lo as *mut Layout) });
    0
}

/// The LayoutValueRecs (name, value) of a LayoutValues array up to name 0.
fn records(values: *const u32) -> Vec<(*const u32, u32, u32)> {
    let mut v = Vec::new();
    if values.is_null() {
        return v;
    }
    for i in 0..64 {
        let p = unsafe { values.add(2 * i) };
        let name = unsafe { p.read_unaligned() };
        if name == 0 {
            break;
        }
        v.push((p, name, unsafe { p.add(1).read_unaligned() }));
    }
    v
}

/// layout_object_setvalues(LayoutObject, LayoutValues, pIndexReturned).
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_setvalues(lo: u32, values: *const u32, index: *mut i32) -> i32 {
    if lo == 0 {
        return EINVAL;
    }
    let l = unsafe { &mut *(lo as *mut Layout) };
    for (_, name, value) in records(values) {
        l.values.insert(name, value);
    }
    if !index.is_null() {
        unsafe { index.write_unaligned(-1) };
    }
    0
}

/// layout_object_getvalues(LayoutObject, LayoutValues, pIndexReturned):
/// each record's value points where the value goes.
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_getvalues(lo: u32, values: *const u32, index: *mut i32) -> i32 {
    if lo == 0 {
        return EINVAL;
    }
    let l = unsafe { &*(lo as *const Layout) };
    for (_, name, out) in records(values) {
        if out != 0 {
            unsafe { (out as *mut u32).write_unaligned(l.values.get(&name).copied().unwrap_or(0)) };
        }
    }
    if !index.is_null() {
        unsafe { index.write_unaligned(-1) };
    }
    0
}

/// Copy `*pin` bytes (or up to the NUL when *pin is -1 or pin is null)
/// to `out` (*pout bytes there; with no buffer only the size needed is
/// returned), the identity in the index maps and level 0 for each.
fn copy_text(inp: *const u8, pin: *mut u32, out: *mut u8, pout: *mut u32, i2o: *mut u32, o2i: *mut u32, levels: *mut u8) -> i32 {
    if inp.is_null() || pout.is_null() {
        return EINVAL;
    }
    let n = if pin.is_null() || unsafe { pin.read_unaligned() } == u32::MAX {
        (0..).take_while(|&i| unsafe { *inp.add(i) } != 0).count()
    } else {
        unsafe { pin.read_unaligned() as usize }
    };
    let room = unsafe { pout.read_unaligned() } as usize;
    if out.is_null() || room == 0 {
        unsafe { pout.write_unaligned(n as u32) };
        return 0;
    }
    let m = n.min(room);
    unsafe {
        core::ptr::copy_nonoverlapping(inp, out, m);
        pout.write_unaligned(m as u32);
        if !pin.is_null() {
            pin.write_unaligned(m as u32);
        }
        for i in 0..m {
            if !i2o.is_null() {
                i2o.add(i).write_unaligned(i as u32);
            }
            if !o2i.is_null() {
                o2i.add(i).write_unaligned(i as u32);
            }
            if !levels.is_null() {
                *levels.add(i) = 0;
            }
        }
    }
    if m < n { E2BIG } else { 0 }
}

/// layout_object_transform(LayoutObject, InpBuf, InpSize, OutBuf,
/// OutSize, InpToOut, OutToInp, BidiLvl).
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_transform(lo: u32, inp: *const u8, pin: *mut u32, out: *mut u8, pout: *mut u32, i2o: *mut u32, o2i: *mut u32, levels: *mut u8) -> i32 {
    if lo == 0 {
        return EINVAL;
    }
    copy_text(inp, pin, out, pout, i2o, o2i, levels)
}

/// layout_object_editshape(LayoutObject, EditType, index, InpBuf,
/// InpSize, OutBuf, OutSize).
#[unsafe(no_mangle)]
pub extern "C" fn layout_object_editshape(lo: u32, _edit: u32, _index: *mut u32, inp: *const u8, pin: *mut u32, out: *mut u8, pout: *mut u32) -> i32 {
    if lo == 0 {
        return EINVAL;
    }
    copy_text(inp, pin, out, pout, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
}
