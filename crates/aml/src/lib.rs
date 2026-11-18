//! An ACPI Machine Language interpreter, small enough for the kernel.
//!
//! Definition blocks (the DSDT, then the SSDTs) are loaded into a
//! namespace by running their top-level code, as ACPI's "module level
//! code" is run: named objects are created, `If`s at the top level are
//! decided. Control methods can then be called: locals and arguments,
//! `If`/`Else`/`While`, integer arithmetic and logic, strings, buffers and
//! packages (built, indexed and stored into), buffer fields, `Field`,
//! `IndexField` and `BankField` units on operation regions, mutexes and
//! events (the interpreter runs on one thread: acquiring never waits),
//! `Notify` (recorded for the caller), `Sleep`/`Stall`, and `_OSI` (rs2
//! answers as Windows does, as Linux does, since firmware turns features
//! on for Windows). Region accesses go to the [`Host`]: system memory,
//! I/O ports, PCI configuration space, the embedded controller.
//!
//! Not done: `Load`/`LoadTable`/`Unload` (dynamic SSDTs), `DataRegion`,
//! GenericSerialBus/SMBus/GPIO regions (the host may answer them),
//! `Connection`.
//!
//! Names are dotted paths of 4-character segments without the leading
//! backslash: `_SB_.PCI0.LPCB.EC0_`; the root is "".

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod asm;
mod exec;
mod field;
#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

/// Address space IDs of operation regions.
pub mod space {
    pub const MEMORY: u8 = 0;
    pub const IO: u8 = 1;
    /// Address: `bus << 24 | device << 19 | function << 16 | offset`.
    pub const PCI: u8 = 2;
    pub const EC: u8 = 3;
    pub const SMBUS: u8 = 4;
    pub const CMOS: u8 = 5;
}

/// What the interpreter needs from the machine.
pub trait Host {
    /// Read `bits` (8, 16, 32 or 64) at `addr` of address space `space`.
    /// None: the space is not handled (the AML gets an error).
    fn read(&mut self, space: u8, addr: u64, bits: u8) -> Option<u64>;
    /// Write; false: not handled.
    fn write(&mut self, space: u8, addr: u64, bits: u8, value: u64) -> bool;
    fn sleep_ms(&mut self, _ms: u64) {}
    fn stall_us(&mut self, _us: u64) {}
    /// A monotonic clock in 100 ns units (the `Timer` operator).
    fn timer(&mut self) -> u64 {
        0
    }
    /// `Store (x, Debug)` and the interpreter's own complaints.
    fn debug(&mut self, _s: &str) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Malformed AML at this offset.
    Parse(usize),
    NotFound(String),
    /// An operand of the wrong type.
    Type,
    /// An opcode the interpreter does not run (0x5Bxx for extended ones).
    Unsupported(u16),
    DivideByZero,
    /// A loop or call nesting went on too long.
    Limit,
    /// The host refused a region access.
    Io,
    /// The AML executed `Fatal`.
    Fatal,
}

pub type Result<T> = core::result::Result<T, Error>;

/// An object: a value, or a named object of the namespace.
#[derive(Clone, Debug, Default)]
pub enum Obj {
    #[default]
    Uninit,
    Int(u64),
    Str(String),
    Buf(Rc<RefCell<Vec<u8>>>),
    Pkg(Rc<RefCell<Vec<Obj>>>),
    /// A name in a package (its absolute path when it was found).
    NameRef(String),
    /// What `RefOf` and `Index` give.
    Ref(Rc<Reference>),
    Method(Rc<Method>),
    /// Scopes the definition blocks open with `Scope` and the
    /// predefined ones.
    Scope,
    Device,
    Processor,
    PowerRes,
    ThermalZone,
    Event,
    Mutex,
    Region(Rc<Region>),
    Field(Rc<Field>),
    BufField(Rc<BufField>),
    /// Another name for the object at this path.
    Alias(String),
}

#[derive(Debug)]
pub enum Reference {
    Named(String),
    /// Element `n` of a package, buffer or string.
    Index(Obj, usize),
}

pub struct Method {
    pub args: u8,
    code: Rc<[u8]>,
    start: usize,
    end: usize,
    native: Option<Native>,
}

impl core::fmt::Debug for Method {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "Method({} args, {} bytes)", self.args, self.end - self.start)
    }
}

#[derive(Clone, Copy, Debug)]
enum Native {
    Osi,
}

#[derive(Debug)]
pub struct Region {
    pub space: u8,
    pub base: u64,
    pub len: u64,
    /// Where the region was declared (for PCI regions: the device).
    scope: String,
    /// PCI regions: bus << 8 | device << 3 | function, found on first use.
    pci: Cell<Option<u32>>,
}

#[derive(Debug)]
pub struct Field {
    kind: FieldKind,
    bit_off: u64,
    bits: u64,
    /// Access width in bytes.
    access: u8,
    /// Update rule: 0 preserve, 1 write as ones, 2 write as zeros.
    update: u8,
}

#[derive(Debug)]
enum FieldKind {
    Region(Rc<Region>),
    /// IndexField: the index and data fields' paths.
    Index { index: String, data: String },
    /// BankField: the bank selector field's path and its value.
    Bank { region: Rc<Region>, bank: String, value: u64 },
}

#[derive(Debug)]
pub struct BufField {
    buf: Rc<RefCell<Vec<u8>>>,
    bit_off: u64,
    bits: u64,
}

impl Obj {
    pub fn int(&self) -> Option<u64> {
        match self {
            Obj::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// A string, or a buffer holding text (up to its first NUL).
    pub fn text(&self) -> Option<String> {
        match self {
            Obj::Str(s) => Some(s.clone()),
            Obj::Buf(b) => Some(b.borrow().iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()),
            _ => None,
        }
    }

    /// The elements of a package.
    pub fn elements(&self) -> Option<Vec<Obj>> {
        match self {
            Obj::Pkg(p) => Some(p.borrow().clone()),
            _ => None,
        }
    }

    pub fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Obj::Buf(b) => Some(b.borrow().clone()),
            _ => None,
        }
    }

    pub fn buffer(v: Vec<u8>) -> Obj {
        Obj::Buf(Rc::new(RefCell::new(v)))
    }

    pub fn package(v: Vec<Obj>) -> Obj {
        Obj::Pkg(Rc::new(RefCell::new(v)))
    }

    /// A copy that shares nothing with `self` (Store's semantics).
    pub fn deep_copy(&self) -> Obj {
        match self {
            Obj::Buf(b) => Obj::buffer(b.borrow().clone()),
            Obj::Pkg(p) => Obj::package(p.borrow().iter().map(|o| o.deep_copy()).collect()),
            o => o.clone(),
        }
    }

    /// ACPI's ObjectType number.
    pub fn type_number(&self) -> u64 {
        match self {
            Obj::Uninit => 0,
            Obj::Int(_) => 1,
            Obj::Str(_) | Obj::NameRef(_) => 2,
            Obj::Buf(_) => 3,
            Obj::Pkg(_) => 4,
            Obj::Field(_) => 5,
            Obj::Device | Obj::Scope => 6,
            Obj::Event => 7,
            Obj::Method(_) => 8,
            Obj::Mutex => 9,
            Obj::Region(_) => 10,
            Obj::PowerRes => 11,
            Obj::Processor => 12,
            Obj::ThermalZone => 13,
            Obj::BufField(_) => 14,
            Obj::Ref(_) | Obj::Alias(_) => 0,
        }
    }

    fn is_container(&self) -> bool {
        matches!(self, Obj::Scope | Obj::Device | Obj::Processor | Obj::PowerRes | Obj::ThermalZone)
    }
}

/// How many nested method calls are allowed (each costs kernel stack).
const MAX_DEPTH: u32 = 16;
/// Operators one top-level evaluation may run (stops endless loops on
/// hardware that never answers).
const BUDGET: u64 = 2_000_000;

/// The `_OSI` strings rs2 answers yes to: those of Windows up to 11 22H2,
/// as Linux does, and the features it understands.
const OSI: &[&str] = &[
    "Windows 2000",
    "Windows 2001",
    "Windows 2001 SP1",
    "Windows 2001.1",
    "Windows 2001 SP2",
    "Windows 2001.1 SP1",
    "Windows 2006",
    "Windows 2006.1",
    "Windows 2006 SP1",
    "Windows 2006 SP2",
    "Windows 2009",
    "Windows 2012",
    "Windows 2013",
    "Windows 2015",
    "Windows 2016",
    "Windows 2017",
    "Windows 2017.2",
    "Windows 2018",
    "Windows 2018.2",
    "Windows 2019",
    "Windows 2020",
    "Windows 2021",
    "Windows 2022",
    "Module Device",
    "Processor Device",
    "3.0 Thermal Model",
    "Extended Address Space Descriptor",
];

pub struct Interp<H: Host> {
    pub host: H,
    ns: BTreeMap<String, Obj>,
    /// Integers are 64-bit (definition block revision 2 and above).
    int64: bool,
    notifications: Vec<(String, u64)>,
    depth: u32,
    budget: u64,
    /// Errors met while loading (skipped terms).
    pub load_errors: u32,
}

impl<H: Host> Interp<H> {
    /// An empty namespace with the predefined objects. `revision`: the
    /// DSDT's revision (below 2: 32-bit integers).
    pub fn new(host: H, revision: u8) -> Self {
        let mut ns = BTreeMap::new();
        for s in ["", "_GPE", "_PR_", "_SB_", "_SI_", "_TZ_"] {
            ns.insert(String::from(s), Obj::Scope);
        }
        ns.insert(String::from("_OS_"), Obj::Str(String::from("Microsoft Windows NT")));
        ns.insert(String::from("_REV"), Obj::Int(2));
        ns.insert(String::from("_GL_"), Obj::Mutex);
        let empty: Rc<[u8]> = Rc::from(&[][..]);
        ns.insert(String::from("_OSI"), Obj::Method(Rc::new(Method { args: 1, code: empty, start: 0, end: 0, native: Some(Native::Osi) })));
        Interp { host, ns, int64: revision >= 2, notifications: Vec::new(), depth: 0, budget: BUDGET, load_errors: 0 }
    }

    /// Load a definition block: its AML, after the 36-byte table header.
    pub fn load(&mut self, aml: &[u8]) {
        let code: Rc<[u8]> = Rc::from(aml);
        self.budget = BUDGET;
        let mut f = exec::Frame::new(String::new(), true);
        let _ = self.terms(&code, 0, code.len(), &mut f);
    }

    /// The object at `path` (aliases followed), not evaluated.
    pub fn get(&self, path: &str) -> Option<&Obj> {
        let mut p = path;
        for _ in 0..8 {
            match self.ns.get(p)? {
                Obj::Alias(t) => p = t,
                o => return Some(o),
            }
        }
        None
    }

    pub fn exists(&self, path: &str) -> bool {
        self.get(path).is_some()
    }

    /// Evaluate `path`: run it if it is a method (with `args`), else its
    /// value (fields are read).
    pub fn eval(&mut self, path: &str, args: Vec<Obj>) -> Result<Obj> {
        let o = self.get(path).cloned().ok_or_else(|| Error::NotFound(String::from(path)))?;
        if self.depth == 0 {
            self.budget = BUDGET;
        }
        let r = match o {
            Obj::Method(m) => self.call(path, &m, args),
            o => self.value(o),
        };
        r.and_then(|o| self.resolve_result(o))
    }

    /// `eval`, as an integer.
    pub fn eval_int(&mut self, path: &str, args: Vec<Obj>) -> Result<u64> {
        let o = self.eval(path, args)?;
        self.to_int(&o)
    }

    /// References in a result (a method returning `Index (...)`) become
    /// the object they point at.
    fn resolve_result(&mut self, o: Obj) -> Result<Obj> {
        match o {
            Obj::Ref(r) => self.deref(&r),
            o => Ok(o),
        }
    }

    /// `Notify`s since the last call: (object path, value).
    pub fn take_notifications(&mut self) -> Vec<(String, u64)> {
        core::mem::take(&mut self.notifications)
    }

    /// The paths of all devices, parents before children.
    pub fn devices(&self) -> Vec<String> {
        self.ns.iter().filter(|(_, o)| matches!(o, Obj::Device)).map(|(p, _)| p.clone()).collect()
    }

    /// The paths directly below `scope`.
    pub fn children(&self, scope: &str) -> Vec<String> {
        let prefix = if scope.is_empty() { String::new() } else { alloc::format!("{scope}.") };
        self.ns.keys().filter(|k| k.len() == prefix.len() + 4 && k.starts_with(&prefix)).cloned().collect()
    }

    /// A device's `_STA` (0xF, present and working, without one; 0 when
    /// it fails).
    pub fn sta(&mut self, dev: &str) -> u64 {
        let p = alloc::format!("{dev}._STA");
        if !self.exists(&p) {
            return 0x0f;
        }
        self.eval_int(&p, Vec::new()).unwrap_or(0)
    }

    /// A device's hardware ID and compatible IDs, as text ("PNP0C0A").
    pub fn ids(&mut self, dev: &str) -> Vec<String> {
        let mut v = Vec::new();
        for n in ["_HID", "_CID"] {
            let p = alloc::format!("{dev}.{n}");
            if !self.exists(&p) {
                continue;
            }
            let Ok(o) = self.eval(&p, Vec::new()) else { continue };
            let items = o.elements().unwrap_or_else(|| alloc::vec![o]);
            for i in items {
                match i {
                    Obj::Int(n) => v.push(eisa_id(n as u32)),
                    Obj::Str(s) => v.push(s),
                    _ => {}
                }
            }
        }
        v
    }

    /// The devices with hardware or compatible ID `id`.
    pub fn find_devices(&mut self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        for d in self.devices() {
            if self.ids(&d).iter().any(|i| i == id) {
                out.push(d);
            }
        }
        out
    }

    /// Run `_INI` as an operating system does at start-up: `\_SB._INI`,
    /// then every device's whose `_STA` says present, parents first; the
    /// devices below one neither present nor functioning are skipped.
    /// Returns how many `_INI`s failed.
    pub fn initialize(&mut self) -> u32 {
        let mut failed = 0;
        if self.exists("_SB_._INI") && self.eval("_SB_._INI", Vec::new()).is_err() {
            failed += 1;
        }
        let mut skipped: Vec<String> = Vec::new();
        for d in self.devices() {
            if skipped.iter().any(|s| d.starts_with(s.as_str()) && d.as_bytes().get(s.len()) == Some(&b'.')) {
                continue;
            }
            let sta = self.sta(&d);
            if sta & 1 == 0 {
                if sta & 8 == 0 {
                    skipped.push(d);
                }
                continue;
            }
            let ini = alloc::format!("{d}._INI");
            if matches!(self.get(&ini), Some(Obj::Method(_))) {
                if let Err(e) = self.eval(&ini, Vec::new()) {
                    self.host.debug(&alloc::format!("aml: {ini}: {e:?}"));
                    failed += 1;
                }
            }
        }
        failed
    }

    /// Tell the AML that address space `space` can be used now: `_REG
    /// (space, 1)` of every device that declares a region of that space.
    pub fn reg(&mut self, space: u8) {
        let mut devs: Vec<String> = Vec::new();
        for (p, o) in &self.ns {
            if let Obj::Region(r) = o {
                if r.space == space {
                    let parent = parent_of(p);
                    if !devs.iter().any(|d| d == parent) {
                        devs.push(String::from(parent));
                    }
                }
            }
        }
        for d in devs {
            let m = if d.is_empty() { String::from("_REG") } else { alloc::format!("{d}._REG") };
            if matches!(self.get(&m), Some(Obj::Method(_))) {
                if let Err(e) = self.eval(&m, alloc::vec![Obj::Int(space as u64), Obj::Int(1)]) {
                    self.host.debug(&alloc::format!("aml: {m}: {e:?}"));
                }
            }
        }
    }

    /// The names of the namespace (for listings).
    pub fn paths(&self) -> impl Iterator<Item = (&String, &Obj)> {
        self.ns.iter()
    }
}

fn parent_of(p: &str) -> &str {
    p.rfind('.').map_or("", |i| &p[..i])
}

/// An EISA ID (`EisaId ("PNP0303")` compressed into a dword) as text.
pub fn eisa_id(v: u32) -> String {
    let b = v.to_le_bytes();
    let hex = |n: u8| char::from_digit(n as u32, 16).unwrap_or('0').to_ascii_uppercase();
    let mut s = String::new();
    s.push((((b[0] >> 2) & 0x1f) + 0x40) as char);
    s.push(((((b[0] & 3) << 3) | (b[1] >> 5)) + 0x40) as char);
    s.push(((b[1] & 0x1f) + 0x40) as char);
    for n in [b[2] >> 4, b[2] & 0xf, b[3] >> 4, b[3] & 0xf] {
        s.push(hex(n));
    }
    s
}
