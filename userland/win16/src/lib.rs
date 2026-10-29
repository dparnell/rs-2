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

mod atom;
mod bitmap;
mod caret;
mod clip;
mod commdlg;
mod compat;
mod ctl;
mod dde;
mod ddeml;
mod dialog;
mod dos;
mod draw;
mod edit;
mod gdi;
mod icon;
mod kernel;
mod listbox;
mod lz;
mod mdi;
mod mem16;
mod menu;
mod misc;
mod module;
mod ole;
mod pm;
mod print;
mod progman;
mod profile;
mod reg;
mod region;
mod res;
mod scroll;
mod stubs;
mod toolhelp;
mod w16lock;
mod user;
mod ver;

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
    } else {
        toolhelp::task_exiting(0);
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

/// A line on standard error (the command prompt a program was started
/// from; nothing when there is none).
pub fn stderr_line(text: &str) {
    let mut b = text.as_bytes().to_vec();
    b.extend_from_slice(b"\r\n");
    let mut n = 0;
    unsafe { os2::DosWrite(2, b.as_ptr(), b.len() as u32, &mut n) };
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
    ".globl w16_fpmath",
    "w16_fpmath:",
    "int 0xe1",
    ".byte 0xcb",
    // An import WIN16 does not have (loader: WIN16_UNDEFINED).
    ".globl w16_undefined",
    "w16_undefined:",
    "int 0xe4",
    ".byte 0xcb",
    // Catch(lpCatchBuf), Throw(lpCatchBuf, nThrowBack)
    ".globl w16_catch",
    "w16_catch:",
    "int 0xe2",
    ".byte 0xca, 4, 0",
    ".globl w16_throw",
    "w16_throw:",
    "int 0xe3",
    ".byte 0xca, 6, 0",
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
    p_mdiclient = 9, 10;
    // The common dialogs' dialog procedure and DDEML's window procedure.
    p_commdlg = 11, 10;
    p_ddeml = 12, 10;
    // The Program Manager stand-in's window procedure (progman.rs).
    p_progman = 10, 10;
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
    k_loadmodule = 1045, 8;
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
    k_accessresource = 1064, 4;
    k_sizeofresource = 1065, 4;
    k_initatomtable = 1068, 2;
    k_findatom = 1069, 4;
    k_addatom = 1070, 4;
    k_deleteatom = 1071, 2;
    k_getatomname = 1072, 8;
    k_getatomhandle = 1073, 2;
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
    k_getcodehandle = 1093, 4;
    k_directedyield = 1150, 2;
    k_loadlibrary = 1095, 4;
    k_freelibrary = 1096, 2;
    k_gettempfilename = 1097, 12;
    k_seterrormode = 1107, 2;
    k_outputdebugstring = 1115, 4;
    k_getprivateprofileint = 1127, 14;
    k_getprivateprofilestring = 1128, 22;
    k_writeprivateprofilestring = 1129, 16;
    k_getdosenvironment = 1131, 0;
    k_getwinflags = 1132, 0;
    k_getexeptr = 1133, 2;
    k_getheapspaces = 1138, 2;
    k_getwindowsdirectory = 1134, 6;
    k_getsystemdirectory = 1135, 6;
    k_getdrivetype = 1136, 2;
    k_fatalappexit = 1137, 6;
    k_getnumtasks = 1152, 0;
    k_winexec = 1166, 6;
    k_getfreespace = 1169, 2;
    k_alloccstodsalias = 1170, 2;
    k_allocdstocsalias = 1171, 2;
    k_allocselector = 1175, 2;
    k_freeselector = 1176, 2;
    k_prestochangoselector = 1177, 4;
    k_globaldosalloc = 1184, 4;
    k_globaldosfree = 1185, 2;
    k_getselectorbase = 1186, 2;
    k_setselectorbase = 1187, 6;
    k_getselectorlimit = 1188, 2;
    k_setselectorlimit = 1189, 6;
    k_sethandlecount = 1199, 2;
    k_isdbcsleadbyte = 1207, 2;
    k_istask = 1320, 2;
    k_isbadreadptr = 1334, 6;
    k_isbadwriteptr = 1335, 6;
    k_isbadcodeptr = 1336, 4;
    k_isbadstringptr = 1337, 6;
    k_isbadhugereadptr = 1346, 8;
    k_isbadhugewriteptr = 1347, 8;
    k_hmemcpy = 1348, 12;
    k__hread = 1349, 10;
    k__hwrite = 1350, 10;
    k_lstrcpyn = 1353, 10;
    // USER
    u_messagebox = 2001, 12;
    u_initapp = 2005, 2;
    u_postquitmessage = 2006, 2;
    u_exitwindows = 2007, 6;
    u_settimer = 2010, 10;
    u_killtimer = 2012, 4;
    u_gettickcount = 2013, 0;
    u_gettimerresolution = 2014, 0;
    u_getcurrenttime = 2015, 0;
    u_getcursorpos = 2017, 4;
    u_setcapture = 2018, 2;
    u_releasecapture = 2019, 0;
    u_setdoubleclicktime = 2020, 2;
    u_getdoubleclicktime = 2021, 0;
    u_setfocus = 2022, 2;
    u_getfocus = 2023, 0;
    u_removeprop = 2024, 6;
    u_getprop = 2025, 6;
    u_setprop = 2026, 8;
    u_enumprops = 2027, 6;
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
    u_closewindow = 2043, 2;
    u_openicon = 2044, 2;
    u_bringwindowtotop = 2045, 2;
    u_getparent = 2046, 2;
    u_iswindow = 2047, 2;
    u_ischild = 2048, 4;
    u_iswindowvisible = 2049, 2;
    u_findwindow = 2050, 8;
    u_anypopup = 2052, 0;
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
    u_replymessage = 2115, 4;
    u_postappmessage = 2116, 10;
    u_registerwindowmessage = 2118, 4;
    u_getmessagepos = 2119, 0;
    u_getmessagetime = 2120, 0;
    u_setwindowshook = 2121, 6;
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
    u_loadbitmap = 2175, 6;
    u_loadstring = 2176, 10;
    u_loadaccelerators = 2177, 6;
    u_translateaccelerator = 2178, 8;
    u_getsystemmetrics = 2179, 2;
    u_getsyscolor = 2180, 2;
    u_setsyscolors = 2181, 10;
    u_getcaretpos = 2183, 4;
    u_querysendmessage = 2184, 10;
    u_graystring = 2185, 22;
    u_swapmousebutton = 2186, 2;
    u_setsysmodalwindow = 2188, 2;
    u_getsysmodalwindow = 2189, 0;
    u_getupdaterect = 2190, 8;
    u_childwindowfrompoint = 2191, 6;
    u_insendmessage = 2192, 0;
    u_getsystemdebugstate = 2231, 0;
    u_isclipboardformatavailable = 2193, 2;
    u_tabbedtextout = 2196, 20;
    u_gettabbedtextextent = 2197, 14;
    u_dialogboxindirect = 2218, 10;
    u_createdialogindirect = 2219, 12;
    u_loadmenuindirect = 2220, 4;
    u_scrolldc = 2221, 20;
    u_getkeyboardstate = 2222, 4;
    u_setkeyboardstate = 2223, 4;
    u_switchtothiswindow = 2172, 4;
    u_endmenu = 2187, 0;
    u_getwindowtask = 2224, 2;
    u_enumtaskwindows = 2225, 10;
    u_getnextdlggroupitem = 2227, 6;
    u_getnextdlgtabitem = 2228, 6;
    u_gettopwindow = 2229, 2;
    u_getnextwindow = 2230, 4;
    u_setwindowpos = 2232, 14;
    u_setparent = 2233, 4;
    u_unhookwindowshook = 2234, 6;
    u_defhookproc = 2235, 12;
    u_getcapture = 2236, 0;
    u_getupdatergn = 2237, 6;
    u_excludeupdatergn = 2238, 4;
    u_dialogboxparam = 2239, 16;
    u_dialogboxindirectparam = 2240, 14;
    u_createdialogparam = 2241, 16;
    u_createdialogindirectparam = 2242, 16;
    u_getdialogbaseunits = 2243, 0;
    u_equalrect = 2244, 8;
    u_getcursor = 2247, 0;
    u_getopenclipboardwindow = 2248, 0;
    u_getasynckeystate = 2249, 2;
    u_getmenustate = 2250, 6;
    u_mapwindowpoints = 2258, 10;
    u_begindeferwindowpos = 2259, 2;
    u_deferwindowpos = 2260, 16;
    u_enddeferwindowpos = 2261, 2;
    u_getwindow = 2262, 4;
    u_getmenuitemcount = 2263, 2;
    u_getmenuitemid = 2264, 4;
    u_showownedpopups = 2265, 4;
    u_setmessagequeue = 2266, 2;
    u_showscrollbar = 2267, 6;
    u_globaladdatom = 2268, 4;
    u_globaldeleteatom = 2269, 2;
    u_globalfindatom = 2270, 4;
    u_globalgetatomname = 2271, 8;
    u_iszoomed = 2272, 2;
    u_getdlgctrlid = 2277, 2;
    u_getdesktophwnd = 2278, 0;
    u_selectpalette = 2282, 6;
    u_realizepalette = 2283, 2;
    u_getfreesystemresources = 2284, 2;
    u_getdesktopwindow = 2286, 0;
    u_getlastactivepopup = 2287, 2;
    u_getmessageextrainfo = 2288, 0;
    u_redrawwindow = 2290, 10;
    u_setwindowshookex = 2291, 10;
    u_unhookwindowshookex = 2292, 4;
    u_callnexthookex = 2293, 12;
    u_lockwindowupdate = 2294, 2;
    u_defdlgproc = 2308, 10;
    u_getqueuestatus = 2334, 2;
    u_getinputstate = 2335, 0;
    u_ismenu = 2358, 2;
    u_getdcex = 2359, 8;
    u_copyicon = 2368, 4;
    u_copycursor = 2369, 4;
    u_getwindowplacement = 2370, 6;
    u_setwindowplacement = 2371, 6;
    u_subtractrect = 2373, 12;
    u_unregisterclass = 2403, 6;
    u_getclassinfo = 2404, 10;
    u_createcursor = 2406, 18;
    u_createicon = 2407, 18;
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
    u_defframeproc = 2445, 12;
    u_defmdichildproc = 2447, 10;
    u_translatemdisysaccel = 2451, 6;
    u_createwindowex = 2452, 34;
    u_adjustwindowrectex = 2454, 14;
    u_destroyicon = 2457, 2;
    u_destroycursor = 2458, 2;
    u_drawfocusrect = 2466, 6;
    u_lstrcmpi = 2471, 8;
    u_ansinext = 2472, 4;
    u_ansiprev = 2473, 8;
    u_enablescrollbar = 2482, 6;
    u_systemparametersinfo = 2483, 10;
    u_wnetgetconnection = 2512, 12;
    u_wnetgetcaps = 2513, 2;
    u_wnetgetuser = 2516, 8;
    u_wnetaddconnection = 2517, 12;
    u_wnetcancelconnection = 2518, 6;
    u_wnetgeterror = 2519, 4;
    u_wnetgeterrortext = 2520, 10;
    u_wnetgetdirectorytype = 2530, 8;
    // GDI
    g_setbkcolor = 3001, 6;
    g_setbkmode = 3002, 4;
    g_setmapmode = 3003, 4;
    g_setrop2 = 3004, 4;
    g_setpolyfillmode = 3006, 4;
    g_setstretchbltmode = 3007, 4;
    g_settextcharacterextra = 3008, 4;
    g_settextcolor = 3009, 6;
    g_settextjustification = 3010, 6;
    g_setwindoworg = 3011, 6;
    g_setwindowext = 3012, 6;
    g_setviewportorg = 3013, 6;
    g_setviewportext = 3014, 6;
    g_offsetwindoworg = 3015, 6;
    g_scalewindowext = 3016, 10;
    g_offsetviewportorg = 3017, 6;
    g_scaleviewportext = 3018, 10;
    g_lineto = 3019, 6;
    g_moveto = 3020, 6;
    g_excludecliprect = 3021, 10;
    g_intersectcliprect = 3022, 10;
    g_arc = 3023, 18;
    g_ellipse = 3024, 10;
    g_floodfill = 3025, 10;
    g_pie = 3026, 18;
    g_rectangle = 3027, 10;
    g_roundrect = 3028, 14;
    g_patblt = 3029, 14;
    g_savedc = 3030, 2;
    g_setpixel = 3031, 10;
    g_offsetcliprgn = 3032, 6;
    g_textout = 3033, 12;
    g_bitblt = 3034, 20;
    g_stretchblt = 3035, 24;
    g_polygon = 3036, 8;
    g_polyline = 3037, 8;
    g_restoredc = 3039, 4;
    g_fillrgn = 3040, 6;
    g_framergn = 3041, 10;
    g_invertrgn = 3042, 4;
    g_paintrgn = 3043, 4;
    g_selectcliprgn = 3044, 4;
    g_selectobject = 3045, 4;
    g_combinergn = 3047, 8;
    g_createbitmap = 3048, 12;
    g_createbitmapindirect = 3049, 4;
    g_createbrushindirect = 3050, 4;
    g_createcompatiblebitmap = 3051, 6;
    g_createcompatibledc = 3052, 2;
    g_createdc = 3053, 16;
    g_createellipticrgn = 3054, 8;
    g_createellipticrgnindirect = 3055, 4;
    g_createfont = 3056, 30;
    g_createfontindirect = 3057, 4;
    g_createhatchbrush = 3058, 6;
    g_createpatternbrush = 3060, 2;
    g_createpen = 3061, 8;
    g_createpenindirect = 3062, 4;
    g_createpolygonrgn = 3063, 8;
    g_createrectrgn = 3064, 8;
    g_createrectrgnindirect = 3065, 4;
    g_createsolidbrush = 3066, 4;
    g_dptolp = 3067, 8;
    g_deletedc = 3068, 2;
    g_deleteobject = 3069, 2;
    g_enumfonts = 3070, 14;
    g_equalrgn = 3072, 4;
    g_getbitmapbits = 3074, 10;
    g_getbkcolor = 3075, 2;
    g_getbkmode = 3076, 2;
    g_getclipbox = 3077, 6;
    g_getcurrentposition = 3078, 2;
    g_getdcorg = 3079, 2;
    g_getdevicecaps = 3080, 4;
    g_getmapmode = 3081, 2;
    g_getobject = 3082, 8;
    g_getpixel = 3083, 6;
    g_getpolyfillmode = 3084, 2;
    g_getrop2 = 3085, 2;
    g_getstockobject = 3087, 2;
    g_getstretchbltmode = 3088, 2;
    g_gettextcharacterextra = 3089, 2;
    g_gettextcolor = 3090, 2;
    g_gettextextent = 3091, 8;
    g_gettextface = 3092, 8;
    g_gettextmetrics = 3093, 6;
    g_getviewportext = 3094, 2;
    g_getviewportorg = 3095, 2;
    g_getwindowext = 3096, 2;
    g_getwindoworg = 3097, 2;
    g_lptodp = 3099, 8;
    g_linedda = 3100, 16;
    g_offsetrgn = 3101, 6;
    g_ptvisible = 3103, 6;
    g_rectvisible = 3104, 6;
    g_setbitmapbits = 3106, 10;
    g_muldiv = 3128, 6;
    g_getrgnbox = 3134, 6;
    g_setbrushorg = 3148, 6;
    g_unrealizeobject = 3150, 2;
    g_createic = 3153, 16;
    g_getnearestcolor = 3154, 6;
    g_creatediscardablebitmap = 3156, 6;
    g_ptinregion = 3161, 6;
    g_getbitmapdimension = 3162, 2;
    g_setbitmapdimension = 3163, 6;
    g_setrectrgn = 3172, 10;
    g_rectinregion = 3181, 6;
    g_getcharabcwidths = 3307, 10;
    g_getrasterizercaps = 3313, 6;
    g_enumfontfamilies = 3330, 14;
    g_gettextalign = 3345, 2;
    g_settextalign = 3346, 4;
    g_chord = 3348, 18;
    g_getcharwidth = 3350, 10;
    g_exttextout = 3351, 22;
    g_createpalette = 3360, 4;
    g_getpaletteentries = 3363, 10;
    g_setpaletteentries = 3364, 10;
    g_updatecolors = 3366, 2;
    g_animatepalette = 3367, 10;
    g_resizepalette = 3368, 4;
    g_getnearestpaletteindex = 3370, 6;
    g_extfloodfill = 3372, 12;
    g_setsystempaletteuse = 3373, 4;
    g_getsystempaletteuse = 3374, 2;
    g_getsystempaletteentries = 3375, 10;
    g_stretchdibits = 3439, 32;
    g_setdibits = 3440, 18;
    g_getdibits = 3441, 18;
    g_createdibitmap = 3442, 20;
    g_setdibitstodevice = 3443, 28;
    g_createroundrectrgn = 3444, 12;
    g_createdibpatternbrush = 3445, 4;
    g_polypolygon = 3450, 12;
    g_createpolypolygonrgn = 3451, 12;
    g_isgdiobject = 3462, 2;
    g_getbitmapdimensionex = 3468, 6;
    g_getbrushorgex = 3469, 6;
    g_getcurrentpositionex = 3470, 6;
    g_gettextextentpoint = 3471, 12;
    g_getviewportextex = 3472, 6;
    g_getviewportorgex = 3473, 6;
    g_getwindowextex = 3474, 6;
    g_getwindoworgex = 3475, 6;
    g_offsetviewportorgex = 3476, 10;
    g_offsetwindoworgex = 3477, 10;
    g_setbitmapdimensionex = 3478, 10;
    g_setviewportextex = 3479, 10;
    g_setviewportorgex = 3480, 10;
    g_setwindowextex = 3481, 10;
    g_setwindoworgex = 3482, 10;
    g_movetoex = 3483, 10;
    g_scaleviewportextex = 3484, 14;
    g_scalewindowextex = 3485, 14;
    g_getaspectratiofilterex = 3486, 6;
    // KEYBOARD
    kb_ansitooem = 4005, 8;
    kb_oemtoansi = 4006, 8;
    kb_getkeyboardtype = 4130, 2;
    kb_getkbcodepage = 4132, 0;
    kb_ansitooembuff = 4134, 10;
    kb_oemtoansibuff = 4135, 10;
    // TOOLHELP
    th_globalhandletosel = 6250, 2;
    th_globalfirst = 6251, 6;
    th_globalnext = 6252, 6;
    th_globalinfo = 6253, 4;
    th_globalentryhandle = 6254, 6;
    th_globalentrymodule = 6255, 8;
    th_localinfo = 6256, 6;
    th_localfirst = 6257, 6;
    th_localnext = 6258, 4;
    th_modulefirst = 6259, 4;
    th_modulenext = 6260, 4;
    th_modulefindname = 6261, 8;
    th_modulefindhandle = 6262, 6;
    th_taskfirst = 6263, 4;
    th_tasknext = 6264, 4;
    th_taskfindhandle = 6265, 6;
    th_stacktracefirst = 6266, 6;
    th_stacktracecsipfirst = 6267, 12;
    th_stacktracenext = 6268, 4;
    th_classfirst = 6269, 4;
    th_classnext = 6270, 4;
    th_systemheapinfo = 6271, 4;
    th_memmaninfo = 6272, 4;
    th_notifyregister = 6273, 8;
    th_notifyunregister = 6274, 2;
    th_interruptregister = 6275, 6;
    th_interruptunregister = 6276, 2;
    th_terminateapp = 6277, 4;
    th_memoryread = 6278, 14;
    th_memorywrite = 6279, 14;
    th_timercount = 6280, 4;
    th_tasksetcsip = 6281, 6;
    th_taskgetcsip = 6282, 2;
    th_taskswitch = 6283, 6;
    // COMPOBJ, OLE2
    co_coinitialize = 7002, 4;
    co_couninitialize = 7003, 0;
    co_cogetmalloc = 7004, 8;
    co_isequalguid = 7018, 8;
    ol_oleinitialize = 7102, 4;
    ol_oleuninitialize = 7103, 0;
    // VER
    v_getfileresourcesize = 6302, 16;
    v_getfileresource = 6303, 24;
    v_getfileversioninfosize = 6306, 8;
    v_getfileversioninfo = 6307, 16;
    v_verfindfile = 6308, 30;
    v_verinstallfile = 6309, 30;
    v_verlanguagename = 6310, 8;
    v_verqueryvalue = 6311, 16;
    // LZEXPAND
    lz_lzcopy = 6401, 4;
    lz_lzopenfile = 6402, 10;
    lz_lzinit = 6403, 2;
    lz_lzseek = 6404, 8;
    lz_lzread = 6405, 8;
    lz_lzclose = 6406, 2;
    lz_lzstart = 6407, 0;
    lz_copylzfile = 6408, 4;
    lz_lzdone = 6409, 0;
    lz_getexpandedname = 6410, 8;
    // SHELL
    sh_regopenkey = 6501, 12;
    sh_regcreatekey = 6502, 12;
    sh_regclosekey = 6503, 4;
    sh_regdeletekey = 6504, 8;
    sh_regsetvalue = 6505, 20;
    sh_regqueryvalue = 6506, 16;
    sh_regenumkey = 6507, 16;
    sh_dragacceptfiles = 6509, 4;
    sh_shellexecute = 6520, 20;
    sh_extracticon = 6534, 8;
    // Stage 3: printing (GDI, print.rs), COMMDLG (commdlg.rs), DDEML
    // (ddeml.rs).
    g_escape = 3038, 14;
    g_queryabort = 3155, 4;
    g_startdoc = 3377, 6;
    g_enddoc = 3378, 2;
    g_startpage = 3379, 2;
    g_endpage = 3380, 2;
    g_setabortproc = 3381, 6;
    g_abortdoc = 3382, 2;
    cd_getopenfilename = 6001, 4;
    cd_getsavefilename = 6002, 4;
    cd_choosecolor = 6005, 4;
    cd_findtext = 6011, 4;
    cd_replacetext = 6012, 4;
    cd_choosefont = 6015, 4;
    cd_printdlg = 6020, 4;
    cd_commdlgextendederror = 6026, 0;
    cd_getfiletitle = 6027, 10;
    dm_ddeinitialize = 6102, 16;
    dm_ddeuninitialize = 6103, 4;
    dm_ddeconnectlist = 6104, 20;
    dm_ddequerynextserver = 6105, 8;
    dm_ddedisconnectlist = 6106, 4;
    dm_ddeconnect = 6107, 16;
    dm_ddedisconnect = 6108, 4;
    dm_ddequeryconvinfo = 6109, 12;
    dm_ddesetuserhandle = 6110, 12;
    dm_ddeclienttransaction = 6111, 28;
    dm_ddeabandontransaction = 6112, 12;
    dm_ddepostadvise = 6113, 12;
    dm_ddecreatedatahandle = 6114, 24;
    dm_ddeadddata = 6115, 16;
    dm_ddegetdata = 6116, 16;
    dm_ddeaccessdata = 6117, 8;
    dm_ddeunaccessdata = 6118, 4;
    dm_ddefreedatahandle = 6119, 4;
    dm_ddegetlasterror = 6120, 4;
    dm_ddecreatestringhandle = 6121, 10;
    dm_ddefreestringhandle = 6122, 8;
    dm_ddequerystring = 6123, 18;
    dm_ddekeepstringhandle = 6124, 8;
    dm_ddeenablecallback = 6126, 10;
    dm_ddenameservice = 6127, 14;
    dm_ddecmpstringhandles = 6136, 8;
    dm_ddereconnect = 6137, 4;
}

extern "C" fn dispatch(idx: u32, args: *const u8) -> u32 {
    if !tracing() || idx == 1 {
        return dispatch_(idx, args);
    }
    // The arguments as words, first argument first (_pascal pushes them
    // left to right), then the result.
    let n = arg_size(idx) / 2;
    let mut s = alloc::string::String::new();
    let words: alloc::vec::Vec<u16> = (0..n).rev().map(|i| unsafe { (args.add(2 * i) as *const u16).read_unaligned() }).collect();
    for w in &words {
        let _ = core::fmt::write(&mut s, format_args!(" {w:04x}"));
    }
    // The strings of calls that name files, modules and resources (the
    // word index of each far pointer's selector).
    let strings: &[usize] = match idx {
        1045 | 1047 | 1074 | 1083 | 1085 | 1095 | 1166 => &[0],
        1050 => &[1],
        1060 => &[1, 3],
        2001 => &[1, 3],
        2050 => &[0, 2],
        6121 => &[2],
        _ => &[],
    };
    for &k in strings {
        if let (Some(&sel), Some(&off)) = (words.get(k), words.get(k + 1)) {
            if sel != 0 {
                if let Some(t) = far_str((sel as u32) << 16 | off as u32) {
                    let _ = core::fmt::write(&mut s, format_args!(" \"{}\"", t.iter().take(80).map(|&b| b as char).collect::<alloc::string::String>()));
                }
            }
        }
    }
    trace(format_args!("W16 {}{}", idx, s));
    let r = dispatch_(idx, args);
    trace(format_args!("W16 {} -> {:x}", idx, r));
    r
}

fn dispatch_(idx: u32, args: *const u8) -> u32 {
    let size = arg_size(idx);
    let mut a = Pascal::new(args, size);
    // Printers' DCs and the printing calls (before bitmap.rs's display
    // DCs).
    if let Some(r) = print::call(idx, &mut Pascal::new(args, size)) {
        return r;
    }
    if let Some(r) = toolhelp::call(idx, &mut a) {
        return r;
    }
    if let Some(r) = bitmap::call(idx, &mut a).or_else(|| region::call(idx, &mut a)).or_else(|| atom::call(idx, &mut a)).or_else(|| compat::call(idx, &mut a)).or_else(|| icon::call(idx, &mut a)).or_else(|| mdi::call(idx, &mut a))
        .or_else(|| draw::call(idx, &mut a))
        .or_else(|| lz::call(idx, &mut a))
        .or_else(|| ver::call(idx, &mut a))
        .or_else(|| reg::call(idx, &mut a))
        .or_else(|| ole::call(idx, &mut a))
    {
        return r;
    }
    match idx {
        1 => {
            dos::interrupt(thunk16::flat(a.l()) as *mut u16);
            0
        }
        2..=12 => {
            let (h, m, wp, lp) = (a.w(), a.w(), a.w(), a.l());
            native(idx, h, m, wp, lp)
        }
        1000..2000 => kernel::call(idx - 1000, &mut a),
        2000..3000 => user::call(idx - 2000, &mut a),
        3000..4000 => gdi::call(idx - 3000, &mut a),
        4000..5000 => kernel::keyboard(idx - 4000, &mut a),
        6000..6100 => commdlg::call(idx - 6000, &mut a),
        6100..6200 => ddeml::call(idx - 6100, &mut a),
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
/// 2-8), MDICLIENT's (9), the Program Manager stand-in's (10), the
/// common dialogs' (11), DDEML's (12),
/// DefWindowProc (2107) or DefDlgProc (2308).
pub fn native(idx: u32, h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    *NATIVE_DEPTH.get() += 1;
    let r = match idx {
        2..=8 => ctl::proc_(idx, h, m, wp, lp),
        9 => mdi::client_proc(h, m, wp, lp),
        11 => commdlg::dlg_proc(h, m, wp, lp),
        12 => ddeml::wnd_proc(h, m, wp, lp),
        10 => progman::wnd_proc(h, m, wp, lp),
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
