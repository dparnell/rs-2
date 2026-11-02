//! rs2's interface repository: `\OS2\ETC\SOM.IR` written from
//! `userland/somir/som-ir.txt` (made by tools/scripts/gen_som_ir.py from
//! the IDL of the SOM classes rs2 ships). The file format is in
//! docs/formats/som-ir.md: big-endian integers, a 32-byte header, fixed
//! and variable records for each definition, contents records for the
//! containers and reference lists for the interfaces' derived interfaces.
//!
//! The text has one definition per line (tab-separated), its modifiers
//! (`mod`) after it, and `end` closing a module, interface or operation:
//!
//! ```text
//! interface NAME BASES INSTANCE-DATA-TYPECODE
//! operation NAME MODE RESULT CONTEXTS RAISES
//! parameter NAME MODE TYPECODE        attribute NAME MODE TYPECODE
//! typedef NAME TYPECODE   const NAME TYPECODE VALUE   exception NAME TYPECODE
//! module NAME             mod KEY VALUE (\0: none)    end
//! ```
//!
//! TypeCodes are written `long`, `string(0)`, `objref(::SOMClass)`,
//! `pointer(T)`, `sequence(T,N)`, `array(T,N)`, `struct(NAME,M,T,...)`,
//! `union(NAME,SWITCH,FLAG,LABEL,M,T,...)`, `enum(NAME,A,B,...)`,
//! `self(NAME)` and `foreign("NAME","CONTEXT",SIZE)`; `-` is none.

use std::collections::HashMap;

const MAGIC: i32 = 0x0517_9979;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Constant = 0,
    TypeDef = 1,
    Exception = 2,
    Interface = 3,
    Module = 4,
    Attribute = 5,
    Operation = 6,
    Parameter = 7,
}

#[derive(Debug, Clone)]
enum Tc {
    None,
    Basic(i32),
    Str(i32),
    Objref(String),
    Struct(String, Vec<(String, Tc)>),
    Union(String, Box<Tc>, Vec<(i32, i32, String, Tc)>),
    Enum(String, Vec<String>),
    Seq(Box<Tc>, i32),
    Array(Box<Tc>, i32),
    Pointer(Box<Tc>),
    SelfRef(String),
    Foreign(String, String, i32),
}

struct Def {
    kind: Kind,
    name: String,
    mods: Vec<(String, Option<String>)>,
    mode: i32,
    tc: Tc,
    value: String,
    bases: Vec<String>,
    contexts: Vec<String>,
    raises: Vec<String>,
    children: Vec<Def>,
}

impl Def {
    fn new(kind: Kind, name: &str) -> Def {
        Def {
            kind,
            name: name.to_string(),
            mods: Vec::new(),
            mode: 0,
            tc: Tc::None,
            value: String::new(),
            bases: Vec::new(),
            contexts: Vec::new(),
            raises: Vec::new(),
            children: Vec::new(),
        }
    }
}

// ---- Reading the text ----

struct TcParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl TcParser<'_> {
    fn word(&mut self) -> String {
        if self.s.get(self.i) == Some(&b'"') {
            self.i += 1;
            let st = self.i;
            while self.i < self.s.len() && self.s[self.i] != b'"' {
                self.i += 1;
            }
            let w = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
            self.i += 1;
            return w;
        }
        let st = self.i;
        while self.i < self.s.len() && !matches!(self.s[self.i], b'(' | b')' | b',') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).into_owned()
    }
    fn eat(&mut self, c: u8) {
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
        }
    }
    fn more(&self) -> bool {
        self.s.get(self.i) == Some(&b',')
    }
    fn int(&mut self) -> i32 {
        let w = self.word();
        w.parse().unwrap_or(0)
    }
    fn tc(&mut self) -> Tc {
        let w = self.word();
        let basic = |k| Tc::Basic(k);
        match w.as_str() {
            "-" | "" => Tc::None,
            "null" => basic(1),
            "void" => basic(2),
            "short" => basic(3),
            "long" => basic(4),
            "ushort" => basic(5),
            "ulong" => basic(6),
            "float" => basic(7),
            "double" => basic(8),
            "boolean" => basic(9),
            "char" => basic(10),
            "octet" => basic(11),
            "any" => basic(12),
            "TypeCode" => basic(13),
            "Principal" => basic(14),
            _ => {
                self.eat(b'(');
                let t = match w.as_str() {
                    "string" => Tc::Str(self.int()),
                    "objref" => Tc::Objref(self.word()),
                    "self" => Tc::SelfRef(self.word()),
                    "pointer" => Tc::Pointer(Box::new(self.tc())),
                    "sequence" | "array" => {
                        let el = self.tc();
                        self.eat(b',');
                        let n = self.int();
                        if w == "array" { Tc::Array(Box::new(el), n) } else { Tc::Seq(Box::new(el), n) }
                    }
                    "struct" => {
                        let name = self.word();
                        let mut m = Vec::new();
                        while self.more() {
                            self.eat(b',');
                            let n = self.word();
                            self.eat(b',');
                            m.push((n, self.tc()));
                        }
                        Tc::Struct(name, m)
                    }
                    "enum" => {
                        let name = self.word();
                        let mut m = Vec::new();
                        while self.more() {
                            self.eat(b',');
                            m.push(self.word());
                        }
                        Tc::Enum(name, m)
                    }
                    "union" => {
                        let name = self.word();
                        self.eat(b',');
                        let sw = self.tc();
                        let mut m = Vec::new();
                        while self.more() {
                            self.eat(b',');
                            let flag = self.int();
                            self.eat(b',');
                            let label = self.int();
                            self.eat(b',');
                            let n = self.word();
                            self.eat(b',');
                            m.push((flag, label, n, self.tc()));
                        }
                        Tc::Union(name, Box::new(sw), m)
                    }
                    "foreign" => {
                        let name = self.word();
                        self.eat(b',');
                        let ctx = self.word();
                        self.eat(b',');
                        Tc::Foreign(name, ctx, self.int())
                    }
                    _ => panic!("som-ir.txt: unknown TypeCode {w}"),
                };
                self.eat(b')');
                t
            }
        }
    }
}

fn tc(s: &str) -> Tc {
    TcParser { s: s.as_bytes(), i: 0 }.tc()
}

fn unescape(s: &str) -> Option<String> {
    if s == "\\0" {
        return None;
    }
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn list(s: &str) -> Vec<String> {
    s.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect()
}

fn parse(text: &str) -> Vec<Def> {
    // A stack of open containers; the last definition gets the modifiers.
    let mut stack: Vec<Def> = vec![Def::new(Kind::Module, "")];
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let g = |i: usize| f.get(i).copied().unwrap_or("");
        let top = stack.last_mut().unwrap();
        let d = match f[0] {
            "mod" => {
                // The modifiers follow their definition: the container's
                // own come before its first definition.
                let v = (g(1).to_string(), unescape(g(2)));
                match top.children.last_mut() {
                    Some(c) if !matches!(c.kind, Kind::Interface | Kind::Module | Kind::Operation) => c.mods.push(v),
                    _ => top.mods.push(v),
                }
                continue;
            }
            "end" => {
                let d = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(d);
                continue;
            }
            "module" => {
                stack.push(Def::new(Kind::Module, g(1)));
                continue;
            }
            "interface" => {
                let mut d = Def::new(Kind::Interface, g(1));
                d.bases = list(g(2));
                d.tc = tc(g(3));
                stack.push(d);
                continue;
            }
            "operation" => {
                let mut d = Def::new(Kind::Operation, g(1));
                d.mode = g(2).parse().unwrap_or(1);
                d.tc = tc(g(3));
                d.contexts = list(g(4));
                d.raises = list(g(5));
                stack.push(d);
                continue;
            }
            "parameter" | "attribute" => {
                let mut d = Def::new(if f[0] == "parameter" { Kind::Parameter } else { Kind::Attribute }, g(1));
                d.mode = g(2).parse().unwrap_or(1);
                d.tc = tc(g(3));
                d
            }
            "typedef" | "exception" => {
                let mut d = Def::new(if f[0] == "typedef" { Kind::TypeDef } else { Kind::Exception }, g(1));
                d.tc = tc(g(2));
                d
            }
            "const" => {
                let mut d = Def::new(Kind::Constant, g(1));
                d.tc = tc(g(2));
                d.value = unescape(g(3)).unwrap_or_default();
                d
            }
            k => panic!("som-ir.txt: unknown line kind {k}"),
        };
        stack.last_mut().unwrap().children.push(d);
    }
    stack.pop().unwrap().children
}

// ---- Writing the file ----

struct W {
    b: Vec<u8>,
    /// Interface id -> its fixed record (for the reference lists).
    ifaces: HashMap<String, u32>,
    /// Interface fixed record -> (offset of its reference list field, bases).
    refs: Vec<(u32, u32, Vec<String>)>,
}

impl W {
    fn int(&mut self, v: i32) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn str(&mut self, s: Option<&str>) {
        match s {
            None => {
                self.int(-1);
                self.b.push(0);
            }
            Some(s) => {
                self.int(s.len() as i32);
                self.b.extend_from_slice(s.as_bytes());
                self.b.push(0);
            }
        }
    }
    fn strs(&mut self, v: &[String]) {
        self.int(v.len() as i32);
        for s in v {
            self.str(Some(s));
        }
    }
    fn patch(&mut self, at: u32, v: i32) {
        self.b[at as usize..at as usize + 4].copy_from_slice(&v.to_be_bytes());
    }
    fn here(&self) -> u32 {
        self.b.len() as u32
    }
    /// Start a record: its size is patched by `end`.
    fn begin(&mut self, kind: i32) -> u32 {
        let at = self.here();
        self.int(0);
        self.int(kind);
        at
    }
    fn end(&mut self, at: u32) {
        let n = self.here() - at;
        self.patch(at, n as i32);
    }

    fn tc(&mut self, t: &Tc) {
        let kind = match t {
            Tc::None => {
                self.int(-1);
                return;
            }
            Tc::Basic(k) => *k,
            Tc::Str(_) => 19,
            Tc::Objref(_) => 15,
            Tc::SelfRef(_) => 102,
            Tc::Pointer(_) => 101,
            Tc::Seq(..) => 20,
            Tc::Array(..) => 21,
            Tc::Struct(..) => 16,
            Tc::Enum(..) => 18,
            Tc::Union(..) => 17,
            Tc::Foreign(..) => 103,
        };
        self.int(kind);
        self.int(0);
        self.int(0);
        self.int(0);
        match t {
            Tc::None | Tc::Basic(_) => {}
            Tc::Str(n) => self.int(*n),
            Tc::Objref(s) | Tc::SelfRef(s) => self.str(Some(s)),
            Tc::Pointer(t) => self.tc(t),
            Tc::Seq(t, n) | Tc::Array(t, n) => {
                self.tc(t);
                self.int(*n);
            }
            Tc::Struct(name, m) => {
                self.str(Some(name));
                self.int(m.len() as i32);
                for (n, t) in m {
                    self.str(Some(n));
                    self.tc(t);
                }
            }
            Tc::Enum(name, m) => {
                self.str(Some(name));
                self.strs(m);
            }
            Tc::Union(name, sw, m) => {
                self.str(Some(name));
                self.tc(sw);
                self.int(m.len() as i32);
                for (flag, label, n, t) in m {
                    self.int(*flag);
                    if *flag == 1 {
                        self.int(*label);
                    }
                    self.str(Some(n));
                    self.tc(t);
                }
            }
            Tc::Foreign(n, ctx, size) => {
                self.str(Some(n));
                self.str(Some(ctx));
                self.int(*size);
            }
        }
    }

    /// A definition (and what it contains): its fixed record's IID.
    fn def(&mut self, d: &Def, scope: &str, parent: u32) -> u32 {
        let id = format!("{scope}::{}", d.name);
        let fixed = self.begin(d.kind as i32);
        self.str(Some(&d.name));
        self.int(0);
        self.int(parent as i32);
        let ref_at = self.here();
        self.int(0);
        let var_at = self.here();
        self.int(0);
        let mut contents_at = None;
        match d.kind {
            Kind::Attribute | Kind::Parameter => self.int(d.mode),
            Kind::Interface | Kind::Module => {
                contents_at = Some(self.here());
                self.int(0);
            }
            Kind::Operation => {
                contents_at = Some(self.here());
                self.int(0);
                self.int(0);
                self.int(d.mode);
            }
            _ => {}
        }
        self.end(fixed);
        if d.kind == Kind::Interface {
            self.ifaces.insert(id.clone(), fixed);
            self.refs.push((fixed, ref_at, d.bases.clone()));
        }
        // The variable record: modifiers and the class's part.
        let var = self.begin(11);
        self.int(fixed as i32);
        self.int(0);
        self.int(d.mods.len() as i32);
        for (k, v) in &d.mods {
            self.str(Some(k));
            self.str(v.as_deref());
        }
        match d.kind {
            Kind::Attribute | Kind::Parameter | Kind::TypeDef | Kind::Exception => self.tc(&d.tc),
            Kind::Constant => {
                self.tc(&d.tc);
                self.str(Some(&d.value));
            }
            Kind::Interface => {
                self.strs(&d.bases);
                self.tc(&d.tc);
            }
            Kind::Operation => {
                self.tc(&d.tc);
                self.strs(&d.contexts);
                self.strs(&d.raises);
            }
            Kind::Module => {}
        }
        self.end(var);
        self.patch(var_at, var as i32);
        if let Some(at) = contents_at {
            let c = self.contents(&d.children, &id, fixed);
            self.patch(at, c as i32);
        }
        fixed
    }

    /// A container's definitions and their contents record.
    fn contents(&mut self, defs: &[Def], scope: &str, parent: u32) -> u32 {
        let iids: Vec<u32> = defs.iter().map(|d| self.def(d, scope, parent)).collect();
        let c = self.begin(8);
        self.int(defs.len() as i32);
        for (d, iid) in defs.iter().zip(iids) {
            self.int(iid as i32);
            self.int(d.kind as i32);
            self.str(Some(&d.name));
        }
        self.end(c);
        c
    }
}

/// The repository file for the text.
pub fn build(text: &str) -> Vec<u8> {
    let defs = parse(text);
    let mut w = W { b: Vec::new(), ifaces: HashMap::new(), refs: Vec::new() };
    w.int(MAGIC);
    w.int(200);
    w.int(0);
    w.int(0);
    for _ in 0..4 {
        w.int(0);
    }
    let root = w.contents(&defs, "", 0);
    w.patch(8, root as i32);
    // Reference lists: each interface's derived interfaces.
    let mut derived: HashMap<u32, Vec<u32>> = HashMap::new();
    for (fixed, _, bases) in &w.refs {
        for b in bases {
            if let Some(&base) = w.ifaces.get(b) {
                derived.entry(base).or_default().push(*fixed);
            }
        }
    }
    let refs: Vec<(u32, u32)> = w.refs.iter().map(|(f, r, _)| (*f, *r)).collect();
    for (fixed, ref_at) in refs {
        if let Some(list) = derived.get(&fixed) {
            let r = w.begin(9);
            w.int(list.len() as i32);
            for &d in list {
                w.int(d as i32);
            }
            w.end(r);
            w.patch(ref_at, r as i32);
        }
    }
    w.b
}
