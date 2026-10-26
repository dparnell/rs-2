//! Modules: the program and the Windows DLLs it uses (implicitly, or
//! with LoadLibrary). A Windows DLL is loaded by rs2's loader as an NE
//! module of the process (docs/WIN16.md, "Windows DLLs"); here it has
//! its instance handle (its DGROUP selector, as on Windows) and its file,
//! from which its resources are read.
//!
//! Windows tells modules and instances apart (the module database's
//! selector, DGROUP); one task per process makes them one thing here: a
//! module's handle is its instance handle, and either finds it.

use alloc::vec::Vec;

use crate::pm::trace;
use crate::{cstr, far_str, Global};

pub struct Module {
    /// The instance handle: DGROUP's selector (a module without data: its
    /// first segment's); for a DLL, a second selector for DGROUP below
    /// 8000h (programs compare instance handles as signed numbers, and a
    /// DLL's own selectors are near the top of the LDT).
    pub hinst: u16,
    /// DGROUP's own selector.
    pub dgroup: u16,
    /// The OS/2 module handle.
    pub hmod: u32,
    /// The base name, upper case ("CTL3DV2").
    pub name: Vec<u8>,
    /// LoadLibrary calls not yet freed.
    pub loads: u32,
    /// The file, once a resource is needed.
    pub image: Option<crate::res::Image>,
}

static MODULES: Global<Vec<Module>> = Global::new(Vec::new());

/// The system modules WIN16 implements: (name, handle, WIN16 ordinal
/// base). Their handles are fixed selectors no program has.
pub const SYSTEM: &[(&[u8], u16, u32)] = &[
    (b"KERNEL", 0x0f07, 1000),
    (b"USER", 0x0f0f, 2000),
    (b"GDI", 0x0f17, 3000),
    (b"KEYBOARD", 0x0f1f, 4000),
    (b"WIN87EM", 0x0f27, 5000),
    (b"SOUND", 0x0f2f, 5200),
    (b"SYSTEM", 0x0f37, 5400),
    (b"COMMDLG", 0x0f3f, 6000),
    (b"DDEML", 0x0f47, 6100),
    (b"TOOLHELP", 0x0f4f, 6200),
    (b"VER", 0x0f57, 6300),
    (b"LZEXPAND", 0x0f5f, 6400),
    (b"SHELL", 0x0f67, 6500),
    (b"COMPOBJ", 0x0f77, 7000),
    (b"OLE2", 0x0f7f, 7100),
    (b"STORAGE", 0x0f87, 7300),
    (b"OLE2DISP", 0x0f8f, 7400),
    (b"OLE2NLS", 0x0f97, 7600),
    (b"TYPELIB", 0x0f9f, 7700),
    (b"MMSYSTEM", 0x0f6f, 10000),
];

pub fn system(h: u16) -> Option<&'static (&'static [u8], u16, u32)> {
    SYSTEM.iter().find(|m| m.1 == h)
}

/// "C:\DIR\NAME.EXT" -> "NAME" (upper case).
pub fn base_name(path: &[u8]) -> Vec<u8> {
    let b = path.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(path);
    b.split(|&c| c == b'.').next().unwrap_or(b).to_ascii_uppercase()
}

/// The program: registered by InitTask.
pub fn register_exe(hinst: u16) {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    if pib.is_null() {
        return;
    }
    let hmod = unsafe { (*pib).hmte };
    let name = path_of(hmod).map(|p| base_name(&p)).unwrap_or_default();
    MODULES.get().insert(0, Module { hinst, dgroup: hinst, hmod, name, loads: 0, image: None });
}

/// The program's instance.
pub fn exe() -> u16 {
    MODULES.get().first().map_or(0, |m| m.hinst)
}

/// The module with this instance or module handle (0: the program).
pub fn find(h: u16) -> Option<&'static mut Module> {
    let ms = MODULES.get();
    if h == 0 {
        return ms.first_mut();
    }
    if let Some(i) = ms.iter().position(|m| m.hinst == h || (m.hinst | 7) == (h | 7) || (m.dgroup | 7) == (h | 7)) {
        return MODULES.get().get_mut(i);
    }
    // A DLL loaded with the program (not through LoadLibrary): the module
    // whose segment the handle is.
    if system(h).is_some() {
        return None;
    }
    let hmod = unsafe { crate::pm::Rs2Win16Selector(8, h as u32, 0) };
    if hmod == 0 {
        return None;
    }
    register(hmod);
    MODULES.get().iter_mut().find(|m| m.hmod == hmod)
}

/// The file name of an OS/2 module.
pub fn path_of(hmod: u32) -> Option<Vec<u8>> {
    let mut path = [0u8; 260];
    if unsafe { crate::pm::DosQueryModuleName(hmod, 260, path.as_mut_ptr()) } != 0 {
        return None;
    }
    Some(unsafe { cstr(path.as_ptr()) }.to_vec())
}

/// The instance handle of a loaded OS/2 module (registering it): its
/// automatic data segment's selector.
fn register(hmod: u32) -> u16 {
    let ms = MODULES.get();
    if let Some(m) = ms.iter().find(|m| m.hmod == hmod) {
        return m.hinst;
    }
    let Some(path) = path_of(hmod) else { return 0 };
    // The NE header's automatic data segment number.
    let mut seg = 1;
    if let Some(head) = read_file(&path, 0x400) {
        let ne = crate::res::l32(&head, 0x3c) as usize;
        if crate::res::w16(&head, ne) == 0x454e && crate::res::w16(&head, ne + 0x0e) != 0 {
            seg = crate::res::w16(&head, ne + 0x0e) as u32;
        }
    }
    let dgroup = unsafe { crate::pm::Rs2Win16Selector(6, hmod, seg) } as u16;
    if dgroup == 0 {
        return 0;
    }
    let alias = if dgroup >= 0x8000 { unsafe { crate::pm::Rs2Win16Selector(0, dgroup as u32, 1) as u16 } } else { 0 };
    let hinst = if alias != 0 && alias < 0x8000 { alias } else { dgroup };
    ms.push(Module { hinst, dgroup, hmod, name: base_name(&path), loads: 0, image: None });
    hinst
}

/// GetModuleHandle(name): the system modules', the program's, or a loaded
/// DLL's (by its module name).
pub fn handle(name: u32) -> u16 {
    let Some(n) = far_str(name) else {
        return find(name as u16).map_or(0, |m| m.hinst);
    };
    let base = base_name(n);
    if let Some(m) = SYSTEM.iter().find(|m| m.0 == &base[..]) {
        return m.1;
    }
    if let Some(m) = MODULES.get().iter().find(|m| m.name == base) {
        return m.hinst;
    }
    // A DLL the program imports: loaded with it.
    let mut z = base.clone();
    z.push(0);
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(z.as_ptr(), &mut hmod) } == 0 {
        return register(hmod);
    }
    0
}

/// LoadLibrary(name): the instance handle, or an error below 32 (2: file
/// not found).
pub fn load_library(name: u32) -> u16 {
    let Some(n) = far_str(name) else { return 2 };
    let base = base_name(n);
    if let Some(m) = SYSTEM.iter().find(|m| m.0 == &base[..]) {
        return m.1;
    }
    if let Some(m) = MODULES.get().iter_mut().find(|m| m.name == base) {
        m.loads += 1;
        return m.hinst;
    }
    // Windows looks in the current directory, the Windows directories,
    // the program's directory and the PATH; OS/2 takes a name with a dot
    // as a file name, and looks for a bare module name along LIBPATH.
    let mut tries: Vec<Vec<u8>> = Vec::new();
    let has_dir = n.iter().any(|&c| c == b'\\' || c == b':');
    let with_ext = if n.contains(&b'.') { n.to_vec() } else { [n, b".DLL"].concat() };
    tries.push(with_ext.clone());
    if !has_dir {
        if let Some(dir) = find(0).and_then(|m| path_of(m.hmod)) {
            let d = &dir[..dir.iter().rposition(|&c| c == b'\\').map_or(0, |i| i + 1)];
            tries.push([d, &with_ext].concat());
        }
        for d in [crate::profile::SYSTEM_DIR, crate::profile::WINDOWS_DIR] {
            tries.push([d, b"\\", &with_ext].concat());
        }
        tries.push(base.clone());
    }
    for t in &tries {
        let mut z = t.clone();
        z.push(0);
        let mut hmod = 0u32;
        let mut fail = [0u8; 64];
        let r = unsafe { os2::DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, z.as_ptr(), &mut hmod) };
        if r == 0 {
            let h = register(hmod);
            if let Some(m) = MODULES.get().iter_mut().find(|m| m.hmod == hmod) {
                m.loads += 1;
            }
            if h != 0 {
                return h;
            }
        }
    }
    if crate::tracing() {
        trace(format_args!("W16 LoadLibrary({}) failed", n.iter().map(|&c| c as char).collect::<alloc::string::String>()));
    }
    2
}

/// FreeLibrary(h)
pub fn free_library(h: u16) {
    let ms = MODULES.get();
    let Some(i) = ms.iter().position(|m| m.hinst == h) else { return };
    if i == 0 || ms[i].loads == 0 {
        return;
    }
    ms[i].loads -= 1;
    if ms[i].loads == 0 {
        let (hmod, hinst, dgroup) = (ms[i].hmod, ms[i].hinst, ms[i].dgroup);
        ms.remove(i);
        if hinst != dgroup {
            unsafe { crate::pm::Rs2Win16Selector(4, hinst as u32, 0) };
        }
        unsafe { os2::DosFreeModule(hmod) };
    }
}

/// GetProcAddress(h, name or ordinal) in a DLL (the system modules' are
/// WIN16's, kernel.rs).
pub fn proc_address(h: u16, name: u32) -> u32 {
    let Some(m) = find(h) else { return 0 };
    let hmod = m.hmod;
    let mut f = 0usize;
    let r = match far_str(name) {
        Some(n) => {
            let mut z = n.to_ascii_uppercase();
            z.push(0);
            let r = unsafe { os2::DosQueryProcAddr(hmod, 0, z.as_ptr(), &mut f) };
            if r != 0 {
                let mut z = n.to_vec();
                z.push(0);
                unsafe { os2::DosQueryProcAddr(hmod, 0, z.as_ptr(), &mut f) }
            } else {
                r
            }
        }
        None => unsafe { os2::DosQueryProcAddr(hmod, name & 0xffff, core::ptr::null(), &mut f) },
    };
    // A 16-bit module's procedures come back as 16:16 addresses.
    if r != 0 { 0 } else { f as u32 }
}

/// The whole file (or its first `max` bytes).
pub fn read_file(path: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut z = path.to_vec();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut file = Vec::new();
    let mut buf = alloc::vec![0u8; 0x8000];
    while file.len() < max {
        let mut n = 0u32;
        let want = buf.len().min(max - file.len()) as u32;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), want, &mut n) } != 0 || n == 0 {
            break;
        }
        file.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(file)
}
