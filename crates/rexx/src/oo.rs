//! Object REXX: the object world (built-in classes, .environment, .local),
//! packages and their directives, message sending and method
//! activations, and the instructions Object REXX adds.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use crate::exec::*;
use crate::lex::{Tok, Token, upper};
use crate::num::int_str;
use crate::parse::*;
use crate::value::*;
use crate::vars::{Key as VKey, Pool, PoolRef};
use crate::{CallType, Err, ExtArg, ExtErr};

/// Where the method search starts.
#[derive(Clone)]
pub enum Start {
    Normal,
    /// After this scope (`~msg:super`, FORWARD CLASS (super)).
    Super(ObjRef, bool),
    /// At this class.
    At(ObjRef),
}

/// A method found for a message.
#[derive(Clone)]
pub struct Found {
    pub m: Method,
    pub scope: Option<ObjRef>,
    pub class_side: bool,
}

/// What the interpreter calls of Object REXX, through the world (so that
/// a classic REXX that never makes one does not link it).
pub struct Ops {
    pub load_package: fn(&[u8], &[u8]) -> Result<Rc<Package>, LoadErr>,
    pub install_package: fn(&mut Interp<'_>, &mut Frame, &Rc<Package>) -> R<()>,
    pub obj_string: fn(&mut Interp<'_>, &mut Frame, &ObjRef) -> R<Val>,
    pub stem_object: fn(&mut Interp<'_>, &[u8]) -> Value,
    pub unary_op: fn(&mut Interp<'_>, &mut Frame, UnOp, &Value, u32) -> R<Option<Value>>,
    pub binary_op: fn(&mut Interp<'_>, &mut Frame, BinOp, Value, Value, u32) -> R<Value>,
    pub eval_oo: fn(&mut Interp<'_>, &mut Frame, &Expr, u32) -> R<Value>,
    pub call_builtin_oo: fn(&mut Interp<'_>, &mut Frame, &[u8], &[Option<Value>]) -> Option<R<Option<Value>>>,
    pub oo_bif: fn(&[u8], &[Option<Val>]) -> Option<R<Val>>,
    pub call_package_routine: fn(&mut Interp<'_>, &mut Frame, &[u8], Vec<Option<Value>>, bool, u32) -> Option<R<Option<Value>>>,
    pub(crate) exec_oo: fn(&mut Interp<'_>, &mut Frame, &Clause, u32) -> R<Next>,
    pub over_items: fn(&mut Interp<'_>, &mut Frame, &Value) -> R<Vec<Value>>,
    pub(crate) reply_elsewhere: fn(&mut Interp<'_>, &Frame, &Rc<Program>, usize, u32, &[Ctl]) -> bool,
    pub secondary: fn(u32) -> Option<&'static [u8]>,
}

fn install(it: &mut Interp<'_>, f: &mut Frame, p: &Rc<Package>) -> R<()> {
    it.install_package(f, p)
}

pub static OPS: Ops = Ops {
    load_package,
    install_package: install,
    obj_string,
    stem_object,
    unary_op,
    binary_op,
    eval_oo,
    call_builtin_oo,
    oo_bif,
    call_package_routine,
    exec_oo,
    over_items,
    reply_elsewhere,
    secondary: crate::classes::secondary,
};

pub struct World {
    /// The interpreter's way into Object REXX.
    pub ops: &'static Ops,
    next: Cell<u64>,
    pub nil: ObjRef,
    /// The built-in classes, by `Builtin as usize`.
    pub classes: Vec<ObjRef>,
    pub env: ObjRef,
    pub local: ObjRef,
    /// Packages loaded by ::REQUIRES, by full name.
    pub packages: RefCell<BTreeMap<Vec<u8>, Rc<Package>>>,
    /// Proxy classes of other object systems, by (kind, class id).
    pub ext_classes: RefCell<BTreeMap<(u32, Vec<u8>), ObjRef>>,
    /// REXX objects handed to other object systems (ExtArg::Rexx).
    pub handed: RefCell<Vec<Value>>,
    /// The activities that ran (for GUARD bookkeeping).
    pub activities: Cell<u64>,
}

const BUILTINS: &[(Builtin, &[u8])] = &[
    (Builtin::Object, b"Object"),
    (Builtin::Class, b"Class"),
    (Builtin::String, b"String"),
    (Builtin::Array, b"Array"),
    (Builtin::List, b"List"),
    (Builtin::Queue, b"Queue"),
    (Builtin::Directory, b"Directory"),
    (Builtin::Table, b"Table"),
    (Builtin::Relation, b"Relation"),
    (Builtin::Set, b"Set"),
    (Builtin::Bag, b"Bag"),
    (Builtin::Stem, b"Stem"),
    (Builtin::Supplier, b"Supplier"),
    (Builtin::Message, b"Message"),
    (Builtin::Method, b"Method"),
    (Builtin::Monitor, b"Monitor"),
    (Builtin::Stream, b"Stream"),
    (Builtin::Alarm, b"Alarm"),
];

fn raw_obj(id: u64, class: Option<ObjRef>, data: Data) -> ObjRef {
    Rc::new(Object {
        id,
        class: RefCell::new(class),
        data: RefCell::new(data),
        vars: RefCell::new(Vec::new()),
        methods: RefCell::new(None),
        name: RefCell::new(None),
        guard: Cell::new((0, 0)),
    })
}

impl World {
    pub fn new() -> Rc<World> {
        let mut next = 1u64;
        let mut classes: Vec<ObjRef> = Vec::new();
        for (b, id) in BUILTINS {
            let mut ci = ClassInfo::new(id.to_vec());
            ci.builtin = Some(*b);
            classes.push(raw_obj(next, None, Data::Class(alloc::boxed::Box::new(ci))));
            next += 1;
        }
        let object = classes[0].clone();
        let class = classes[1].clone();
        for (i, c) in classes.iter().enumerate() {
            *c.class.borrow_mut() = Some(class.clone());
            if i > 0 {
                if let Data::Class(ci) = &mut *c.data.borrow_mut() {
                    ci.supers.push(object.clone());
                }
                if let Data::Class(ci) = &mut *object.data.borrow_mut() {
                    ci.subs.push(Rc::downgrade(c));
                }
            }
        }
        // the special collections
        let nil = raw_obj(next, Some(object.clone()), Data::Nil);
        next += 1;
        let dir = classes[Builtin::Directory as usize].clone();
        let env = raw_obj(next, Some(dir.clone()), Data::Directory(DirData::default()));
        next += 1;
        let local = raw_obj(next, Some(dir.clone()), Data::Directory(DirData::default()));
        next += 1;
        let w = World {
            ops: &OPS,
            next: Cell::new(next),
            nil,
            classes,
            env,
            local,
            packages: RefCell::new(BTreeMap::new()),
            ext_classes: RefCell::new(BTreeMap::new()),
            handed: RefCell::new(Vec::new()),
            activities: Cell::new(1),
        };
        {
            let mut e = w.env.data.borrow_mut();
            if let Data::Directory(d) = &mut *e {
                for (i, (_, id)) in BUILTINS.iter().enumerate() {
                    d.map.insert(upper(id), Value::O(w.classes[i].clone()));
                }
                d.map.insert(b"NIL".to_vec(), Value::O(w.nil.clone()));
                d.map.insert(b"TRUE".to_vec(), Value::S(b"1".to_vec()));
                d.map.insert(b"FALSE".to_vec(), Value::S(b"0".to_vec()));
                d.map.insert(b"ENVIRONMENT".to_vec(), Value::O(w.env.clone()));
            }
        }
        // the standard streams and the monitors on them
        let stream = w.classes[Builtin::Stream as usize].clone();
        let monitor = w.classes[Builtin::Monitor as usize].clone();
        let mut entries = Vec::new();
        for (name, mon) in [(&b"STDIN"[..], &b"INPUT"[..]), (b"STDOUT", b"OUTPUT"), (b"STDERR", b"ERROR")] {
            let s = w.new_obj(&stream, Data::Stream(StreamData { name: name.to_vec() }));
            let m = w.new_obj(&monitor, Data::Monitor(Value::O(s.clone())));
            entries.push((name.to_vec(), Value::O(s)));
            entries.push((mon.to_vec(), Value::O(m)));
        }
        if let Data::Directory(d) = &mut *w.local.data.borrow_mut() {
            for (k, v) in entries {
                d.map.insert(k, v);
            }
            d.map.insert(b"LOCAL".to_vec(), Value::O(w.local.clone()));
        }
        Rc::new(w)
    }

    /// A new activity's number.
    pub fn next_activity(&self) -> u64 {
        let n = self.activities.get() + 1;
        self.activities.set(n);
        n
    }

    pub fn id(&self) -> u64 {
        let n = self.next.get();
        self.next.set(n + 1);
        n
    }

    pub fn class(&self, b: Builtin) -> ObjRef {
        self.classes[b as usize].clone()
    }

    pub fn new_obj(&self, class: &ObjRef, data: Data) -> ObjRef {
        raw_obj(self.id(), Some(class.clone()), data)
    }

    pub fn obj(&self, b: Builtin, data: Data) -> Value {
        Value::O(self.new_obj(&self.class(b), data))
    }

    pub fn array(&self, items: Vec<Value>) -> Value {
        self.obj(Builtin::Array, Data::Array(ArrayData { items: items.into_iter().map(Some).collect(), dims: Vec::new() }))
    }

    pub fn array_opt(&self, items: Vec<Option<Value>>) -> Value {
        self.obj(Builtin::Array, Data::Array(ArrayData { items, dims: Vec::new() }))
    }

    pub fn nil(&self) -> Value {
        Value::O(self.nil.clone())
    }

    /// A new class object.
    pub fn new_class(&self, id: Vec<u8>, supers: Vec<ObjRef>, meta: ObjRef) -> ObjRef {
        let mut ci = ClassInfo::new(id);
        ci.supers = supers.clone();
        let c = self.new_obj(&meta, Data::Class(alloc::boxed::Box::new(ci)));
        for s in &supers {
            if let Data::Class(si) = &mut *s.data.borrow_mut() {
                si.subs.push(Rc::downgrade(&c));
            }
        }
        c
    }

    /// The program ended: nothing to do yet (alarms and other activities
    /// end with it).
    pub fn finish(&self) {}

    pub fn dir_get(&self, d: &ObjRef, k: &[u8]) -> Option<Value> {
        match &*d.data.borrow() {
            Data::Directory(d) => d.map.get(k).cloned(),
            _ => None,
        }
    }
}

// ---- classes ----

pub fn class_info<T>(c: &ObjRef, f: impl FnOnce(&ClassInfo) -> T) -> Option<T> {
    match &*c.data.borrow() {
        Data::Class(ci) => Some(f(ci)),
        _ => None,
    }
}

/// The method resolution order of a class: the class, its mixins, its
/// superclass chain; a class shared by several paths comes last.
pub fn mro(c: &ObjRef) -> Vec<ObjRef> {
    let mut all = Vec::new();
    fn walk(c: &ObjRef, out: &mut Vec<ObjRef>) {
        out.push(c.clone());
        let supers = class_info(c, |ci| ci.supers.clone()).unwrap_or_default();
        // mixins (inherited) first, then the superclass
        for s in supers.iter().skip(1) {
            walk(s, out);
        }
        if let Some(s) = supers.first() {
            walk(s, out);
        }
    }
    walk(c, &mut all);
    let mut out: Vec<ObjRef> = Vec::new();
    for (i, c) in all.iter().enumerate() {
        if !all[i + 1..].iter().any(|x| Rc::ptr_eq(x, c)) {
            out.push(c.clone());
        }
    }
    out
}

/// The search chain for a receiver: (class, class side) levels.
fn chain(w: &World, recv: &Value) -> Vec<(ObjRef, bool)> {
    match recv {
        Value::S(_) => mro(&w.class(Builtin::String)).into_iter().map(|c| (c, false)).collect(),
        Value::O(o) => {
            let mut v = Vec::new();
            if o.is_class() {
                for c in mro(o) {
                    v.push((c, true));
                }
            }
            for c in mro(&o.class()) {
                v.push((c, false));
            }
            v
        }
    }
}

/// Find the method for a message.
pub fn lookup(w: &World, recv: &Value, name: &[u8], start: &Start) -> Option<Found> {
    if let (Value::O(o), Start::Normal) = (recv, start) {
        if let Some(m) = o.methods.borrow().as_ref().and_then(|t| t.get(name).cloned()) {
            return Some(Found { m, scope: None, class_side: false });
        }
    }
    let ch = chain(w, recv);
    let from = match start {
        Start::Normal => 0,
        Start::Super(s, side) => match ch.iter().position(|(c, sd)| Rc::ptr_eq(c, s) && sd == side) {
            Some(i) => i + 1,
            None => ch.iter().position(|(c, _)| Rc::ptr_eq(c, s)).map_or(ch.len(), |i| i + 1),
        },
        Start::At(c) => ch.iter().position(|(x, _)| Rc::ptr_eq(x, c)).unwrap_or(ch.len()),
    };
    for (c, side) in &ch[from..] {
        let r = class_info(c, |ci| {
            let t = if *side { &ci.cls } else { &ci.inst };
            if let Some(m) = t.get(name) {
                return Some(m.clone());
            }
            if let Some(b) = ci.builtin {
                if crate::classes::has_native(b, *side, name) {
                    let private = b == Builtin::Object && !*side && matches!(name, b"RUN" | b"SETMETHOD" | b"UNSETMETHOD");
                    return Some(Method { body: Body::Native(b), private, guarded: false, protected: false });
                }
            }
            None
        })
        .flatten();
        if let Some(m) = r {
            if let Body::Null = m.body {
                return None;
            }
            return Some(Found { m, scope: Some(c.clone()), class_side: *side });
        }
    }
    None
}

/// The superclass of a method's scope (the SUPER variable).
fn super_of(w: &World, scope: &Option<ObjRef>) -> Value {
    match scope {
        Some(c) => class_info(c, |ci| ci.supers.first().cloned()).flatten().map(Value::O).unwrap_or_else(|| w.nil()),
        None => w.nil(),
    }
}

/// An object's variable pool for a scope.
pub fn obj_pool(o: &ObjRef, scope: &Option<ObjRef>, side: bool) -> PoolRef {
    let key = scope.as_ref().map_or(0, |c| c.id * 2 + side as u64);
    let mut vars = o.vars.borrow_mut();
    if let Some((_, p)) = vars.iter().find(|(k, _)| *k == key) {
        return p.clone();
    }
    let p: PoolRef = Rc::new(RefCell::new(Pool::default()));
    vars.push((key, p.clone()));
    p
}

// ---- strings of objects ----

/// The string of an object: its STRING method if REXX code defines one,
/// else its default string.
pub fn obj_string(it: &mut Interp, f: &mut Frame, o: &ObjRef) -> R<Val> {
    let w = it.w.clone().unwrap();
    let v = Value::O(o.clone());
    if let Some(found) = lookup(&w, &v, b"STRING", &Start::Normal) {
        if !matches!(found.m.body, Body::Native(_)) {
            let r = invoke(it, f, &v, b"STRING", Vec::new(), found, 0)?;
            return match r {
                Some(Value::S(s)) => Ok(s),
                Some(Value::O(x)) => Ok(x.fast_string()),
                None => Err(Err::new(44)),
            };
        }
    }
    Ok(o.fast_string())
}

/// The stem object of a stem variable.
pub fn stem_object(it: &mut Interp, name: &[u8]) -> Value {
    let w = it.w.clone().unwrap();
    let s = it.st.pools.stem(it.st.cur_pool, name);
    w.obj(Builtin::Stem, Data::Stem(s, name.to_vec()))
}

// ---- packages ----

fn sym_or_str(t: &Token) -> Option<(Vec<u8>, bool)> {
    match &t.tok {
        Tok::Sym(s) => Some((upper(s), false)),
        Tok::Str { val, .. } => Some((val.clone(), true)),
        _ => None,
    }
}

/// Parse a program file's directives into a package.
pub fn load_package(src: &[u8], name: &[u8]) -> Result<Rc<Package>, LoadErr> {
    let secs = crate::parse::sections(src)?;
    let mut it = secs.into_iter();
    let main = it.next().unwrap();
    let mut pkg = Package {
        name: name.to_vec(),
        main: Rc::new(main.prog),
        routines: BTreeMap::new(),
        classes: Vec::new(),
        floating: Vec::new(),
        requires: Vec::new(),
        installed: RefCell::new(Installed::default()),
    };
    for s in it {
        let d = s.dir.unwrap_or_default();
        let line = s.line;
        let e = |n: u32| (Err::new(n), line);
        let kw = d.first().and_then(|t| t.upper()).ok_or(e(99))?;
        let mut i = 1;
        let next_word = |i: &mut usize| -> Option<Vec<u8>> {
            let u = d.get(*i).and_then(|t| t.upper());
            if u.is_some() {
                *i += 1;
            }
            u
        };
        match kw.as_slice() {
            b"CLASS" => {
                let (nm, quoted) = d.get(i).and_then(sym_or_str).ok_or(e(19))?;
                i += 1;
                let mut cd = ClassDir {
                    name: upper(&nm),
                    id: if quoted { nm.clone() } else { upper(&nm) },
                    public: false,
                    metaclass: None,
                    subclass: None,
                    mixin: false,
                    inherit: Vec::new(),
                    external: None,
                    methods: Vec::new(),
                    line,
                };
                while i < d.len() {
                    let w = next_word(&mut i).ok_or(e(25))?;
                    match w.as_slice() {
                        b"PUBLIC" => cd.public = true,
                        b"METACLASS" => cd.metaclass = Some(upper(&d.get(i).and_then(sym_or_str).ok_or(e(19))?.0)),
                        b"SUBCLASS" | b"MIXINCLASS" => {
                            cd.mixin = w.as_slice() == b"MIXINCLASS";
                            cd.subclass = Some(upper(&d.get(i).and_then(sym_or_str).ok_or(e(19))?.0));
                        }
                        b"INHERIT" => {
                            while let Some((n, _)) = d.get(i).and_then(sym_or_str) {
                                cd.inherit.push(upper(&n));
                                i += 1;
                            }
                            continue;
                        }
                        b"EXTERNAL" => cd.external = Some(d.get(i).and_then(sym_or_str).ok_or(e(19))?.0),
                        _ => return Err(e(25)),
                    }
                    if matches!(w.as_slice(), b"METACLASS" | b"SUBCLASS" | b"MIXINCLASS" | b"EXTERNAL") {
                        i += 1;
                    }
                }
                if pkg.classes.iter().any(|c| c.name == cd.name) {
                    return Err(e(99));
                }
                pkg.classes.push(cd);
                if !s.prog.clauses.is_empty() {
                    // code after ::CLASS belongs to no method
                    return Err((Err::new(99), s.prog.clauses[0].line));
                }
            }
            b"METHOD" => {
                let (nm, _) = d.get(i).and_then(sym_or_str).ok_or(e(19))?;
                i += 1;
                let mut md = MethodDir {
                    name: upper(&nm),
                    class_side: false,
                    attribute: false,
                    private: false,
                    guarded: true,
                    protected: false,
                    external: None,
                    prog: Rc::new(Program::empty()),
                    source: Vec::new(),
                    line,
                };
                while i < d.len() {
                    let w = next_word(&mut i).ok_or(e(25))?;
                    match w.as_slice() {
                        b"CLASS" => md.class_side = true,
                        b"ATTRIBUTE" => md.attribute = true,
                        b"PRIVATE" => md.private = true,
                        b"PUBLIC" => md.private = false,
                        b"GUARDED" => md.guarded = true,
                        b"UNGUARDED" => md.guarded = false,
                        b"PROTECTED" => md.protected = true,
                        b"UNPROTECTED" => md.protected = false,
                        b"EXTERNAL" => {
                            md.external = Some(d.get(i).and_then(sym_or_str).ok_or(e(19))?.0);
                            i += 1;
                        }
                        _ => return Err(e(25)),
                    }
                }
                md.prog = Rc::new(s.prog);
                md.source = s.source;
                match pkg.classes.last_mut() {
                    Some(c) => {
                        if c.methods.iter().any(|m| m.name == md.name && m.class_side == md.class_side) {
                            return Err(e(99));
                        }
                        c.methods.push(md);
                    }
                    None => {
                        if md.class_side {
                            return Err(e(99));
                        }
                        pkg.floating.push(md);
                    }
                }
            }
            b"ROUTINE" => {
                let (nm, _) = d.get(i).and_then(sym_or_str).ok_or(e(19))?;
                i += 1;
                let mut public = false;
                while i < d.len() {
                    let w = next_word(&mut i).ok_or(e(25))?;
                    match w.as_slice() {
                        b"PUBLIC" => public = true,
                        b"EXTERNAL" => i += 1,
                        _ => return Err(e(25)),
                    }
                }
                let name = upper(&nm);
                if pkg.routines.contains_key(&name) {
                    return Err(e(99));
                }
                pkg.routines.insert(name, RoutineDir { prog: Rc::new(s.prog), public, source: s.source });
            }
            b"REQUIRES" => {
                let (nm, _) = d.get(i).and_then(sym_or_str).ok_or(e(19))?;
                if !pkg.classes.is_empty() || !pkg.routines.is_empty() || !pkg.floating.is_empty() {
                    return Err(e(99));
                }
                if !s.prog.clauses.is_empty() {
                    return Err((Err::new(99), s.prog.clauses[0].line));
                }
                pkg.requires.push((nm, line));
            }
            _ => return Err(e(99)),
        }
    }
    Ok(Rc::new(pkg))
}

/// A class by name as the code of a package sees it: its own classes,
/// public classes of required packages, then .local and .environment.
pub fn find_class(w: &World, pkg: Option<&Rc<Package>>, name: &[u8]) -> Option<ObjRef> {
    if let Some(p) = pkg {
        let ins = p.installed.borrow();
        if let Some(c) = ins.classes.get(name) {
            return Some(c.clone());
        }
        for imp in &ins.imports {
            let ii = imp.installed.borrow();
            if let Some(c) = ii.classes.get(name) {
                if imp.classes.iter().any(|cd| cd.name == name && cd.public) {
                    return Some(c.clone());
                }
            }
        }
    }
    for d in [&w.local, &w.env] {
        if let Some(Value::O(o)) = w.dir_get(d, name) {
            if o.is_class() {
                return Some(o);
            }
        }
    }
    None
}

fn code_of(pkg: &Rc<Package>, name: &[u8], prog: &Rc<Program>, source: &[Vec<u8>]) -> Rc<Code> {
    Rc::new(Code { prog: prog.clone(), pkg: pkg.clone(), name: name.to_vec(), source: source.to_vec() })
}

fn method_of(pkg: &Rc<Package>, md: &MethodDir) -> Vec<(Vec<u8>, Method)> {
    let mk = |body: Body| Method { body, private: md.private, guarded: md.guarded, protected: md.protected };
    if md.attribute {
        let mut set = md.name.clone();
        set.push(b'=');
        return alloc::vec![(md.name.clone(), mk(Body::Get(md.name.clone()))), (set, mk(Body::Set(md.name.clone())))];
    }
    alloc::vec![(md.name.clone(), mk(Body::Rexx(code_of(pkg, &md.name, &md.prog, &md.source))))]
}

impl Interp<'_> {
    /// Do a package's ::REQUIRES and ::CLASS directives (once).
    pub(crate) fn install_package(&mut self, f: &mut Frame, pkg: &Rc<Package>) -> R<()> {
        if pkg.installed.borrow().done {
            return Ok(());
        }
        pkg.installed.borrow_mut().done = true;
        let w = self.w.clone().unwrap();
        // ::REQUIRES: load and run each file once
        for (name, line) in &pkg.requires {
            let found = self.host.find_program(name);
            let Some((src, full)) = found else {
                self.err_at = Some((pkg.name.clone(), *line));
                self.err_msg = msg_subst(crate::classes::secondary(43901).unwrap(), &[name]);
                return Err(Err::Syntax(43901));
            };
            let key = upper(&full);
            let existing = w.packages.borrow().get(&key).cloned();
            let p = match existing {
                Some(p) => p,
                None => {
                    let p = match load_package(&src, &full) {
                        Ok(p) => p,
                        Err((e, l)) => {
                            self.err_at = Some((full.clone(), l));
                            return Err(e);
                        }
                    };
                    w.packages.borrow_mut().insert(key, p.clone());
                    // its directives, then its main code
                    self.install_package(f, &p)?;
                    let saved_pool = self.st.cur_pool;
                    let plen = self.st.pools.pools.len();
                    self.st.pools.pools.push(Rc::new(RefCell::new(Pool::default())));
                    self.st.cur_pool = plen;
                    let saved_args = core::mem::take(&mut self.st.args);
                    let mut nf = Frame::new(p.main.clone(), &full, &f.addr.0, CallType::Subroutine, plen);
                    nf.pkg = Some(p.clone());
                    self.depth += 1;
                    let r = self.run(&mut nf, &p.main.clone(), 0, 0);
                    self.depth -= 1;
                    self.st.pools.pop_to(plen);
                    self.st.cur_pool = saved_pool;
                    self.st.args = saved_args;
                    match r {
                        Ok(_) | Err(Err::Exit(_)) => {}
                        Err(e) => return Err(e),
                    }
                    p
                }
            };
            pkg.installed.borrow_mut().imports.push(p);
        }
        // ::CLASS: make the classes, superclasses first
        let n = pkg.classes.len();
        let mut made: Vec<Option<ObjRef>> = alloc::vec![None; n];
        let mut busy = alloc::vec![false; n];
        for i in 0..n {
            self.make_class(&w, pkg, i, &mut made, &mut busy)?;
        }
        // floating methods: .METHODS
        let mut dd = DirData::default();
        for md in &pkg.floating {
            for (name, m) in method_of(pkg, md) {
                let mo = w.obj(Builtin::Method, Data::Method(m));
                dd.map.insert(name, mo);
            }
        }
        if !pkg.floating.is_empty() {
            pkg.installed.borrow_mut().methods = Some(w.new_obj(&w.class(Builtin::Directory), Data::Directory(dd)));
        }
        // each class gets INIT
        for c in made.into_iter().flatten() {
            let v = Value::O(c);
            self.send(f, &v, b"INIT", Vec::new(), Start::Normal, 0)?;
        }
        Ok(())
    }

    fn make_class(&mut self, w: &Rc<World>, pkg: &Rc<Package>, i: usize, made: &mut Vec<Option<ObjRef>>, busy: &mut Vec<bool>) -> R<ObjRef> {
        if let Some(c) = &made[i] {
            return Ok(c.clone());
        }
        let cd = &pkg.classes[i];
        if busy[i] {
            self.err_at = Some((pkg.name.clone(), cd.line));
            return Err(Err::new(99));
        }
        busy[i] = true;
        let mut resolve = |it: &mut Self, name: &[u8]| -> R<ObjRef> {
            if let Some(j) = pkg.classes.iter().position(|c| c.name == name) {
                return it.make_class(w, pkg, j, made, busy);
            }
            match find_class(w, Some(pkg), name) {
                Some(c) => Ok(c),
                None => {
                    it.err_at = Some((pkg.name.clone(), cd.line));
                    it.err_msg = msg_subst(crate::classes::secondary(98909).unwrap(), &[name]);
                    Err(Err::Syntax(98909))
                }
            }
        };
        let sup = match &cd.subclass {
            Some(s) => resolve(self, s)?,
            None => w.class(Builtin::Object),
        };
        let mut supers = alloc::vec![sup.clone()];
        for inh in &cd.inherit {
            supers.push(resolve(self, inh)?);
        }
        let meta = match &cd.metaclass {
            Some(m) => resolve(self, m)?,
            None => class_info(&sup, |ci| ci.meta.clone()).flatten().unwrap_or_else(|| sup.class()),
        };
        let c = w.new_class(cd.id.clone(), supers, meta.clone());
        let mut ext = None;
        if let Some(spec) = &cd.external {
            let mut st = core::mem::replace(&mut self.st, State::empty());
            let r = self.host.ext_class(&mut st, spec);
            self.st = st;
            match r {
                Some((kind, h, _id)) => ext = Some((kind, h)),
                None => {
                    self.err_at = Some((pkg.name.clone(), cd.line));
                    self.err_msg = msg_subst(crate::classes::secondary(90997).unwrap(), &[spec]);
                    return Err(Err::Syntax(90997));
                }
            }
        }
        if let Data::Class(ci) = &mut *c.data.borrow_mut() {
            ci.mixin = cd.mixin;
            ci.ext = ext;
            ci.pkg = Some(pkg.clone());
            if cd.metaclass.is_some() {
                ci.meta = Some(meta.clone());
            } else {
                ci.meta = class_info(&sup, |s| s.meta.clone()).flatten();
            }
            for md in &cd.methods {
                for (name, m) in method_of(pkg, md) {
                    if md.class_side {
                        ci.cls.insert(name, m);
                    } else {
                        ci.inst.insert(name, m);
                    }
                }
            }
        }
        if let Some((kind, _)) = ext {
            w.ext_classes.borrow_mut().entry((kind, cd.id.clone())).or_insert(c.clone());
        }
        pkg.installed.borrow_mut().classes.insert(cd.name.clone(), c.clone());
        made[i] = Some(c.clone());
        Ok(c)
    }

    // ---- sending messages ----

    /// Send a message. `lvl` is the trace level of the sender.
    pub fn send(&mut self, f: &mut Frame, recv: &Value, name: &[u8], args: Vec<Option<Value>>, start: Start, lvl: u32) -> R<Option<Value>> {
        let w = self.w.clone().unwrap();
        match lookup(&w, recv, name, &start) {
            Some(found) => {
                if found.m.private && !f.act.as_ref().is_some_and(|a| a.recv.same(recv)) {
                    return self.nomethod(f, recv, name);
                }
                invoke(self, f, recv, name, args, found, lvl)
            }
            None => {
                // objects of other object systems
                if let Some(r) = self.ext_message(f, recv, name, &args)? {
                    return Ok(r);
                }
                if let Some(found) = lookup(&w, recv, b"UNKNOWN", &start) {
                    let a = w.array_opt(args);
                    return invoke(self, f, recv, b"UNKNOWN", alloc::vec![Some(Value::S(name.to_vec())), Some(a)], found, lvl);
                }
                self.nomethod(f, recv, name)
            }
        }
    }

    /// No method for a message: the NOMETHOD condition, else error 97.
    fn nomethod(&mut self, f: &mut Frame, recv: &Value, name: &[u8]) -> R<Option<Value>> {
        let mut info = CondInfo::new(Cond::NoMethod, name.to_vec());
        info.add = Some(recv.clone());
        if self.raise_info(f, info)? {
            return Ok(None);
        }
        let s = match recv {
            Value::S(s) => s.clone(),
            Value::O(o) => o.fast_string(),
        };
        self.err_msg = msg_subst(crate::classes::secondary(97001).unwrap(), &[&s, name]);
        Err(Err::Syntax(97001))
    }

    /// A message to an object of another object system.
    fn ext_message(&mut self, f: &mut Frame, recv: &Value, name: &[u8], args: &[Option<Value>]) -> R<Option<Option<Value>>> {
        let Value::O(o) = recv else { return Ok(None) };
        let target = match &*o.data.borrow() {
            Data::Ext(k, h) => Some((*k, *h)),
            Data::Class(ci) => ci.ext,
            _ => None,
        };
        let target = match target {
            Some(t) => Some(t),
            None => {
                // an instance of a REXX subclass of an external class
                let mut t = None;
                for c in mro(&o.class()) {
                    if let Some(e) = class_info(&c, |ci| ci.ext).flatten() {
                        t = Some(e);
                        break;
                    }
                }
                if t.is_some() && !o.is_class() { None } else { t }
            }
        };
        let Some((kind, h)) = target else { return Ok(None) };
        let mut xa = Vec::with_capacity(args.len());
        for a in args {
            xa.push(match a {
                None => ExtArg::None,
                Some(v) => self.to_ext(f, v)?,
            });
        }
        let mut st = core::mem::replace(&mut self.st, State::empty());
        let r = self.host.ext_send(&mut st, kind, h, name, &xa);
        self.st = st;
        match r {
            Ok(v) => Ok(Some(self.from_ext(v))),
            Err(ExtErr::NoMethod) => Ok(None),
            Err(ExtErr::Error(code, msg)) => {
                self.err_msg = msg;
                Err(Err::Syntax(code))
            }
        }
    }

    fn to_ext(&mut self, f: &mut Frame, v: &Value) -> R<ExtArg> {
        if let Value::O(o) = v {
            let e = match &*o.data.borrow() {
                Data::Ext(k, h) => Some((*k, *h, false)),
                Data::Class(ci) => ci.ext.map(|(k, h)| (k, h, true)),
                _ => None,
            };
            if let Some((k, h, c)) = e {
                let id = if c { o.class_id() } else { o.class().class_id() };
                return Ok(ExtArg::Obj(k, h, c, id));
            }
            if matches!(&*o.data.borrow(), Data::Nil) {
                return Ok(ExtArg::None);
            }
            // other objects: their strings (numbers, names)
            let s = self.to_str(f, v)?;
            return Ok(ExtArg::Str(s));
        }
        Ok(ExtArg::Str(v.fast_string()))
    }

    pub(crate) fn from_ext(&mut self, v: ExtArg) -> Option<Value> {
        let w = self.w.clone().unwrap();
        match v {
            ExtArg::None => None,
            ExtArg::Nil => Some(w.nil()),
            ExtArg::Str(s) => Some(Value::S(s)),
            ExtArg::Rexx(i) => w.handed.borrow().get(i).cloned(),
            ExtArg::Obj(k, h, is_class, id) => {
                if is_class {
                    let c = self.ext_proxy_class(k, &id, Some(h));
                    Some(Value::O(c))
                } else {
                    let c = self.ext_proxy_class(k, &id, None);
                    Some(Value::O(w.new_obj(&c, Data::Ext(k, h))))
                }
            }
        }
    }

    /// The REXX class standing for a class of another object system.
    fn ext_proxy_class(&mut self, kind: u32, id: &[u8], handle: Option<usize>) -> ObjRef {
        let w = self.w.clone().unwrap();
        let key = (kind, id.to_vec());
        if let Some(c) = w.ext_classes.borrow().get(&key) {
            if let Some(h) = handle {
                if let Data::Class(ci) = &mut *c.data.borrow_mut() {
                    if ci.ext.is_none_or(|e| e.1 == 0) {
                        ci.ext = Some((kind, h));
                    }
                }
            }
            return c.clone();
        }
        let object = w.class(Builtin::Object);
        let meta = w.class(Builtin::Class);
        let c = w.new_class(id.to_vec(), alloc::vec![object], meta);
        if let Data::Class(ci) = &mut *c.data.borrow_mut() {
            ci.ext = Some((kind, handle.unwrap_or(0)));
        }
        w.ext_classes.borrow_mut().insert(key, c.clone());
        c
    }

    /// Run a REXX method or ::ROUTINE's code in a new activation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_code(&mut self, f: &mut Frame, code: &Rc<Code>, act: Option<Rc<Act>>, obj_pool: Option<PoolRef>, args: Vec<Option<Value>>, ct: CallType, lvl: u32) -> R<Option<Value>> {
        if self.depth >= MAX_DEPTH {
            return Err(Err::new(11));
        }
        let plen = self.st.pools.pools.len();
        let saved_pool = self.st.cur_pool;
        let mp = match obj_pool {
            Some(p) => {
                let op = self.st.pools.push_pool(p);
                self.st.pools.push(op)
            }
            None => {
                self.st.pools.pools.push(Rc::new(RefCell::new(Pool::default())));
                self.st.pools.pools.len() - 1
            }
        };
        self.st.cur_pool = mp;
        let saved_args = core::mem::replace(&mut self.st.args, args);
        let mut nf = Frame::new(code.prog.clone(), &code.pkg.name, &f.addr.0, ct, plen);
        nf.pkg = Some(code.pkg.clone());
        nf.base = lvl + 1;
        nf.trace = f.trace;
        nf.expose_ok = act.is_some();
        if let Some(a) = &act {
            let w = self.w.clone().unwrap();
            self.set_simple_v(b"SELF", a.recv.clone());
            let sup = super_of(&w, &a.scope);
            self.set_simple_v(b"SUPER", sup);
        }
        nf.act = act.clone();
        self.depth += 1;
        let prog = code.prog.clone();
        let r = self.run(&mut nf, &prog, 0, 0);
        self.depth -= 1;
        self.st.pools.pop_to(plen);
        self.st.cur_pool = saved_pool;
        self.st.args = saved_args;
        self.st.nextv = None;
        let replied = act.as_ref().and_then(|a| a.reply.borrow_mut().take());
        let r = match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(None),
            Err(Err::Cond(info)) => self.cond_in_caller(f, *info, true),
            // EXIT leaves the method or routine
            Err(Err::Exit(v)) => Ok(v),
            Err(e) => Err(e),
        };
        match replied {
            Some(v) => r.map(|_| v),
            None => r,
        }
    }

    /// Call a ::ROUTINE of the package (or a public one of a required
    /// package).
    pub(crate) fn call_routine_code(&mut self, f: &mut Frame, pkg: &Rc<Package>, name: &[u8], rd: &RoutineDir, args: Vec<Option<Value>>, func: bool, lvl: u32) -> R<Option<Value>> {
        let code = code_of(pkg, name, &rd.prog, &rd.source);
        let ct = if func { CallType::Function } else { CallType::Subroutine };
        self.run_code(f, &code, None, None, args, ct, lvl)
    }
}

/// Run the method found for a message.
pub fn invoke(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Vec<Option<Value>>, found: Found, lvl: u32) -> R<Option<Value>> {
    match &found.m.body {
        Body::Rexx(code) => {
            let pool = match recv {
                Value::O(o) => Some(obj_pool(o, &found.scope, found.class_side)),
                Value::S(_) => Some(Rc::new(RefCell::new(Pool::default()))),
            };
            let act = Rc::new(Act {
                recv: recv.clone(),
                scope: found.scope.clone(),
                class_side: found.class_side,
                name: name.to_vec(),
                args: args.clone(),
                reply: RefCell::new(None),
                guarded: Cell::new(false),
                replied: Cell::new(false),
                obj_pool: 0,
            });
            // a guarded method has the object to itself
            let obj = if found.m.guarded { recv.obj().cloned() } else { None };
            if let Some(o) = &obj {
                acquire_guard(it, o)?;
                act.guarded.set(true);
            }
            let code = code.clone();
            let r = it.run_code(f, &code, Some(act.clone()), pool, args, CallType::Subroutine, lvl);
            if act.guarded.get() {
                if let Some(o) = recv.obj() {
                    release_guard(it, o);
                }
                act.guarded.set(false);
            }
            r
        }
        Body::Get(var) => match recv {
            Value::O(o) => {
                let p = obj_pool(o, &found.scope, found.class_side);
                let v = p.borrow().vars.get(var).cloned();
                Ok(Some(v.unwrap_or_else(|| Value::S(var.clone()))))
            }
            Value::S(_) => Ok(Some(Value::S(var.clone()))),
        },
        Body::Set(var) => {
            if let Value::O(o) = recv {
                let p = obj_pool(o, &found.scope, found.class_side);
                match args.into_iter().next().flatten() {
                    Some(v) => {
                        p.borrow_mut().vars.insert(var.clone(), v);
                    }
                    None => {
                        p.borrow_mut().vars.remove(var);
                    }
                }
            }
            Ok(None)
        }
        Body::Native(b) => {
            let b = *b;
            crate::classes::native(it, f, b, found.class_side, recv, name, args, &found, lvl)
        }
        Body::Null => Ok(None),
    }
}

/// Fill in `&1`, `&2`... of a message.
pub fn msg_subst(t: &[u8], subs: &[&[u8]]) -> Val {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'&' && i + 1 < t.len() && t[i + 1].is_ascii_digit() {
            let k = (t[i + 1] - b'1') as usize;
            if let Some(s) = subs.get(k) {
                out.extend_from_slice(s);
            }
            i += 2;
        } else {
            out.push(t[i]);
            i += 1;
        }
    }
    out
}

// ---- expressions ----

pub fn eval_oo(it: &mut Interp, f: &mut Frame, e: &Expr, lvl: u32) -> R<Value> {
    match e {
        Expr::Env(name) => {
            let v = env_symbol(it, f, name);
            if it.trace_int(f) {
                let s = v.fast_string();
                it.trace_val(b">.>", &s, lvl);
            }
            Ok(v)
        }
        Expr::Send(s) => {
            let recv = it.eval_v(f, &s.recv, lvl)?;
            let start = match &s.scope {
                None => Start::Normal,
                Some(se) => scope_start(it, f, se, &recv, lvl)?,
            };
            let mut args = Vec::with_capacity(s.args.len());
            for a in &s.args {
                args.push(match a {
                    Some(x) => Some(it.eval_top_v(f, x, lvl)?),
                    None => None,
                });
            }
            let r = it.send(f, &recv, &s.name, args, start, lvl)?;
            if s.cascade {
                return Ok(recv);
            }
            let r = match r {
                Some(r) => r,
                None => {
                    it.err_msg = msg_subst(crate::classes::secondary(91999).unwrap(), &[&s.name]);
                    return Err(Err::Syntax(91999));
                }
            };
            if it.trace_int(f) {
                let t = r.fast_string();
                it.trace_val(b">M>", &t, lvl);
            }
            Ok(r)
        }
        Expr::Index(r, a) => {
            let recv = it.eval_v(f, r, lvl)?;
            let mut args = Vec::with_capacity(a.len());
            for x in a {
                args.push(match x {
                    Some(x) => Some(it.eval_v(f, x, lvl)?),
                    None => None,
                });
            }
            match it.send(f, &recv, b"[]", args, Start::Normal, lvl)? {
                Some(v) => Ok(v),
                None => Err(Err::new(91)),
            }
        }
        _ => Err(Err::new(35)),
    }
}

/// `~msg:scope`: SUPER starts after the method's scope; a class at it.
fn scope_start(it: &mut Interp, f: &mut Frame, se: &Expr, recv: &Value, lvl: u32) -> R<Start> {
    if let Expr::Var(VarRef::Simple(n)) = se {
        if n.as_slice() == b"SUPER" {
            if let Some(a) = &f.act {
                if let Some(s) = &a.scope {
                    let _ = recv;
                    return Ok(Start::Super(s.clone(), a.class_side));
                }
            }
        }
    }
    match it.eval_v(f, se, lvl)? {
        Value::O(c) if c.is_class() => Ok(Start::At(c)),
        _ => Err(Err::new(98)),
    }
}

/// The value of an environment symbol.
pub fn env_symbol(it: &mut Interp, f: &mut Frame, name: &[u8]) -> Value {
    let w = it.w.clone().unwrap();
    if let Some(c) = find_class(&w, f.pkg.as_ref(), name) {
        // own and required classes come before .local and .environment
        return Value::O(c);
    }
    match name {
        b"LOCAL" => return Value::O(w.local.clone()),
        b"ENVIRONMENT" => return Value::O(w.env.clone()),
        b"METHODS" => {
            if let Some(m) = f.pkg.as_ref().and_then(|p| p.installed.borrow().methods.clone()) {
                return Value::O(m);
            }
        }
        _ => {}
    }
    for d in [&w.local, &w.env] {
        if let Some(v) = w.dir_get(d, name) {
            return v;
        }
    }
    // the Workplace Shell, SOM...
    let mut st = core::mem::replace(&mut it.st, State::empty());
    let r = it.host.ext_lookup(&mut st, name);
    it.st = st;
    if let Some((k, h, c, id)) = r {
        if let Some(v) = it.from_ext(ExtArg::Obj(k, h, c, id)) {
            // the same object next time
            if let Data::Directory(d) = &mut *w.local.data.borrow_mut() {
                d.map.insert(name.to_vec(), v.clone());
            }
            return v;
        }
    }
    let mut v = b".".to_vec();
    v.extend_from_slice(name);
    Value::S(v)
}

fn op_name(op: BinOp) -> &'static [u8] {
    match op {
        BinOp::Add => b"+",
        BinOp::Sub => b"-",
        BinOp::Mul => b"*",
        BinOp::Div => b"/",
        BinOp::IDiv => b"%",
        BinOp::Rem => b"//",
        BinOp::Pow => b"**",
        BinOp::Concat => b"||",
        BinOp::Blank => b" ",
        BinOp::Eq => b"=",
        BinOp::Ne => b"\\=",
        BinOp::Gt => b">",
        BinOp::Lt => b"<",
        BinOp::Ge => b">=",
        BinOp::Le => b"<=",
        BinOp::SEq => b"==",
        BinOp::SNe => b"\\==",
        BinOp::SGt => b">>",
        BinOp::SLt => b"<<",
        BinOp::SGe => b">>=",
        BinOp::SLe => b"<<=",
        BinOp::And => b"&",
        BinOp::Or => b"|",
        BinOp::Xor => b"&&",
    }
}

/// An operator whose left operand is an object: a message to it (the
/// Object class knows only the comparisons for identity; strings convert
/// their argument).
pub fn binary_op(it: &mut Interp, f: &mut Frame, op: BinOp, x: Value, y: Value, lvl: u32) -> R<Value> {
    if let Value::O(_) = &x {
        let name = op_name(op);
        return it.send(f, &x, name, alloc::vec![Some(y)], Start::Normal, lvl)?.ok_or(Err::new(91));
    }
    let a = it.to_str(f, &x)?;
    let b = it.to_str(f, &y)?;
    Ok(Value::S(binop(op, &a, &b, &f.numeric)?))
}

/// A prefix operator on an object: a message to it.
pub fn unary_op(it: &mut Interp, f: &mut Frame, op: UnOp, v: &Value, lvl: u32) -> R<Option<Value>> {
    let name: &[u8] = match op {
        UnOp::Not => b"\\",
        UnOp::Neg => b"-",
        UnOp::Plus => b"+",
    };
    Ok(Some(it.send(f, v, name, Vec::new(), Start::Normal, lvl)?.ok_or(Err::new(91))?))
}

// ---- built-in functions with objects ----

/// Built-in functions that deal in objects (ARG, CONDITION's A and O,
/// VALUE of .ENVIRONMENT).
pub fn call_builtin_oo(it: &mut Interp, f: &mut Frame, name: &[u8], args: &[Option<Value>]) -> Option<R<Option<Value>>> {
    let w = it.w.clone().unwrap();
    match name {
        b"ARG" if args.len() == 1 => {
            let n = args[0].as_ref()?.fast_string();
            let n = crate::num::Num::parse(&n)?.to_whole(9)?;
            if n < 1 {
                return None;
            }
            Some(Ok(Some(it.st.args.get(n as usize - 1).cloned().flatten().unwrap_or(Value::S(Vec::new())))))
        }
        b"CONDITION" if args.len() == 1 => {
            let o = args[0].as_ref()?.fast_string();
            match o.first().map(|c| c.to_ascii_uppercase()) {
                Some(b'A') => Some(Ok(Some(f.cond.as_ref().and_then(|c| c.add.clone()).unwrap_or_else(|| w.nil())))),
                Some(b'O') => Some(Ok(Some(match &f.cond {
                    Some(c) => cond_object(it, c),
                    None => w.nil(),
                }))),
                Some(b'C') => Some(Ok(Some(Value::S(f.cond.as_ref().map(|c| c.name()).unwrap_or_default())))),
                _ => None,
            }
        }
        b"VALUE" if args.len() >= 3 && args[2].as_ref().is_some_and(|s| s.fast_string().is_empty()) => {
            let n = upper(&args[0].as_ref()?.fast_string());
            let old = w.dir_get(&w.env, &n).unwrap_or_else(|| {
                let mut v = b".".to_vec();
                v.extend_from_slice(&n);
                Value::S(v)
            });
            if let Some(Some(nv)) = args.get(1) {
                if let Data::Directory(d) = &mut *w.env.data.borrow_mut() {
                    d.map.insert(n, nv.clone());
                }
            }
            Some(Ok(Some(old)))
        }
        _ => None,
    }
}

/// Object REXX's built-in functions classic REXX does not have
/// (CHANGESTR, COUNTSTR), on strings.
pub fn oo_bif(name: &[u8], a: &[Option<Val>]) -> Option<R<Val>> {
    match name {
        b"CHANGESTR" => {
            if a.len() != 3 || a.iter().any(|x| x.is_none()) {
                return Some(Err(Err::new(40)));
            }
            let (needle, hay, new) = (a[0].as_ref().unwrap(), a[1].as_ref().unwrap(), a[2].as_ref().unwrap());
            if needle.is_empty() {
                return Some(Ok(hay.clone()));
            }
            let mut out = Vec::new();
            let mut i = 0;
            while i < hay.len() {
                if hay[i..].starts_with(needle) {
                    out.extend_from_slice(new);
                    i += needle.len();
                } else {
                    out.push(hay[i]);
                    i += 1;
                }
            }
            Some(Ok(out))
        }
        b"COUNTSTR" => {
            if a.len() != 2 || a.iter().any(|x| x.is_none()) {
                return Some(Err(Err::new(40)));
            }
            let (needle, hay) = (a[0].as_ref().unwrap(), a[1].as_ref().unwrap());
            let mut n = 0;
            let mut i = 0;
            if !needle.is_empty() {
                while i + needle.len() <= hay.len() {
                    if hay[i..].starts_with(needle) {
                        n += 1;
                        i += needle.len();
                    } else {
                        i += 1;
                    }
                }
            }
            Some(Ok(int_str(n)))
        }
        _ => None,
    }
}

/// CONDITION('O'): a directory describing the condition.
pub fn cond_object(it: &mut Interp, c: &CondInfo) -> Value {
    let w = it.w.clone().unwrap();
    let mut d = DirData::default();
    d.map.insert(b"CONDITION".to_vec(), Value::S(c.name()));
    d.map.insert(b"DESCRIPTION".to_vec(), Value::S(c.desc.clone()));
    d.map.insert(b"INSTRUCTION".to_vec(), Value::S(if c.call { b"CALL".to_vec() } else { b"SIGNAL".to_vec() }));
    d.map.insert(b"ADDITIONAL".to_vec(), c.add.clone().unwrap_or_else(|| w.nil()));
    d.map.insert(b"RESULT".to_vec(), c.result.clone().unwrap_or_else(|| w.nil()));
    d.map.insert(b"PROPAGATED".to_vec(), Value::S(b"0".to_vec()));
    if c.cond == Cond::Syntax {
        let (major, minor) = if c.code >= 1000 { (c.code / 1000, c.code % 1000) } else { (c.code, 0) };
        d.map.insert(b"RC".to_vec(), Value::S(int_str(major as i64)));
        let mut code = int_str(major as i64);
        if minor > 0 {
            code.push(b'.');
            code.extend_from_slice(&int_str(minor as i64));
        }
        d.map.insert(b"CODE".to_vec(), Value::S(code));
        let text = it.host.message(major).unwrap_or_default();
        d.map.insert(b"ERRORTEXT".to_vec(), Value::S(text));
        d.map.insert(b"MESSAGE".to_vec(), if c.msg.is_empty() { w.nil() } else { Value::S(c.msg.clone()) });
        d.map.insert(b"POSITION".to_vec(), Value::S(int_str(c.line as i64)));
        d.map.insert(b"PROGRAM".to_vec(), Value::S(it.st.source.rsplit(|&b| b == b' ').next().unwrap_or_default().to_vec()));
        let mut l = ListData::default();
        for t in &c.tb {
            l.items.push_back((l.next, Value::S(t.clone())));
            l.next += 1;
        }
        d.map.insert(b"TRACEBACK".to_vec(), w.obj(Builtin::List, Data::List(l)));
    }
    w.obj(Builtin::Directory, Data::Directory(d))
}

/// A ::ROUTINE by name (the package's own, then public ones of required
/// packages).
pub fn call_package_routine(it: &mut Interp, f: &mut Frame, name: &[u8], args: Vec<Option<Value>>, func: bool, lvl: u32) -> Option<R<Option<Value>>> {
    let pkg = f.pkg.clone()?;
    if let Some(rd) = pkg.routines.get(name) {
        let rd = RoutineDir { prog: rd.prog.clone(), public: rd.public, source: rd.source.clone() };
        return Some(it.call_routine_code(f, &pkg, name, &rd, args, func, lvl));
    }
    let imports = pkg.installed.borrow().imports.clone();
    for p in imports.iter() {
        if let Some(rd) = p.routines.get(name).filter(|r| r.public) {
            let rd = RoutineDir { prog: rd.prog.clone(), public: true, source: rd.source.clone() };
            return Some(it.call_routine_code(f, p, name, &rd, args, func, lvl));
        }
        // and what those packages require in turn
        let deeper = p.installed.borrow().imports.clone();
        for q in deeper.iter() {
            if let Some(rd) = q.routines.get(name).filter(|r| r.public) {
                let rd = RoutineDir { prog: rd.prog.clone(), public: true, source: rd.source.clone() };
                return Some(it.call_routine_code(f, q, name, &rd, args, func, lvl));
            }
        }
    }
    None
}

/// The items DO OVER goes through.
pub fn over_items(it: &mut Interp, f: &mut Frame, coll: &Value) -> R<Vec<Value>> {
    if let Value::O(o) = coll {
        let quick = match &*o.data.borrow() {
            Data::Stem(s, _) => Some(s.borrow().tails.iter().filter(|(_, v)| v.is_some()).map(|(k, _)| Value::S(k.clone())).collect()),
            _ => None,
        };
        if let Some(q) = quick {
            return Ok(q);
        }
    }
    let a = it.send(f, coll, b"MAKEARRAY", Vec::new(), Start::Normal, 0)?.ok_or(Err::new(98))?;
    match &a {
        Value::O(o) => match &*o.data.borrow() {
            Data::Array(ad) => Ok(ad.items.iter().flatten().cloned().collect()),
            _ => Err(Err::new(98)),
        },
        _ => Err(Err::new(98)),
    }
}

// ---- instructions ----

pub(crate) fn exec_oo(it: &mut Interp, f: &mut Frame, c: &Clause, lvl: u32) -> R<Next> {
    match &c.instr {
        Instr::Message(e) => {
            let r = match e {
                Expr::Send(s) => {
                    let recv = it.eval_v(f, &s.recv, lvl)?;
                    let start = match &s.scope {
                        None => Start::Normal,
                        Some(se) => scope_start(it, f, se, &recv, lvl)?,
                    };
                    let mut args = Vec::with_capacity(s.args.len());
                    for a in &s.args {
                        args.push(match a {
                            Some(x) => Some(it.eval_v(f, x, lvl)?),
                            None => None,
                        });
                    }
                    let r = it.send(f, &recv, &s.name, args, start, lvl)?;
                    if s.cascade { Some(recv) } else { r }
                }
                Expr::Index(r, a) => {
                    let recv = it.eval_v(f, r, lvl)?;
                    let mut args = Vec::new();
                    for x in a {
                        args.push(match x {
                            Some(x) => Some(it.eval_v(f, x, lvl)?),
                            None => None,
                        });
                    }
                    it.send(f, &recv, b"[]", args, Start::Normal, lvl)?
                }
                _ => None,
            };
            match r {
                Some(v) => {
                    if it.trace_res(f) {
                        let s = v.fast_string();
                        it.trace_val(b">>>", &s, lvl);
                    }
                    it.set_simple_v(b"RESULT", v)
                }
                None => it.st.pools.drop_var(it.st.cur_pool, &VKey::Simple(b"RESULT")),
            }
            Ok(Next::Go)
        }
        Instr::AssignMsg(target, value) => {
            let (recv_e, mut name, scope, arg_es) = match target {
                Expr::Send(s) => (&s.recv, s.name.clone(), s.scope.clone(), s.args.clone()),
                Expr::Index(r, a) => (&**r, b"[]".to_vec(), None, a.clone()),
                _ => return Err(Err::new(35)),
            };
            name.push(b'=');
            let recv = it.eval_v(f, recv_e, lvl)?;
            let v = it.eval_top_v(f, value, lvl)?;
            let start = match &scope {
                None => Start::Normal,
                Some(se) => scope_start(it, f, se, &recv, lvl)?,
            };
            let mut args = alloc::vec![Some(v)];
            for a in &arg_es {
                args.push(match a {
                    Some(x) => Some(it.eval_v(f, x, lvl)?),
                    None => None,
                });
            }
            it.send(f, &recv, &name, args, start, lvl)?;
            Ok(Next::Go)
        }
        Instr::Use(names) => {
            let args = it.st.args.clone();
            for (i, n) in names.iter().enumerate() {
                let Some(v) = n else { continue };
                match args.get(i).cloned().flatten() {
                    Some(a) => {
                        if it.trace_res(f) {
                            let s = a.fast_string();
                            it.trace_val(b">>>", &s, lvl);
                        }
                        it.set_var_v(v, a)
                    }
                    None => it.drop_ref(v),
                }
            }
            Ok(Next::Go)
        }
        Instr::Expose(items) => {
            if f.act.is_none() {
                it.err_msg = crate::classes::secondary(99907).unwrap().to_vec();
                return Err(Err::Syntax(99907));
            }
            if !f.expose_ok {
                it.err_msg = crate::classes::secondary(99907).unwrap().to_vec();
                return Err(Err::Syntax(99907));
            }
            let np = it.st.cur_pool;
            it.expose(np, items)?;
            Ok(Next::Go)
        }
        Instr::Guard(on, when) => {
            if f.act.is_none() {
                it.err_msg = crate::classes::secondary(99911).unwrap().to_vec();
                return Err(Err::Syntax(99911));
            }
            let a = f.act.clone().unwrap();
            let obj = a.recv.obj().cloned();
            if let Some(e) = when {
                // wait until the expression is true, letting other
                // activities run (and change the object's variables)
                loop {
                    if *on && !a.guarded.get() {
                        if let Some(o) = &obj {
                            acquire_guard(it, o)?;
                        }
                        a.guarded.set(true);
                    }
                    if it.eval_bool(f, e, lvl)? {
                        break;
                    }
                    if a.guarded.get() {
                        if let Some(o) = &obj {
                            release_guard(it, o);
                        }
                        a.guarded.set(false);
                    }
                    let mut st = core::mem::replace(&mut it.st, State::empty());
                    let ok = it.host.wait_guard(&mut st);
                    it.st = st;
                    if !ok {
                        // no other activity can make it true
                        it.err_msg = crate::classes::secondary(98905).unwrap().to_vec();
                        return Err(Err::Syntax(98905));
                    }
                }
            }
            if *on && !a.guarded.get() {
                if let Some(o) = &obj {
                    acquire_guard(it, o)?;
                }
                a.guarded.set(true);
            } else if !*on && a.guarded.get() {
                if let Some(o) = &obj {
                    release_guard(it, o);
                }
                a.guarded.set(false);
            }
            Ok(Next::Go)
        }
        Instr::Reply(e) => {
            let Some(a) = f.act.clone() else {
                it.err_msg = crate::classes::secondary(99919).unwrap().to_vec();
                return Err(Err::Syntax(99919));
            };
            if a.reply.borrow().is_some() || a.replied.get() {
                it.err_msg = crate::classes::secondary(98935).unwrap().to_vec();
                return Err(Err::Syntax(98935));
            }
            let v = match e {
                Some(e) => Some(it.eval_top_v(f, e, lvl)?),
                None => None,
            };
            a.replied.set(true);
            Ok(Next::Reply(v))
        }
        Instr::Raise(spec) => exec_raise(it, f, spec, lvl),
        Instr::Forward(spec) => exec_forward(it, f, spec, lvl),
        _ => Ok(Next::Go),
    }
}

// ---- activities ----

/// Take an object's guard (exclusive use of its variables) for this
/// activity, waiting while another holds it.
pub(crate) fn acquire_guard(it: &mut Interp, o: &ObjRef) -> R<()> {
    loop {
        let (a, n) = o.guard.get();
        if n == 0 || a == it.activity {
            o.guard.set((it.activity, n + 1));
            return Ok(());
        }
        let mut st = core::mem::replace(&mut it.st, State::empty());
        let ok = it.host.wait_guard(&mut st);
        it.st = st;
        if !ok {
            it.err_msg = crate::classes::secondary(98905).unwrap().to_vec();
            return Err(Err::Syntax(98905));
        }
    }
}

pub(crate) fn release_guard(it: &mut Interp, o: &ObjRef) {
    let (a, n) = o.guard.get();
    if a == it.activity && n > 0 {
        o.guard.set((if n == 1 { 0 } else { a }, n - 1));
    }
}

/// A raw pointer that may cross to another activity's thread (only one
/// activity runs at a time: the host's lock).
struct Pass<T>(T);
unsafe impl<T> Send for Pass<T> {}

/// REPLY in a method's own code: go on with the rest of the method on a
/// new activity. False if the host has no other activities.
pub(crate) fn reply_elsewhere(it: &mut Interp, f: &Frame, code: &Rc<Program>, pc: usize, extra: u32, ctl: &[Ctl]) -> bool {
    let Some(act) = f.act.clone() else { return false };
    let w = it.w.clone().unwrap();
    let mp = it.st.pools.pools[it.st.cur_pool].clone();
    let op = mp.borrow().parent.map(|p| it.st.pools.pools[p].clone());
    let job = Pass((f.clone(), code.clone(), ctl.to_vec(), it.st.args.clone(), w.clone(), mp, op, act.clone()));
    let id = w.next_activity();
    let held = act.guarded.get();
    let ok = it.host.spawn(alloc::boxed::Box::new(move |h: &mut dyn crate::Host| {
        let Pass((mut frame, code, ctl, args, w, mp, op, act)) = job;
        let mut it2 = Interp::new_activity(h, w, id);
        it2.st.pools.pools.clear();
        match op {
            Some(op) => {
                it2.st.pools.pools.push(op);
                mp.borrow_mut().parent = Some(0);
                it2.st.pools.pools.push(mp);
                it2.st.cur_pool = 1;
            }
            None => {
                mp.borrow_mut().parent = None;
                it2.st.pools.pools.push(mp);
                it2.st.cur_pool = 0;
            }
        }
        it2.st.args = args;
        it2.depth = 1;
        frame.top = true;
        let obj = act.recv.obj().cloned();
        // the method keeps the object's guard on its new activity
        if held {
            if let Some(o) = &obj {
                let _ = acquire_guard(&mut it2, o);
            }
        }
        let _ = it2.run_from(&mut frame, &code, pc, extra, ctl);
        if held {
            if let Some(o) = &obj {
                release_guard(&mut it2, o);
            }
        }
    }));
    ok
}

fn exec_raise(it: &mut Interp, f: &mut Frame, spec: &RaiseSpec, lvl: u32) -> R<Next> {
    let w = it.w.clone().unwrap();
    let mut info = if spec.propagate {
        match f.cond.clone() {
            Some(c) => c,
            None => {
                it.err_msg = crate::classes::secondary(98918).unwrap().to_vec();
                return Err(Err::Syntax(98918));
            }
        }
    } else {
        let mut i = CondInfo::new(spec.cond, Vec::new());
        i.user = spec.user.clone();
        i
    };
    if let Some(e) = &spec.code {
        let v = it.eval(f, e, lvl)?;
        match spec.cond {
            Cond::Syntax => {
                // nn or nn.nnn
                let s = crate::exec::strip_blanks(&v).to_vec();
                let (a, b) = match s.iter().position(|&c| c == b'.') {
                    Some(p) => (&s[..p], &s[p + 1..]),
                    None => (&s[..], &b""[..]),
                };
                let major = whole(a, 9)?;
                let minor = if b.is_empty() { 0 } else { whole(b, 9)? };
                info.code = (major * if minor > 0 { 1000 } else { 1 } + minor) as u32;
            }
            _ => {
                info.desc = v.clone();
                // RC for ERROR and FAILURE
                it.set_simple(b"RC", v);
            }
        }
    }
    if let Some(e) = &spec.desc {
        info.desc = it.eval(f, e, lvl)?;
    }
    if let Some(e) = &spec.additional {
        info.add = Some(it.eval_v(f, e, lvl)?);
    }
    if let Some(l) = &spec.array {
        let mut items = Vec::new();
        for x in l {
            items.push(match x {
                Some(x) => Some(it.eval_v(f, x, lvl)?),
                None => None,
            });
        }
        info.add = Some(w.array_opt(items));
    }
    if info.cond == Cond::Syntax {
        // the secondary message from ARRAY/ADDITIONAL substitutions
        let mut subs: Vec<Val> = Vec::new();
        if let Some(Value::O(o)) = &info.add {
            if let Data::Array(a) = &*o.data.borrow() {
                subs = a.items.iter().map(|x| x.as_ref().map(|v| v.fast_string()).unwrap_or_default()).collect();
            }
        }
        let refs: Vec<&[u8]> = subs.iter().map(|s| s.as_slice()).collect();
        if let Some(t) = crate::classes::secondary(info.code) {
            info.msg = msg_subst(t, &refs);
        }
    }
    // RAISE leaves the routine or method and raises the condition in its
    // caller: RETURN in the caller of the routine, EXIT (and RAISE without
    // either) in the caller of the program, method or ::ROUTINE
    match &spec.ret {
        Some((exit, e)) => {
            info.exit = *exit;
            info.result = match e {
                Some(e) => Some(it.eval_v(f, e, lvl)?),
                None => None,
            };
        }
        None => info.exit = true,
    }
    info.line = 0;
    it.err_msg = info.msg.clone();
    Err(Err::Cond(alloc::boxed::Box::new(info)))
}

fn exec_forward(it: &mut Interp, f: &mut Frame, spec: &ForwardSpec, lvl: u32) -> R<Next> {
    let Some(act) = f.act.clone() else {
        it.err_msg = crate::classes::secondary(98947).unwrap().to_vec();
        return Err(Err::Syntax(98947));
    };
    let target = match &spec.to {
        Some(e) => it.eval_v(f, e, lvl)?,
        None => act.recv.clone(),
    };
    let name = match &spec.message {
        Some(e) => upper(&it.eval(f, e, lvl)?),
        None => act.name.clone(),
    };
    let args = if let Some(e) = &spec.arguments {
        let a = it.eval_v(f, e, lvl)?;
        match &a {
            Value::O(o) => match &*o.data.borrow() {
                Data::Array(ad) => ad.items.clone(),
                _ => return Err(Err::new(98)),
            },
            _ => return Err(Err::new(98)),
        }
    } else if let Some(l) = &spec.array {
        let mut v = Vec::new();
        for x in l {
            v.push(match x {
                Some(x) => Some(it.eval_v(f, x, lvl)?),
                None => None,
            });
        }
        v
    } else {
        act.args.clone()
    };
    let start = match &spec.class {
        None => Start::Normal,
        Some(e) => scope_start(it, f, e, &target, lvl)?,
    };
    let r = it.send(f, &target, &name, args, start, lvl)?;
    if spec.cont {
        match r {
            Some(v) => it.set_simple_v(b"RESULT", v),
            None => it.st.pools.drop_var(it.st.cur_pool, &VKey::Simple(b"RESULT")),
        }
        Ok(Next::Go)
    } else {
        Ok(Next::Flow(Flow::Return(r)))
    }
}

/// Look up a class by name for the code running in `f` (SUBCLASS etc.).
pub fn class_named(it: &Interp, f: &Frame, name: &[u8]) -> Option<ObjRef> {
    let w = it.w.as_ref()?;
    find_class(w, f.pkg.as_ref(), &upper(name))
}

#[allow(dead_code)]
fn unused(_: &Token) {}
