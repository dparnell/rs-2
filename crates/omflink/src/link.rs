//! The linker proper: reads the objects and libraries, resolves symbols,
//! combines segments, lays them out into output units (NE segments or LX
//! objects) and applies fix-ups. The output formats are in `ne.rs` and
//! `lx.rs`, the map file in `map.rs`.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::def::{self, Def};
use crate::msg::Msgs;
use crate::omf::{self, Align, Combine, ExtKind, Frame, ImpEntry, Loc, Object, Target};
use crate::{Host, Options};

/// A loaded object module.
pub struct Mod {
    pub obj: Object,
    /// File it came from (with the library module's name for libraries).
    pub file: String,
    /// Local segment index (1-based) -> (logical segment, offset of this
    /// module's part in it).
    pub segmap: Vec<(usize, u32)>,
    /// Local group index -> global group.
    pub grpmap: Vec<usize>,
    /// Local external index -> symbol.
    pub extmap: Vec<usize>,
    /// COMDAT index -> where it was placed (None: another module's kept).
    pub comdat_places: Vec<Option<(usize, u32)>>,
}

#[derive(Debug, Clone)]
pub enum SymDef {
    Undefined,
    /// In a segment of a module (local segment index), with the group the
    /// PUBDEF named (local index, 0 for none).
    Public { module: usize, seg: u16, group: u16, offset: u32 },
    Absolute { frame: u16, offset: u32 },
    Common { far: bool, size: u32, use32: bool },
    Import { module: String, entry: ImpEntry },
    /// Resolved to another symbol (weak externals, aliases).
    Alias(usize),
    /// A symbol the linker places (_edata, _end) in a logical segment.
    Linker { lseg: usize, offset: u32 },
}

#[derive(Debug, Clone)]
pub struct Sym {
    pub name: String,
    pub def: SymDef,
    pub referenced: bool,
    /// Module that first referenced it (for unresolved-external messages).
    pub refby: Option<usize>,
    pub weak_default: Option<usize>,
    pub local: bool,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub module: usize,
    pub seg: u16,
    pub offset: u32,
    pub length: u32,
}

/// A logical segment: same-named segments of the modules combined.
#[derive(Debug, Clone)]
pub struct LSeg {
    pub name: String,
    pub class: String,
    pub combine: Combine,
    pub use32: bool,
    pub align: u32,
    pub size: u32,
    pub parts: Vec<Part>,
    pub group: Option<usize>,
    pub absolute: Option<u16>,
    /// Output unit and offset in it.
    pub unit: usize,
    pub offset: u32,
    /// Communal variables (c_common, FAR_BSS) placed here by the linker.
    pub communals: Vec<(usize, u32)>,
    pub first_module: usize,
}

impl LSeg {
    pub fn is_code(&self) -> bool {
        self.class.to_ascii_uppercase().ends_with("CODE")
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub lsegs: Vec<usize>,
}

/// An output segment (NE) or object (LX).
#[derive(Debug, Clone, Default)]
pub struct Unit {
    pub lsegs: Vec<usize>,
    /// Bytes of address space (virtual size).
    pub size: u32,
    pub data: Vec<u8>,
    /// Bytes holding initialised data (the rest is zero-filled).
    pub init: u32,
    pub code: bool,
    pub use32: bool,
    pub relocs: Vec<Reloc>,
    pub name: String,
    pub class: String,
    pub attrs: def::Attrs,
    /// LX: the object's 16:16 alias is needed (a 16-bit segment or ALIAS).
    pub alias16: bool,
    /// Relative virtual address (LX: from the module's base).
    pub base: u32,
    pub is_stack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    LoByte,
    Selector,
    Ptr1616,
    Offset16,
    Ptr1632,
    Offset32,
    SelfRel16,
    SelfRel32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RTarget {
    Internal { unit: usize, offset: u32 },
    ImportOrd { module: usize, ordinal: u16 },
    ImportName { module: usize, name: String },
}

#[derive(Debug, Clone)]
pub struct Reloc {
    pub offset: u32,
    pub kind: RelocKind,
    pub target: RTarget,
    /// Added to the target (NE: put in the location, additive).
    pub add: i32,
}

#[derive(Debug, Clone)]
pub struct ExportEnt {
    pub name: String,
    /// The ordinal was given (not assigned by the linker).
    pub explicit: bool,
    pub sym: Option<usize>,
    pub ordinal: u16,
    pub resident: bool,
    pub noname: bool,
    pub nodata: bool,
    pub private: bool,
    pub pwords: u8,
    /// Resolved: unit and offset; or forwarded import.
    pub at: Option<(usize, u32)>,
    pub forward: Option<(usize, ImpEntry)>,
    pub absolute: Option<u32>,
}

/// Everything the output writers need.
pub struct Linked {
    pub mods: Vec<Mod>,
    pub syms: Vec<Sym>,
    pub lsegs: Vec<LSeg>,
    pub groups: Vec<Group>,
    pub units: Vec<Unit>,
    pub order: Vec<usize>,
    pub imports: Vec<String>,
    pub exports: Vec<ExportEnt>,
    pub start: Option<(usize, u32)>,
    pub def: Def,
    pub dgroup: Option<usize>,
    /// Unit of the automatic data segment.
    pub autodata: Option<usize>,
    pub stack_unit: Option<(usize, u32)>,
    pub stack_size: u32,
    pub heap_size: u32,
    pub dll: bool,
    pub lx: bool,
    pub module_name: String,
    pub description: String,
    pub errors: u32,
    pub protmem: bool,
}

fn key(s: &str, case: bool) -> String {
    if case { String::from(s) } else { s.to_ascii_uppercase() }
}

pub struct Linker<'h> {
    pub host: &'h mut dyn Host,
    pub opt: Options,
    pub msgs: Msgs,
    pub mods: Vec<Mod>,
    pub syms: Vec<Sym>,
    pub symtab: BTreeMap<String, usize>,
    pub lsegs: Vec<LSeg>,
    segtab: BTreeMap<(String, String), usize>,
    pub groups: Vec<Group>,
    grptab: BTreeMap<String, usize>,
    pub libs: Vec<Lib>,
    pub dosseg: bool,
    pub start: Option<(usize, omf::Fixup)>,
    pub def: Def,
    /// Symbols in the order they became imports (numbers the modules).
    pub import_seq: Vec<usize>,
}

/// A library being searched.
pub struct Lib {
    pub path: String,
    pub data: Vec<u8>,
    /// The modules in file order: offset and the names they define.
    pub modules: Vec<(usize, Vec<String>)>,
    pub loaded: Vec<usize>,
    /// Default library (named by an object), searched after the others.
    pub default: bool,
}

impl<'h> Linker<'h> {
    pub fn new(host: &'h mut dyn Host, opt: Options, msgs: Msgs) -> Self {
        Linker {
            host,
            opt,
            msgs,
            mods: Vec::new(),
            syms: Vec::new(),
            symtab: BTreeMap::new(),
            lsegs: Vec::new(),
            segtab: BTreeMap::new(),
            groups: Vec::new(),
            grptab: BTreeMap::new(),
            libs: Vec::new(),
            dosseg: false,
            start: None,
            def: Def::default(),
            import_seq: Vec::new(),
        }
    }

    fn case(&self) -> bool {
        self.opt.case_sensitive
    }

    pub fn sym(&mut self, name: &str) -> usize {
        let k = key(name, self.case());
        if let Some(&i) = self.symtab.get(&k) {
            return i;
        }
        self.syms.push(Sym {
            name: String::from(name),
            def: SymDef::Undefined,
            referenced: false,
            refby: None,
            weak_default: None,
            local: false,
        });
        self.symtab.insert(k, self.syms.len() - 1);
        self.syms.len() - 1
    }

    /// Follow aliases.
    pub fn resolve(&self, mut s: usize) -> usize {
        for _ in 0..32 {
            match self.syms[s].def {
                SymDef::Alias(t) => s = t,
                _ => break,
            }
        }
        s
    }

    fn group(&mut self, name: &str) -> usize {
        let k = key(name, self.case());
        if let Some(&g) = self.grptab.get(&k) {
            return g;
        }
        self.groups.push(Group { name: String::from(name), lsegs: Vec::new() });
        self.grptab.insert(k, self.groups.len() - 1);
        self.groups.len() - 1
    }

    fn define(&mut self, s: usize, d: SymDef, m: usize) {
        match (&self.syms[s].def, &d) {
            (SymDef::Undefined, _) | (SymDef::Alias(_), _) => self.syms[s].def = d,
            // A communal joins another communal (largest size) and gives way
            // to a real definition.
            (SymDef::Common { far, size, use32 }, SymDef::Common { size: s2, .. }) => {
                let (far, size, use32) = (*far, (*size).max(*s2), *use32);
                self.syms[s].def = SymDef::Common { far, size, use32 };
            }
            (SymDef::Common { .. }, _) => self.syms[s].def = d,
            (_, SymDef::Common { .. }) => {}
            (SymDef::Import { .. }, SymDef::Import { .. }) => {}
            _ => {
                let name = self.syms[s].name.clone();
                let file = self.mods[m].file.clone();
                self.msgs.error(self.host, &file, 2025, &format!("{name} : symbol defined more than once"));
            }
        }
    }

    /// Add an object module: its segments, symbols and references.
    pub fn add_module(&mut self, obj: Object, file: String) {
        let m = self.mods.len();
        let case = self.case();
        let mut segmap = Vec::new();
        for sd in &obj.segs {
            let ls = if sd.combine == Combine::Private || sd.align == Align::Absolute {
                None
            } else {
                self.segtab.get(&(key(&sd.name, case), key(&sd.class, case))).copied()
            };
            let ls = match ls {
                Some(l) => l,
                None => {
                    self.lsegs.push(LSeg {
                        name: sd.name.clone(),
                        class: sd.class.clone(),
                        combine: sd.combine,
                        use32: sd.use32,
                        align: sd.align.bytes(),
                        size: 0,
                        parts: Vec::new(),
                        group: None,
                        absolute: sd.frame,
                        unit: 0,
                        offset: 0,
                        communals: Vec::new(),
                        first_module: m,
                    });
                    let l = self.lsegs.len() - 1;
                    if sd.combine != Combine::Private && sd.align != Align::Absolute {
                        self.segtab.insert((key(&sd.name, case), key(&sd.class, case)), l);
                    }
                    l
                }
            };
            let lseg = &mut self.lsegs[ls];
            if sd.use32 && !lseg.use32 && !lseg.parts.is_empty() {
                let n = lseg.name.clone();
                self.msgs.error(self.host, &file, 1123, &format!("{n} : segment defined both 16- and 32-bit"));
            }
            lseg.use32 |= sd.use32;
            let len = if sd.length == 0xffff_ffff { 0 } else { sd.length };
            let offset = match sd.combine {
                Combine::Common => {
                    lseg.size = lseg.size.max(len);
                    0
                }
                _ => {
                    let a = sd.align.bytes();
                    lseg.align = lseg.align.max(a);
                    let o = lseg.size.next_multiple_of(a);
                    lseg.size = o + len;
                    o
                }
            };
            lseg.parts.push(Part { module: m, seg: segmap.len() as u16 + 1, offset, length: len });
            segmap.push((ls, offset));
        }
        let mut grpmap = Vec::new();
        for g in &obj.grps {
            let gi = self.group(&g.name);
            for &s in &g.segs {
                if let Some(&(ls, _)) = segmap.get(s as usize - 1) {
                    match self.lsegs[ls].group {
                        None => {
                            self.lsegs[ls].group = Some(gi);
                            self.groups[gi].lsegs.push(ls);
                        }
                        Some(o) if o != gi => {
                            let n = self.lsegs[ls].name.clone();
                            self.msgs.warn(self.host, &file, 4031, &format!("{n} : segment declared in more than one group"));
                        }
                        _ => {}
                    }
                }
            }
            grpmap.push(gi);
        }
        self.mods.push(Mod { obj: Object::default(), file: file.clone(), segmap, grpmap, extmap: Vec::new(), comdat_places: Vec::new() });
        // Local publics first, so that LEXTDEFs find them.
        let mut locals: BTreeMap<String, usize> = BTreeMap::new();
        for p in obj.pubs.iter().filter(|p| p.local) {
            self.syms.push(Sym {
                name: p.name.clone(),
                def: if p.seg == 0 {
                    SymDef::Absolute { frame: p.frame, offset: p.offset }
                } else {
                    SymDef::Public { module: m, seg: p.seg, group: p.group, offset: p.offset }
                },
                referenced: false,
                refby: None,
                weak_default: None,
                local: true,
            });
            locals.insert(key(&p.name, case), self.syms.len() - 1);
        }
        for p in obj.pubs.iter().filter(|p| !p.local) {
            let s = self.sym(&p.name);
            let d = if p.seg == 0 {
                SymDef::Absolute { frame: p.frame, offset: p.offset }
            } else {
                SymDef::Public { module: m, seg: p.seg, group: p.group, offset: p.offset }
            };
            self.define(s, d, m);
        }
        // COMDATs: the first definition of a name is kept.
        for c in obj.comdats.iter().filter(|c| !c.continuation) {
            if c.local {
                continue;
            }
            let s = self.sym(&c.name);
            if matches!(self.syms[s].def, SymDef::Undefined | SymDef::Common { .. }) {
                // Placed when segments are laid out.
                self.syms[s].def = SymDef::Public { module: m, seg: 0, group: 0, offset: u32::MAX };
            }
        }
        for imp in &obj.impdefs {
            let s = self.sym(&imp.internal);
            if matches!(self.syms[s].def, SymDef::Undefined) {
                self.syms[s].def = SymDef::Import { module: imp.module.clone(), entry: imp.entry.clone() };
                self.import_seq.push(s);
            }
        }
        let use32 = obj.segs.iter().any(|s| s.use32);
        let mut extmap = Vec::new();
        for e in &obj.exts {
            let s = match (e.local, locals.get(&key(&e.name, case))) {
                (true, Some(&l)) => l,
                _ => self.sym(&e.name),
            };
            if let ExtKind::Common { far, size } = e.kind {
                self.define(s, SymDef::Common { far, size, use32 }, m);
            }
            self.syms[s].referenced = true;
            if self.syms[s].refby.is_none() {
                self.syms[s].refby = Some(m);
            }
            extmap.push(s);
        }
        for &(w, d) in &obj.weak {
            if let (Some(&ws), Some(&ds)) = (extmap.get(w as usize - 1), extmap.get(d as usize - 1)) {
                self.syms[ws].weak_default = Some(ds);
            }
        }
        for (a, s) in &obj.aliases {
            let ai = self.sym(a);
            let si = self.sym(s);
            if matches!(self.syms[ai].def, SymDef::Undefined) && ai != si {
                self.syms[ai].def = SymDef::Alias(si);
                self.syms[si].referenced |= self.syms[ai].referenced;
            }
        }
        if obj.dosseg {
            self.dosseg = true;
        }
        if let Some(st) = obj.start {
            if self.start.is_none() {
                self.start = Some((m, st));
            }
        }
        self.mods[m].extmap = extmap;
        self.mods[m].obj = obj;
    }

    /// Open a library and index its modules' public names.
    pub fn add_library(&mut self, path: String, data: Vec<u8>, default: bool) -> bool {
        let lib = match omf::Library::parse(&data) {
            Ok(l) => l,
            Err(_) => {
                self.msgs.fatal(self.host, "", 1104, &format!("{path} : not a valid library"));
                return false;
            }
        };
        let case = self.case();
        let mut modules = Vec::new();
        for &off in &lib.modules {
            let Ok(o) = Object::parse(&data[off..]) else { continue };
            let names = o
                .pubs
                .iter()
                .filter(|p| !p.local)
                .map(|p| &p.name)
                .chain(o.impdefs.iter().map(|i| &i.internal))
                .chain(o.comdats.iter().filter(|c| !c.local && !c.continuation).map(|c| &c.name))
                .chain(o.aliases.iter().map(|a| &a.0));
            modules.push((off, names.map(|n| key(n, case)).collect()));
        }
        self.libs.push(Lib { path, data, modules, loaded: Vec::new(), default });
        true
    }

    /// Add every module of a library (a library named among the objects).
    pub fn add_whole_library(&mut self, path: &str, data: &[u8]) -> bool {
        let Ok(lib) = omf::Library::parse(data) else {
            self.msgs.fatal(self.host, "", 1104, &format!("{path} : not a valid library"));
            return false;
        };
        for &off in &lib.modules {
            match Object::parse(&data[off..]) {
                Ok(o) => {
                    let n = format!("{}({})", path, o.name);
                    self.add_module(o, n);
                }
                Err(_) => {
                    self.msgs.fatal(self.host, path, 1101, "invalid object module");
                    return false;
                }
            }
        }
        true
    }

    fn undefined(&self) -> Vec<usize> {
        (0..self.syms.len())
            .filter(|&s| self.syms[s].referenced && matches!(self.syms[s].def, SymDef::Undefined))
            .collect()
    }

    /// Pull library modules for undefined symbols until nothing changes.
    pub fn search_libraries(&mut self) {
        let case = self.case();
        loop {
            let mut progress = false;
            for li in 0..self.libs.len() {
                // Passes over the library (as IBM's linkers do): the modules
                // defining the symbols undefined when the pass starts are
                // loaded in file order; until a pass loads nothing.
                let mut mi = 0;
                let mut pass_progress = false;
                let mut wanted_now: Vec<String> = self.undefined().iter().map(|&s| key(&self.syms[s].name, case)).collect();
                wanted_now.sort();
                loop {
                    // One pass per library each round; the next library
                    // then gets its turn (IBM's order).
                    if mi >= self.libs[li].modules.len() {
                        let _ = pass_progress;
                        break;
                    }
                    let off = self.libs[li].modules[mi].0;
                    mi += 1;
                    if self.libs[li].loaded.contains(&off) {
                        continue;
                    }
                    let wanted = self.libs[li].modules[mi - 1].1.iter().any(|k| {
                        wanted_now.binary_search(k).is_ok()
                            && self.symtab.get(k).is_some_and(|&s| matches!(self.syms[s].def, SymDef::Undefined))
                    });
                    if !wanted {
                        continue;
                    }
                    pass_progress = true;
                    self.libs[li].loaded.push(off);
                    match Object::parse(&self.libs[li].data[off..]) {
                        Ok(o) => {
                            if self.opt.information {
                                let s = format!("Searching library {}: {}\r\n", self.libs[li].path, o.name);
                                self.host.out(&s);
                            }
                            let n = format!("{}({})", self.libs[li].path, o.name);
                            // Default libraries named by library modules
                            // are searched too.
                            let libs = o.libs.clone();
                            self.add_module(o, n);
                            for l in libs {
                                self.want_default_library(&l);
                            }
                        }
                        Err(_) => {
                            let p = self.libs[li].path.clone();
                            self.msgs.fatal(self.host, &p, 1101, "invalid object module");
                            return;
                        }
                    }
                    progress = true;
                }
            }
            if !progress {
                // Weak externals still undefined take their defaults.
                let mut changed = false;
                for s in self.undefined() {
                    if let Some(d) = self.syms[s].weak_default {
                        self.syms[s].def = SymDef::Alias(d);
                        self.syms[d].referenced = true;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
    }

    pub fn want_default_library(&mut self, name: &str) {
        if self.opt.nod || self.opt.nod_libs.iter().any(|n| crate::cmd::same_file(n, name)) {
            return;
        }
        let name = crate::cmd::with_ext(name, "LIB");
        if self.libs.iter().any(|l| crate::cmd::same_file(&l.path, &name)) {
            return;
        }
        if self.opt.pending_default_libs.iter().any(|l| crate::cmd::same_file(l, &name)) {
            return;
        }
        self.opt.pending_default_libs.push(name);
    }

    /// Default libraries named so far that are not open yet.
    pub fn open_default_libraries(&mut self) -> bool {
        let mut any = false;
        while let Some(name) = (!self.opt.pending_default_libs.is_empty()).then(|| self.opt.pending_default_libs.remove(0)) {
            match crate::cmd::find_file(self.host, &name, &self.opt.lib_dirs) {
                Some((p, d)) => {
                    if self.add_library(p, d, true) {
                        any = true;
                    }
                }
                None => self.msgs.warn(self.host, "", 4051, &format!("{name} : cannot find library")),
            }
        }
        any
    }
}

/// Segment order: classes in order of first appearance, or DOSSEG's.
fn segment_order(l: &Linker) -> Vec<usize> {
    let mut classes: Vec<String> = Vec::new();
    for s in &l.lsegs {
        let k = s.class.to_ascii_uppercase();
        if !classes.contains(&k) {
            classes.push(k);
        }
    }
    let mut order = Vec::new();
    for c in &classes {
        for (i, s) in l.lsegs.iter().enumerate() {
            if &s.class.to_ascii_uppercase() == c {
                order.push(i);
            }
        }
    }
    // Segments the .DEF file's SEGMENTS statement names come first, in its
    // order (as IBM's LINK numbers them).
    if !l.def.segments.is_empty() {
        let rank = |i: &usize| {
            let n = &l.lsegs[*i].name;
            l.def.segments.iter().position(|d| d.name.eq_ignore_ascii_case(n)).unwrap_or(usize::MAX)
        };
        order.sort_by_key(rank);
    }
    if l.dosseg {
        let dg = l.groups.iter().position(|g| g.name.eq_ignore_ascii_case("DGROUP"));
        let rank = |i: &usize| {
            let s = &l.lsegs[*i];
            let c = s.class.to_ascii_uppercase();
            if s.is_code() {
                0
            } else if dg.is_none() || s.group != dg {
                1
            } else if c == "BEGDATA" {
                2
            } else if c != "BSS" && c != "STACK" {
                3
            } else if c == "BSS" {
                4
            } else {
                5
            }
        };
        order.sort_by_key(rank);
    }
    order
}

impl<'h> Linker<'h> {
    /// Communal variables become data in c_common (near) or FAR_BSS (far).
    fn allocate_communals(&mut self) {
        let mut near: Vec<usize> = Vec::new();
        let mut far: Vec<usize> = Vec::new();
        for (i, s) in self.syms.iter().enumerate() {
            if let SymDef::Common { far: f, .. } = s.def {
                if f && !self.opt.lx { far.push(i) } else { near.push(i) }
            }
        }
        if !near.is_empty() {
            let use32 = self.opt.lx;
            let ls = self.linker_segment("c_common", "BSS", use32, Some("DGROUP"));
            for s in near {
                let SymDef::Common { size, .. } = self.syms[s].def else { continue };
                let a = if size >= 4 && use32 { 4 } else if size >= 2 { 2 } else { 1 };
                let off = self.lsegs[ls].size.next_multiple_of(a);
                self.lsegs[ls].size = off + size;
                self.lsegs[ls].communals.push((s, off));
                self.syms[s].def = SymDef::Linker { lseg: ls, offset: off };
            }
        }
        let mut cur: Option<usize> = None;
        for s in far {
            let SymDef::Common { size, .. } = self.syms[s].def else { continue };
            let ls = match cur {
                Some(c) if self.lsegs[c].size as u64 + size as u64 <= 0x10000 => c,
                _ => {
                    // Each FAR_BSS segment holds up to 64K.
                    self.lsegs.push(LSeg {
                        name: String::from("FAR_BSS"),
                        class: String::from("FAR_BSS"),
                        combine: Combine::Private,
                        use32: false,
                        align: 16,
                        size: 0,
                        parts: Vec::new(),
                        group: None,
                        absolute: None,
                        unit: 0,
                        offset: 0,
                        communals: Vec::new(),
                        first_module: 0,
                    });
                    self.lsegs.len() - 1
                }
            };
            cur = Some(ls);
            let off = self.lsegs[ls].size;
            self.lsegs[ls].size = off + size;
            self.lsegs[ls].communals.push((s, off));
            self.syms[s].def = SymDef::Linker { lseg: ls, offset: off };
        }
    }

    /// A segment the linker makes itself (or the existing one of that name).
    fn linker_segment(&mut self, name: &str, class: &str, use32: bool, group: Option<&str>) -> usize {
        let case = self.case();
        let k = (key(name, case), key(class, case));
        let ls = match self.segtab.get(&k) {
            Some(&l) => l,
            None => {
                self.lsegs.push(LSeg {
                    name: String::from(name),
                    class: String::from(class),
                    combine: Combine::Public,
                    use32,
                    align: if use32 { 4 } else { 2 },
                    size: 0,
                    parts: Vec::new(),
                    group: None,
                    absolute: None,
                    unit: 0,
                    offset: 0,
                    communals: Vec::new(),
                    first_module: 0,
                });
                let l = self.lsegs.len() - 1;
                self.segtab.insert(k, l);
                l
            }
        };
        if let Some(g) = group {
            if self.lsegs[ls].group.is_none() {
                let gi = self.group(g);
                self.lsegs[ls].group = Some(gi);
                self.groups[gi].lsegs.push(ls);
            }
        }
        ls
    }

    /// Place COMDATs: each kept one is appended to its segment.
    fn place_comdats(&mut self) {
        for m in 0..self.mods.len() {
            let n = self.mods[m].obj.comdats.len();
            let mut placed: Vec<Option<(usize, u32)>> = vec![None; n];
            for ci in 0..n {
                let c = &self.mods[m].obj.comdats[ci];
                if c.continuation {
                    // Continues the previous COMDAT of the same name.
                    let prev = (0..ci).rev().find(|&p| self.mods[m].obj.comdats[p].name == c.name);
                    if let Some(p) = prev {
                        placed[ci] = placed[p];
                    }
                    continue;
                }
                let s = if c.local { None } else { self.symtab.get(&key(&c.name, self.case())).copied() };
                let keep = match s {
                    Some(s) => matches!(self.syms[s].def, SymDef::Public { module, seg: 0, offset: u32::MAX, .. } if module == m),
                    None => true,
                };
                if !keep {
                    continue;
                }
                let (alloc, seg, align, len) = (c.alloc, c.seg, c.align, c.offset + c.bytes.len() as u32);
                let ls = match alloc {
                    0 => self.mods[m].segmap.get(seg as usize - 1).map(|x| x.0),
                    1 => Some(self.linker_segment("COMDAT_CODE16", "CODE", false, None)),
                    2 => Some(self.linker_segment("COMDAT_DATA16", "FAR_DATA", false, None)),
                    3 => Some(self.linker_segment("CODE32", "CODE", true, None)),
                    _ => Some(self.linker_segment("DATA32", "DATA", true, Some("DGROUP"))),
                };
                let Some(ls) = ls else { continue };
                let a = match align {
                    0 => self.lsegs[ls].align,
                    a => Align::bytes(match a {
                        1 => Align::Byte,
                        2 => Align::Word,
                        3 => Align::Para,
                        4 => Align::Page,
                        5 => Align::Dword,
                        _ => Align::Page4K,
                    }),
                };
                let off = self.lsegs[ls].size.next_multiple_of(a);
                self.lsegs[ls].size = off + len;
                // A comdat behaves like a segment part of its own.
                self.lsegs[ls].parts.push(Part { module: m, seg: 0, offset: off, length: len });
                placed[ci] = Some((ls, off));
                if let Some(s) = s {
                    self.syms[s].def = SymDef::Linker { lseg: ls, offset: off };
                }
            }
            self.mods[m].comdat_places = placed;
        }
    }
}

impl Mod {
    /// Logical segment and offset of a local segment's start.
    pub fn seg(&self, s: u16) -> Option<(usize, u32)> {
        if s == 0 { None } else { self.segmap.get(s as usize - 1).copied() }
    }
}

/// The resolved address of a symbol or segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Addr {
    /// In a logical segment.
    Seg { lseg: usize, offset: u32 },
    Abs { frame: u16, offset: u32 },
    Import { module: String, entry: ImpEntry, offset: u32 },
    Undefined,
}

pub fn link(l: &mut Linker) -> Option<Linked> {
    l.allocate_communals();
    l.place_comdats();
    // Linker-defined symbols of DOSSEG programs.
    let order0 = segment_order(l);
    if l.dosseg {
        for (name, class) in [("_edata", "BSS"), ("_end", "STACK")] {
            let s = l.sym(name);
            if matches!(l.syms[s].def, SymDef::Undefined) {
                let at = order0.iter().copied().find(|&i| l.lsegs[i].class.eq_ignore_ascii_case(class));
                if let Some(ls) = at {
                    l.syms[s].def = SymDef::Linker { lseg: ls, offset: 0 };
                }
            }
        }
        // Sixteen bytes of zeros at the start of _TEXT (the null-pointer area).
        if !l.opt.nonulls {
            if let Some(t) = l.lsegs.iter().position(|s| s.name.eq_ignore_ascii_case("_TEXT")) {
                let ls = &mut l.lsegs[t];
                for p in &mut ls.parts {
                    p.offset += 16;
                }
                for c in &mut ls.communals {
                    c.1 += 16;
                }
                ls.size += 16;
                for m in &mut l.mods {
                    for e in &mut m.segmap {
                        if e.0 == t {
                            e.1 += 16;
                        }
                    }
                }
            }
        }
    }
    // Unresolved externals.
    let mut errors = 0u32;
    for s in 0..l.syms.len() {
        let r = l.resolve(s);
        if l.syms[s].referenced && matches!(l.syms[r].def, SymDef::Undefined) {
            let file = l.syms[s].refby.map(|m| l.mods[m].file.clone()).unwrap_or_default();
            let n = l.syms[s].name.clone();
            l.msgs.error(l.host, &file, 2029, &format!("'{n}' : unresolved external"));
            errors += 1;
        }
    }
    let order = segment_order(l);
    if order.is_empty() {
        l.msgs.fatal(l.host, "", 2049, "no segments defined");
        return None;
    }
    let lx = l.opt.lx;
    let mut units: Vec<Unit> = Vec::new();
    let dgroup = l.groups.iter().position(|g| g.name.eq_ignore_ascii_case("DGROUP"));
    let flat = l.groups.iter().position(|g| g.name.eq_ignore_ascii_case("FLAT"));
    let mut group_unit: BTreeMap<usize, usize> = BTreeMap::new();
    let def = l.def.clone();
    // Segments' attributes from the .DEF file.
    let seg_attrs = |s: &LSeg| -> (def::Attrs, Option<String>) {
        let base = if s.is_code() { def.code.clone() } else { def.data.clone() };
        match def.segments.iter().find(|d| d.name.eq_ignore_ascii_case(&s.name)) {
            Some(d) => {
                let mut a = base;
                let o = &d.attrs;
                macro_rules! ov {
                    ($f:ident) => {
                        if o.$f.is_some() {
                            a.$f = o.$f;
                        }
                    };
                }
                ov!(preload);
                ov!(readonly);
                ov!(iopl);
                ov!(conforming);
                ov!(shared);
                ov!(discardable);
                ov!(moveable);
                ov!(instance);
                ov!(expanddown);
                a.alias |= o.alias;
                a.mixed1632 |= o.mixed1632;
                (a, d.class.clone())
            }
            None => (base, None),
        }
    };
    let packcode = if lx { l.opt.packcode.or(Some(u32::MAX)) } else { l.opt.packcode };
    let packdata = l.opt.packdata;
    for &i in &order {
        let s = l.lsegs[i].clone();
        if s.absolute.is_some() {
            continue;
        }
        let (attrs, _) = seg_attrs(&s);
        // Which unit: the group's, a packed one, or a new one.
        let grp = s.group.filter(|&g| Some(g) != flat || !lx);
        let mut u = match grp {
            Some(g) => group_unit.get(&g).copied(),
            None => None,
        };
        if u.is_none() && grp.is_none() {
            let limit = if s.is_code() { packcode } else { packdata };
            if let (Some(limit), Some(last)) = (limit, units.len().checked_sub(1)) {
                let lu = &units[last];
                let lastseg = &l.lsegs[*lu.lsegs.last().unwrap()];
                let fits = (lu.size.next_multiple_of(s.align) as u64 + s.size as u64) <= limit as u64
                    && (lx || (lu.size as u64 + s.size as u64) <= 0x10000);
                let same_kind = lu.code == s.is_code() && lu.use32 == s.use32 && lastseg.group.is_none() && lu.attrs == attrs;
                if fits && same_kind && !(lx && !s.use32) {
                    u = Some(last);
                }
            }
        }
        let u = match u {
            Some(u) => u,
            None => {
                units.push(Unit {
                    code: s.is_code(),
                    use32: s.use32,
                    name: s.name.clone(),
                    class: s.class.clone(),
                    attrs: attrs.clone(),
                    alias16: !s.use32 || attrs.alias,
                    is_stack: s.combine == Combine::Stack || s.class.eq_ignore_ascii_case("STACK"),
                    ..Default::default()
                });
                if let Some(g) = grp {
                    group_unit.insert(g, units.len() - 1);
                }
                units.len() - 1
            }
        };
        let un = &mut units[u];
        let off = un.size.next_multiple_of(s.align.max(1));
        un.size = off + s.size;
        un.lsegs.push(i);
        un.use32 |= s.use32;
        if s.combine == Combine::Stack {
            un.is_stack = true;
        }
        l.lsegs[i].unit = u;
        l.lsegs[i].offset = off;
    }
    // A group's segments must be in one 64K segment for 16-bit code.
    for (gi, g) in l.groups.iter().enumerate() {
        if let Some(&u) = group_unit.get(&gi) {
            if !units[u].use32 && units[u].size > 0x10000 {
                let n = g.name.clone();
                l.msgs.fatal(l.host, "", 1074, &format!("{n} : group larger than 64K bytes"));
                return None;
            }
        }
    }
    for u in &units {
        if !u.use32 && u.size > 0x10000 {
            let n = u.name.clone();
            l.msgs.fatal(l.host, "", 1070, &format!("{n} : segment size exceeds 64K"));
            return None;
        }
    }
    for u in &mut units {
        u.data = vec![0; u.size as usize];
    }
    // LX: objects are placed at 64K-aligned addresses from the base.
    if lx {
        let mut va = 0u32;
        for u in &mut units {
            u.base = va;
            va = (va + u.size.max(1)).next_multiple_of(0x10000);
        }
    }
    let mut out = Linked {
        mods: Vec::new(),
        syms: Vec::new(),
        lsegs: Vec::new(),
        groups: Vec::new(),
        units,
        order,
        imports: Vec::new(),
        exports: Vec::new(),
        start: None,
        def: l.def.clone(),
        dgroup,
        autodata: dgroup.and_then(|g| group_unit.get(&g).copied()),
        stack_unit: None,
        stack_size: 0,
        heap_size: 0,
        dll: l.opt.dll,
        lx,
        module_name: String::new(),
        description: String::new(),
        errors,
        protmem: l.mods.iter().any(|m| m.obj.protmem),
    };
    // Imported modules are numbered in the order their names were defined.
    let mut seq = l.import_seq.clone();
    seq.extend(0..l.syms.len());
    for s in seq {
        if !l.syms[s].referenced {
            continue;
        }
        let r = l.resolve(s);
        if let SymDef::Import { module, .. } = &l.syms[r].def {
            if !out.imports.iter().any(|n| n.eq_ignore_ascii_case(module)) {
                out.imports.push(module.to_ascii_uppercase());
            }
        }
    }
    // Place the data.
    let mut fx = Fixer { l, out: &mut out, flat };
    for m in 0..fx.l.mods.len() {
        let data = core::mem::take(&mut fx.l.mods[m].obj.data);
        for d in &data {
            let Some((ls, base)) = fx.l.mods[m].seg(d.seg) else { continue };
            fx.place(m, ls, base + d.offset, &d.bytes, &d.fixups);
        }
        fx.l.mods[m].obj.data = data;
        let comdats = core::mem::take(&mut fx.l.mods[m].obj.comdats);
        for (ci, c) in comdats.iter().enumerate() {
            let Some(Some((ls, base))) = fx.l.mods[m].comdat_places.get(ci).copied() else { continue };
            fx.place(m, ls, base + c.offset, &c.bytes, &c.fixups);
        }
        fx.l.mods[m].obj.comdats = comdats;
        let bakpats = core::mem::take(&mut fx.l.mods[m].obj.bakpats);
        for b in &bakpats {
            let Some((ls, base)) = fx.l.mods[m].seg(b.seg) else { continue };
            let s = &fx.l.lsegs[ls];
            let (u, uo) = (s.unit, s.offset);
            for &(o, v) in &b.items {
                let at = (uo + base + o) as usize;
                let d = &mut fx.out.units[u].data;
                match b.kind {
                    0 if at < d.len() => d[at] = d[at].wrapping_add(v as u8),
                    1 if at + 2 <= d.len() => {
                        let w = u16::from_le_bytes([d[at], d[at + 1]]).wrapping_add(v as u16);
                        d[at..at + 2].copy_from_slice(&w.to_le_bytes());
                    }
                    2 if at + 4 <= d.len() => {
                        let w = u32::from_le_bytes(d[at..at + 4].try_into().unwrap()).wrapping_add(v);
                        d[at..at + 4].copy_from_slice(&w.to_le_bytes());
                    }
                    _ => {}
                }
            }
        }
    }
    // Start address.
    if let Some((m, st)) = fx.l.start {
        match fx.target_addr(m, &st.target, st.disp) {
            Addr::Seg { lseg, offset } => {
                let s = &fx.l.lsegs[lseg];
                let (u, off) = (s.unit, s.offset + offset);
                // 16-bit: the offset is relative to the frame (a group).
                let off = match fx.frame_base(m, &st.frame, &st.target, Some((u, off))) {
                    Some((fu, fo)) if fu == u && !fx.l.opt.lx => off - fo,
                    _ => off,
                };
                fx.out.start = Some((u, off));
            }
            Addr::Import { .. } => {
                fx.l.msgs.fatal(fx.l.host, "", 2000, "the starting address is an imported name");
                fx.out.errors += 1;
            }
            _ => {}
        }
    }
    fx.exports();
    let flatten_errors = fx.out.errors;
    let _ = flatten_errors;
    out.mods = core::mem::take(&mut l.mods);
    out.syms = l.syms.clone();
    out.lsegs = l.lsegs.clone();
    out.groups = l.groups.clone();
    Some(out)
}

struct Fixer<'a, 'h> {
    l: &'a mut Linker<'h>,
    out: &'a mut Linked,
    flat: Option<usize>,
}

impl<'a, 'h> Fixer<'a, 'h> {
    fn sym_addr(&self, s: usize) -> Addr {
        let s = self.l.resolve(s);
        match &self.l.syms[s].def {
            SymDef::Public { module, seg, offset, .. } => match self.l.mods[*module].seg(*seg) {
                Some((ls, base)) => Addr::Seg { lseg: ls, offset: base + offset },
                None => Addr::Undefined,
            },
            SymDef::Linker { lseg, offset } => Addr::Seg { lseg: *lseg, offset: *offset },
            SymDef::Absolute { frame, offset } => Addr::Abs { frame: *frame, offset: *offset },
            SymDef::Import { module, entry } => Addr::Import { module: module.clone(), entry: entry.clone(), offset: 0 },
            _ => Addr::Undefined,
        }
    }

    fn target_addr(&self, m: usize, t: &Target, disp: u32) -> Addr {
        let md = &self.l.mods[m];
        let a = match *t {
            Target::Seg(s) => match md.seg(s) {
                Some((ls, base)) => match self.l.lsegs[ls].absolute {
                    Some(fr) => Addr::Abs { frame: fr, offset: base },
                    None => Addr::Seg { lseg: ls, offset: base },
                },
                None => Addr::Undefined,
            },
            Target::Grp(g) => {
                let Some(&gi) = md.grpmap.get(g as usize - 1) else { return Addr::Undefined };
                if Some(gi) == self.flat {
                    return Addr::Abs { frame: 0, offset: disp };
                }
                match self.group_first(gi) {
                    Some(ls) => Addr::Seg { lseg: ls, offset: 0 },
                    None => Addr::Undefined,
                }
            }
            Target::Ext(e) => match md.extmap.get(e as usize - 1) {
                Some(&s) => self.sym_addr(s),
                None => Addr::Undefined,
            },
            Target::Abs(f) => Addr::Abs { frame: f, offset: 0 },
        };
        match a {
            Addr::Seg { lseg, offset } => Addr::Seg { lseg, offset: offset.wrapping_add(disp) },
            Addr::Abs { frame, offset } => Addr::Abs { frame, offset: offset.wrapping_add(disp) },
            Addr::Import { module, entry, offset } => Addr::Import { module, entry, offset: offset.wrapping_add(disp) },
            u => u,
        }
    }

    /// The lowest-placed segment of a group.
    fn group_first(&self, g: usize) -> Option<usize> {
        self.l.groups[g].lsegs.iter().copied().min_by_key(|&s| (self.l.lsegs[s].unit, self.l.lsegs[s].offset))
    }

    /// The frame's (unit, offset) for 16-bit offsets; None for no frame or
    /// FLAT.
    fn frame_base(&self, m: usize, f: &Frame, t: &Target, loc: Option<(usize, u32)>) -> Option<(usize, u32)> {
        let fb = self.frame_base16(m, f, t, loc);
        // LX: a frame other than FLAT is the whole object its target is in
        // (offsets count from the object's start, as IBM's LINK386 has it).
        // NE: likewise the physical segment (packed segments and groups
        // share its selector).
        fb.map(|(u, _)| (u, 0))
    }

    fn frame_base16(&self, m: usize, f: &Frame, t: &Target, loc: Option<(usize, u32)>) -> Option<(usize, u32)> {
        let md = &self.l.mods[m];
        let seg_frame = |ls: usize| {
            let s = &self.l.lsegs[ls];
            match s.group {
                // A segment of a group shares the group's selector.
                Some(g) if Some(g) != self.flat => {
                    let first = self.group_first(g)?;
                    let fs = &self.l.lsegs[first];
                    Some((fs.unit, fs.offset))
                }
                _ => Some((s.unit, s.offset)),
            }
        };
        match *f {
            Frame::Seg(s) => {
                let (ls, _) = md.seg(s)?;
                // Explicitly a segment's frame: that segment's start.
                let s = &self.l.lsegs[ls];
                Some((s.unit, if s.group.is_some() { seg_frame(ls)?.1 } else { s.offset }))
            }
            Frame::Grp(g) => {
                let gi = *md.grpmap.get(g as usize - 1)?;
                if Some(gi) == self.flat {
                    return None;
                }
                let first = self.group_first(gi)?;
                let fs = &self.l.lsegs[first];
                Some((fs.unit, fs.offset))
            }
            Frame::Ext(e) => {
                let s = self.l.resolve(*md.extmap.get(e as usize - 1)?);
                match &self.l.syms[s].def {
                    SymDef::Public { module, seg, group, .. } => {
                        let mm = &self.l.mods[*module];
                        if *group != 0 {
                            let gi = *mm.grpmap.get(*group as usize - 1)?;
                            if Some(gi) == self.flat {
                                return None;
                            }
                            let first = self.group_first(gi)?;
                            let fs = &self.l.lsegs[first];
                            return Some((fs.unit, fs.offset));
                        }
                        seg_frame(mm.seg(*seg)?.0)
                    }
                    SymDef::Linker { lseg, .. } => seg_frame(*lseg),
                    _ => None,
                }
            }
            Frame::Location => loc.map(|(u, _)| {
                // The location's own segment.
                let ls = self.out.units[u].lsegs.iter().copied().find(|&ls| {
                    let s = &self.l.lsegs[ls];
                    loc.unwrap().1 >= s.offset && loc.unwrap().1 < s.offset + s.size.max(1)
                });
                match ls.and_then(seg_frame) {
                    Some(f) => f,
                    None => (u, 0),
                }
            }),
            Frame::Target => match *t {
                Target::Seg(s) => seg_frame(md.seg(s)?.0),
                Target::Grp(g) => self.frame_base16(m, &Frame::Grp(g), t, loc),
                Target::Ext(e) => self.frame_base16(m, &Frame::Ext(e), t, loc),
                Target::Abs(_) => None,
            },
            Frame::Abs(_) | Frame::None => None,
        }
    }

    fn import_module(&mut self, name: &str) -> usize {
        if let Some(i) = self.out.imports.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            return i;
        }
        self.out.imports.push(name.to_ascii_uppercase());
        self.out.imports.len() - 1
    }

    fn place(&mut self, m: usize, ls: usize, at: u32, bytes: &[u8], fixups: &[omf::Fixup]) {
        let s = &self.l.lsegs[ls];
        if s.absolute.is_some() {
            return;
        }
        let (u, base) = (s.unit, s.offset);
        let start = (base + at) as usize;
        let segend = (base + s.size) as usize;
        {
            let d = &mut self.out.units[u].data;
            if start + bytes.len() > segend.max(start) || start + bytes.len() > d.len() {
                let n = self.l.lsegs[ls].name.clone();
                let file = self.l.mods[m].file.clone();
                self.l.msgs.fatal(self.l.host, &file, 1103, &format!("{n} : attempt to access data outside segment bounds"));
                self.out.errors += 1;
                return;
            }
            d[start..start + bytes.len()].copy_from_slice(bytes);
            let un = &mut self.out.units[u];
            un.init = un.init.max((start + bytes.len()) as u32);
        }
        for f in fixups {
            self.fixup(m, u, (start as u32) + f.offset, f, ls);
        }
    }

    fn fixup_err(&mut self, m: usize, num: u32, what: &str, u: usize, at: u32) {
        let seg = self.out.units[u].name.clone();
        let file = self.l.mods[m].file.clone();
        self.l.msgs.error(self.l.host, &file, num, &format!("{what} at {at:x} in segment {seg}"));
        self.out.errors += 1;
    }

    fn fixup(&mut self, m: usize, u: usize, at: u32, f: &omf::Fixup, _ls: usize) {
        let size = f.loc.size();
        if at as usize + size > self.out.units[u].data.len() {
            self.fixup_err(m, 1103, "fix-up outside the data", u, at);
            return;
        }
        let tgt = self.target_addr(m, &f.target, f.disp);
        let lx = self.l.opt.lx;
        let rd = |d: &[u8], at: usize, n: usize| -> u32 {
            let mut v = 0u32;
            for i in 0..n {
                v |= (d[at + i] as u32) << (8 * i);
            }
            v
        };
        let atu = at as usize;
        let existing = rd(&self.out.units[u].data, atu, size.min(4));
        let wr = |d: &mut Vec<u8>, at: usize, n: usize, v: u32| {
            for i in 0..n {
                d[at + i] = (v >> (8 * i)) as u8;
            }
        };
        match tgt {
            Addr::Undefined => {}
            Addr::Abs { frame, offset } => {
                // Constants: a selector or offset as given.
                let v = match f.loc {
                    Loc::Base => frame as u32,
                    Loc::Ptr1616 => (offset & 0xffff) | (frame as u32) << 16,
                    _ => offset,
                };
                if !f.seg_rel {
                    // Self-relative to an absolute address: only FLAT makes
                    // sense here (LX), and needs a fix-up.
                    if lx {
                        self.out.units[u].relocs.push(Reloc {
                            offset: at,
                            kind: RelocKind::SelfRel32,
                            target: RTarget::Internal { unit: usize::MAX, offset: offset.wrapping_add(existing) },
                            add: 0,
                        });
                    }
                    return;
                }
                let d = &mut self.out.units[u].data;
                match f.loc {
                    Loc::HiByte => wr(d, atu, 1, (existing + (v >> 8)) & 0xff),
                    Loc::Ptr1632 => {
                        wr(d, atu, 4, existing.wrapping_add(offset));
                        wr(d, atu + 4, 2, frame as u32);
                    }
                    _ => wr(d, atu, size.min(4), existing.wrapping_add(v)),
                }
            }
            Addr::Import { module, entry, offset } => {
                let mi = self.import_module(&module);
                let target = match entry {
                    ImpEntry::Ordinal(o) => RTarget::ImportOrd { module: mi, ordinal: o },
                    ImpEntry::Name(n) => RTarget::ImportName { module: mi, name: n },
                };
                let kind = match (f.loc, f.seg_rel) {
                    (Loc::Offset32 | Loc::LoaderOffset32, false) => RelocKind::SelfRel32,
                    (Loc::Offset16 | Loc::LoaderOffset16, false) => RelocKind::SelfRel16,
                    (Loc::LoByte, _) => RelocKind::LoByte,
                    (Loc::Base, _) => RelocKind::Selector,
                    (Loc::Ptr1616, _) => RelocKind::Ptr1616,
                    (Loc::Offset16 | Loc::LoaderOffset16, _) => RelocKind::Offset16,
                    (Loc::Offset32 | Loc::LoaderOffset32, _) => RelocKind::Offset32,
                    (Loc::Ptr1632, _) => RelocKind::Ptr1632,
                    (Loc::HiByte, _) => {
                        self.fixup_err(m, 2005, "fix-up type unsupported", u, at);
                        return;
                    }
                };
                let add = match kind {
                    RelocKind::Selector => 0,
                    RelocKind::Ptr1616 | RelocKind::Offset16 | RelocKind::SelfRel16 => (existing as u16).wrapping_add(offset as u16) as i16 as i32,
                    _ => existing.wrapping_add(offset) as i32,
                };
                if !lx && matches!(kind, RelocKind::SelfRel16 | RelocKind::SelfRel32) {
                    self.fixup_err(m, 2003, "intersegment self-relative fix-up", u, at);
                    return;
                }
                self.out.units[u].relocs.push(Reloc { offset: at, kind, target, add });
            }
            Addr::Seg { lseg, offset } => {
                let s = &self.l.lsegs[lseg];
                let (tu, toff) = (s.unit, s.offset + offset);
                if !f.seg_rel {
                    // Self-relative: displacement from the end of the location.
                    let end = at + size as u32;
                    if tu == u {
                        let v = existing.wrapping_add(toff.wrapping_sub(end));
                        let d = &mut self.out.units[u].data;
                        wr(d, atu, size.min(4), v);
                    } else if lx {
                        self.out.units[u].relocs.push(Reloc {
                            offset: at,
                            kind: if size == 2 { RelocKind::SelfRel16 } else { RelocKind::SelfRel32 },
                            target: RTarget::Internal { unit: tu, offset: toff.wrapping_add(existing) },
                            add: 0,
                        });
                    } else {
                        self.fixup_err(m, 2003, "intersegment self-relative fix-up", u, at);
                    }
                    return;
                }
                // A 32-bit offset into a 32-bit object is a flat address
                // whatever frame the object module names: OS/2 2.x has no
                // other 32-bit segments. (IBM's LINK386 makes it relative to
                // the object when the frame is a segment, which no program
                // can use: Open Watcom's run time names segment frames and
                // crashed when linked by it.)
                let flat_frame = lx
                    && (self.frame_base(m, &f.frame, &f.target, Some((u, at))).is_none()
                        || (matches!(f.loc, Loc::Offset32 | Loc::LoaderOffset32) && self.out.units[tu].use32 && !self.l.opt.ibm_frames));
                match f.loc {
                    Loc::Offset32 | Loc::LoaderOffset32 if flat_frame => {
                        // A flat address: needs a fix-up in LX (objects move).
                        let v = toff.wrapping_add(existing);
                        self.out.units[u].relocs.push(Reloc {
                            offset: at,
                            kind: RelocKind::Offset32,
                            target: RTarget::Internal { unit: tu, offset: v },
                            add: 0,
                        });
                        let b = self.out.units[tu].base;
                        let d = &mut self.out.units[u].data;
                        wr(d, atu, 4, b.wrapping_add(v));
                    }
                    Loc::Offset16 | Loc::LoaderOffset16 | Loc::Offset32 | Loc::LoaderOffset32 | Loc::LoByte | Loc::HiByte => {
                        let fb = self.frame_base(m, &f.frame, &f.target, Some((u, at)));
                        let rel = match fb {
                            Some((fu, fo)) if fu == tu => toff.wrapping_sub(fo),
                            Some(_) => {
                                // Target outside the frame.
                                self.fixup_err(m, 2002, "fix-up overflow", u, at);
                                return;
                            }
                            None => toff,
                        };
                        let v = match f.loc {
                            Loc::HiByte => existing.wrapping_add(rel >> 8),
                            _ => existing.wrapping_add(rel),
                        };
                        if matches!(f.loc, Loc::Offset16 | Loc::LoaderOffset16) && !lx && (rel > 0xffff) {
                            self.fixup_err(m, 2002, "fix-up overflow", u, at);
                            return;
                        }
                        let d = &mut self.out.units[u].data;
                        wr(d, atu, size.min(4), v);
                        if lx && matches!(f.loc, Loc::Offset16 | Loc::LoaderOffset16) && !self.out.units[tu].use32 {
                            // 16-bit offsets into 16-bit objects need nothing
                            // more: the object's tiles keep offsets.
                        }
                    }
                    Loc::Base => {
                        let tu2 = match self.frame_base(m, &f.frame, &f.target, Some((u, at))) {
                            Some((fu, _)) => fu,
                            None => tu,
                        };
                        self.out.units[u].relocs.push(Reloc {
                            offset: at,
                            kind: RelocKind::Selector,
                            target: RTarget::Internal { unit: tu2, offset: 0 },
                            add: 0,
                        });
                    }
                    Loc::Ptr1616 => {
                        let fb = self.frame_base(m, &f.frame, &f.target, Some((u, at)));
                        let (fu, rel) = match fb {
                            Some((fu, fo)) if fu == tu => (fu, toff - fo),
                            _ => (tu, toff),
                        };
                        let rel = rel.wrapping_add(existing & 0xffff);
                        if lx {
                            self.out.units[u].relocs.push(Reloc {
                                offset: at,
                                kind: RelocKind::Ptr1616,
                                target: RTarget::Internal { unit: fu, offset: rel },
                                add: 0,
                            });
                        } else {
                            // NE: the offset is known now; only the selector
                            // is the loader's (IBM's LINK chains it with the
                            // segment's other selector fix-ups).
                            let d = &mut self.out.units[u].data;
                            wr(d, atu, 2, rel);
                            self.out.units[u].relocs.push(Reloc {
                                offset: at + 2,
                                kind: RelocKind::Selector,
                                target: RTarget::Internal { unit: fu, offset: 0 },
                                add: 0,
                            });
                        }
                    }
                    Loc::Ptr1632 if !lx => {
                        let d = &mut self.out.units[u].data;
                        wr(d, atu, 4, toff.wrapping_add(existing));
                        self.out.units[u].relocs.push(Reloc {
                            offset: at + 4,
                            kind: RelocKind::Selector,
                            target: RTarget::Internal { unit: tu, offset: 0 },
                            add: 0,
                        });
                    }
                    Loc::Ptr1632 => {
                        let rel = toff.wrapping_add(existing);
                        self.out.units[u].relocs.push(Reloc {
                            offset: at,
                            kind: RelocKind::Ptr1632,
                            target: RTarget::Internal { unit: tu, offset: rel },
                            add: 0,
                        });
                    }
                }
            }
        }
    }

    /// Exports from the .DEF file and EXPDEF records, with ordinals.
    fn exports(&mut self) {
        let mut list: Vec<ExportEnt> = Vec::new();
        let add = |list: &mut Vec<ExportEnt>, e: ExportEnt| {
            if let Some(p) = list.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&e.name)) {
                if p.ordinal == 0 {
                    p.ordinal = e.ordinal;
                }
                return;
            }
            list.push(e);
        };
        let def_exports = self.l.def.exports.clone();
        for (i, e) in def_exports.iter().enumerate() {
            // LINK takes one export per internal name (L4023); LINK386 allows
            // aliases.
            if !self.l.opt.lx {
                if let Some(p) = def_exports[..i].iter().find(|p| p.internal.eq_ignore_ascii_case(&e.internal)) {
                    let t = format!("{}({}) : export internal name conflict", p.name, p.internal);
                    self.l.msgs.warn(self.l.host, "", 4023, &t);
                    continue;
                }
            }
            add(
                &mut list,
                ExportEnt {
                    name: e.name.clone(),
                    explicit: e.ordinal.is_some(),
                    sym: None,
                    ordinal: e.ordinal.unwrap_or(0),
                    resident: e.resident,
                    noname: e.noname,
                    nodata: e.nodata,
                    private: e.private,
                    pwords: e.pwords,
                    at: None,
                    forward: None,
                    absolute: None,
                },
            );
            if let Some(l) = list.last_mut() {
                l.sym = Some(usize::MAX);
            }
            // Remember the internal name.
            let n = list.len() - 1;
            list[n].forward = None;
            let internal = e.internal.clone();
            let si = self.l.symtab.get(&key(&internal, self.l.opt.case_sensitive)).copied();
            list[n].sym = si.or(Some(usize::MAX - 1));
            if si.is_none() {
                let file = String::new();
                self.l.msgs.error(self.l.host, &file, 2022, &format!("{} (alias {}) : export undefined", e.name, internal));
                self.out.errors += 1;
            }
        }
        for m in 0..self.l.mods.len() {
            for e in self.l.mods[m].obj.expdefs.clone() {
                let si = self.l.symtab.get(&key(&e.internal, self.l.opt.case_sensitive)).copied();
                add(
                    &mut list,
                    ExportEnt {
                        name: e.name.clone(),
                        explicit: e.ordinal.is_some(),
                        sym: si,
                        ordinal: e.ordinal.unwrap_or(0),
                        resident: e.resident,
                        noname: false,
                        nodata: e.nodata,
                        private: false,
                        pwords: e.parms,
                        at: None,
                        forward: None,
                        absolute: None,
                    },
                );
            }
        }
        // Ordinals for the rest, after the highest given one.
        let mut used: Vec<u16> = list.iter().map(|e| e.ordinal).filter(|&o| o != 0).collect();
        used.sort();
        for i in 1..used.len() {
            if used[i] == used[i - 1] {
                let o = used[i];
                self.l.msgs.error(self.l.host, "", 2026, &format!("entry ordinal {o} : multiple definitions for same ordinal"));
                self.out.errors += 1;
            }
        }
        let mut next = 1u16;
        for e in &mut list {
            if e.ordinal == 0 {
                while used.contains(&next) {
                    next += 1;
                }
                e.ordinal = next;
                used.push(next);
            }
        }
        for e in &mut list {
            let Some(s) = e.sym.filter(|&s| s < self.l.syms.len()) else { continue };
            match self.sym_addr(s) {
                Addr::Seg { lseg, offset } => {
                    let ls = &self.l.lsegs[lseg];
                    let off = ls.offset + offset;
                    // 16-bit: offsets are relative to the unit (group) start.
                    e.at = Some((ls.unit, off));
                }
                Addr::Abs { offset, .. } => e.absolute = Some(offset),
                Addr::Import { module, entry, .. } => {
                    let mi = self.import_module(&module);
                    e.forward = Some((mi, entry));
                }
                Addr::Undefined => {}
            }
        }
        self.out.exports = list;
    }
}
