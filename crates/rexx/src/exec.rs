//! The executor: runs a program's clause list with a program counter and
//! a control stack per routine activation, evaluates expressions, traces,
//! raises and traps conditions, and calls routines.

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::num::{Num, Numeric};
use crate::parse::*;
use crate::vars::{Key, Pools};
use crate::{CallType, Err, External, Host};
pub(crate) use crate::run::{cond_index, find, words};

pub type R<T> = Result<T, Err>;
pub type Val = Vec<u8>;

/// Interpreter state reachable from host callbacks (the variable pool
/// interface of REXX.DLL).
pub struct State {
    pub pools: Pools,
    pub cur_pool: usize,
    /// The running routine's arguments.
    pub args: Vec<Option<Val>>,
    /// PARSE SOURCE of the running program.
    pub source: Val,
    pub streams: crate::stream::Streams,
    /// RXSHV_NEXTV position: (pool, snapshot, index).
    pub nextv: Option<(usize, Vec<(Val, Val)>, usize)>,
    pub rand: u64,
    pub rand_seeded: bool,
    /// User data the host may keep with the interpreter.
    pub user: usize,
}

impl State {
    /// A placeholder while the real state is lent to the host.
    pub(crate) fn empty() -> State {
        State {
            pools: Pools { pools: Vec::new() },
            cur_pool: 0,
            args: Vec::new(),
            source: Vec::new(),
            streams: crate::stream::Streams::default(),
            nextv: None,
            rand: 0,
            rand_seeded: false,
            user: 0,
        }
    }

    fn new() -> State {
        State {
            pools: Pools::new(),
            cur_pool: 0,
            args: Vec::new(),
            source: Vec::new(),
            streams: crate::stream::Streams::default(),
            nextv: None,
            rand: 0,
            rand_seeded: false,
            user: 0,
        }
    }

    // ---- the variable pool interface ----

    /// Set a variable by name (`symbolic`: tails are substituted).
    pub fn var_set(&mut self, name: &[u8], val: Val, symbolic: bool) -> Result<(), ()> {
        let (stem, tail) = self.resolve_name(name, symbolic)?;
        self.nextv = None;
        let key = key_of(&stem, &tail);
        self.pools.set(self.cur_pool, &key, val);
        Ok(())
    }

    pub fn var_get(&mut self, name: &[u8], symbolic: bool) -> Result<Option<Val>, ()> {
        let (stem, tail) = self.resolve_name(name, symbolic)?;
        let key = key_of(&stem, &tail);
        Ok(self.pools.get(self.cur_pool, &key))
    }

    pub fn var_drop(&mut self, name: &[u8], symbolic: bool) -> Result<(), ()> {
        let (stem, tail) = self.resolve_name(name, symbolic)?;
        self.nextv = None;
        let key = key_of(&stem, &tail);
        self.pools.drop_var(self.cur_pool, &key);
        Ok(())
    }

    /// Next variable (RXSHV_NEXTV); None at the end (and the cursor
    /// restarts).
    pub fn var_next(&mut self) -> Option<(Val, Val)> {
        let pool = self.cur_pool;
        let snap = match self.nextv.take() {
            Some((p, s, i)) if p == pool => (p, s, i),
            _ => (pool, self.pools.list(pool), 0),
        };
        let (p, s, i) = snap;
        if i < s.len() {
            let r = s[i].clone();
            self.nextv = Some((p, s, i + 1));
            Some(r)
        } else {
            None
        }
    }

    /// The name EXPOSE records for a word of an indirect list.
    pub(crate) fn expose_name(&self, w: &[u8]) -> Option<Val> {
        match self.resolve_name(w, true) {
            Ok((stem, None)) => Some(stem),
            Ok((stem, Some(t))) if t.as_slice() == b"\x00STEM" => Some(stem),
            Ok((mut stem, Some(t))) => {
                stem.extend_from_slice(&t);
                Some(stem)
            }
            Err(_) => None,
        }
    }

    /// Split a variable name into stem and tail. Symbolic names are
    /// upper-cased and their tail symbols replaced by their values; direct
    /// names are used as given (the stem part must be upper case).
    pub(crate) fn resolve_name(&self, name: &[u8], symbolic: bool) -> Result<(Val, Option<Val>), ()> {
        if name.is_empty() {
            return Err(());
        }
        if symbolic {
            if !name.iter().all(|&c| crate::lex::is_sym_char(c)) || name[0].is_ascii_digit() || name[0] == b'.' {
                return Err(());
            }
            let u = crate::lex::upper(name);
            match u.iter().position(|&c| c == b'.') {
                None => Ok((u, None)),
                Some(p) => {
                    let stem = u[..=p].to_vec();
                    if p == u.len() - 1 {
                        return Ok((stem, Some(b"\x00STEM".to_vec())));
                    }
                    let mut tail = Vec::new();
                    for (k, part) in u[p + 1..].split(|&c| c == b'.').enumerate() {
                        if k > 0 {
                            tail.push(b'.');
                        }
                        if part.is_empty() || part[0].is_ascii_digit() {
                            tail.extend_from_slice(part);
                        } else {
                            match self.pools.get(self.cur_pool, &Key::Simple(part)) {
                                Some(v) => tail.extend_from_slice(&v),
                                None => tail.extend_from_slice(part),
                            }
                        }
                    }
                    Ok((stem, Some(tail)))
                }
            }
        } else {
            match name.iter().position(|&c| c == b'.') {
                None => {
                    if name[0].is_ascii_digit() || !name.iter().all(|&c| crate::lex::is_sym_char(c)) {
                        return Err(());
                    }
                    Ok((name.to_vec(), None))
                }
                Some(p) => {
                    let stem = name[..=p].to_vec();
                    if p == name.len() - 1 {
                        return Ok((stem, Some(b"\x00STEM".to_vec())));
                    }
                    Ok((stem, Some(name[p + 1..].to_vec())))
                }
            }
        }
    }
}

fn key_of<'a>(stem: &'a [u8], tail: &'a Option<Val>) -> Key<'a> {
    match tail {
        None => Key::Simple(stem),
        Some(t) if t.as_slice() == b"\x00STEM" => Key::Stem(stem),
        Some(t) => Key::Comp(stem, t),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum TrapState {
    Off,
    On,
    Delay,
}

#[derive(Clone)]
pub struct Trap {
    pub state: TrapState,
    pub call: bool,
    pub label: Vec<u8>,
}

#[derive(Clone)]
pub struct CondInfo {
    pub cond: Cond,
    pub desc: Val,
    pub call: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub struct TraceSet {
    pub opt: u8,
    pub interactive: bool,
    pub inhibit: bool,
}

/// A routine activation.
#[derive(Clone)]
pub struct Frame {
    pub prog: Rc<Program>,
    pub numeric: Numeric,
    pub trace: TraceSet,
    pub addr: (Val, Val),
    pub traps: [Trap; 6],
    pub cond: Option<CondInfo>,
    pub base: u32,
    pub internal: bool,
    pub proc_ok: bool,
    pub pool_len: usize,
    pub calltype: CallType,
    pub name: Val,
    pub elapsed: Option<u64>,
    pub pending: Vec<(Cond, Val)>,
    /// Interactive trace: clauses to skip pausing for.
    pub skip: i64,
}


pub(crate) enum Flow {
    Normal,
    Return(Option<Val>),
    Leave(Option<Name>),
    Iterate(Option<Name>),
}

pub(crate) enum Ctl {
    Do { idx: usize, var: Option<VarRef>, to: Option<Num>, by: Num, for_left: Option<i64>, count: Option<i64>, simple: bool },
    Select { idx: usize, taken: bool },
}

/// What a clause does to the program counter.
pub(crate) enum Next {
    Go,
    Jump(usize),
    /// Re-enter a DO at this index (END or ITERATE).
    Again(usize),
    /// LEAVE the DO at this index.
    LeaveDo(usize),
    Flow(Flow),
}

pub struct Interp<'h> {
    pub host: &'h mut dyn Host,
    pub st: State,
    pub(crate) depth: usize,
    /// Trace output suppressed (interactive trace input being run).
    pub(crate) quiet: bool,
}

pub(crate) const MAX_DEPTH: usize = 2000;

pub(crate) fn whole(v: &[u8], digits: usize) -> R<i64> {
    Num::parse(v).and_then(|n| n.to_whole(digits)).ok_or(Err::new(26))
}

pub fn strip_blanks(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ').map_or(a, |p| p + 1);
    &s[a..b.max(a)]
}

impl<'h> Interp<'h> {
    pub fn new(host: &'h mut dyn Host) -> Interp<'h> {
        Interp { host, st: State::new(), depth: 0, quiet: false }
    }

    /// Run a program. Returns its result (EXIT/RETURN value) or the
    /// negative REXX error number it ended with.
    pub fn run_program(
        &mut self,
        src: &[u8],
        name: &[u8],
        calltype: CallType,
        args: Vec<Option<Val>>,
        env: &[u8],
    ) -> Result<Option<Val>, i32> {
        let prog = match crate::parse::program(src) {
            Ok(p) => Rc::new(p),
            Err((e, line)) => {
                let code = match e {
                    Err::Syntax(c) => c,
                    _ => 49,
                };
                let mut st = core::mem::replace(&mut self.st, State::empty());
                self.host.report_error(&mut st, code, name, line);
                self.st = st;
                return Err(-(code as i32));
            }
        };
        let mut f = Frame {
            prog: prog.clone(),
            numeric: Numeric::default(),
            trace: TraceSet { opt: b'N', interactive: false, inhibit: false },
            addr: (env.to_vec(), env.to_vec()),
            traps: core::array::from_fn(|_| Trap { state: TrapState::Off, call: false, label: Vec::new() }),
            cond: None,
            base: 1,
            internal: false,
            proc_ok: false,
            pool_len: self.st.pools.pools.len(),
            calltype,
            name: name.to_vec(),
            elapsed: None,
            pending: Vec::new(),
            skip: 0,
        };
        let mut source = b"OS/2 ".to_vec();
        source.extend_from_slice(calltype.word());
        source.push(b' ');
        source.extend_from_slice(name);
        let saved = (self.st.cur_pool, core::mem::replace(&mut self.st.args, args), core::mem::replace(&mut self.st.source, source));
        let top = self.depth == 0;
        if !top {
            // an external routine: a fresh pool of its own
            self.st.pools.pools.push(crate::vars::Pool::default());
            self.st.cur_pool = self.st.pools.pools.len() - 1;
        } else {
            self.st.cur_pool = 0;
        }
        self.depth += 1;
        if top {
            let mut st = core::mem::replace(&mut self.st, State::empty());
            self.host.init(&mut st);
            self.st = st;
            if let Some(t) = self.host.trace_request() {
                if t {
                    f.trace = TraceSet { opt: b'R', interactive: true, inhibit: false };
                }
            }
        }
        let r = self.run(&mut f, &prog, 0, 0);
        self.depth -= 1;
        let out = match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(None),
            Err(Err::Exit(v)) => Ok(v),
            Err(Err::Fatal(c)) => Err(-(c as i32)),
            Err(Err::Syntax(c)) => Err(-(c as i32)),
            Err(_) => Err(-16),
        };
        if top {
            let mut st = core::mem::replace(&mut self.st, State::empty());
            self.host.term(&mut st);
            self.st = st;
            self.st.streams.close_all(self.host);
        }
        self.st.pools.pop_to(f.pool_len);
        self.st.cur_pool = saved.0;
        self.st.args = saved.1;
        self.st.source = saved.2;
        out
    }

    // ---- tracing ----

    pub(crate) fn tracing_all(&self, f: &Frame) -> bool {
        !self.quiet && matches!(f.trace.opt, b'A' | b'R' | b'I')
    }

    pub(crate) fn trace_line(&mut self, line: &[u8]) {
        let mut st = core::mem::replace(&mut self.st, State::empty());
        self.host.trace_out(&mut st, line);
        self.st = st;
    }

    pub(crate) fn trace_clause(&mut self, c: &Clause, lvl: u32) {
        let mut s = alloc::format!("{:>6} *-*", c.line).into_bytes();
        for _ in 0..(1 + 2 * lvl) {
            s.push(b' ');
        }
        s.extend_from_slice(&c.text);
        self.trace_line(&s);
    }

    pub(crate) fn trace_val(&mut self, tag: &[u8], v: &[u8], lvl: u32) {
        let mut s = b"       ".to_vec();
        s.extend_from_slice(tag);
        for _ in 0..(3 + 2 * lvl) {
            s.push(b' ');
        }
        s.push(b'"');
        s.extend_from_slice(v);
        s.push(b'"');
        self.trace_line(&s);
    }

    pub(crate) fn trace_int(&self, f: &Frame) -> bool {
        !self.quiet && f.trace.opt == b'I'
    }

    pub(crate) fn trace_res(&self, f: &Frame) -> bool {
        !self.quiet && matches!(f.trace.opt, b'R' | b'I')
    }

    // ---- variables ----

    pub(crate) fn tail_of(&self, tails: &[Tail]) -> Val {
        let mut t = Vec::new();
        for (k, part) in tails.iter().enumerate() {
            if k > 0 {
                t.push(b'.');
            }
            match part {
                Tail::Const(c) => t.extend_from_slice(c),
                Tail::Var(n) => match self.st.pools.get(self.st.cur_pool, &Key::Simple(n)) {
                    Some(v) => t.extend_from_slice(&v),
                    None => t.extend_from_slice(n),
                },
            }
        }
        t
    }

    pub fn set_var(&mut self, v: &VarRef, val: Val) {
        self.st.nextv = None;
        match v {
            VarRef::Simple(n) => self.st.pools.set(self.st.cur_pool, &Key::Simple(n), val),
            VarRef::Stem(n) => self.st.pools.set(self.st.cur_pool, &Key::Stem(n), val),
            VarRef::Comp(s, t) => {
                let tail = self.tail_of(t);
                self.st.pools.set(self.st.cur_pool, &Key::Comp(s, &tail), val)
            }
        }
    }

    pub(crate) fn set_simple(&mut self, n: &[u8], val: Val) {
        self.st.pools.set(self.st.cur_pool, &Key::Simple(n), val);
    }

    /// (value or None, derived name)
    pub(crate) fn get_var(&self, v: &VarRef) -> (Option<Val>, Val) {
        match v {
            VarRef::Simple(n) => (self.st.pools.get(self.st.cur_pool, &Key::Simple(n)), n.clone()),
            VarRef::Stem(n) => (self.st.pools.get(self.st.cur_pool, &Key::Stem(n)), n.clone()),
            VarRef::Comp(s, t) => {
                let tail = self.tail_of(t);
                let mut name = s.clone();
                name.extend_from_slice(&tail);
                (self.st.pools.get(self.st.cur_pool, &Key::Comp(s, &tail)), name)
            }
        }
    }

    pub(crate) fn drop_ref(&mut self, v: &VarRef) {
        self.st.nextv = None;
        match v {
            VarRef::Simple(n) => self.st.pools.drop_var(self.st.cur_pool, &Key::Simple(n)),
            VarRef::Stem(n) => self.st.pools.drop_var(self.st.cur_pool, &Key::Stem(n)),
            VarRef::Comp(s, t) => {
                let tail = self.tail_of(t);
                self.st.pools.drop_var(self.st.cur_pool, &Key::Comp(s, &tail))
            }
        }
    }

    // ---- conditions ----

    /// Raise a condition: trapped by SIGNAL ON it leaves the clause; by
    /// CALL ON it is remembered and the handler runs after the clause.
    /// Returns Ok(true) if a trap took it.
    pub fn raise(&mut self, f: &mut Frame, c: Cond, desc: Val) -> R<bool> {
        let t = f.traps[cond_index(c)].clone();
        match t.state {
            TrapState::On if !t.call => {
                f.traps[cond_index(c)].state = TrapState::Off;
                f.cond = Some(CondInfo { cond: c, desc, call: false });
                Err(Err::Signal { label: t.label })
            }
            TrapState::On => {
                f.pending.push((c, desc));
                Ok(true)
            }
            TrapState::Delay => Ok(true),
            TrapState::Off => Ok(false),
        }
    }

    pub(crate) fn novalue(&mut self, f: &mut Frame, name: &[u8]) -> R<()> {
        if f.traps[cond_index(Cond::NoValue)].state == TrapState::On {
            self.raise(f, Cond::NoValue, name.to_vec())?;
        }
        Ok(())
    }

    // ---- expressions ----

    pub fn eval(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<Val> {
        match e {
            Expr::Lit(v) => {
                if self.trace_int(f) {
                    self.trace_val(b">L>", v, lvl);
                }
                Ok(v.clone())
            }
            Expr::Var(v) => {
                let (val, name) = self.get_var(v);
                if self.trace_int(f) {
                    if let VarRef::Comp(..) = v {
                        self.trace_val(b">C>", &name, lvl);
                    }
                }
                match val {
                    Some(x) => {
                        if self.trace_int(f) {
                            self.trace_val(b">V>", &x, lvl);
                        }
                        Ok(x)
                    }
                    None => {
                        if self.trace_int(f) {
                            self.trace_val(b">L>", &name, lvl);
                        }
                        self.novalue(f, &name)?;
                        Ok(name)
                    }
                }
            }
            Expr::Call(name, quoted, args) => {
                let mut av = Vec::with_capacity(args.len());
                for a in args {
                    av.push(match a {
                        Some(x) => Some(self.eval(f, x, lvl)?),
                        None => None,
                    });
                }
                let r = self.call_routine(f, name, *quoted, av, true, lvl)?;
                let r = r.ok_or(Err::new(44))?;
                if self.trace_int(f) {
                    self.trace_val(b">F>", &r, lvl);
                }
                Ok(r)
            }
            Expr::Un(op, x) => {
                let v = self.eval(f, x, lvl)?;
                let r = match op {
                    UnOp::Not => match v.as_slice() {
                        b"0" => b"1".to_vec(),
                        b"1" => b"0".to_vec(),
                        _ => return Err(Err::new(34)),
                    },
                    UnOp::Neg => {
                        let n = Num::parse(&v).ok_or(Err::new(41))?;
                        let mut r = n.neg().operand(f.numeric.digits);
                        r.round(f.numeric.digits);
                        r.format(&f.numeric)
                    }
                    UnOp::Plus => {
                        let n = Num::parse(&v).ok_or(Err::new(41))?;
                        n.operand(f.numeric.digits).format(&f.numeric)
                    }
                };
                if self.trace_int(f) {
                    self.trace_val(b">P>", &r, lvl);
                }
                Ok(r)
            }
            Expr::Bin(op, a, b) => {
                let x = self.eval(f, a, lvl)?;
                let y = self.eval(f, b, lvl)?;
                let r = binop(*op, &x, &y, &f.numeric)?;
                if self.trace_int(f) {
                    self.trace_val(b">O>", &r, lvl);
                }
                Ok(r)
            }
        }
    }

    /// Evaluate an instruction's expression, tracing its result.
    pub(crate) fn eval_top(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<Val> {
        let v = self.eval(f, e, lvl)?;
        if self.trace_res(f) {
            self.trace_val(b">>>", &v, lvl);
        }
        Ok(v)
    }

    pub(crate) fn eval_bool(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<bool> {
        let v = self.eval_top(f, e, lvl)?;
        match v.as_slice() {
            b"0" => Ok(false),
            b"1" => Ok(true),
            _ => Err(Err::new(34)),
        }
    }

    // ---- routines ----

    /// Call a routine by name: internal label, built-in function, external.
    pub fn call_routine(&mut self, f: &mut Frame, name: &[u8], quoted: bool, args: Vec<Option<Val>>, func: bool, lvl: u32) -> R<Option<Val>> {
        if !quoted {
            if let Some(&idx) = f.prog.labels.get(name) {
                return self.call_internal(f, idx, args, if func { CallType::Function } else { CallType::Subroutine }, lvl);
            }
        }
        if let Some(r) = crate::builtins::call(self, f, name, &args) {
            return r.map(Some);
        }
        let mut st = core::mem::replace(&mut self.st, State::empty());
        let ext = self.host.external(&mut st, name, &args, func);
        self.st = st;
        match ext {
            External::Value(v) => Ok(v),
            External::NotFound => Err(Err::new(43)),
            External::Error(n) => Err(Err::new(n)),
            External::Program { source, path } => {
                if self.depth >= MAX_DEPTH {
                    return Err(Err::new(11));
                }
                let ct = if func { CallType::Function } else { CallType::Subroutine };
                let env = f.addr.0.clone();
                // NUMERIC etc. start fresh; streams and the queue are shared
                match self.run_program(&source, &path, ct, args, &env) {
                    Ok(v) => Ok(v),
                    Err(c) => Err(Err::Fatal((-c) as u32)),
                }
            }
        }
    }

    pub(crate) fn call_internal(&mut self, f: &mut Frame, idx: usize, args: Vec<Option<Val>>, ct: CallType, lvl: u32) -> R<Option<Val>> {
        if self.depth >= MAX_DEPTH {
            return Err(Err::new(11));
        }
        let mut nf = f.clone();
        nf.base = lvl + 1;
        nf.internal = true;
        nf.proc_ok = true;
        nf.pool_len = self.st.pools.pools.len();
        nf.calltype = ct;
        nf.pending = Vec::new();
        nf.skip = 0;
        let saved_pool = self.st.cur_pool;
        let saved_args = core::mem::replace(&mut self.st.args, args);
        self.depth += 1;
        let prog = f.prog.clone();
        let r = self.run(&mut nf, &prog, idx, 0);
        self.depth -= 1;
        self.st.pools.pop_to(nf.pool_len);
        self.st.cur_pool = saved_pool;
        self.st.args = saved_args;
        self.st.nextv = None;
        match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn logical(v: &[u8]) -> R<bool> {
    match v {
        b"0" => Ok(false),
        b"1" => Ok(true),
        _ => Err(Err::new(34)),
    }
}

/// Non-strict comparison: numeric if both are numbers, else the strings
/// without leading and trailing blanks, the shorter padded with blanks.
pub fn compare(a: &[u8], b: &[u8], nm: &Numeric) -> core::cmp::Ordering {
    if let (Some(x), Some(y)) = (Num::parse(a), Num::parse(b)) {
        return Num::compare(&x, &y, nm);
    }
    let a = strip_blanks(a);
    let b = strip_blanks(b);
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(b' ');
        let y = b.get(i).copied().unwrap_or(b' ');
        if x != y {
            return x.cmp(&y);
        }
    }
    core::cmp::Ordering::Equal
}

fn bool_val(b: bool) -> Val {
    if b { b"1".to_vec() } else { b"0".to_vec() }
}

pub fn binop(op: BinOp, x: &[u8], y: &[u8], nm: &Numeric) -> R<Val> {
    use core::cmp::Ordering::*;
    let arith = |f: &dyn Fn(&Num, &Num) -> R<Num>| -> R<Val> {
        let a = Num::parse(x).ok_or(Err::new(41))?;
        let b = Num::parse(y).ok_or(Err::new(41))?;
        Ok(f(&a, &b)?.format(nm))
    };
    Ok(match op {
        BinOp::Add => arith(&|a, b| Ok(Num::add(a, b, nm)))?,
        BinOp::Sub => arith(&|a, b| Ok(Num::sub(a, b, nm)))?,
        BinOp::Mul => arith(&|a, b| Ok(Num::mul(a, b, nm)))?,
        BinOp::Div => arith(&|a, b| Num::div(a, b, nm))?,
        BinOp::IDiv => arith(&|a, b| Num::idiv(a, b, nm))?,
        BinOp::Rem => arith(&|a, b| Num::rem(a, b, nm))?,
        BinOp::Pow => arith(&|a, b| Num::power(a, b, nm))?,
        BinOp::Concat => {
            let mut v = x.to_vec();
            v.extend_from_slice(y);
            v
        }
        BinOp::Blank => {
            let mut v = x.to_vec();
            v.push(b' ');
            v.extend_from_slice(y);
            v
        }
        BinOp::Eq => bool_val(compare(x, y, nm) == Equal),
        BinOp::Ne => bool_val(compare(x, y, nm) != Equal),
        BinOp::Gt => bool_val(compare(x, y, nm) == Greater),
        BinOp::Lt => bool_val(compare(x, y, nm) == Less),
        BinOp::Ge => bool_val(compare(x, y, nm) != Less),
        BinOp::Le => bool_val(compare(x, y, nm) != Greater),
        BinOp::SEq => bool_val(x == y),
        BinOp::SNe => bool_val(x != y),
        BinOp::SGt => bool_val(x.cmp(y) == Greater),
        BinOp::SLt => bool_val(x.cmp(y) == Less),
        BinOp::SGe => bool_val(x.cmp(y) != Less),
        BinOp::SLe => bool_val(x.cmp(y) != Greater),
        BinOp::And => bool_val(logical(x)? & logical(y)?),
        BinOp::Or => bool_val(logical(x)? | logical(y)?),
        BinOp::Xor => bool_val(logical(x)? ^ logical(y)?),
    })
}
