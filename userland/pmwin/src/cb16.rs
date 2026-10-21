//! 16-bit procedures and PMWIN: window procedures and hooks of 16-bit
//! programs (16:16 `_pascal` functions) that 32-bit PMWIN calls, 32-bit
//! window procedures that 16-bit programs call, and the conversion of
//! message parameters between the two (pointers are 16:16 in one and flat
//! in the other, and some structures have 16-bit fields in OS/2 1.x).
//!
//! Inside PMWIN every procedure is a 32-bit address. A 16-bit procedure
//! handed to PMWIN becomes one of the 32-bit stubs below (`wrap32`),
//! which converts the call; a 32-bit procedure handed to a 16-bit program
//! (the old procedure of a subclassed window) becomes one of the 16-bit
//! stubs of win16.rs (`wrap16`). Each turns back into the original when
//! it goes the other way. As IBM's PMMERGE does (its message thunks are
//! table driven by message number), the canonical form of the parameters
//! is the 32-bit one: messages from 16-bit code are converted when they
//! come in (`MsgIn`) and again on the way to a 16-bit procedure
//! (`MsgOut`).

use core::sync::atomic::{AtomicU32, Ordering};

use thunk16::{alloc16, far, flat, set_sp16, sp16, PascalCall};

/// What a 32-bit stub calls.
pub const WNDPROC: u32 = 1;
/// HK_HELP: (hab, usMode, idTopic, idSubTopic, prcPosition) -> BOOL.
pub const HELPHOOK: u32 = 2;
/// Other hooks (recorded, not called by PMWIN).
pub const HOOK: u32 = 3;

const N32: usize = 256;
/// Per process (DLL instance data): 16:16 target and kind of each stub.
static CB_FAR: [AtomicU32; N32] = [const { AtomicU32::new(0) }; N32];
static CB_KIND: [AtomicU32; N32] = [const { AtomicU32::new(0) }; N32];

// The 32-bit stubs: `push i; jmp common`, 16 bytes apart. The common code
// calls cb_dispatch(i, &args) (cdecl: the caller removes the arguments).
core::arch::global_asm!(
    ".pushsection .text.w16cb,\"ax\"",
    ".p2align 4",
    ".globl w16_cb_stubs",
    "w16_cb_stubs:",
    ".set .Lw16cb_i, 0",
    ".rept 256",
    ".byte 0x68",
    ".long .Lw16cb_i",
    ".byte 0xe9",
    ".long .Lw16cb_common - . - 4",
    ".p2align 4",
    ".set .Lw16cb_i, .Lw16cb_i + 1",
    ".endr",
    ".Lw16cb_common:",
    "pop eax",
    "lea ecx, [esp + 4]",
    "push ebp",
    "mov ebp, esp",
    "and esp, -16",
    "sub esp, 8",
    "push ecx",
    "push eax",
    "call {dispatch}",
    "mov esp, ebp",
    "pop ebp",
    "ret",
    ".popsection",
    dispatch = sym cb_dispatch,
);

unsafe extern "C" {
    static w16_cb_stubs: u8;
}

fn stubs() -> u32 {
    unsafe { &raw const w16_cb_stubs as u32 }
}

/// The stub index of a 32-bit address that is one of the stubs.
fn stub_index(p: u32) -> Option<usize> {
    let b = stubs();
    (p >= b && p < b + 16 * N32 as u32 && (p - b) % 16 == 0).then(|| ((p - b) / 16) as usize)
}

/// Is `p` a 32-bit stub of a 16-bit procedure (application code, called
/// without the PM lock)?
pub fn is_stub(p: u32) -> bool {
    let b = stubs();
    p >= b && p < b + 16 * N32 as u32
}

/// The 16-bit procedure behind a stub (0 if `p` is not one).
pub fn target16(p: u32) -> u32 {
    stub_index(p).map_or(0, |i| CB_FAR[i].load(Ordering::Relaxed))
}

/// A 32-bit procedure for the 16:16 procedure `far16` (0 stays 0; one of
/// win16.rs's 16-bit stubs of a 32-bit procedure gives that procedure).
pub fn wrap32(far16: u32, kind: u32) -> u32 {
    if far16 == 0 {
        return 0;
    }
    if let Some(p) = crate::win16::proc32_of(far16) {
        return p;
    }
    for i in 0..N32 {
        if CB_FAR[i].load(Ordering::Relaxed) == far16 && CB_KIND[i].load(Ordering::Relaxed) == kind {
            return stubs() + 16 * i as u32;
        }
    }
    for i in 0..N32 {
        if CB_FAR[i].compare_exchange(0, far16, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
            CB_KIND[i].store(kind, Ordering::Release);
            return stubs() + 16 * i as u32;
        }
    }
    0
}

/// A 16:16 procedure for 16-bit code from a 32-bit window procedure.
pub fn wrap16(p32: u32) -> u32 {
    if p32 == 0 {
        return 0;
    }
    match stub_index(p32) {
        Some(i) => CB_FAR[i].load(Ordering::Relaxed),
        None => crate::win16::proc16_for(p32),
    }
}

extern "C" fn cb_dispatch(i: u32, a: *const u32) -> u32 {
    let i = i as usize % N32;
    let target = CB_FAR[i].load(Ordering::Acquire);
    let arg = |k: usize| unsafe { a.add(k).read() };
    match CB_KIND[i].load(Ordering::Relaxed) {
        WNDPROC => wndproc16(target, arg(0), arg(1), arg(2), arg(3)),
        HELPHOOK => {
            let mark = sp16();
            let rect = arg(4) as *const u8;
            let r16 = if rect.is_null() { 0 } else { copy16(rect, 16) };
            let r = PascalCall::new()
                .l(arg(0))
                .w(arg(1) as u16)
                .w(arg(2) as u16)
                .w(arg(3) as u16)
                .l(r16)
                .call(target);
            set_sp16(mark);
            r & 0xffff
        }
        _ => 0,
    }
}

/// Call a 16-bit window procedure.
pub fn wndproc16(target: u32, h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let mark = sp16();
    let mut out = MsgOut::new(msg, mp1, mp2);
    let (m1, m2) = out.params();
    let r = PascalCall::new().l(h).w(msg as u16).l(m1).l(m2).call(target);
    out.done();
    set_sp16(mark);
    r
}

/// Copy `n` bytes to the 16-bit stack: their 16:16 address.
fn copy16(p: *const u8, n: usize) -> u32 {
    let f = alloc16(n);
    unsafe { core::ptr::copy_nonoverlapping(p, flat(f), n) };
    f
}

fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}
fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}
fn wr16(p: *mut u8, o: usize, v: u16) {
    unsafe { (p.add(o) as *mut u16).write_unaligned(v) }
}
fn wr32(p: *mut u8, o: usize, v: u32) {
    unsafe { (p.add(o) as *mut u32).write_unaligned(v) }
}

/// A pointer parameter from 16-bit code: flat (a value that is not a
/// tiled 16:16 pointer stays as it is).
pub fn ptr_in(v: u32) -> u32 {
    if v == 0 || (v >> 16) & 7 != 7 { v } else { flat(v) as u32 }
}

/// A pointer parameter for 16-bit code.
pub fn ptr_out(v: u32) -> u32 {
    if v == 0 || v >= 0x2000_0000 { v } else { far(v as *const u8) }
}

// ---- Structures ----

/// SWP: 16-bit (fs, cy, cx, y, x as 16-bit, hwndInsertBehind, hwnd; 18
/// bytes) and 32-bit (fl, cy, cx, y, x as 32-bit, two windows, two
/// reserved; 36 bytes).
pub const SWP16: usize = 18;
pub const SWP32: usize = 36;

pub fn swp_in(s16: *const u8, s32: *mut u8) {
    wr32(s32, 0, rd16(s16, 0) as u32);
    for k in 0..4 {
        wr32(s32, 4 + 4 * k, rd16(s16, 2 + 2 * k) as i16 as i32 as u32);
    }
    wr32(s32, 20, rd32(s16, 10));
    wr32(s32, 24, rd32(s16, 14));
    wr32(s32, 28, 0);
    wr32(s32, 32, 0);
}

pub fn swp_out(s32: *const u8, s16: *mut u8) {
    wr16(s16, 0, rd32(s32, 0) as u16);
    for k in 0..4 {
        wr16(s16, 2 + 2 * k, rd32(s32, 4 + 4 * k) as u16);
    }
    wr32(s16, 10, rd32(s32, 20));
    wr32(s16, 14, rd32(s32, 24));
}

/// QMSG: 16-bit (hwnd, msg as 16-bit, mp1, mp2, time, ptl; 26 bytes) and
/// 32-bit (32 bytes, msg 32-bit and a reserved field at the end).
pub const QMSG16: usize = 26;

pub fn qmsg_in(q16: *const u8, q32: *mut u8) {
    wr32(q32, 0, rd32(q16, 0));
    wr32(q32, 4, rd16(q16, 4) as u32);
    for k in 0..5 {
        wr32(q32, 8 + 4 * k, rd32(q16, 6 + 4 * k));
    }
    wr32(q32, 28, 0);
}

pub fn qmsg_out(q32: *const u8, q16: *mut u8) {
    wr32(q16, 0, rd32(q32, 0));
    wr16(q16, 4, rd32(q32, 4) as u16);
    for k in 0..5 {
        wr32(q16, 6 + 4 * k, rd32(q32, 8 + 4 * k));
    }
}

/// CREATESTRUCT: pPresParams, pCtlData, id, hwndInsertBehind, hwndOwner,
/// cy, cx, y, x, flStyle, pszText, pszClass, hwndParent; id and the
/// coordinates are 16-bit in OS/2 1.x (42 bytes against 52).
const CREATE16: usize = 42;
const CREATE32: usize = 52;

fn create_in(c16: *const u8, c32: *mut u8) {
    wr32(c32, 0, ptr_in(rd32(c16, 0)));
    wr32(c32, 4, ptr_in(rd32(c16, 4)));
    wr32(c32, 8, rd16(c16, 8) as u32);
    wr32(c32, 12, rd32(c16, 10));
    wr32(c32, 16, rd32(c16, 14));
    for k in 0..4 {
        wr32(c32, 20 + 4 * k, rd16(c16, 18 + 2 * k) as i16 as i32 as u32);
    }
    wr32(c32, 36, rd32(c16, 26));
    wr32(c32, 40, ptr_in(rd32(c16, 30)));
    wr32(c32, 44, ptr_in(rd32(c16, 34)));
    wr32(c32, 48, rd32(c16, 38));
}

fn create_out(c32: *const u8, c16: *mut u8) {
    wr32(c16, 0, ptr_out(rd32(c32, 0)));
    wr32(c16, 4, ptr_out(rd32(c32, 4)));
    wr16(c16, 8, rd32(c32, 8) as u16);
    wr32(c16, 10, rd32(c32, 12));
    wr32(c16, 14, rd32(c32, 16));
    for k in 0..4 {
        wr16(c16, 18 + 2 * k, rd32(c32, 20 + 4 * k) as u16);
    }
    wr32(c16, 26, rd32(c32, 36));
    wr32(c16, 30, ptr_out(rd32(c32, 40)));
    wr32(c16, 34, ptr_out(rd32(c32, 44)));
    wr32(c16, 38, rd32(c32, 48));
}

/// WNDPARAMS: fsStatus, cchText, pszText, cbPresParams, pPresParams,
/// cbCtlData, pCtlData; the counts are 16-bit in OS/2 1.x (20 bytes
/// against 28).
const WP16: usize = 20;
const WP32: usize = 28;

fn wndparams_in(w16: *const u8, w32: *mut u8) {
    wr32(w32, 0, rd16(w16, 0) as u32);
    wr32(w32, 4, rd16(w16, 2) as u32);
    wr32(w32, 8, ptr_in(rd32(w16, 4)));
    wr32(w32, 12, rd16(w16, 8) as u32);
    wr32(w32, 16, ptr_in(rd32(w16, 10)));
    wr32(w32, 20, rd16(w16, 14) as u32);
    wr32(w32, 24, ptr_in(rd32(w16, 16)));
}

fn wndparams_out(w32: *const u8, w16: *mut u8) {
    wr16(w16, 0, rd32(w32, 0) as u16);
    wr16(w16, 2, rd32(w32, 4) as u16);
    wr32(w16, 4, ptr_out(rd32(w32, 8)));
    wr16(w16, 8, rd32(w32, 12) as u16);
    wr32(w16, 10, ptr_out(rd32(w32, 16)));
    wr16(w16, 14, rd32(w32, 20) as u16);
    wr32(w16, 16, ptr_out(rd32(w32, 24)));
}

/// OWNERITEM: hwnd, hps, fsState, fsAttribute, fsStateOld,
/// fsAttributeOld, rclItem, idItem, hItem; the states and idItem are
/// 16-bit in OS/2 1.x (38 bytes against 48).
const OI16: usize = 38;
const OI32: usize = 48;

fn owneritem_in(o16: *const u8, o32: *mut u8) {
    wr32(o32, 0, rd32(o16, 0));
    wr32(o32, 4, rd32(o16, 4));
    for k in 0..4 {
        wr32(o32, 8 + 4 * k, rd16(o16, 8 + 2 * k) as u32);
    }
    for k in 0..4 {
        wr32(o32, 24 + 4 * k, rd32(o16, 16 + 4 * k));
    }
    wr32(o32, 40, rd16(o16, 32) as i16 as i32 as u32);
    wr32(o32, 44, rd32(o16, 34));
}

fn owneritem_out(o32: *const u8, o16: *mut u8) {
    wr32(o16, 0, rd32(o32, 0));
    wr32(o16, 4, rd32(o32, 4));
    for k in 0..4 {
        wr16(o16, 8 + 2 * k, rd32(o32, 8 + 4 * k) as u16);
    }
    for k in 0..4 {
        wr32(o16, 16 + 4 * k, rd32(o32, 24 + 4 * k));
    }
    wr16(o16, 32, rd32(o32, 40) as u16);
    wr32(o16, 34, rd32(o32, 44));
}

/// TRACKINFO: six border/grid/keyboard sizes (16-bit in OS/2 1.x),
/// rclTrack, rclBoundary, ptlMinTrackSize, ptlMaxTrackSize, fs (16-bit in
/// OS/2 1.x): 62 bytes against 76.
const TI16: usize = 62;
const TI32: usize = 76;

pub fn trackinfo_in(t16: *const u8, t32: *mut u8) {
    for k in 0..6 {
        wr32(t32, 4 * k, rd16(t16, 2 * k) as i16 as i32 as u32);
    }
    unsafe { core::ptr::copy_nonoverlapping(t16.add(12), t32.add(24), 48) };
    wr32(t32, 72, rd16(t16, 60) as u32);
}

pub fn trackinfo_out(t32: *const u8, t16: *mut u8) {
    for k in 0..6 {
        wr16(t16, 2 * k, rd32(t32, 4 * k) as u16);
    }
    unsafe { core::ptr::copy_nonoverlapping(t32.add(24), t16.add(12), 48) };
    wr16(t16, 60, rd32(t32, 72) as u16);
}

// ---- Messages ----

const WM_CREATE: u32 = 0x0001;
const WM_ADJUSTWINDOWPOS: u32 = 0x0008;
const WM_CALCVALIDRECTS: u32 = 0x0009;
const WM_SETWINDOWPARAMS: u32 = 0x000a;
const WM_QUERYWINDOWPARAMS: u32 = 0x000b;
const WM_DRAWITEM: u32 = 0x0036;
const WM_MINMAXFRAME: u32 = 0x0046;
const WM_TRANSLATEACCEL: u32 = 0x004b;
const WM_QUERYTRACKINFO: u32 = 0x004c;
const WM_ERASEBACKGROUND: u32 = 0x004f;
const WM_OWNERPOSCHANGE: u32 = 0x0052;
const WM_CALCFRAMERECT: u32 = 0x0053;
const WM_WINDOWPOSCHANGED: u32 = 0x0055;
const WM_ADJUSTFRAMEPOS: u32 = 0x0056;
const WM_QUERYCONVERTPOS: u32 = 0x00b0;
const LM_INSERTITEM: u32 = 0x0161;
const LM_SETITEMTEXT: u32 = 0x0166;
const LM_QUERYITEMTEXT: u32 = 0x0168;
const LM_SEARCHSTRING: u32 = 0x016b;
const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_SETITEM: u32 = 0x0183;
const MM_QUERYITEMTEXT: u32 = 0x018b;
const MM_SETITEMTEXT: u32 = 0x018e;
const MM_QUERYITEMRECT: u32 = 0x0194;
const MLM_INSERT: u32 = 0x01c9;
const MLM_SETIMPORTEXPORT: u32 = 0x01c2;
const MLM_QUERYSELTEXT: u32 = 0x01cc;
const MLM_QUERYFONT: u32 = 0x01d0;
const MLM_SETFONT: u32 = 0x01d1;
const MLM_SETFORMATRECT: u32 = 0x01b2;
const MLM_QUERYFORMATRECT: u32 = 0x01b3;

/// How a message's parameters differ between 16-bit and 32-bit code.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    None,
    /// mp1 / mp2 / both point to data laid out alike.
    Ptr1,
    Ptr2,
    Ptr12,
    /// mp1 points to a SWP (count 1 or 2).
    Swp1(usize),
    /// mp1 a RECTL pair, mp2 a SWP.
    CalcValid,
    Create,
    WndParams,
    OwnerItem,
    TrackInfo,
    Qmsg1,
}

fn kind(msg: u32) -> Kind {
    match msg {
        WM_CREATE => Kind::Create,
        WM_ADJUSTWINDOWPOS | WM_MINMAXFRAME | WM_ADJUSTFRAMEPOS => Kind::Swp1(1),
        WM_WINDOWPOSCHANGED | WM_OWNERPOSCHANGE => Kind::Swp1(2),
        WM_CALCVALIDRECTS => Kind::CalcValid,
        WM_SETWINDOWPARAMS | WM_QUERYWINDOWPARAMS => Kind::WndParams,
        WM_DRAWITEM => Kind::OwnerItem,
        WM_QUERYTRACKINFO => Kind::TrackInfo,
        WM_TRANSLATEACCEL => Kind::Qmsg1,
        WM_CALCFRAMERECT | WM_QUERYCONVERTPOS | MLM_INSERT | MLM_SETIMPORTEXPORT | MLM_QUERYSELTEXT
        | MLM_QUERYFONT | MLM_SETFONT | MLM_SETFORMATRECT => Kind::Ptr1,
        MLM_QUERYFORMATRECT => Kind::Ptr1,
        WM_ERASEBACKGROUND | LM_INSERTITEM | LM_SETITEMTEXT | LM_QUERYITEMTEXT | LM_SEARCHSTRING | MM_QUERYITEM
        | MM_SETITEM | MM_QUERYITEMTEXT | MM_SETITEMTEXT | MM_QUERYITEMRECT => Kind::Ptr2,
        MM_INSERTITEM => Kind::Ptr12,
        _ => Kind::None,
    }
}

/// The parameters of a message from 16-bit code in 32-bit form: converted
/// structures live in `b` (`params` and `done` on the same, unmoved value).
pub struct MsgIn {
    kind: Kind,
    mp1: u32,
    mp2: u32,
    b: [u32; 40],
}

impl MsgIn {
    pub fn new(msg: u32, mp1: u32, mp2: u32) -> MsgIn {
        MsgIn { kind: kind(msg), mp1, mp2, b: [0; 40] }
    }

    fn buf(&mut self, at: usize) -> *mut u8 {
        unsafe { (self.b.as_mut_ptr() as *mut u8).add(at) }
    }

    pub fn params(&mut self) -> (u32, u32) {
        let (mp1, mp2) = (self.mp1, self.mp2);
        let p1 = flat(mp1);
        let p2 = flat(mp2);
        match self.kind {
            Kind::None => (mp1, mp2),
            Kind::Ptr1 => (ptr_in(mp1), mp2),
            Kind::Ptr2 => (mp1, ptr_in(mp2)),
            Kind::Ptr12 => (ptr_in(mp1), ptr_in(mp2)),
            Kind::Swp1(n) if !p1.is_null() => {
                for k in 0..n {
                    let d = self.buf(k * SWP32);
                    swp_in(unsafe { p1.add(k * SWP16) }, d);
                }
                (self.buf(0) as u32, mp2)
            }
            Kind::CalcValid => {
                if !p2.is_null() {
                    let d = self.buf(0);
                    swp_in(p2, d);
                }
                (ptr_in(mp1), if p2.is_null() { 0 } else { self.buf(0) as u32 })
            }
            Kind::Create => {
                if !p2.is_null() {
                    let d = self.buf(0);
                    create_in(p2, d);
                }
                (ptr_in(mp1), if p2.is_null() { 0 } else { self.buf(0) as u32 })
            }
            Kind::WndParams if !p1.is_null() => {
                let d = self.buf(0);
                wndparams_in(p1, d);
                (d as u32, mp2)
            }
            Kind::OwnerItem if !p2.is_null() => {
                let d = self.buf(0);
                owneritem_in(p2, d);
                (mp1, d as u32)
            }
            Kind::TrackInfo if !p2.is_null() => {
                let d = self.buf(0);
                trackinfo_in(p2, d);
                (mp1, d as u32)
            }
            Kind::Qmsg1 if !p1.is_null() => {
                let d = self.buf(0);
                qmsg_in(p1, d);
                (d as u32, mp2)
            }
            _ => (mp1, mp2),
        }
    }

    /// Copy what the receiver changed back to the 16-bit structures.
    pub fn done(&mut self) {
        let p1 = flat(self.mp1);
        let p2 = flat(self.mp2);
        match self.kind {
            Kind::Swp1(n) if !p1.is_null() => {
                for k in 0..n {
                    let s = self.buf(k * SWP32);
                    swp_out(s, unsafe { p1.add(k * SWP16) });
                }
            }
            Kind::CalcValid if !p2.is_null() => {
                let s = self.buf(0);
                swp_out(s, p2);
            }
            Kind::WndParams if !p1.is_null() => {
                // Only the status and counts change (the pointers stay).
                let s = self.buf(0);
                wr16(p1, 0, rd32(s, 0) as u16);
                wr16(p1, 2, rd32(s, 4) as u16);
                wr16(p1, 8, rd32(s, 12) as u16);
                wr16(p1, 14, rd32(s, 20) as u16);
            }
            Kind::OwnerItem if !p2.is_null() => {
                let s = self.buf(0);
                owneritem_out(s, p2);
            }
            Kind::TrackInfo if !p2.is_null() => {
                let s = self.buf(0);
                trackinfo_out(s, p2);
            }
            Kind::Qmsg1 if !p1.is_null() => {
                let s = self.buf(0);
                qmsg_out(s, p1);
            }
            _ => {}
        }
    }
}

/// The parameters of a message for a 16-bit procedure: converted
/// structures go on the 16-bit stack (the caller puts `sp16` back).
pub struct MsgOut {
    kind: Kind,
    mp1: u32,
    mp2: u32,
    s1: u32,
    s2: u32,
}

impl MsgOut {
    pub fn new(msg: u32, mp1: u32, mp2: u32) -> MsgOut {
        MsgOut { kind: kind(msg), mp1, mp2, s1: 0, s2: 0 }
    }

    pub fn params(&mut self) -> (u32, u32) {
        let (mp1, mp2) = (self.mp1, self.mp2);
        let p1 = mp1 as *mut u8;
        let p2 = mp2 as *mut u8;
        match self.kind {
            Kind::None => (mp1, mp2),
            Kind::Ptr1 => (ptr_out(mp1), mp2),
            Kind::Ptr2 => (mp1, ptr_out(mp2)),
            Kind::Ptr12 => (ptr_out(mp1), ptr_out(mp2)),
            Kind::Swp1(n) if mp1 != 0 => {
                self.s1 = alloc16(n * SWP16);
                for k in 0..n {
                    swp_out(unsafe { p1.add(k * SWP32) }, unsafe { flat(self.s1).add(k * SWP16) });
                }
                (self.s1, mp2)
            }
            Kind::CalcValid => {
                if mp2 != 0 {
                    self.s2 = alloc16(SWP16);
                    swp_out(p2, flat(self.s2));
                }
                (ptr_out(mp1), self.s2)
            }
            Kind::Create => {
                if mp2 != 0 {
                    self.s2 = alloc16(CREATE16);
                    create_out(p2, flat(self.s2));
                }
                (ptr_out(mp1), self.s2)
            }
            Kind::WndParams if mp1 != 0 => {
                self.s1 = alloc16(WP16);
                wndparams_out(p1, flat(self.s1));
                (self.s1, mp2)
            }
            Kind::OwnerItem if mp2 != 0 => {
                self.s2 = alloc16(OI16);
                owneritem_out(p2, flat(self.s2));
                (mp1, self.s2)
            }
            Kind::TrackInfo if mp2 != 0 => {
                self.s2 = alloc16(TI16);
                trackinfo_out(p2, flat(self.s2));
                (mp1, self.s2)
            }
            Kind::Qmsg1 if mp1 != 0 => {
                self.s1 = alloc16(QMSG16);
                qmsg_out(p1, flat(self.s1));
                (self.s1, mp2)
            }
            _ => (mp1, mp2),
        }
    }

    /// Copy what the 16-bit procedure changed back.
    pub fn done(&mut self) {
        let p1 = self.mp1 as *mut u8;
        let p2 = self.mp2 as *mut u8;
        match self.kind {
            Kind::Swp1(n) if self.s1 != 0 => {
                for k in 0..n {
                    swp_in(unsafe { flat(self.s1).add(k * SWP16) }, unsafe { p1.add(k * SWP32) });
                }
            }
            Kind::CalcValid if self.s2 != 0 => swp_in(flat(self.s2), p2),
            Kind::WndParams if self.s1 != 0 => {
                let s = flat(self.s1);
                wr32(p1, 0, rd16(s, 0) as u32);
                wr32(p1, 4, rd16(s, 2) as u32);
                wr32(p1, 12, rd16(s, 8) as u32);
                wr32(p1, 20, rd16(s, 14) as u32);
            }
            Kind::OwnerItem if self.s2 != 0 => owneritem_in(flat(self.s2), p2),
            Kind::TrackInfo if self.s2 != 0 => trackinfo_in(flat(self.s2), p2),
            Kind::Qmsg1 if self.s1 != 0 => qmsg_in(flat(self.s1), p1),
            _ => {}
        }
    }
}

#[allow(dead_code)]
const _SIZES: () = assert!(CREATE32 == 52 && WP32 == 28 && OI32 == 48 && TI32 == 76);
