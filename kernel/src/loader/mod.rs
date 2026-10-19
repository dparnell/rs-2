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
    /// Fixed-up initial contents for per-process instance data.
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
}

pub struct Module {
    pub hmte: u16,
    /// Module name (upper case), as used in import tables.
    pub name: String,
    /// Full path of the file.
    pub path: String,
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
            let a = gate::slot_address(m, ordinal);
            // Kernel APIs reachable both flat and 16:16.
            let mut t = Target::flat32(a, true);
            t.is16 = crate::api::is_16bit(m, ordinal);
            return Ok(t);
        }
        match self.exports.get(ordinal.wrapping_sub(1) as usize) {
            Some(Export::At(t)) => Ok(*t),
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
    REGISTRY.lock().modules.iter().find(|m| m.name.eq_ignore_ascii_case(name)).cloned()
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
            format: Format::Virtual,
            is_dll: true,
            is16: false,
            app_type: 0,
            objects: Vec::new(),
            exports: Vec::new(),
            names: BTreeMap::new(),
            imports: Vec::new(),
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

/// Load (or find) a DLL system-wide without attaching it anywhere.
pub fn load_global(name: &str) -> Result<Arc<Module>, u32> {
    let mname = module_name(name);
    if let Some(m) = find_loaded(&mname) {
        return Ok(m);
    }
    let (path, data) = crate::fs::find_module(name).ok_or_else(|| {
        crate::println!("loader: module {name} not found");
        set_fail(name);
        err::MOD_NOT_FOUND
    })?;
    let m = load_image(&path, data, true, None).inspect_err(|e| crate::println!("loader: {path}: error {e}"))?;
    register(m.clone());
    Ok(m)
}

/// Load (or find) a DLL and attach it to the address space `space`
/// locks. Loading reads files, which may wait for the disk, so the
/// address space (a spin lock) is only locked for the attaching.
pub fn load_dll<G: core::ops::DerefMut<Target = AddressSpace>>(name: &str, space: impl FnOnce() -> G) -> Result<Arc<Module>, u32> {
    let m = load_global(name)?;
    attach(&m, &mut space())?;
    Ok(m)
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
    for imp in &m.imports {
        attach(imp, space)?;
    }
    Ok(())
}

/// Load an executable into a fresh process address space.
pub fn load_exe(path: &str, data: Arc<[u8]>, space: &mut AddressSpace) -> Result<Arc<Module>, u32> {
    let m = load_image(path, data, false, Some(space))?;
    register(m.clone());
    attach(&m, space)?;
    Ok(m)
}

/// Parse and relocate a module. DLLs go to the shared arena (`space` is
/// None); an EXE is loaded straight into `space`.
fn load_image(path: &str, data: Arc<[u8]>, dll: bool, space: Option<&mut AddressSpace>) -> Result<Arc<Module>, u32> {
    match os2_exe::detect(&data) {
        Ok(os2_exe::Format::Lx { header }) => lxload::load(path, data.clone(), header, dll, space),
        Ok(os2_exe::Format::Ne { header }) => neload::load(path, data.clone(), header, dll, space),
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

/// Load the modules a module imports (system-wide, not attached).
fn load_imports(names: &[String]) -> Result<Vec<Arc<Module>>, u32> {
    names.iter().map(|n| load_global(n)).collect()
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
