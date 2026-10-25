//! WIN16.DLL: rs2's Windows 3.x KERNEL, USER and GDI (docs/WIN16.md).
//!
//! Windows programs (NE modules with target OS 2) run as ordinary OS/2
//! processes; the loader sends their KERNEL, USER, GDI, KEYBOARD...
//! imports here, Windows ordinal n of module M being this DLL's ordinal
//! base(M) + n (kernel loader/mod.rs, WIN16_MODULES: KERNEL 1000, USER
//! 2000, GDI 3000, KEYBOARD 4000). The entries are `_pascal far` 16-bit
//! procedures reached through thunk16's stubs; the few register-based
//! ones (InitTask, DOS3Call) are 16-bit code below that executes an
//! INT, which the kernel reflects to `w16_int` (api/win16.rs), as are the
//! program's own INT 21h calls.
//!
//! USER puts each Windows window on a PM window (a frame for top-level
//! windows) and translates messages and coordinates (Windows' y grows
//! down); GDI draws on the PM presentation space of the window.

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

mod dos;
mod gdi;
mod kernel;
mod pm;
mod user;

use core::cell::UnsafeCell;
use thunk16::Pascal;

pub use pm::trace;

/// Per-process state (the DLL's data is instance data): one Windows
/// task per process, one thread in it.
pub struct Global<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Global<T> {}
impl<T> Global<T> {
    pub const fn new(v: T) -> Self {
        Global(UnsafeCell::new(v))
    }
    /// Callers keep the borrow short: never across a call to 16-bit
    /// code (a window procedure may call back in).
    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// `SET W16TRACE=1`: trace the Windows calls to the serial port.
static TRACING: Global<bool> = Global::new(false);

pub fn tracing() -> bool {
    *TRACING.get()
}

/// _DLL_InitTerm(hmod, flag), entered with only 4-byte stack alignment.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
#[unsafe(naked)]
pub extern "C" fn _start() {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
        "mov eax, [ebp + 12]", "mov [esp], eax",
        "call {init}",
        "mov esp, ebp", "pop ebp", "mov eax, 1", "ret",
        init = sym init,
    );
}

extern "C" fn init(flag: u32) {
    if flag == 0 {
        *TRACING.get() = env_set(b"W16TRACE");
        // The program's INT instructions (and InitTask's) come to w16_int.
        unsafe extern "C" {
            fn w16_int();
        }
        let f = thunk16::far(w16_int as *const u8);
        unsafe { pm::Rs2Win16SetIntHandler(f) };
    }
}

/// Is the environment variable `name` set to something other than 0?
fn env_set(name: &[u8]) -> bool {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    if pib.is_null() {
        return false;
    }
    let mut p = unsafe { (*pib).pchenv };
    if p.is_null() {
        return false;
    }
    loop {
        let s = unsafe { cstr(p) };
        if s.is_empty() {
            return false;
        }
        if s.len() > name.len() && s[name.len()] == b'=' && s[..name.len()].eq_ignore_ascii_case(name) {
            return s[name.len() + 1..] != *b"0";
        }
        p = unsafe { p.add(s.len() + 1) };
    }
}

/// The bytes of a NUL-terminated string.
pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// A string argument (far pointer) as bytes; a resource ID (selector 0)
/// as None.
pub fn far_str<'a>(far: u32) -> Option<&'a [u8]> {
    if far >> 16 == 0 { None } else { Some(unsafe { cstr(thunk16::flat(far)) }) }
}

/// The DS of the 16-bit caller (Local* calls work on it).
pub fn caller_ds() -> u16 {
    let v: u32;
    unsafe { core::arch::asm!("mov {0}, dword ptr fs:[0x38]", out(reg) v, options(nostack, readonly)) };
    v as u16
}

// The register-based KERNEL entries: an INT the kernel reflects to
// w16_int, which reads and sets the registers; then a far return.
core::arch::global_asm!(
    ".pushsection .text.w16regs,\"ax\"",
    ".p2align 4",
    ".code16",
    ".globl w16_inittask",
    "w16_inittask:",
    "int 0xe0",
    ".byte 0xcb",
    ".globl w16_dos3call",
    "w16_dos3call:",
    "int 0x21",
    ".byte 0xcb",
    ".code32",
    ".popsection",
);

/// The 16-bit entries and the size of their arguments.
macro_rules! entries {
    ($($name:ident = $idx:literal, $size:literal;)*) => {
        thunk16::entries16_pascal! {
            dispatch = dispatch;
            $($name = $idx, $size;)*
        }
        fn arg_size(idx: u32) -> usize {
            match idx { $($idx => $size,)* _ => 0 }
        }
    };
}

// `name = WIN16 ordinal, bytes of arguments`.
entries! {
    w16_int = 1, 4;
    // KERNEL
    k_fatalexit = 1001, 2;
    k_getversion = 1003, 0;
    k_localinit = 1004, 6;
    k_localalloc = 1005, 4;
    k_localrealloc = 1006, 6;
    k_localfree = 1007, 2;
    k_locallock = 1008, 2;
    k_localunlock = 1009, 2;
    k_localsize = 1010, 2;
    k_localhandle = 1011, 2;
    k_localflags = 1012, 2;
    k_localcompact = 1013, 2;
    k_globalalloc = 1015, 6;
    k_globalrealloc = 1016, 8;
    k_globalfree = 1017, 2;
    k_globallock = 1018, 2;
    k_globalunlock = 1019, 2;
    k_globalsize = 1020, 2;
    k_globalhandle = 1021, 2;
    k_globalflags = 1022, 2;
    k_locksegment = 1023, 2;
    k_unlocksegment = 1024, 2;
    k_globalcompact = 1025, 4;
    k_yield = 1029, 0;
    k_waitevent = 1030, 2;
    k_getcurrenttask = 1036, 0;
    k_getcurrentpdb = 1037, 0;
    k_getmodulehandle = 1047, 4;
    k_getmoduleusage = 1048, 2;
    k_getmodulefilename = 1049, 8;
    k_getprocaddress = 1050, 6;
    k_makeprocinstance = 1051, 6;
    k_freeprocinstance = 1052, 4;
    k_getinstancedata = 1054, 6;
    k_lstrcpy = 1088, 8;
    k_lstrcat = 1089, 8;
    k_lstrlen = 1090, 4;
    k_seterrormode = 1107, 2;
    k_outputdebugstring = 1115, 4;
    k_getdosenvironment = 1131, 0;
    k_getwinflags = 1132, 0;
    k_fatalappexit = 1137, 6;
    k_sethandlecount = 1199, 2;
    // USER
    u_messagebox = 2001, 12;
    u_initapp = 2005, 2;
    u_postquitmessage = 2006, 2;
    u_settimer = 2010, 10;
    u_killtimer = 2012, 4;
    u_gettickcount = 2013, 0;
    u_getcursorpos = 2017, 4;
    u_setfocus = 2022, 2;
    u_getfocus = 2023, 0;
    u_clienttoscreen = 2028, 6;
    u_screentoclient = 2029, 6;
    u_getwindowrect = 2032, 6;
    u_getclientrect = 2033, 6;
    u_enablewindow = 2034, 4;
    u_getwindowtext = 2036, 8;
    u_setwindowtext = 2037, 6;
    u_beginpaint = 2039, 6;
    u_endpaint = 2040, 6;
    u_createwindow = 2041, 30;
    u_showwindow = 2042, 4;
    u_getparent = 2046, 2;
    u_iswindow = 2047, 2;
    u_iswindowvisible = 2049, 2;
    u_destroywindow = 2053, 2;
    u_movewindow = 2056, 12;
    u_registerclass = 2057, 4;
    u_getactivewindow = 2060, 0;
    u_getdc = 2066, 2;
    u_releasedc = 2068, 4;
    u_setcursor = 2069, 2;
    u_fillrect = 2081, 8;
    u_drawtext = 2085, 14;
    u_messagebeep = 2104, 2;
    u_defwindowproc = 2107, 10;
    u_getmessage = 2108, 10;
    u_peekmessage = 2109, 12;
    u_postmessage = 2110, 10;
    u_sendmessage = 2111, 10;
    u_translatemessage = 2113, 4;
    u_dispatchmessage = 2114, 4;
    u_callwindowproc = 2122, 14;
    u_updatewindow = 2124, 2;
    u_invalidaterect = 2125, 8;
    u_validaterect = 2127, 6;
    u_getwindowword = 2133, 4;
    u_setwindowword = 2134, 6;
    u_getwindowlong = 2135, 4;
    u_setwindowlong = 2136, 8;
    u_loadcursor = 2173, 6;
    u_loadicon = 2174, 6;
    u_getsystemmetrics = 2179, 2;
    u_getsyscolor = 2180, 2;
    u_setwindowpos = 2232, 14;
    // GDI
    g_setbkcolor = 3001, 6;
    g_setbkmode = 3002, 4;
    g_settextcolor = 3009, 6;
    g_lineto = 3019, 6;
    g_moveto = 3020, 6;
    g_rectangle = 3027, 10;
    g_textout = 3033, 12;
    g_selectobject = 3045, 4;
    g_createpen = 3061, 8;
    g_createsolidbrush = 3066, 4;
    g_deleteobject = 3069, 2;
    g_getdevicecaps = 3080, 4;
    g_getstockobject = 3087, 2;
    g_gettextextent = 3091, 8;
    g_gettextmetrics = 3093, 6;
    // KEYBOARD
    kb_ansitooem = 4005, 8;
    kb_oemtoansi = 4006, 8;
    kb_getkeyboardtype = 4130, 2;
}

extern "C" fn dispatch(idx: u32, args: *const u8) -> u32 {
    let size = arg_size(idx);
    let mut a = Pascal::new(args, size);
    if tracing() && idx != 1 {
        trace(format_args!("W16 {}", idx));
    }
    match idx {
        1 => {
            dos::interrupt(thunk16::flat(a.l()) as *mut u16);
            0
        }
        1000..2000 => kernel::call(idx - 1000, &mut a),
        2000..3000 => user::call(idx - 2000, &mut a),
        3000..4000 => gdi::call(idx - 3000, &mut a),
        4000..5000 => kernel::keyboard(idx - 4000, &mut a),
        _ => 0,
    }
}

/// Read a 16-bit RECT (left, top, right, bottom) at a far pointer.
pub fn read_rect(far: u32) -> Option<[i32; 4]> {
    let p = thunk16::flat(far) as *const i16;
    if p.is_null() {
        return None;
    }
    Some(core::array::from_fn(|i| unsafe { p.add(i).read_unaligned() } as i32))
}

pub fn write_rect(far: u32, r: [i32; 4]) {
    let p = thunk16::flat(far) as *mut i16;
    if p.is_null() {
        return;
    }
    for (i, v) in r.iter().enumerate() {
        unsafe { p.add(i).write_unaligned(*v as i16) };
    }
}
