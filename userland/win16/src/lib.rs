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

mod caret;
mod ctl;
mod dialog;
mod dos;
mod edit;
mod gdi;
mod kernel;
mod listbox;
mod mem16;
mod menu;
mod misc;
mod pm;
mod res;
mod scroll;
mod stubs;
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
    // The built-in controls' window procedures (ctl.rs).
    p_button = 2, 10;
    p_static = 3, 10;
    p_edit = 4, 10;
    p_listbox = 5, 10;
    p_combo = 6, 10;
    p_scrollbar = 7, 10;
    p_combolbox = 8, 10;
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
    k_getprofileint = 1057, 10;
    k_getprofilestring = 1058, 18;
    k_writeprofilestring = 1059, 12;
    k_findresource = 1060, 10;
    k_loadresource = 1061, 4;
    k_lockresource = 1062, 2;
    k_freeresource = 1063, 2;
    k_sizeofresource = 1065, 4;
    k_openfile = 1074, 10;
    k_lclose = 1081, 2;
    k_lread = 1082, 8;
    k_lcreat = 1083, 6;
    k_llseek = 1084, 8;
    k_lopen = 1085, 6;
    k_lwrite = 1086, 8;
    k_lstrcpy = 1088, 8;
    k_lstrcat = 1089, 8;
    k_lstrlen = 1090, 4;
    k_gettempdrive = 1092, 2;
    k_loadlibrary = 1095, 4;
    k_freelibrary = 1096, 2;
    k_seterrormode = 1107, 2;
    k_outputdebugstring = 1115, 4;
    k_getprivateprofileint = 1127, 14;
    k_getprivateprofilestring = 1128, 22;
    k_writeprivateprofilestring = 1129, 16;
    k_getdosenvironment = 1131, 0;
    k_getwinflags = 1132, 0;
    k_getwindowsdirectory = 1134, 6;
    k_getsystemdirectory = 1135, 6;
    k_fatalappexit = 1137, 6;
    k_winexec = 1166, 6;
    k_sethandlecount = 1199, 2;
    k_isdbcsleadbyte = 1207, 2;
    // USER
    u_messagebox = 2001, 12;
    u_initapp = 2005, 2;
    u_postquitmessage = 2006, 2;
    u_settimer = 2010, 10;
    u_killtimer = 2012, 4;
    u_gettickcount = 2013, 0;
    u_getcurrenttime = 2015, 0;
    u_getcursorpos = 2017, 4;
    u_setcapture = 2018, 2;
    u_releasecapture = 2019, 0;
    u_setdoubleclicktime = 2020, 2;
    u_getdoubleclicktime = 2021, 0;
    u_setfocus = 2022, 2;
    u_getfocus = 2023, 0;
    u_clienttoscreen = 2028, 6;
    u_screentoclient = 2029, 6;
    u_windowfrompoint = 2030, 4;
    u_isiconic = 2031, 2;
    u_getwindowrect = 2032, 6;
    u_getclientrect = 2033, 6;
    u_enablewindow = 2034, 4;
    u_iswindowenabled = 2035, 2;
    u_getwindowtext = 2036, 8;
    u_setwindowtext = 2037, 6;
    u_getwindowtextlength = 2038, 2;
    u_beginpaint = 2039, 6;
    u_endpaint = 2040, 6;
    u_createwindow = 2041, 30;
    u_showwindow = 2042, 4;
    u_bringwindowtotop = 2045, 2;
    u_getparent = 2046, 2;
    u_iswindow = 2047, 2;
    u_ischild = 2048, 4;
    u_iswindowvisible = 2049, 2;
    u_findwindow = 2050, 8;
    u_destroywindow = 2053, 2;
    u_enumwindows = 2054, 8;
    u_enumchildwindows = 2055, 10;
    u_movewindow = 2056, 12;
    u_registerclass = 2057, 4;
    u_getclassname = 2058, 8;
    u_setactivewindow = 2059, 2;
    u_getactivewindow = 2060, 0;
    u_scrollwindow = 2061, 14;
    u_setscrollpos = 2062, 8;
    u_getscrollpos = 2063, 4;
    u_setscrollrange = 2064, 10;
    u_getscrollrange = 2065, 12;
    u_getdc = 2066, 2;
    u_getwindowdc = 2067, 2;
    u_releasedc = 2068, 4;
    u_setcursor = 2069, 2;
    u_setcursorpos = 2070, 4;
    u_showcursor = 2071, 2;
    u_setrect = 2072, 12;
    u_setrectempty = 2073, 4;
    u_copyrect = 2074, 8;
    u_isrectempty = 2075, 4;
    u_ptinrect = 2076, 8;
    u_offsetrect = 2077, 8;
    u_inflaterect = 2078, 8;
    u_intersectrect = 2079, 12;
    u_unionrect = 2080, 12;
    u_fillrect = 2081, 8;
    u_invertrect = 2082, 6;
    u_framerect = 2083, 8;
    u_drawicon = 2084, 8;
    u_drawtext = 2085, 14;
    u_dialogbox = 2087, 12;
    u_enddialog = 2088, 4;
    u_createdialog = 2089, 12;
    u_isdialogmessage = 2090, 6;
    u_getdlgitem = 2091, 4;
    u_setdlgitemtext = 2092, 8;
    u_getdlgitemtext = 2093, 10;
    u_setdlgitemint = 2094, 8;
    u_getdlgitemint = 2095, 10;
    u_checkradiobutton = 2096, 8;
    u_checkdlgbutton = 2097, 6;
    u_isdlgbuttonchecked = 2098, 4;
    u_dlgdirselect = 2099, 8;
    u_dlgdirlist = 2100, 12;
    u_senddlgitemmessage = 2101, 12;
    u_adjustwindowrect = 2102, 10;
    u_mapdialogrect = 2103, 6;
    u_messagebeep = 2104, 2;
    u_flashwindow = 2105, 4;
    u_getkeystate = 2106, 2;
    u_defwindowproc = 2107, 10;
    u_getmessage = 2108, 10;
    u_peekmessage = 2109, 12;
    u_postmessage = 2110, 10;
    u_sendmessage = 2111, 10;
    u_waitmessage = 2112, 0;
    u_translatemessage = 2113, 4;
    u_dispatchmessage = 2114, 4;
    u_postappmessage = 2116, 10;
    u_registerwindowmessage = 2118, 4;
    u_getmessagepos = 2119, 0;
    u_getmessagetime = 2120, 0;
    u_callwindowproc = 2122, 14;
    u_updatewindow = 2124, 2;
    u_invalidaterect = 2125, 8;
    u_invalidatergn = 2126, 6;
    u_validaterect = 2127, 6;
    u_validatergn = 2128, 4;
    u_getclassword = 2129, 4;
    u_setclassword = 2130, 6;
    u_getclasslong = 2131, 4;
    u_setclasslong = 2132, 8;
    u_getwindowword = 2133, 4;
    u_setwindowword = 2134, 6;
    u_getwindowlong = 2135, 4;
    u_setwindowlong = 2136, 8;
    u_openclipboard = 2137, 2;
    u_closeclipboard = 2138, 0;
    u_emptyclipboard = 2139, 0;
    u_getclipboardowner = 2140, 0;
    u_setclipboarddata = 2141, 4;
    u_getclipboarddata = 2142, 2;
    u_countclipboardformats = 2143, 0;
    u_enumclipboardformats = 2144, 2;
    u_registerclipboardformat = 2145, 4;
    u_getclipboardformatname = 2146, 8;
    u_setclipboardviewer = 2147, 2;
    u_getclipboardviewer = 2148, 0;
    u_changeclipboardchain = 2149, 4;
    u_loadmenu = 2150, 6;
    u_createmenu = 2151, 0;
    u_destroymenu = 2152, 2;
    u_changemenu = 2153, 12;
    u_checkmenuitem = 2154, 6;
    u_enablemenuitem = 2155, 6;
    u_getsystemmenu = 2156, 4;
    u_getmenu = 2157, 2;
    u_setmenu = 2158, 4;
    u_getsubmenu = 2159, 4;
    u_drawmenubar = 2160, 2;
    u_getmenustring = 2161, 12;
    u_hilitemenuitem = 2162, 8;
    u_createcaret = 2163, 8;
    u_destroycaret = 2164, 0;
    u_setcaretpos = 2165, 4;
    u_hidecaret = 2166, 2;
    u_showcaret = 2167, 2;
    u_setcaretblinktime = 2168, 2;
    u_getcaretblinktime = 2169, 0;
    u_winhelp = 2171, 12;
    u_loadcursor = 2173, 6;
    u_loadicon = 2174, 6;
    u_loadstring = 2176, 10;
    u_loadaccelerators = 2177, 6;
    u_translateaccelerator = 2178, 8;
    u_getsystemmetrics = 2179, 2;
    u_getsyscolor = 2180, 2;
    u_getcaretpos = 2183, 4;
    u_getupdaterect = 2190, 8;
    u_childwindowfrompoint = 2191, 6;
    u_insendmessage = 2192, 0;
    u_isclipboardformatavailable = 2193, 2;
    u_dialogboxindirect = 2218, 10;
    u_createdialogindirect = 2219, 12;
    u_loadmenuindirect = 2220, 4;
    u_getkeyboardstate = 2222, 4;
    u_setkeyboardstate = 2223, 4;
    u_getwindowtask = 2224, 2;
    u_getnextdlggroupitem = 2227, 6;
    u_getnextdlgtabitem = 2228, 6;
    u_gettopwindow = 2229, 2;
    u_getnextwindow = 2230, 4;
    u_setwindowpos = 2232, 14;
    u_setparent = 2233, 4;
    u_getcapture = 2236, 0;
    u_dialogboxparam = 2239, 16;
    u_dialogboxindirectparam = 2240, 14;
    u_createdialogparam = 2241, 16;
    u_createdialogindirectparam = 2242, 16;
    u_getdialogbaseunits = 2243, 0;
    u_equalrect = 2244, 8;
    u_getcursor = 2247, 0;
    u_getasynckeystate = 2249, 2;
    u_getmenustate = 2250, 6;
    u_mapwindowpoints = 2258, 10;
    u_getwindow = 2262, 4;
    u_getmenuitemcount = 2263, 2;
    u_getmenuitemid = 2264, 4;
    u_showscrollbar = 2267, 6;
    u_iszoomed = 2272, 2;
    u_getdlgctrlid = 2277, 2;
    u_getdesktophwnd = 2278, 0;
    u_getdesktopwindow = 2286, 0;
    u_getlastactivepopup = 2287, 2;
    u_redrawwindow = 2290, 10;
    u_defdlgproc = 2308, 10;
    u_ismenu = 2358, 2;
    u_subtractrect = 2373, 12;
    u_unregisterclass = 2403, 6;
    u_getclassinfo = 2404, 10;
    u_insertmenu = 2410, 12;
    u_appendmenu = 2411, 10;
    u_removemenu = 2412, 6;
    u_deletemenu = 2413, 6;
    u_modifymenu = 2414, 12;
    u_createpopupmenu = 2415, 0;
    u_trackpopupmenu = 2416, 16;
    u_getmenucheckmarkdimensions = 2417, 0;
    u_setmenuitembitmaps = 2418, 10;
    u_wvsprintf = 2421, 12;
    u_lstrcmp = 2430, 8;
    u_ansiupper = 2431, 4;
    u_ansilower = 2432, 4;
    u_ischaralpha = 2433, 2;
    u_ischaralphanumeric = 2434, 2;
    u_ischarupper = 2435, 2;
    u_ischarlower = 2436, 2;
    u_ansiupperbuff = 2437, 6;
    u_ansilowerbuff = 2438, 6;
    u_createwindowex = 2452, 34;
    u_adjustwindowrectex = 2454, 14;
    u_drawfocusrect = 2466, 6;
    u_lstrcmpi = 2471, 8;
    u_ansinext = 2472, 4;
    u_ansiprev = 2473, 8;
    u_enablescrollbar = 2482, 6;
    // GDI
    g_setbkcolor = 3001, 6;
    g_setbkmode = 3002, 4;
    g_setmapmode = 3003, 4;
    g_setrop2 = 3004, 4;
    g_settextcolor = 3009, 6;
    g_lineto = 3019, 6;
    g_moveto = 3020, 6;
    g_excludecliprect = 3021, 10;
    g_intersectcliprect = 3022, 10;
    g_ellipse = 3024, 10;
    g_rectangle = 3027, 10;
    g_roundrect = 3028, 14;
    g_patblt = 3029, 14;
    g_savedc = 3030, 2;
    g_setpixel = 3031, 10;
    g_textout = 3033, 12;
    g_polygon = 3036, 8;
    g_polyline = 3037, 8;
    g_restoredc = 3039, 4;
    g_selectobject = 3045, 4;
    g_createbrushindirect = 3050, 4;
    g_createfont = 3056, 30;
    g_createfontindirect = 3057, 4;
    g_createhatchbrush = 3058, 6;
    g_createpen = 3061, 8;
    g_createpenindirect = 3062, 4;
    g_createsolidbrush = 3066, 4;
    g_deleteobject = 3069, 2;
    g_getbkcolor = 3075, 2;
    g_getbkmode = 3076, 2;
    g_getclipbox = 3077, 6;
    g_getcurrentposition = 3078, 2;
    g_getdevicecaps = 3080, 4;
    g_getmapmode = 3081, 2;
    g_getobject = 3082, 8;
    g_getpixel = 3083, 6;
    g_getrop2 = 3085, 2;
    g_getstockobject = 3087, 2;
    g_gettextcolor = 3090, 2;
    g_gettextextent = 3091, 8;
    g_gettextface = 3092, 8;
    g_gettextmetrics = 3093, 6;
    g_muldiv = 3128, 6;
    g_gettextalign = 3345, 2;
    g_settextalign = 3346, 4;
    g_getcharwidth = 3350, 10;
    g_exttextout = 3351, 22;
    // KEYBOARD
    kb_ansitooem = 4005, 8;
    kb_oemtoansi = 4006, 8;
    kb_getkeyboardtype = 4130, 2;
    kb_getkbcodepage = 4132, 0;
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
        2..=8 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            native(idx, h, m, wp, lp)
        }
        1000..2000 => kernel::call(idx - 1000, &mut a),
        2000..3000 => user::call(idx - 2000, &mut a),
        3000..4000 => gdi::call(idx - 3000, &mut a),
        4000..5000 => kernel::keyboard(idx - 4000, &mut a),
        _ => 0,
    }
}

/// How many of WIN16's own window procedures are running (the windows
/// destroyed meanwhile are freed when none is).
static NATIVE_DEPTH: Global<u32> = Global::new(0);

pub fn native_depth() -> u32 {
    *NATIVE_DEPTH.get()
}

/// Call one of WIN16's window procedures: a built-in control's (entry
/// 2-8), DefWindowProc (2107) or DefDlgProc (2308).
pub fn native(idx: u32, h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    *NATIVE_DEPTH.get() += 1;
    let r = match idx {
        2..=8 => ctl::proc_(idx, h, m, wp, lp),
        2107 => user::def_window_proc(h, m, wp, lp),
        2308 => dialog::def_dlg_proc(h, m, wp, lp),
        _ => 0,
    };
    *NATIVE_DEPTH.get() -= 1;
    r
}

// wsprintf is `_cdecl` (its arguments vary).
thunk16::entries16! {
    dispatch = dispatch_cdecl;
    u_wsprintf = 2420;
}

extern "C" fn dispatch_cdecl(idx: u32, args: *const u8) -> u32 {
    let a = thunk16::Args(args);
    match idx {
        // wsprintf(buf, format, ...)
        2420 => misc::sprintf(a.p(0), far_str(a.l(4)).unwrap_or(b""), unsafe { args.add(8) }) as u32,
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
