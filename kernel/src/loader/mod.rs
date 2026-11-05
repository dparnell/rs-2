//! Module loader: executables and dynamic link libraries.
//!
//! A DLL is loaded once system-wide. Its objects are placed in the shared
//! arena so they have the same address in every process; fixups are applied
//! once to a pristine image. Shared objects (code, read-only and `SHARED`
//! data) are backed by one [`SharedMem`]; every process that attaches gets a
//! private copy of the pristine instance data at the same address.
//!
//! Executables are loaded into the private arena of their process.

pub mod gate;
mod lxload;
mod neload;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::SpinLock;
use crate::vm::{AddressSpace, SegKind, SharedMem};

/// OS/2 error codes used by the loader.
pub mod err {
    pub const FILE_NOT_FOUND: u32 = 2;
    pub const PATH_NOT_FOUND: u32 = 3;
    pub const NOT_ENOUGH_MEMORY: u32 = 8;
    pub const BAD_FORMAT: u32 = 11;
    pub const INVALID_ORDINAL: u32 = 182;
    pub const INVALID_MODULETYPE: u32 = 190;
    pub const INVALID_EXE_SIGNATURE: u32 = 191;
    pub const EXE_MARKED_INVALID: u32 = 192;
    pub const BAD_EXE_FORMAT: u32 = 193;
    pub const INVALID_NAME: u32 = 123;
    pub const MOD_NOT_FOUND: u32 = 126;
    pub const PROC_NOT_FOUND: u32 = 127;
    pub const INVALID_SEGMENT_NUMBER: u32 = 180;
    pub const RELOC_CHAIN_XEEDS_SEGLIM: u32 = 201;
}

/// Where an exported entry point lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    /// Flat linear address.
    pub flat: u32,
    /// 16:16 form (tiled alias for 32-bit targets).
    pub sel: u16,
    pub off: u16,
    /// Selector for a 16:32 pointer (flat code or data selector).
    pub flat_sel: u16,
    /// The target is in a 16-bit segment.
    pub is16: bool,
}

impl Target {
    pub fn flat32(addr: u32, code: bool) -> Self {
        Target {
            flat: addr,
            sel: crate::vm::tile_selector(addr),
            off: addr as u16,
            flat_sel: if code { crate::arch::gdt::USER_CS } else { crate::arch::gdt::USER_DS },
            is16: false,
        }
    }

    pub fn far16(sel: u16, off: u16, flat: u32) -> Self {
        Target { flat, sel, off, flat_sel: crate::arch::gdt::USER_CS, is16: true }
    }

    pub fn constant(v: u16) -> Self {
        Target { flat: v as u32, sel: 0, off: v, flat_sel: 0, is16: true }
    }

    /// A 32-bit absolute value (an entry in no object).
    pub fn absolute(v: u32) -> Self {
        Target { flat: v, sel: 0, off: v as u16, flat_sel: 0, is16: false }
    }
}

#[derive(Clone, Debug)]
pub enum Export {
    None,
    At(Target),
    /// Forwarded to another module by ordinal or by name.
    ForwardOrdinal { module: String, ordinal: u32 },
    ForwardName { module: String, name: String },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Implemented by the kernel through the API gate.
    Virtual,
    Lx,
    Ne,
}

/// A loaded object (LX) or segment (NE).
pub struct Object {
    pub base: u32,
    pub size: u32,
    /// Protection flags (`vm::pag::*`).
    pub prot: u32,
    pub seg: SegKind,
    pub is16: bool,
    pub code: bool,
    /// Explicit segment limit for 16-bit segments.
    pub limit: Option<u32>,
    /// Shared backing (DLL shared objects).
    pub shared: Option<Arc<SharedMem>>,
    /// Fixed-up initial contents for per-process instance data (and for
    /// writable shared data, see `load_global`).
    pub pristine: Option<Vec<u8>>,
}

impl Object {
    pub fn selector(&self) -> u16 {
        crate::vm::tile_selector(self.base)
    }
}

/// Where execution starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Start {
    pub entry: Target,
    pub stack: Target,
    /// Auto data segment / object (0-based index), if any.
    pub autodata: Option<usize>,
    /// Size of the automatic data segment (16-bit), 0 = 64K.
    pub autodata_size: u32,
    /// A Windows program (docs/WIN16.md): its local heap and stack sizes,
    /// which it gets in CX and BX at the start for InitTask.
    pub windows: Option<(u16, u16)>,
}

pub struct Module {
    pub hmte: u16,
    /// Module name (upper case), as used in import tables.
    pub name: String,
    /// Full path of the file.
    pub path: String,
    /// Loaded by its path (OS/2's CLASS_SPECIFIC): imports and loads by
    /// name do not find it. Python's `lib-dynload\\bz2.pyd` has the internal
    /// name of the `BZ2.DLL` it imports.
    pub specific: bool,
    pub format: Format,
    pub is_dll: bool,
    pub is16: bool,
    /// `lx::mflags` / `ne::flags` translated to an application type.
    pub app_type: u32,
    pub objects: Vec<Object>,
    /// Index `ordinal - 1`.
    pub exports: Vec<Export>,
    pub names: BTreeMap<String, u16>,
    pub imports: Vec<Arc<Module>>,
    /// Modules this one imports that were still loading when it was
    /// loaded (circular references, see [`Pending`]): added once they are
    /// loaded. [`Module::deps`] is both lists.
    pub cyclic: SpinLock<Vec<Arc<Module>>>,
    pub start: Option<Start>,
    /// DLL initialisation entry, if the module wants it.
    pub init: Option<Start>,
    pub per_process_init: bool,
    /// LX: call the init/term routine with flag 1 when each process ends.
    pub per_process_term: bool,
    /// Global (once per system) initialisation has run.
    pub global_init_done: core::sync::atomic::AtomicBool,
    /// Kept for resources.
    pub image: Option<Arc<[u8]>>,
}

impl Module {
    /// The modules this one imports: its import table's, and those of a
    /// circular reference that were added later.
    pub fn deps(&self) -> Vec<Arc<Module>> {
        let mut v = self.imports.clone();
        v.extend(self.cyclic.lock().iter().cloned());
        v
    }

    /// Resolve an export by ordinal, following forwarders.
    pub fn resolve_ordinal(&self, ordinal: u32, depth: u32) -> Result<Target, u32> {
        if self.format == Format::Virtual {
            let m = gate::module_index(&self.name).ok_or(err::INVALID_ORDINAL)?;
            if ordinal == 0 || ordinal >= gate::ORDINALS {
                return Err(err::INVALID_ORDINAL);
            }
            // Absolute exports: THK32R3DS (295) and DOS32FLATDS (370), the
            // flat data selector used by 16->32 thunks.
            if m == 0 && (ordinal == 295 || ordinal == 370) {
                return Ok(Target::constant(crate::arch::gdt::USER_DS));
            }
            // DOSHUGESHIFT (135) and DOSHUGEINCR (136): tiled selectors of
            // a huge segment are 8 apart.
            if m == 0 && ordinal == 135 {
                return Ok(Target::constant(3));
            }
            if m == 0 && ordinal == 136 {
                return Ok(Target::constant(8));
            }
            // DOSCALLS' other constants (the "?" imports of kLIBC's
            // dos-data.imp): DosMaxPathLen (214), DosPageSize (215), the
            // local (216) and global (217) information segments' tiled
            // selectors (programs turn them into flat pointers),
            // DosFlatCS (369) and DOS32TIB (419), the TIB selector.
            if m == 0 {
                let v = match ordinal {
                    214 => Some(260),
                    215 => Some(crate::vm::PAGE as u16),
                    // Each thread's own copy (mm::tlm), so that tidCurrent
                    // is right with threads on several CPUs.
                    216 => Some(crate::vm::tile_selector(crate::mm::tlm::LIS_BASE)),
                    217 => Some(crate::vm::tile_selector(crate::infoseg::base())),
                    369 => Some(crate::arch::gdt::USER_CS),
                    419 => Some(crate::arch::gdt::TIB_SEL),
                    _ => None,
                };
                if let Some(v) = v {
                    return Ok(Target::constant(v));
                }
            }
            let a = gate::slot_address(m, ordinal);
            // Kernel APIs reachable both flat and 16:16.
            let mut t = Target::flat32(a, true);
            t.is16 = crate::api::is_16bit(m, ordinal);
            return Ok(t);
        }
        match self.exports.get(ordinal.wrapping_sub(1) as usize) {
            Some(Export::At(t)) => Ok(*t),
            // A forwarder to a DLL still loading (MMPM/2's HHP forwards
            // everything to MMPM, which imports it through FILT): its
            // pending exports.
            Some(Export::ForwardOrdinal { module, ordinal }) if Pending::has(&module_name(module)) => {
                Pending::target(&module_name(module), ImportBy::Ordinal(*ordinal))
            }
            Some(Export::ForwardName { module, name }) if Pending::has(&module_name(module)) => {
                Pending::target(&module_name(module), ImportBy::Name(name))
            }
            Some(Export::ForwardOrdinal { module, ordinal }) if depth < 8 => {
                let m = load_global(module)?;
                m.resolve_ordinal(*ordinal, depth + 1)
            }
            Some(Export::ForwardName { module, name }) if depth < 8 => {
                let m = load_global(module)?;
                m.resolve_name(name, depth + 1)
            }
            _ => Err(err::INVALID_ORDINAL),
        }
    }

    pub fn resolve_name(&self, name: &str, depth: u32) -> Result<Target, u32> {
        let ord = self.ordinal_of(name).ok_or(err::PROC_NOT_FOUND)?;
        self.resolve_ordinal(ord as u32, depth)
    }

    pub fn ordinal_of(&self, name: &str) -> Option<u16> {
        if self.format == Format::Virtual {
            return crate::api::names::ordinal(&self.name, name);
        }
        self.names.get(name).copied().or_else(|| {
            // Import names are matched case-sensitively first, then without
            // case (16-bit names are upper case).
            self.names.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, &v)| v)
        })
    }
}

struct Registry {
    modules: Vec<Arc<Module>>,
    next_hmte: u16,
}

static REGISTRY: SpinLock<Registry> = SpinLock::new(Registry { modules: Vec::new(), next_hmte: 0x100 });

fn alloc_hmte() -> u16 {
    let mut r = REGISTRY.lock();
    // Handles of programs that ended are reused (after wrapping around).
    loop {
        let h = r.next_hmte;
        r.next_hmte = if h == 0xfff0 { 0x100 } else { h + 1 };
        if !r.modules.iter().any(|m| m.hmte == h) {
            return h;
        }
    }
}

pub fn find_loaded(name: &str) -> Option<Arc<Module>> {
    let r = REGISTRY.lock();
    let named = |m: &&Arc<Module>| m.name.eq_ignore_ascii_case(name);
    r.modules.iter().filter(named).find(|m| !m.specific).or(r.modules.iter().find(named)).cloned()
}

/// A loaded DLL by the path of its file.
pub fn find_loaded_path(path: &str) -> Option<Arc<Module>> {
    let path = path.replace('/', "\\");
    REGISTRY.lock().modules.iter().find(|m| m.is_dll && m.path.eq_ignore_ascii_case(&path)).cloned()
}

/// Every module loaded in the system (DosQuerySysState).
pub fn all_modules() -> Vec<Arc<Module>> {
    REGISTRY.lock().modules.clone()
}

pub fn by_handle(hmte: u16) -> Option<Arc<Module>> {
    REGISTRY.lock().modules.iter().find(|m| m.hmte == hmte).cloned()
}

fn register(m: Arc<Module>) {
    REGISTRY.lock().modules.push(m);
}

/// A process ended: its executable module goes away (DLLs stay loaded).
pub fn unregister_exe(m: &Arc<Module>) {
    if !m.is_dll {
        REGISTRY.lock().modules.retain(|x| !Arc::ptr_eq(x, m));
    }
}

pub fn init() {
    gate::init();
    // Thread-local memory's range, kept free in every address space.
    crate::vm::reserve_shared(crate::mm::tlm::TLM_SIZE, Some(crate::mm::tlm::TLM_BASE)).expect("thread-local memory range");
    for name in gate::MODULES {
        register(Arc::new(Module {
            hmte: alloc_hmte(),
            name: name.to_string(),
            path: alloc::format!("<kernel>\\{name}"),
            specific: false,
            format: Format::Virtual,
            is_dll: true,
            is16: false,
            app_type: 0,
            objects: Vec::new(),
            exports: Vec::new(),
            names: BTreeMap::new(),
            imports: Vec::new(),
            cyclic: SpinLock::new(Vec::new()),
            start: None,
            init: None,
            per_process_init: false,
            per_process_term: false,
            global_init_done: core::sync::atomic::AtomicBool::new(true),
            image: None,
        }));
    }
}

/// Module name from a path or import reference: base name without the
/// extension, upper case.
pub fn module_name(s: &str) -> String {
    let base = s.rsplit(['\\', '/', ':']).next().unwrap_or(s);
    let base = base.split('.').next().unwrap_or(base);
    let up = base.to_ascii_uppercase();
    // DOSCALL1.DLL is where OS/2 implements DOSCALLS; here both are the
    // kernel's API module.
    if up == "DOSCALL1" { String::from("DOSCALLS") } else { up }
}

/// Name of the module behind the last load failure (what DosLoadModule and
/// DosExecPgm report in their failure-name buffer).
static FAIL_NAME: SpinLock<Option<String>> = SpinLock::new(None);

pub fn set_fail(name: &str) {
    *FAIL_NAME.lock() = Some(module_name(name));
}

/// Take the recorded failing module name.
pub fn take_fail() -> Option<String> {
    FAIL_NAME.lock().take()
}

/// Load (or find) a DLL system-wide without attaching it anywhere. A
/// program of the same name is not it: OS/2's PMFORMAT.EXE loads
/// PMFORMAT.DLL for its resources by that name.
///
/// As in OS/2 (_ldrFindModule's classes), a name finds the modules that
/// were found by name, and a path the module of that file: a DLL loaded by
/// its path may have the name of another, and both stay loaded.
pub fn load_global(name: &str) -> Result<Arc<Module>, u32> {
    let mname = module_name(name);
    let by_path = name.contains(['\\', '/', ':']);
    let found = {
        let r = REGISTRY.lock();
        let named = |m: &&Arc<Module>| m.is_dll && m.name.eq_ignore_ascii_case(&mname);
        let m = if by_path {
            // The kernel's API modules have no file.
            r.modules.iter().filter(named).find(|m| m.format == Format::Virtual)
        } else {
            r.modules.iter().filter(named).find(|m| !m.specific)
        };
        m.cloned()
    }
    .or_else(|| if by_path { find_loaded_path(name) } else { None });
    if let Some(m) = found {
        if rewritten(&m) {
            // A 16-bit DLL whose file was written since it was loaded (Delphi
            // rebuilds its component library COMPLIB.DCL, frees it and loads
            // it again): the new file, under a new handle. Programs using the
            // old one keep it.
            REGISTRY.lock().modules.retain(|x| !Arc::ptr_eq(x, &m));
        } else {
            // OS/2 discards a DLL no process uses any more and reads it again
            // on the next load. rs2 keeps it: its shared data starts over and
            // a global init runs again (the C runtime of VisualAge Smalltalk's
            // ABTBMP50.DLL keeps its state there and freed it at termination).
            let used = in_use(&m);
            if crate::api::tracing() {
                crate::trace!("    {} found, in use {used}", m.name);
            }
            if !used {
                reset(&m);
            }
            return Ok(m);
        }
    }
    let (path, data) = match crate::fs::find_module(name) {
        Ok(x) => x,
        Err(e) => {
            // Not on the LIBPATH: one of that name loaded by its path will do.
            let specific = if by_path { None } else { find_loaded(&mname).filter(|m| m.is_dll) };
            if let Some(m) = specific {
                return Ok(m);
            }
            crate::trace!("loader: module {name} not found (error {e})");
            set_fail(name);
            return Err(if e == err::FILE_NOT_FOUND { err::MOD_NOT_FOUND } else { e });
        }
    };
    // The LIBPATH led to a file already loaded by its path.
    if let Some(m) = find_loaded_path(&path) {
        return Ok(m);
    }
    let m = load_image(&path, data, true, by_path, None).inspect_err(|e| crate::trace!("loader: {path}: error {e}"))?;
    register(m.clone());
    Pending::loaded(&m);
    Ok(m)
}

/// Is a loaded 16-bit DLL's file no longer what was loaded?
fn rewritten(m: &Module) -> bool {
    if m.format != Format::Ne {
        return false;
    }
    let Some(img) = &m.image else { return false };
    crate::fs::read_file(&m.path).is_some_and(|d| d[..] != img[..])
}

/// Whether a live process uses `m`: its program, a DLL it loaded, or
/// what they import.
fn in_use(m: &Arc<Module>) -> bool {
    fn uses(x: &Arc<Module>, m: &Arc<Module>, seen: &mut Vec<u16>) -> bool {
        if Arc::ptr_eq(x, m) {
            return true;
        }
        if seen.contains(&x.hmte) {
            return false;
        }
        seen.push(x.hmte);
        x.deps().iter().any(|i| uses(i, m, seen))
    }
    crate::task::process::all().iter().any(|p| {
        let mut seen = Vec::new();
        uses(&p.exe, m, &mut seen) || p.loaded.lock().iter().any(|(x, _)| uses(x, m, &mut seen))
    })
}

/// A DLL's writable shared data back to how it was loaded, its global
/// initialisation not done.
fn reset(m: &Module) {
    for o in &m.objects {
        let (Some(sh), Some(img)) = (&o.shared, &o.pristine) else { continue };
        for (i, page) in img.chunks(crate::vm::PAGE as usize).enumerate() {
            if let Some(fr) = sh.frame(i) {
                unsafe { core::ptr::copy_nonoverlapping(page.as_ptr(), crate::mm::phys::phys_to_virt(fr), page.len()) };
            }
        }
    }
    m.global_init_done.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Load (or find) a DLL and attach it to the address space `space`
/// locks. Loading reads files, which may wait for the disk, so the
/// address space (a spin lock) is only locked for the attaching.
pub fn load_dll<G: core::ops::DerefMut<Target = AddressSpace>>(name: &str, space: impl FnOnce() -> G) -> Result<Arc<Module>, u32> {
    let m = load_global(name)?;
    attach(&m, &mut space())?;
    Ok(m)
}

/// `load_dll` for a process that uses the modules `used`. Instance data
/// still mapped for a DLL (or an import of it) the process let go of starts
/// over: the C runtime of a DLL freed its heap at termination and keeps the
/// pointers in that data (RXULS.DLL, loaded by REXX for each RxFuncAdd).
pub fn load_dll_again<G: core::ops::DerefMut<Target = AddressSpace>>(name: &str, used: &[u16], space: impl FnOnce() -> G) -> Result<Arc<Module>, u32> {
    let m = load_global(name)?;
    let mut sp = space();
    refresh_instance(&m, used, &mut Vec::new(), &mut sp);
    attach(&m, &mut sp)?;
    Ok(m)
}

fn refresh_instance(m: &Arc<Module>, used: &[u16], seen: &mut Vec<u16>, space: &mut AddressSpace) {
    if used.contains(&m.hmte) || seen.contains(&m.hmte) {
        return;
    }
    seen.push(m.hmte);
    for o in m.objects.iter().filter(|o| o.shared.is_none()) {
        if !m.is_dll || space.region(o.base).is_none() {
            continue;
        }
        let mut img = alloc::vec![0u8; o.size as usize];
        if let Some(p) = &o.pristine {
            let n = p.len().min(img.len());
            img[..n].copy_from_slice(&p[..n]);
        }
        space.poke_bytes(o.base, &img);
    }
    for d in &m.deps() {
        refresh_instance(d, used, seen, space);
    }
}

/// Map a module's objects into an address space (if not already) and attach
/// its imports.
pub fn attach(m: &Arc<Module>, space: &mut AddressSpace) -> Result<(), u32> {
    if m.format == Format::Virtual {
        return Ok(());
    }
    if m.is_dll {
        if let Some(o) = m.objects.first() {
            if space.region(o.base).is_some() {
                return Ok(());
            }
        }
    }
    if m.is_dll {
        for o in &m.objects {
            if let Some(sh) = &o.shared {
                space.map_shared(sh.clone(), o.prot, o.seg);
                if let Some(l) = o.limit {
                    space.set_seg_limit(o.base, l);
                }
            } else {
                space
                    .alloc_instance(o.base, o.size, o.prot | crate::vm::pag::COMMIT, o.seg)
                    .map_err(|_| err::NOT_ENOUGH_MEMORY)?;
                if let Some(l) = o.limit {
                    space.set_seg_limit(o.base, l);
                }
                if let Some(img) = &o.pristine {
                    if !space.poke_bytes(o.base, img) {
                        return Err(err::NOT_ENOUGH_MEMORY);
                    }
                }
            }
        }
    }
    for imp in &m.deps() {
        attach(imp, space)?;
    }
    Ok(())
}

/// Load an executable into a fresh process address space.
pub fn load_exe(path: &str, data: Arc<[u8]>, space: &mut AddressSpace) -> Result<Arc<Module>, u32> {
    let m = load_image(path, data, false, false, Some(space))?;
    register(m.clone());
    attach(&m, space)?;
    Ok(m)
}

/// Parse and relocate a module. DLLs go to the shared arena (`space` is
/// None); an EXE is loaded straight into `space`.
fn load_image(path: &str, data: Arc<[u8]>, dll: bool, specific: bool, space: Option<&mut AddressSpace>) -> Result<Arc<Module>, u32> {
    match os2_exe::detect(&data) {
        Ok(os2_exe::Format::Lx { header }) => lxload::load(path, data.clone(), header, dll, specific, space),
        Ok(os2_exe::Format::Ne { header }) => neload::load(path, data.clone(), header, dll, specific, space),
        Ok(_) => Err(err::BAD_EXE_FORMAT),
        Err(_) => Err(err::INVALID_EXE_SIGNATURE),
    }
}

enum ImportBy<'a> {
    Ordinal(u32),
    Name(&'a str),
}

fn import_target(imports: &[Arc<Module>], module: u16, by: ImportBy<'_>) -> Result<Target, u32> {
    let m = imports.get((module as usize).wrapping_sub(1)).ok_or(err::BAD_EXE_FORMAT)?;
    match by {
        ImportBy::Ordinal(o) => m.resolve_ordinal(o, 0),
        ImportBy::Name(n) => m.resolve_name(n, 0),
    }
}

/// DLLs being loaded: name, exports, exported names. DLLs may import
/// each other (MMPM/2's MMPM, FILT and HHP; Resource Workshop's WORKLIB1
/// and WORKLIB2), as OS/2's loader allows: while a DLL loads, its exports
/// (its objects have their addresses already) are listed here, and a
/// module importing it meanwhile resolves those imports from here and gets
/// it in its [`Module::cyclic`] list once it is loaded. Loading the
/// import again instead would recurse until the kernel stack overflowed.
static PENDING: SpinLock<Vec<(String, Vec<Export>, BTreeMap<String, u16>)>> = SpinLock::new(Vec::new());

/// Modules that imported a pending DLL: (importer, DLL).
static WAITING: SpinLock<Vec<(String, String)>> = SpinLock::new(Vec::new());

/// A DLL listed in PENDING while it loads (removed when dropped).
struct Pending(String);

impl Pending {
    fn add(name: &str, exports: &[Export], names: &BTreeMap<String, u16>) -> Pending {
        let e = exports.iter().map(|e| match e {
            Export::At(t) => Export::At(*t),
            _ => Export::None,
        });
        PENDING.lock().push((name.to_string(), e.collect(), names.clone()));
        Pending(name.to_string())
    }

    fn has(name: &str) -> bool {
        PENDING.lock().iter().any(|p| p.0.eq_ignore_ascii_case(name))
    }

    fn target(name: &str, by: ImportBy<'_>) -> Result<Target, u32> {
        let l = PENDING.lock();
        let p = l.iter().find(|p| p.0.eq_ignore_ascii_case(name)).ok_or(err::MOD_NOT_FOUND)?;
        let ord = match by {
            ImportBy::Ordinal(o) => o,
            ImportBy::Name(n) => p.2.get(n).or_else(|| p.2.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v)).copied().ok_or(err::PROC_NOT_FOUND)? as u32,
        };
        match p.1.get(ord.wrapping_sub(1) as usize) {
            Some(Export::At(t)) => Ok(*t),
            // A forwarder of a DLL still loading.
            _ => Err(err::INVALID_ORDINAL),
        }
    }

    /// A pending DLL `m` was loaded: it becomes a dependency of the
    /// modules that imported it meanwhile.
    fn loaded(m: &Arc<Module>) {
        let importers: Vec<String> = {
            let mut w = WAITING.lock();
            let v = w.iter().filter(|x| x.1.eq_ignore_ascii_case(&m.name)).map(|x| x.0.clone()).collect();
            w.retain(|x| !x.1.eq_ignore_ascii_case(&m.name));
            v
        };
        for i in importers {
            let found = REGISTRY.lock().modules.iter().find(|x| x.name.eq_ignore_ascii_case(&i)).cloned();
            if let Some(x) = found {
                let mut c = x.cyclic.lock();
                if !c.iter().any(|y| Arc::ptr_eq(y, m)) {
                    c.push(m.clone());
                }
            }
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let mut l = PENDING.lock();
        if let Some(i) = l.iter().rposition(|p| p.0 == self.0) {
            l.remove(i);
        }
    }
}

/// A module's imports: loaded modules, or the names of DLLs still loading.
struct Imports(Vec<Result<Arc<Module>, String>>);

impl Imports {
    /// Load the modules `own` imports (system-wide, not attached); those
    /// still loading (a circular reference) are resolved from PENDING.
    fn load(own: &str, names: &[String]) -> Result<Imports, u32> {
        let mut v = Vec::with_capacity(names.len());
        for n in names {
            let mname = module_name(n);
            if Pending::has(&mname) {
                if crate::api::tracing() {
                    crate::trace!("    {own} imports {mname}, which is loading (circular reference)");
                }
                WAITING.lock().push((own.to_string(), mname.clone()));
                v.push(Err(mname));
            } else {
                v.push(Ok(load_global(n)?));
            }
        }
        Ok(Imports(v))
    }

    fn target(&self, module: u16, by: ImportBy<'_>) -> Result<Target, u32> {
        match self.0.get((module as usize).wrapping_sub(1)) {
            Some(Ok(m)) => import_target(core::slice::from_ref(m), 1, by),
            Some(Err(name)) => Pending::target(name, by),
            None => Err(err::BAD_EXE_FORMAT),
        }
    }

    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|i| match i {
            Ok(m) => m.name.eq_ignore_ascii_case(name),
            Err(n) => n.eq_ignore_ascii_case(name),
        })
    }

    fn push(&mut self, m: Arc<Module>) {
        self.0.push(Ok(m));
    }

    /// The loaded ones, for [`Module::imports`].
    fn modules(self) -> Vec<Arc<Module>> {
        self.0.into_iter().flatten().collect()
    }
}

/// rs2's Win16 layer (docs/WIN16.md): the Windows modules WIN16.DLL
/// implements, with the base its ordinals for each start at (Windows
/// ordinal n of KERNEL is WIN16 ordinal 1000 + n).
pub const WIN16_DLL: &str = "WIN16";
pub const WIN16_MODULES: &[(&str, u32)] = &[
    ("KERNEL", 1000),
    ("USER", 2000),
    ("GDI", 3000),
    ("KEYBOARD", 4000),
    ("WIN87EM", 5000),
    ("SOUND", 5200),
    ("SYSTEM", 5400),
    ("COMMDLG", 6000),
    ("DDEML", 6100),
    ("TOOLHELP", 6200),
    ("VER", 6300),
    ("LZEXPAND", 6400),
    ("SHELL", 6500),
    ("COMPOBJ", 7000),
    ("OLE2", 7100),
    ("STORAGE", 7300),
    ("OLE2DISP", 7400),
    ("OLE2NLS", 7600),
    ("TYPELIB", 7700),
    ("MMSYSTEM", 10000),
];

/// KERNEL's selectors for real-mode memory (__0040H 193; __0000H 183,
/// __A000H 174, __B000H 181, __B800H 182, __C000H 195, __D000H 179,
/// __E000H 190, __F000H 194): rs2's BIOS data area page.
pub const WIN16_BIOS_SELECTORS: &[u16] = &[193, 183, 174, 181, 182, 195, 179, 190, 194];

/// WIN16's entry for imports it does not have (a call ends the program).
pub const WIN16_UNDEFINED: u32 = 999;

/// KERNEL's absolute entries (WIN16 ordinals): __AHSHIFT, __AHINCR (a
/// huge block's tiles are 8 selectors apart) and __WINFLAGS (protected
/// mode, 386, enhanced mode, coprocessor).
pub const WIN16_CONSTANTS: &[(u32, u16)] = &[(1113, 3), (1114, 8), (1178, 0x0425)];

/// Where a Windows module's import of `name` goes: WIN16 and the
/// ordinal base for the system modules, else `name` itself (base 0).
pub fn win16_import(name: &str) -> (String, u32) {
    let m = module_name(name);
    match WIN16_MODULES.iter().find(|(n, _)| *n == m) {
        Some(&(_, base)) => (String::from(WIN16_DLL), base),
        None => (m, 0),
    }
}

/// Write a fixup value of `size` bytes at `at` in an image buffer.
fn patch(buf: &mut [u8], at: isize, bytes: &[u8]) {
    for (i, &b) in bytes.iter().enumerate() {
        let p = at + i as isize;
        if p >= 0 && (p as usize) < buf.len() {
            buf[p as usize] = b;
        }
    }
}

fn app_type_from_flags(pm_bits: u32) -> u32 {
    // FAPPTYP_NOTWINDOWCOMPAT 1, WINDOWCOMPAT 2, WINDOWAPI 3.
    pm_bits & 3
}
