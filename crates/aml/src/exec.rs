//! Running AML: the term list walker, expressions and stores.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::*;

/// One method activation (or the top level of a definition block).
pub(crate) struct Frame {
    locals: [Obj; 8],
    args: [Obj; 7],
    /// The scope names are looked up and created in.
    pub(crate) scope: String,
    /// In a method: the names it created, removed when it returns.
    method: bool,
    temps: Vec<String>,
}

impl Frame {
    pub(crate) fn new(scope: String, load: bool) -> Frame {
        Frame { locals: Default::default(), args: Default::default(), scope, method: !load, temps: Vec::new() }
    }
}

pub(crate) enum Flow {
    Next,
    Return(Obj),
    Break,
    Continue,
}

/// Where a result goes.
pub(crate) enum Target {
    Null,
    Local(usize),
    Arg(usize),
    Named(String),
    Debug,
    Ref(Rc<Reference>),
}

/// A NameString: root prefix, parent prefixes, segments.
pub(crate) struct Name {
    root: bool,
    up: usize,
    segs: Vec<String>,
}

fn byte(b: &[u8], p: &mut usize) -> Result<u8> {
    let v = *b.get(*p).ok_or(Error::Parse(*p))?;
    *p += 1;
    Ok(v)
}

fn le(b: &[u8], p: &mut usize, n: usize) -> Result<u64> {
    let s = b.get(*p..*p + n).ok_or(Error::Parse(*p))?;
    *p += n;
    Ok(s.iter().rev().fold(0u64, |a, &c| a << 8 | c as u64))
}

/// A PkgLength's value (its own bytes included).
pub(crate) fn pkg_len(b: &[u8], p: &mut usize) -> Result<usize> {
    let lead = byte(b, p)?;
    let n = (lead >> 6) as usize;
    let mut len = if n == 0 { (lead & 0x3f) as usize } else { (lead & 0x0f) as usize };
    for i in 0..n {
        len |= (byte(b, p)? as usize) << (4 + 8 * i);
    }
    Ok(len)
}

/// A PkgLength: where the package ends.
pub(crate) fn pkg_end(b: &[u8], p: &mut usize) -> Result<usize> {
    let start = *p;
    let end = start + pkg_len(b, p)?;
    if end > b.len() || end < *p {
        return Err(Error::Parse(start));
    }
    Ok(end)
}

fn name_seg(b: &[u8], p: &mut usize) -> Result<String> {
    let s = b.get(*p..*p + 4).ok_or(Error::Parse(*p))?;
    let ok = (s[0].is_ascii_uppercase() || s[0] == b'_') && s[1..].iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
    if !ok {
        return Err(Error::Parse(*p));
    }
    *p += 4;
    Ok(s.iter().map(|&c| c as char).collect())
}

pub(crate) fn name_string(b: &[u8], p: &mut usize) -> Result<Name> {
    let mut n = Name { root: false, up: 0, segs: Vec::new() };
    match b.get(*p) {
        Some(b'\\') => {
            n.root = true;
            *p += 1;
        }
        _ => {
            while b.get(*p) == Some(&b'^') {
                n.up += 1;
                *p += 1;
            }
        }
    }
    match byte(b, p)? {
        0x00 => {}
        0x2e => {
            n.segs.push(name_seg(b, p)?);
            n.segs.push(name_seg(b, p)?);
        }
        0x2f => {
            let c = byte(b, p)?;
            for _ in 0..c {
                n.segs.push(name_seg(b, p)?);
            }
        }
        _ => {
            *p -= 1;
            n.segs.push(name_seg(b, p)?);
        }
    }
    Ok(n)
}

pub(crate) fn is_name_start(c: u8) -> bool {
    c.is_ascii_uppercase() || c == b'_' || c == b'\\' || c == b'^' || c == 0x2e || c == 0x2f
}

/// The absolute path of `n` used in `scope` (no search).
pub(crate) fn join(scope: &str, n: &Name) -> String {
    let mut v: Vec<&str> = if n.root { Vec::new() } else { scope.split('.').filter(|s| !s.is_empty()).collect() };
    for _ in 0..n.up {
        v.pop();
    }
    v.extend(n.segs.iter().map(|s| s.as_str()));
    v.join(".")
}

fn join_seg(scope: &str, seg: &str) -> String {
    if scope.is_empty() { String::from(seg) } else { format!("{scope}.{seg}") }
}

/// Where a term that could not be run ends, if it says (PkgLength).
fn skip_term(b: &[u8], at: usize) -> Option<usize> {
    let mut q = at + 1;
    match *b.get(at)? {
        0x10 | 0x14 | 0xa0 | 0xa1 | 0xa2 | 0x11 | 0x12 | 0x13 => pkg_end(b, &mut q).ok(),
        0x5b if matches!(b.get(at + 1), Some(0x81..=0x87)) => {
            q += 1;
            pkg_end(b, &mut q).ok()
        }
        _ => None,
    }
}

fn access_bytes(t: u8) -> u8 {
    match t & 0x0f {
        2 => 2,
        3 => 4,
        4 => 8,
        _ => 1,
    }
}

fn cmp_bytes(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    a.cmp(b)
}

impl<H: Host> Interp<H> {
    pub(crate) fn ones(&self) -> u64 {
        if self.int64 { u64::MAX } else { 0xffff_ffff }
    }

    fn mask(&self, v: u64) -> u64 {
        v & self.ones()
    }

    fn truth(&self, b: bool) -> Obj {
        Obj::Int(if b { self.ones() } else { 0 })
    }

    fn tick(&mut self) -> Result<()> {
        self.budget = self.budget.checked_sub(1).ok_or(Error::Limit)?;
        Ok(())
    }

    /// Find an existing object: single segments are searched in the
    /// scope and the ones above (ACPI's rule); aliases are followed.
    pub(crate) fn lookup(&self, scope: &str, n: &Name) -> Option<String> {
        let found = if n.root || n.up > 0 || n.segs.len() != 1 {
            let p = join(scope, n);
            self.ns.contains_key(&p).then_some(p)
        } else {
            let mut s: Vec<&str> = scope.split('.').filter(|x| !x.is_empty()).collect();
            loop {
                let mut p = s.join(".");
                if !p.is_empty() {
                    p.push('.');
                }
                p.push_str(&n.segs[0]);
                if self.ns.contains_key(&p) {
                    break Some(p);
                }
                if s.pop().is_none() {
                    break None;
                }
            }
        };
        let mut p = found?;
        for _ in 0..8 {
            match self.ns.get(&p) {
                Some(Obj::Alias(t)) => p = t.clone(),
                _ => break,
            }
        }
        Some(p)
    }

    fn define(&mut self, f: &mut Frame, path: String, o: Obj) {
        if let Some(old) = self.ns.get(&path) {
            if old.is_container() && o.is_container() {
                return;
            }
        } else if f.method {
            f.temps.push(path.clone());
        }
        self.ns.insert(path, o);
    }

    /// Run a term list; at the top level of a definition block a term
    /// that fails is skipped when its length is known (else the rest of
    /// its scope is).
    pub(crate) fn terms(&mut self, b: &Rc<[u8]>, start: usize, end: usize, f: &mut Frame) -> Result<Flow> {
        let mut p = start;
        while p < end {
            let at = p;
            match self.term(b, &mut p, f) {
                Ok(Flow::Next) => {}
                Ok(fl) => return Ok(fl),
                Err(e) if !f.method && e != Error::Limit => {
                    self.load_errors += 1;
                    self.host.debug(&format!("aml: load: {e:?} at {at:#x} in \\{}", f.scope));
                    match skip_term(b, at) {
                        Some(n) if n > at && n <= end => p = n,
                        _ => return Ok(Flow::Next),
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::Next)
    }

    fn scoped(&mut self, b: &Rc<[u8]>, start: usize, end: usize, f: &mut Frame, path: String) -> Result<Flow> {
        let saved = core::mem::replace(&mut f.scope, path);
        let r = self.terms(b, start, end, f);
        f.scope = saved;
        r
    }

    fn term(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Flow> {
        self.tick()?;
        let at = *p;
        let op = *b.get(at).ok_or(Error::Parse(at))?;
        match op {
            // Name.
            0x08 => {
                *p += 1;
                let n = name_string(b, p)?;
                let path = join(&f.scope, &n);
                let o = self.expr(b, p, f)?;
                self.define(f, path, o);
            }
            // Scope.
            0x10 => {
                *p += 1;
                let end = pkg_end(b, p)?;
                let n = name_string(b, p)?;
                let path = self.lookup(&f.scope, &n).unwrap_or_else(|| join(&f.scope, &n));
                let r = self.scoped(b, *p, end, f, path)?;
                *p = end;
                if !matches!(r, Flow::Next) {
                    return Ok(r);
                }
            }
            // Method.
            0x14 => {
                *p += 1;
                let end = pkg_end(b, p)?;
                let n = name_string(b, p)?;
                let flags = byte(b, p)?;
                let path = join(&f.scope, &n);
                let m = Method { args: flags & 7, code: b.clone(), start: *p, end, native: None };
                self.define(f, path, Obj::Method(Rc::new(m)));
                *p = end;
            }
            // Alias.
            0x06 => {
                *p += 1;
                let src = name_string(b, p)?;
                let n = name_string(b, p)?;
                let target = self.lookup(&f.scope, &src).unwrap_or_else(|| join(&f.scope, &src));
                let path = join(&f.scope, &n);
                self.define(f, path, Obj::Alias(target));
            }
            // External.
            0x15 => {
                *p += 1;
                name_string(b, p)?;
                *p += 2;
            }
            // If (Else).
            0xa0 => {
                *p += 1;
                let end = pkg_end(b, p)?;
                let c = self.int(b, p, f)?;
                let body = *p;
                *p = end;
                let mut els = None;
                if b.get(end) == Some(&0xa1) {
                    let mut q = end + 1;
                    let e = pkg_end(b, &mut q)?;
                    els = Some((q, e));
                    *p = e;
                }
                if c != 0 {
                    return self.terms(b, body, end, f);
                } else if let Some((s, e)) = els {
                    return self.terms(b, s, e, f);
                }
            }
            // An Else without its If.
            0xa1 => {
                *p += 1;
                *p = pkg_end(b, p)?;
            }
            // While.
            0xa2 => {
                *p += 1;
                let end = pkg_end(b, p)?;
                let cond = *p;
                *p = end;
                loop {
                    let mut q = cond;
                    if self.int(b, &mut q, f)? == 0 {
                        break;
                    }
                    match self.terms(b, q, end, f)? {
                        Flow::Break => break,
                        Flow::Return(o) => return Ok(Flow::Return(o)),
                        Flow::Next | Flow::Continue => {}
                    }
                }
            }
            // Noop, BreakPoint.
            0xa3 | 0xcc => *p += 1,
            // Return.
            0xa4 => {
                *p += 1;
                let v = self.expr(b, p, f)?;
                return Ok(Flow::Return(v));
            }
            0xa5 => {
                *p += 1;
                return Ok(Flow::Break);
            }
            0x9f => {
                *p += 1;
                return Ok(Flow::Continue);
            }
            // Notify.
            0x86 => {
                *p += 1;
                let t = self.target(b, p, f)?;
                let v = self.int(b, p, f)?;
                let path = match t {
                    Target::Named(s) => s,
                    Target::Ref(r) => match &*r {
                        Reference::Named(s) => s.clone(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                self.notifications.push((path, v));
            }
            0x8a | 0x8b | 0x8c | 0x8d | 0x8f => {
                *p += 1;
                let src = self.expr(b, p, f)?;
                let idx = self.int(b, p, f)?;
                let n = name_string(b, p)?;
                let (bit_off, bits) = match op {
                    0x8a => (idx * 8, 32),
                    0x8b => (idx * 8, 16),
                    0x8c => (idx * 8, 8),
                    0x8d => (idx, 1),
                    _ => (idx * 8, 64),
                };
                self.buffer_field(f, src, bit_off, bits, &n)?;
            }
            0x5b => return self.ext_term(b, p, f),
            _ => {
                self.expr(b, p, f)?;
            }
        }
        Ok(Flow::Next)
    }

    fn buffer_field(&mut self, f: &mut Frame, src: Obj, bit_off: u64, bits: u64, n: &Name) -> Result<()> {
        let src = self.resolve_result(src)?;
        let Obj::Buf(buf) = src else { return Err(Error::Type) };
        let path = join(&f.scope, n);
        self.define(f, path, Obj::BufField(Rc::new(BufField { buf, bit_off, bits })));
        Ok(())
    }

    fn ext_term(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Flow> {
        let at = *p;
        let ext = *b.get(at + 1).ok_or(Error::Parse(at))?;
        match ext {
            // Mutex, Event.
            0x01 | 0x02 => {
                *p += 2;
                let n = name_string(b, p)?;
                if ext == 0x01 {
                    *p += 1;
                }
                let path = join(&f.scope, &n);
                self.define(f, path, if ext == 1 { Obj::Mutex } else { Obj::Event });
            }
            // CreateField.
            0x13 => {
                *p += 2;
                let src = self.expr(b, p, f)?;
                let bit = self.int(b, p, f)?;
                let bits = self.int(b, p, f)?;
                let n = name_string(b, p)?;
                self.buffer_field(f, src, bit, bits, &n)?;
            }
            // Stall, Sleep.
            0x21 => {
                *p += 2;
                let us = self.int(b, p, f)?;
                self.host.stall_us(us.min(1000));
            }
            0x22 => {
                *p += 2;
                let ms = self.int(b, p, f)?;
                self.host.sleep_ms(ms.min(10_000));
            }
            // Signal, Reset, Release.
            0x24 | 0x26 | 0x27 => {
                *p += 2;
                self.target(b, p, f)?;
            }
            // Fatal.
            0x32 => {
                *p += 2;
                let ty = byte(b, p)?;
                let code = le(b, p, 4)?;
                let arg = self.int(b, p, f)?;
                self.host.debug(&format!("aml: Fatal ({ty:#x}, {code:#x}, {arg:#x})"));
                return Err(Error::Fatal);
            }
            // OperationRegion.
            0x80 => {
                *p += 2;
                let n = name_string(b, p)?;
                let space = byte(b, p)?;
                let base = self.int(b, p, f)?;
                let len = self.int(b, p, f)?;
                let path = join(&f.scope, &n);
                let r = Region { space, base, len, scope: f.scope.clone(), pci: Cell::new(None) };
                self.define(f, path, Obj::Region(Rc::new(r)));
            }
            // Field.
            0x81 => {
                *p += 2;
                let end = pkg_end(b, p)?;
                let rn = name_string(b, p)?;
                let region = self.region(&f.scope, &rn)?;
                let flags = byte(b, p)?;
                self.field_list(b, p, end, f, flags, &|| FieldKind::Region(region.clone()))?;
            }
            // IndexField.
            0x86 => {
                *p += 2;
                let end = pkg_end(b, p)?;
                let i = name_string(b, p)?;
                let d = name_string(b, p)?;
                let index = self.lookup(&f.scope, &i).ok_or_else(|| Error::NotFound(join(&f.scope, &i)))?;
                let data = self.lookup(&f.scope, &d).ok_or_else(|| Error::NotFound(join(&f.scope, &d)))?;
                let flags = byte(b, p)?;
                self.field_list(b, p, end, f, flags, &|| FieldKind::Index { index: index.clone(), data: data.clone() })?;
            }
            // BankField.
            0x87 => {
                *p += 2;
                let end = pkg_end(b, p)?;
                let rn = name_string(b, p)?;
                let bn = name_string(b, p)?;
                let region = self.region(&f.scope, &rn)?;
                let bank = self.lookup(&f.scope, &bn).ok_or_else(|| Error::NotFound(join(&f.scope, &bn)))?;
                let value = self.int(b, p, f)?;
                let flags = byte(b, p)?;
                self.field_list(b, p, end, f, flags, &|| FieldKind::Bank { region: region.clone(), bank: bank.clone(), value })?;
            }
            // Device, Processor, PowerResource, ThermalZone.
            0x82 | 0x83 | 0x84 | 0x85 => {
                *p += 2;
                let end = pkg_end(b, p)?;
                let n = name_string(b, p)?;
                let (o, skip) = match ext {
                    0x82 => (Obj::Device, 0),
                    0x83 => (Obj::Processor, 6),
                    0x84 => (Obj::PowerRes, 3),
                    _ => (Obj::ThermalZone, 0),
                };
                *p += skip;
                let path = join(&f.scope, &n);
                self.define(f, path.clone(), o);
                let r = self.scoped(b, *p, end, f, path)?;
                *p = end;
                if !matches!(r, Flow::Next) {
                    return Ok(r);
                }
            }
            _ => {
                self.expr(b, p, f)?;
            }
        }
        Ok(Flow::Next)
    }

    fn region(&self, scope: &str, n: &Name) -> Result<Rc<Region>> {
        let path = self.lookup(scope, n).ok_or_else(|| Error::NotFound(join(scope, n)))?;
        match self.ns.get(&path) {
            Some(Obj::Region(r)) => Ok(r.clone()),
            _ => Err(Error::Type),
        }
    }

    fn field_list(&mut self, b: &Rc<[u8]>, p: &mut usize, end: usize, f: &mut Frame, flags: u8, mk: &dyn Fn() -> FieldKind) -> Result<()> {
        let mut bit = 0u64;
        let mut access = access_bytes(flags);
        let update = (flags >> 5) & 3;
        while *p < end {
            match b[*p] {
                0x00 => {
                    *p += 1;
                    bit += pkg_len(b, p)? as u64;
                }
                0x01 => {
                    access = access_bytes(*b.get(*p + 1).ok_or(Error::Parse(*p))?);
                    *p += 3;
                }
                // Connection (GPIO, serial bus): not used.
                0x02 => {
                    *p += 1;
                    if b.get(*p) == Some(&0x11) {
                        self.expr(b, p, f)?;
                    } else {
                        name_string(b, p)?;
                    }
                }
                0x03 => {
                    access = access_bytes(*b.get(*p + 1).ok_or(Error::Parse(*p))?);
                    *p += 4;
                }
                _ => {
                    let seg = name_seg(b, p)?;
                    let bits = pkg_len(b, p)? as u64;
                    let path = join_seg(&f.scope, &seg);
                    let fl = Field { kind: mk(), bit_off: bit, bits, access, update };
                    self.define(f, path, Obj::Field(Rc::new(fl)));
                    bit += bits;
                }
            }
        }
        *p = end;
        Ok(())
    }

    /// Call method `m` (at `path`).
    pub(crate) fn call(&mut self, path: &str, m: &Rc<Method>, args: Vec<Obj>) -> Result<Obj> {
        if let Some(Native::Osi) = m.native {
            let s = args.first().and_then(|a| a.text()).unwrap_or_default();
            let yes = OSI.contains(&s.as_str());
            return Ok(self.truth(yes));
        }
        if self.depth >= MAX_DEPTH {
            return Err(Error::Limit);
        }
        let mut f = Frame::new(String::from(path), false);
        for (i, a) in args.into_iter().enumerate().take(7) {
            f.args[i] = a;
        }
        self.depth += 1;
        let code = m.code.clone();
        let r = self.terms(&code, m.start, m.end, &mut f);
        self.depth -= 1;
        for t in f.temps.drain(..) {
            self.ns.remove(&t);
        }
        match r? {
            Flow::Return(o) => Ok(o),
            _ => Ok(Obj::Int(0)),
        }
    }

    /// The value of an object as an operand: fields are read.
    pub(crate) fn value(&mut self, o: Obj) -> Result<Obj> {
        match o {
            Obj::Field(fl) => self.read_field(&fl),
            Obj::BufField(bf) => Ok(self.read_buffield(&bf)),
            o => Ok(o),
        }
    }

    pub(crate) fn deref(&mut self, r: &Reference) -> Result<Obj> {
        match r {
            Reference::Named(p) => {
                let o = self.get(p).cloned().ok_or_else(|| Error::NotFound(p.clone()))?;
                self.value(o)
            }
            Reference::Index(c, i) => match c {
                Obj::Pkg(v) => v.borrow().get(*i).cloned().ok_or(Error::Type),
                Obj::Buf(v) => v.borrow().get(*i).map(|&x| Obj::Int(x as u64)).ok_or(Error::Type),
                Obj::Str(s) => s.as_bytes().get(*i).map(|&x| Obj::Int(x as u64)).ok_or(Error::Type),
                _ => Err(Error::Type),
            },
        }
    }

    pub(crate) fn to_int(&mut self, o: &Obj) -> Result<u64> {
        Ok(match o {
            Obj::Int(v) => self.mask(*v),
            Obj::Str(s) => {
                let t = s.trim_start();
                let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
                let mut v = 0u64;
                for c in t.chars() {
                    let Some(d) = c.to_digit(16) else { break };
                    v = v.wrapping_shl(4) | d as u64;
                }
                self.mask(v)
            }
            Obj::Buf(b) => {
                let n = if self.int64 { 8 } else { 4 };
                b.borrow().iter().take(n).rev().fold(0, |a, &c| a << 8 | c as u64)
            }
            Obj::Field(_) | Obj::BufField(_) => {
                let v = self.value(o.clone())?;
                return self.to_int(&v);
            }
            Obj::Ref(r) => {
                let v = self.deref(r)?;
                return self.to_int(&v);
            }
            Obj::NameRef(p) => {
                let p = p.clone();
                let v = self.get(&p).cloned().ok_or(Error::NotFound(p))?;
                let v = self.value(v)?;
                return self.to_int(&v);
            }
            _ => return Err(Error::Type),
        })
    }

    fn int_bytes(&self, v: u64) -> Vec<u8> {
        v.to_le_bytes()[..if self.int64 { 8 } else { 4 }].to_vec()
    }

    pub(crate) fn to_bytes(&mut self, o: &Obj) -> Result<Vec<u8>> {
        Ok(match o {
            Obj::Int(v) => self.int_bytes(*v),
            Obj::Str(s) => s.bytes().collect(),
            Obj::Buf(b) => b.borrow().clone(),
            Obj::Field(_) | Obj::BufField(_) => {
                let v = self.value(o.clone())?;
                return self.to_bytes(&v);
            }
            Obj::Ref(r) => {
                let v = self.deref(r)?;
                return self.to_bytes(&v);
            }
            _ => return Err(Error::Type),
        })
    }

    fn to_str(&mut self, o: &Obj) -> Result<String> {
        Ok(match o {
            Obj::Str(s) => s.clone(),
            Obj::Int(v) => {
                if self.int64 {
                    format!("{v:016X}")
                } else {
                    format!("{v:08X}")
                }
            }
            Obj::Buf(b) => b.borrow().iter().map(|c| format!("{c:02X}")).collect::<Vec<_>>().join(" "),
            Obj::Field(_) | Obj::BufField(_) | Obj::Ref(_) => {
                let v = match o {
                    Obj::Ref(r) => self.deref(r)?,
                    _ => self.value(o.clone())?,
                };
                return self.to_str(&v);
            }
            _ => return Err(Error::Type),
        })
    }

    pub(crate) fn int(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<u64> {
        let o = self.expr(b, p, f)?;
        self.to_int(&o)
    }

    /// A SuperName or Target.
    fn target(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Target> {
        let c = *b.get(*p).ok_or(Error::Parse(*p))?;
        Ok(match c {
            0x00 => {
                *p += 1;
                Target::Null
            }
            0x60..=0x67 => {
                *p += 1;
                Target::Local((c - 0x60) as usize)
            }
            0x68..=0x6e => {
                *p += 1;
                Target::Arg((c - 0x68) as usize)
            }
            0x5b if b.get(*p + 1) == Some(&0x31) => {
                *p += 2;
                Target::Debug
            }
            c if is_name_start(c) => {
                let n = name_string(b, p)?;
                let path = self.lookup(&f.scope, &n).ok_or_else(|| Error::NotFound(join(&f.scope, &n)))?;
                if let Some(Obj::Method(m)) = self.ns.get(&path).cloned() {
                    let mut args = Vec::new();
                    for _ in 0..m.args {
                        args.push(self.expr(b, p, f)?);
                    }
                    match self.call(&path, &m, args)? {
                        Obj::Ref(r) => Target::Ref(r),
                        _ => return Err(Error::Type),
                    }
                } else {
                    Target::Named(path)
                }
            }
            // DerefOf: what the reference points at.
            0x83 => {
                *p += 1;
                match self.expr(b, p, f)? {
                    Obj::Ref(r) => Target::Ref(r),
                    Obj::Str(s) => Target::Named(s),
                    _ => return Err(Error::Type),
                }
            }
            _ => match self.expr(b, p, f)? {
                Obj::Ref(r) => Target::Ref(r),
                _ => return Err(Error::Type),
            },
        })
    }

    fn read_target(&mut self, t: &Target, f: &Frame) -> Result<Obj> {
        match t {
            Target::Local(i) => Ok(f.locals[*i].clone()),
            Target::Arg(i) => match &f.args[*i] {
                Obj::Ref(r) => {
                    let r = r.clone();
                    self.deref(&r)
                }
                o => Ok(o.clone()),
            },
            Target::Named(p) => {
                let o = self.get(p).cloned().ok_or_else(|| Error::NotFound(p.clone()))?;
                self.value(o)
            }
            Target::Ref(r) => self.deref(r),
            Target::Null | Target::Debug => Err(Error::Type),
        }
    }

    /// The object a SuperName names, unevaluated (ObjectType, SizeOf).
    fn supername_obj(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Obj> {
        match self.target(b, p, f)? {
            Target::Named(path) => self.get(&path).cloned().ok_or(Error::NotFound(path)),
            Target::Ref(r) => self.deref(&r),
            t => self.read_target(&t, f),
        }
    }

    pub(crate) fn store(&mut self, t: &Target, v: Obj, f: &mut Frame) -> Result<()> {
        match t {
            Target::Null => {}
            Target::Debug => {
                let s = match &v {
                    Obj::Str(s) => s.clone(),
                    Obj::Int(n) => format!("{n:#x}"),
                    o => format!("{o:?}"),
                };
                self.host.debug(&format!("aml: Debug: {s}"));
            }
            Target::Local(i) => f.locals[*i] = v.deep_copy(),
            Target::Arg(i) => {
                if let Obj::Ref(r) = &f.args[*i] {
                    let r = r.clone();
                    let v = self.resolve_result(v)?;
                    self.store_ref(&r, v)?;
                } else {
                    f.args[*i] = v.deep_copy();
                }
            }
            Target::Named(p) => {
                let v = self.resolve_result(v)?;
                self.store_named(p, v)?;
            }
            Target::Ref(r) => {
                let v = self.resolve_result(v)?;
                self.store_ref(r, v)?;
            }
        }
        Ok(())
    }

    fn store_ref(&mut self, r: &Reference, v: Obj) -> Result<()> {
        match r {
            Reference::Named(p) => self.store_named(p, v),
            Reference::Index(c, i) => match c {
                Obj::Pkg(pk) => {
                    let v = v.deep_copy();
                    let mut x = pk.borrow_mut();
                    *x.get_mut(*i).ok_or(Error::Type)? = v;
                    Ok(())
                }
                Obj::Buf(bf) => {
                    let n = self.to_int(&v)? as u8;
                    *bf.borrow_mut().get_mut(*i).ok_or(Error::Type)? = n;
                    Ok(())
                }
                _ => Err(Error::Type),
            },
        }
    }

    /// Store into a named object, converting to its type as ACPI does.
    pub(crate) fn store_named(&mut self, path: &str, v: Obj) -> Result<()> {
        let mut key = String::from(path);
        for _ in 0..8 {
            match self.ns.get(&key) {
                Some(Obj::Alias(t)) => key = t.clone(),
                _ => break,
            }
        }
        let cur = self.ns.get(&key).cloned().ok_or_else(|| Error::NotFound(key.clone()))?;
        match cur {
            Obj::Field(fl) => self.write_field(&fl, &v)?,
            Obj::BufField(bf) => {
                let bytes = self.to_bytes(&v)?;
                self.write_buffield(&bf, &bytes);
            }
            Obj::Int(_) => {
                let x = self.to_int(&v)?;
                self.ns.insert(key, Obj::Int(x));
            }
            Obj::Str(_) => {
                let s = self.to_str(&v)?;
                self.ns.insert(key, Obj::Str(s));
            }
            Obj::Buf(rc) => {
                let bytes = self.to_bytes(&v)?;
                let mut x = rc.borrow_mut();
                if bytes.len() > x.len() {
                    x.resize(bytes.len(), 0);
                }
                x.fill(0);
                x[..bytes.len()].copy_from_slice(&bytes);
            }
            Obj::Pkg(_) | Obj::Uninit | Obj::NameRef(_) | Obj::Ref(_) => {
                self.ns.insert(key, v.deep_copy());
            }
            _ => return Err(Error::Type),
        }
        Ok(())
    }

    fn name_expr(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Obj> {
        let n = name_string(b, p)?;
        let path = self.lookup(&f.scope, &n).ok_or_else(|| Error::NotFound(join(&f.scope, &n)))?;
        let o = self.ns.get(&path).cloned().ok_or(Error::NotFound(path.clone()))?;
        match o {
            Obj::Method(m) => {
                let mut args = Vec::new();
                for _ in 0..m.args {
                    args.push(self.expr(b, p, f)?);
                }
                self.call(&path, &m, args)
            }
            o => self.value(o),
        }
    }

    fn pkg_elements(&mut self, b: &Rc<[u8]>, p: &mut usize, end: usize, f: &mut Frame) -> Result<Vec<Obj>> {
        let mut v = Vec::new();
        while *p < end {
            let c = b[*p];
            if is_name_start(c) {
                let n = name_string(b, p)?;
                v.push(Obj::NameRef(self.lookup(&f.scope, &n).unwrap_or_else(|| join(&f.scope, &n))));
            } else {
                v.push(self.expr(b, p, f)?);
            }
        }
        *p = end;
        Ok(v)
    }

    /// A TermArg.
    pub(crate) fn expr(&mut self, b: &Rc<[u8]>, p: &mut usize, f: &mut Frame) -> Result<Obj> {
        self.tick()?;
        let at = *p;
        let op = byte(b, p)?;
        Ok(match op {
            0x00 => Obj::Int(0),
            0x01 => Obj::Int(1),
            0xff => Obj::Int(self.ones()),
            0x0a => Obj::Int(le(b, p, 1)?),
            0x0b => Obj::Int(le(b, p, 2)?),
            0x0c => Obj::Int(le(b, p, 4)?),
            0x0e => Obj::Int(self.mask(le(b, p, 8)?)),
            0x0d => {
                let mut s = String::new();
                loop {
                    let c = byte(b, p)?;
                    if c == 0 {
                        break;
                    }
                    s.push(c as char);
                }
                Obj::Str(s)
            }
            0x11 => {
                let end = pkg_end(b, p)?;
                let size = self.int(b, p, f)? as usize;
                let mut v = b.get(*p..end).ok_or(Error::Parse(*p))?.to_vec();
                *p = end;
                if size > v.len() {
                    if size > 1 << 20 {
                        return Err(Error::Limit);
                    }
                    v.resize(size, 0);
                }
                Obj::buffer(v)
            }
            0x12 | 0x13 => {
                let end = pkg_end(b, p)?;
                let count = if op == 0x12 { byte(b, p)? as usize } else { self.int(b, p, f)? as usize };
                let mut v = self.pkg_elements(b, p, end, f)?;
                if count > v.len() && count < 4096 {
                    v.resize(count, Obj::Uninit);
                }
                Obj::package(v)
            }
            0x60..=0x67 => f.locals[(op - 0x60) as usize].clone(),
            0x68..=0x6e => f.args[(op - 0x68) as usize].clone(),
            c if is_name_start(c) => {
                *p = at;
                self.name_expr(b, p, f)?
            }
            // Store.
            0x70 => {
                let v = self.expr(b, p, f)?;
                let t = self.target(b, p, f)?;
                self.store(&t, v.clone(), f)?;
                v
            }
            // RefOf.
            0x71 => match self.target(b, p, f)? {
                Target::Named(s) => Obj::Ref(Rc::new(Reference::Named(s))),
                Target::Ref(r) => Obj::Ref(r),
                t => self.read_target(&t, f)?,
            },
            0x72 | 0x74 | 0x77 | 0x79 | 0x7a | 0x7b | 0x7c | 0x7d | 0x7e | 0x7f | 0x85 => {
                let x = self.int(b, p, f)?;
                let y = self.int(b, p, f)?;
                let r = match op {
                    0x72 => x.wrapping_add(y),
                    0x74 => x.wrapping_sub(y),
                    0x77 => x.wrapping_mul(y),
                    0x79 => {
                        if y >= 64 {
                            0
                        } else {
                            x << y
                        }
                    }
                    0x7a => {
                        if y >= 64 {
                            0
                        } else {
                            x >> y
                        }
                    }
                    0x7b => x & y,
                    0x7c => !(x & y),
                    0x7d => x | y,
                    0x7e => !(x | y),
                    0x7f => x ^ y,
                    _ => {
                        if y == 0 {
                            return Err(Error::DivideByZero);
                        }
                        x % y
                    }
                };
                let r = Obj::Int(self.mask(r));
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // Concatenate.
            0x73 => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let x = self.value(x)?;
                let y = self.expr(b, p, f)?;
                let r = match &x {
                    Obj::Int(v) => {
                        let mut bytes = self.int_bytes(*v);
                        let yv = self.to_int(&y)?;
                        bytes.extend(self.int_bytes(yv));
                        Obj::buffer(bytes)
                    }
                    Obj::Buf(_) => {
                        let mut bytes = self.to_bytes(&x)?;
                        bytes.extend(self.to_bytes(&y)?);
                        Obj::buffer(bytes)
                    }
                    _ => {
                        let mut s = self.to_str(&x)?;
                        s.push_str(&self.to_str(&y)?);
                        Obj::Str(s)
                    }
                };
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // Increment, Decrement.
            0x75 | 0x76 => {
                let t = self.target(b, p, f)?;
                let v = self.read_target(&t, f)?;
                let v = self.to_int(&v)?;
                let r = Obj::Int(self.mask(if op == 0x75 { v.wrapping_add(1) } else { v.wrapping_sub(1) }));
                self.store(&t, r.clone(), f)?;
                r
            }
            // Divide.
            0x78 => {
                let x = self.int(b, p, f)?;
                let y = self.int(b, p, f)?;
                if y == 0 {
                    return Err(Error::DivideByZero);
                }
                let rem = self.target(b, p, f)?;
                self.store(&rem, Obj::Int(x % y), f)?;
                let q = self.target(b, p, f)?;
                self.store(&q, Obj::Int(x / y), f)?;
                Obj::Int(x / y)
            }
            // Not, FindSetLeftBit, FindSetRightBit, FromBCD-likes.
            0x80 | 0x81 | 0x82 => {
                let x = self.int(b, p, f)?;
                let r = match op {
                    0x80 => self.mask(!x),
                    0x81 => {
                        if x == 0 {
                            0
                        } else {
                            64 - x.leading_zeros() as u64
                        }
                    }
                    _ => {
                        if x == 0 {
                            0
                        } else {
                            x.trailing_zeros() as u64 + 1
                        }
                    }
                };
                let t = self.target(b, p, f)?;
                self.store(&t, Obj::Int(r), f)?;
                Obj::Int(r)
            }
            // DerefOf.
            0x83 => {
                let o = self.expr(b, p, f)?;
                match o {
                    Obj::Ref(r) => self.deref(&r)?,
                    Obj::Str(s) => {
                        let o = self.get(&s).cloned().ok_or(Error::NotFound(s))?;
                        self.value(o)?
                    }
                    o => o,
                }
            }
            // ConcatenateResTemplate.
            0x84 => {
                let x = self.expr(b, p, f)?;
                let mut x = self.to_bytes(&x)?;
                let y = self.expr(b, p, f)?;
                let mut y = self.to_bytes(&y)?;
                if x.len() >= 2 && x[x.len() - 2] == 0x79 {
                    x.truncate(x.len() - 2);
                }
                if y.len() < 2 || y[y.len() - 2] != 0x79 {
                    y.extend([0x79, 0]);
                }
                x.extend(y);
                let r = Obj::buffer(x);
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // SizeOf.
            0x87 => {
                let o = self.supername_obj(b, p, f)?;
                let o = self.value(o)?;
                Obj::Int(match o {
                    Obj::Str(s) => s.len() as u64,
                    Obj::Buf(v) => v.borrow().len() as u64,
                    Obj::Pkg(v) => v.borrow().len() as u64,
                    _ => return Err(Error::Type),
                })
            }
            // Index.
            0x88 => {
                let src = self.expr(b, p, f)?;
                let src = self.resolve_result(src)?;
                let src = self.value(src)?;
                let i = self.int(b, p, f)? as usize;
                let r = Obj::Ref(Rc::new(Reference::Index(src, i)));
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // Match.
            0x89 => {
                let pk = self.expr(b, p, f)?;
                let pk = self.resolve_result(pk)?;
                let op1 = byte(b, p)?;
                let o1 = self.int(b, p, f)?;
                let op2 = byte(b, p)?;
                let o2 = self.int(b, p, f)?;
                let start = self.int(b, p, f)? as usize;
                let items = pk.elements().ok_or(Error::Type)?;
                let test = |op: u8, e: u64, o: u64| match op {
                    0 => true,
                    1 => e == o,
                    2 => e <= o,
                    3 => e < o,
                    4 => e >= o,
                    5 => e > o,
                    _ => false,
                };
                let mut found = self.ones();
                for (i, e) in items.iter().enumerate().skip(start) {
                    let Ok(e) = self.to_int(e) else { continue };
                    if test(op1, e, o1) && test(op2, e, o2) {
                        found = i as u64;
                        break;
                    }
                }
                Obj::Int(found)
            }
            // ObjectType.
            0x8e => {
                let o = self.supername_obj(b, p, f)?;
                Obj::Int(o.type_number())
            }
            // LAnd, LOr.
            0x90 | 0x91 => {
                let x = self.int(b, p, f)? != 0;
                let y = self.int(b, p, f)? != 0;
                self.truth(if op == 0x90 { x && y } else { x || y })
            }
            // LNot.
            0x92 => {
                let x = self.int(b, p, f)?;
                self.truth(x == 0)
            }
            // LEqual, LGreater, LLess.
            0x93 | 0x94 | 0x95 => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let x = self.value(x)?;
                let y = self.expr(b, p, f)?;
                let ord = match &x {
                    Obj::Str(s) => {
                        let ys = self.to_str(&y)?;
                        cmp_bytes(s.as_bytes(), ys.as_bytes())
                    }
                    Obj::Buf(v) => {
                        let yb = self.to_bytes(&y)?;
                        let xb = v.borrow().clone();
                        cmp_bytes(&xb, &yb)
                    }
                    _ => {
                        let xv = self.to_int(&x)?;
                        let yv = self.to_int(&y)?;
                        xv.cmp(&yv)
                    }
                };
                use core::cmp::Ordering::*;
                self.truth(match op {
                    0x93 => ord == Equal,
                    0x94 => ord == Greater,
                    _ => ord == Less,
                })
            }
            // ToBuffer.
            0x96 => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let mut v = self.to_bytes(&x)?;
                if matches!(x, Obj::Str(_)) {
                    v.push(0);
                }
                let r = Obj::buffer(v);
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // ToDecimalString, ToHexString.
            0x97 | 0x98 => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let x = self.value(x)?;
                let s = match &x {
                    Obj::Int(v) => {
                        if op == 0x97 {
                            format!("{v}")
                        } else {
                            format!("{v:X}")
                        }
                    }
                    Obj::Str(s) => s.clone(),
                    Obj::Buf(v) => v
                        .borrow()
                        .iter()
                        .map(|c| if op == 0x97 { format!("{c}") } else { format!("0x{c:02X}") })
                        .collect::<Vec<_>>()
                        .join(","),
                    _ => return Err(Error::Type),
                };
                let r = Obj::Str(s);
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // ToInteger.
            0x99 => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let x = self.value(x)?;
                let v = match &x {
                    Obj::Str(s) => {
                        let t = s.trim();
                        if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                            h.chars().map_while(|c| c.to_digit(16)).fold(0u64, |a, d| a.wrapping_shl(4) | d as u64)
                        } else {
                            t.chars().map_while(|c| c.to_digit(10)).fold(0u64, |a, d| a.wrapping_mul(10).wrapping_add(d as u64))
                        }
                    }
                    o => self.to_int(o)?,
                };
                let r = Obj::Int(self.mask(v));
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // ToString.
            0x9c => {
                let x = self.expr(b, p, f)?;
                let bytes = self.to_bytes(&x)?;
                let n = self.int(b, p, f)?;
                let s: String = bytes.iter().take(if n == self.ones() { usize::MAX } else { n as usize }).take_while(|&&c| c != 0).map(|&c| c as char).collect();
                let r = Obj::Str(s);
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            // CopyObject.
            0x9d => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let t = self.target(b, p, f)?;
                match &t {
                    Target::Named(path) => {
                        self.ns.insert(path.clone(), x.deep_copy());
                    }
                    t => self.store(t, x.clone(), f)?,
                }
                x
            }
            // Mid.
            0x9e => {
                let x = self.expr(b, p, f)?;
                let x = self.resolve_result(x)?;
                let x = self.value(x)?;
                let i = self.int(b, p, f)? as usize;
                let n = self.int(b, p, f)? as usize;
                let r = match &x {
                    Obj::Str(s) => Obj::Str(s.chars().skip(i).take(n).collect()),
                    Obj::Buf(v) => Obj::buffer(v.borrow().iter().skip(i).take(n).copied().collect()),
                    _ => return Err(Error::Type),
                };
                let t = self.target(b, p, f)?;
                self.store(&t, r.clone(), f)?;
                r
            }
            0x5b => {
                let ext = byte(b, p)?;
                match ext {
                    // CondRefOf.
                    0x12 => {
                        let c = *b.get(*p).ok_or(Error::Parse(*p))?;
                        let r = if is_name_start(c) {
                            let n = name_string(b, p)?;
                            self.lookup(&f.scope, &n).map(|s| Obj::Ref(Rc::new(Reference::Named(s))))
                        } else {
                            match self.target(b, p, f)? {
                                Target::Ref(r) => Some(Obj::Ref(r)),
                                Target::Named(s) => Some(Obj::Ref(Rc::new(Reference::Named(s)))),
                                t => Some(self.read_target(&t, f)?).filter(|o| !matches!(o, Obj::Uninit)),
                            }
                        };
                        let t = self.target(b, p, f)?;
                        match r {
                            Some(r) => {
                                self.store(&t, r, f)?;
                                self.truth(true)
                            }
                            None => Obj::Int(0),
                        }
                    }
                    // Acquire (mutex, timeout): the interpreter is the only
                    // user; it always gets it.
                    0x23 => {
                        self.target(b, p, f)?;
                        *p += 2;
                        Obj::Int(0)
                    }
                    // Wait (event, timeout).
                    0x25 => {
                        self.target(b, p, f)?;
                        self.int(b, p, f)?;
                        Obj::Int(0)
                    }
                    // FromBCD, ToBCD.
                    0x28 | 0x29 => {
                        let mut x = self.int(b, p, f)?;
                        let mut r = 0u64;
                        let mut m = 1u64;
                        if ext == 0x28 {
                            while x != 0 {
                                r += (x & 0xf) * m;
                                m = m.wrapping_mul(10);
                                x >>= 4;
                            }
                        } else {
                            let mut sh = 0;
                            while x != 0 && sh < 64 {
                                r |= (x % 10) << sh;
                                sh += 4;
                                x /= 10;
                            }
                        }
                        let t = self.target(b, p, f)?;
                        self.store(&t, Obj::Int(r), f)?;
                        Obj::Int(r)
                    }
                    0x30 => Obj::Int(2),
                    0x31 => Obj::Uninit,
                    0x33 => Obj::Int(self.host.timer()),
                    _ => return Err(Error::Unsupported(0x5b00 | ext as u16)),
                }
            }
            _ => return Err(Error::Unsupported(op as u16)),
        })
    }

    /// Evaluate the object `path` from inside the interpreter (no budget
    /// reset).
    pub(crate) fn eval_inner(&mut self, path: &str) -> Result<Obj> {
        let o = self.get(path).cloned().ok_or_else(|| Error::NotFound(String::from(path)))?;
        match o {
            Obj::Method(m) => self.call(path, &m, vec![]),
            o => self.value(o),
        }
    }
}
