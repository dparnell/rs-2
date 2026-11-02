//! The executor: runs a program's clause list with a program counter and
//! a control stack per routine activation, evaluates expressions, traces,
//! raises and traps conditions, and calls routines.

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::num::{Num, Numeric};
use crate::parse::*;
use crate::value::{ObjRef, Package, Value};
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
    pub args: Vec<Option<Value>>,
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
        self.pools.set(self.cur_pool, &key, Value::S(val));
        Ok(())
    }

    pub fn var_get(&mut self, name: &[u8], symbolic: bool) -> Result<Option<Val>, ()> {
        Ok(self.var_get_v(name, symbolic)?.map(|v| v.fast_string()))
    }

    /// A variable's value (objects as they are).
    pub fn var_get_v(&mut self, name: &[u8], symbolic: bool) -> Result<Option<Value>, ()> {
        let (stem, tail) = self.resolve_name(name, symbolic)?;
        let key = key_of(&stem, &tail);
        Ok(self.pools.get(self.cur_pool, &key))
    }

    pub fn var_set_v(&mut self, name: &[u8], val: Value, symbolic: bool) -> Result<(), ()> {
        let (stem, tail) = self.resolve_name(name, symbolic)?;
        self.nextv = None;
        let key = key_of(&stem, &tail);
        self.pools.set(self.cur_pool, &key, val);
        Ok(())
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
                                Some(v) => tail.extend_from_slice(&v.fast_string()),
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

impl Trap {
    pub fn off() -> Trap {
        Trap { state: TrapState::Off, call: false, label: Vec::new() }
    }
}

/// The number of conditions with a trap slot (ERROR ... SYNTAX, and Object
/// REXX's LOSTDIGITS, NOMETHOD, NOSTRING, ANY).
pub const NCOND: usize = 10;

#[derive(Clone)]
pub struct CondInfo {
    pub cond: Cond,
    /// USER conditions: the name.
    pub user: Option<Name>,
    pub desc: Val,
    pub call: bool,
    /// ADDITIONAL information.
    pub add: Option<Value>,
    /// SYNTAX: the error (major, or major * 1000 + minor).
    pub code: u32,
    /// RAISE ... RETURN/EXIT: the result.
    pub result: Option<Value>,
    /// RAISE ... EXIT.
    pub exit: bool,
    /// SYNTAX: the secondary message.
    pub msg: Val,
    pub line: u32,
    /// The condition object (made when first asked for).
    pub obj: Option<ObjRef>,
    /// SYNTAX: the traceback (the failing clause first).
    pub tb: Vec<Val>,
}

impl CondInfo {
    pub fn new(cond: Cond, desc: Val) -> CondInfo {
        CondInfo { cond, user: None, desc, call: false, add: None, code: 0, result: None, exit: false, msg: Vec::new(), line: 0, obj: None, tb: Vec::new() }
    }
    /// CONDITION('C'): "USER NAME" for user conditions.
    pub fn name(&self) -> Val {
        match &self.user {
            Some(u) => {
                let mut v = b"USER ".to_vec();
                v.extend_from_slice(u);
                v
            }
            None => self.cond.name().to_vec(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct TraceSet {
    pub opt: u8,
    pub interactive: bool,
    pub inhibit: bool,
}

/// An Object REXX method activation: the message being run.
pub struct Act {
    pub recv: Value,
    /// The class whose method runs, and whether it is a class method.
    pub scope: Option<ObjRef>,
    pub class_side: bool,
    pub name: Vec<u8>,
    pub args: Vec<Option<Value>>,
    /// REPLY was run: the value it gave.
    pub reply: core::cell::RefCell<Option<Option<Value>>>,
    /// The method holds the object's guard.
    pub guarded: core::cell::Cell<bool>,
    /// REPLY was run.
    pub replied: core::cell::Cell<bool>,
    /// The object variable pool's place on the pool stack (EXPOSE).
    pub obj_pool: usize,
}

/// A routine activation.
#[derive(Clone)]
pub struct Frame {
    pub prog: Rc<Program>,
    pub numeric: Numeric,
    pub trace: TraceSet,
    pub addr: (Val, Val),
    pub traps: [Trap; NCOND],
    /// SIGNAL ON / CALL ON USER conditions.
    pub utraps: Vec<(Name, Trap)>,
    pub cond: Option<CondInfo>,
    pub base: u32,
    pub internal: bool,
    pub proc_ok: bool,
    pub pool_len: usize,
    pub calltype: CallType,
    pub name: Val,
    pub elapsed: Option<u64>,
    pub pending: Vec<CondInfo>,
    /// Interactive trace: clauses to skip pausing for.
    pub skip: i64,
    /// Object REXX: the method being run.
    pub act: Option<Rc<Act>>,
    /// Object REXX: the package (classes, routines) of the code.
    pub pkg: Option<Rc<Package>>,
    /// EXPOSE (and USE, then GUARD) may still come.
    pub expose_ok: bool,
    /// The outermost program: untrapped errors are reported here.
    pub top: bool,
}

impl Frame {
    pub fn new(prog: Rc<Program>, name: &[u8], env: &[u8], calltype: CallType, pool_len: usize) -> Frame {
        Frame {
            prog,
            numeric: Numeric::default(),
            trace: TraceSet { opt: b'N', interactive: false, inhibit: false },
            addr: (env.to_vec(), env.to_vec()),
            traps: core::array::from_fn(|_| Trap::off()),
            utraps: Vec::new(),
            cond: None,
            base: 1,
            internal: false,
            proc_ok: false,
            pool_len,
            calltype,
            name: name.to_vec(),
            elapsed: None,
            pending: Vec::new(),
            skip: 0,
            act: None,
            pkg: None,
            expose_ok: false,
            top: false,
        }
    }
}

pub(crate) enum Flow {
    Normal,
    Return(Option<Value>),
    Leave(Option<Name>),
    Iterate(Option<Name>),
}

#[derive(Clone)]
pub(crate) enum Ctl {
    Do { idx: usize, var: Option<VarRef>, to: Option<Num>, by: Num, for_left: Option<i64>, count: Option<i64>, simple: bool, over: Option<(Vec<Value>, usize)> },
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
    /// REPLY with its value.
    Reply(Option<Value>),
}

pub struct Interp<'h> {
    pub host: &'h mut dyn Host,
    pub st: State,
    pub(crate) depth: usize,
    /// Trace output suppressed (interactive trace input being run).
    pub(crate) quiet: bool,
    /// The interactive-trace banner was shown.
    pub(crate) banner: bool,
    /// Object REXX: the objects (None for classic REXX).
    pub w: Option<Rc<crate::oo::World>>,
    /// An error on its way to a caller that may trap it: the traceback
    /// lines so far, and the program and line it happened at.
    pub(crate) tb: Vec<Val>,
    pub(crate) err_at: Option<(Val, u32)>,
    /// The secondary message of the error being raised.
    pub(crate) err_msg: Val,
    /// This interpreter's activity (thread) number, for GUARD.
    pub activity: u64,
    /// A condition an external program RAISEd in its caller.
    pub(crate) raised: Option<CondInfo>,
    /// Clauses run (activities take turns every so many).
    pub(crate) ticks: u32,
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
        Interp { host, st: State::new(), depth: 0, quiet: false, banner: false, w: None, tb: Vec::new(), err_at: None, err_msg: Vec::new(), activity: 1, raised: None, ticks: 0 }
    }

    /// An Object REXX interpreter (`PARSE VERSION` gives OBJREXX 6.00).
    pub fn new_oo(host: &'h mut dyn Host) -> Interp<'h> {
        let mut it = Interp::new(host);
        it.w = Some(crate::oo::World::new());
        it
    }

    /// An interpreter for another activity of an Object REXX program
    /// (START, REPLY): the same objects.
    pub fn new_activity(host: &'h mut dyn Host, w: Rc<crate::oo::World>, activity: u64) -> Interp<'h> {
        let mut it = Interp::new(host);
        it.w = Some(w);
        it.activity = activity;
        it
    }

    pub fn oo(&self) -> bool {
        self.w.is_some()
    }

    /// Object REXX's entry points (only reached with Object REXX).
    pub(crate) fn ops(&self) -> &'static crate::oo::Ops {
        self.w.as_ref().expect("Object REXX").ops
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
        let args = args.into_iter().map(|a| a.map(Value::S)).collect();
        match self.run_program_v(src, name, calltype, args, env) {
            Ok(Some(v)) => Ok(Some(self.fast_str(&v))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The string of a value for the outside world (RexxStart's result).
    pub fn fast_str(&mut self, v: &Value) -> Val {
        match v {
            Value::S(s) => s.clone(),
            Value::O(_) => {
                // run the object's STRING method if it has its own
                let mut f = Frame::new(Rc::new(Program::empty()), b"", b"CMD", CallType::Function, self.st.pools.pools.len());
                self.to_str(&mut f, v).unwrap_or_else(|_| v.fast_string())
            }
        }
    }

    pub fn run_program_v(
        &mut self,
        src: &[u8],
        name: &[u8],
        calltype: CallType,
        args: Vec<Option<Value>>,
        env: &[u8],
    ) -> Result<Option<Value>, i32> {
        let top = self.depth == 0;
        let parsed = if self.oo() { (self.ops().load_package)(src, name).map(|p| (p.main.clone(), Some(p))) } else { crate::parse::program(src).map(|p| (Rc::new(p), None)) };
        let (prog, pkg) = match parsed {
            Ok(p) => p,
            Err((e, line)) => {
                let code = match e {
                    Err::Syntax(c) => c,
                    _ => 49,
                };
                if !top && self.oo() {
                    // the caller may trap it
                    self.err_at = Some((name.to_vec(), line));
                    return Err(-(code as i32) - 0x1000_0000);
                }
                let mut st = core::mem::replace(&mut self.st, State::empty());
                self.host.report_error(&mut st, if code >= 1000 { code / 1000 } else { code }, name, line);
                self.st = st;
                self.report_detail(code);
                return Err(-((code / if code >= 1000 { 1000 } else { 1 }) as i32));
            }
        };
        let mut f = Frame::new(prog.clone(), name, env, calltype, self.st.pools.pools.len());
        f.pkg = pkg.clone();
        f.top = top;
        let mut source = b"OS/2 ".to_vec();
        source.extend_from_slice(calltype.word());
        source.push(b' ');
        source.extend_from_slice(name);
        let saved = (self.st.cur_pool, core::mem::replace(&mut self.st.args, args), core::mem::replace(&mut self.st.source, source));
        if !top {
            // an external routine: a fresh pool of its own
            self.st.pools.pools.push(Rc::new(core::cell::RefCell::new(crate::vars::Pool::default())));
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
        // Object REXX: ::REQUIRES and ::CLASS directives are done first
        let mut r = Ok(Flow::Normal);
        if let Some(p) = &pkg {
            if let Err(e) = (self.ops().install_package)(self, &mut f, p) {
                r = Err(e);
            }
        }
        if r.is_ok() {
            r = self.run(&mut f, &prog, 0, 0);
        }
        let r = match r {
            Err(Err::Syntax(no)) if f.top || !self.oo() => {
                // an error in the directives
                let line = self.err_at.as_ref().map_or(1, |e| e.1);
                let mut st = core::mem::replace(&mut self.st, State::empty());
                self.host.report_error(&mut st, if no >= 1000 { no / 1000 } else { no }, name, line);
                self.st = st;
                self.report_detail(no);
                Err(Err::Fatal(no))
            }
            r => r,
        };
        self.depth -= 1;
        let out = match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(None),
            Err(Err::Exit(v)) => Ok(v),
            Err(Err::Fatal(c)) => Err(-((c / if c >= 1000 { 1000 } else { 1 }) as i32)),
            // passed to the caller (Object REXX)
            Err(Err::Syntax(c)) => Err(-(c as i32) - 0x1000_0000),
            Err(Err::Cond(info)) if !top => {
                self.raised = Some(*info);
                Err(-0x0fff_ffff)
            }
            Err(_) => Err(-16),
        };
        if top {
            let mut st = core::mem::replace(&mut self.st, State::empty());
            self.host.term(&mut st);
            self.st = st;
            self.st.streams.close_all(self.host);
            if let Some(w) = &self.w {
                w.finish();
            }
        }
        self.st.pools.pop_to(f.pool_len);
        self.st.cur_pool = saved.0;
        self.st.args = saved.1;
        self.st.source = saved.2;
        out
    }

    /// The secondary message of an Object REXX error (`Error 40.1: ...`).
    pub(crate) fn report_detail(&mut self, code: u32) {
        if code >= 1000 {
            let msg = core::mem::take(&mut self.err_msg);
            let mut st = core::mem::replace(&mut self.st, State::empty());
            self.host.report_detail(&mut st, code / 1000, code % 1000, &msg);
            self.st = st;
        }
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
        let s = self.clause_line(c, lvl, b"*-*");
        self.trace_line(&s);
    }

    /// A traced clause: classic REXX rebuilds it from its tokens and
    /// indents by nesting and call depth; Object REXX shows it as written,
    /// indented by its nesting.
    pub(crate) fn clause_line(&self, c: &Clause, lvl: u32, tag: &[u8]) -> Val {
        let mut s = alloc::format!("{:>6} ", c.line).into_bytes();
        s.extend_from_slice(tag);
        if self.oo() {
            s.push(b' ');
            for _ in 0..(2 * c.olevel) {
                s.push(b' ');
            }
            s.extend_from_slice(&c.src);
            return s;
        }
        for _ in 0..(1 + 2 * lvl) {
            s.push(b' ');
        }
        s.extend_from_slice(&c.text);
        s
    }

    pub(crate) fn trace_val(&mut self, tag: &[u8], v: &[u8], lvl: u32) {
        let mut s = b"       ".to_vec();
        s.extend_from_slice(tag);
        let lvl = if self.oo() { 0 } else { lvl };
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
                    Some(v) => t.extend_from_slice(&v.fast_string()),
                    None => t.extend_from_slice(n),
                },
            }
        }
        t
    }

    pub fn set_var(&mut self, v: &VarRef, val: Val) {
        self.set_var_v(v, Value::S(val))
    }

    pub fn set_var_v(&mut self, v: &VarRef, val: Value) {
        self.st.nextv = None;
        match v {
            VarRef::Simple(n) => self.st.pools.set(self.st.cur_pool, &Key::Simple(n), val),
            VarRef::Stem(n) => {
                // Object REXX: a stem object assigned to a stem makes the
                // name refer to it
                if let Value::O(o) = &val {
                    if let crate::value::Data::Stem(s, _) = &*o.data.borrow() {
                        self.st.pools.bind_stem(self.st.cur_pool, n, s.clone());
                        return;
                    }
                }
                self.st.pools.set(self.st.cur_pool, &Key::Stem(n), val)
            }
            VarRef::Comp(s, t) => {
                let tail = self.tail_of(t);
                self.st.pools.set(self.st.cur_pool, &Key::Comp(s, &tail), val)
            }
        }
    }

    pub(crate) fn set_simple(&mut self, n: &[u8], val: Val) {
        self.st.pools.set(self.st.cur_pool, &Key::Simple(n), Value::S(val));
    }

    pub(crate) fn set_simple_v(&mut self, n: &[u8], val: Value) {
        self.st.pools.set(self.st.cur_pool, &Key::Simple(n), val);
    }

    /// (value or None, derived name), as strings.
    pub(crate) fn get_var(&self, v: &VarRef) -> (Option<Val>, Val) {
        let (val, name) = self.get_var_v(v);
        (val.map(|x| x.fast_string()), name)
    }

    /// (value or None, derived name).
    pub(crate) fn get_var_v(&self, v: &VarRef) -> (Option<Value>, Val) {
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
        self.raise_info(f, CondInfo::new(c, desc))
    }

    pub(crate) fn raise_info(&mut self, f: &mut Frame, info: CondInfo) -> R<bool> {
        let slot: Option<Trap> = match &info.user {
            Some(u) => f.utraps.iter().find(|(n, _)| n == u).map(|(_, t)| t.clone()),
            None => Some(f.traps[cond_index(info.cond)].clone()),
        };
        // ANY traps every condition (CALL ON ANY not SYNTAX or NOVALUE)
        let mut any = false;
        let t = match slot {
            Some(t) if t.state != TrapState::Off => t,
            _ => {
                let a = f.traps[cond_index(Cond::Any)].clone();
                if a.state == TrapState::Off || (a.call && matches!(info.cond, Cond::Syntax | Cond::NoValue)) {
                    return Ok(false);
                }
                any = true;
                a
            }
        };
        let set_state = |f: &mut Frame, s: TrapState| {
            if any {
                f.traps[cond_index(Cond::Any)].state = s;
            } else if let Some(u) = &info.user {
                if let Some(x) = f.utraps.iter_mut().find(|(n, _)| n == u) {
                    x.1.state = s;
                }
            } else {
                f.traps[cond_index(info.cond)].state = s;
            }
        };
        match t.state {
            TrapState::On if !t.call => {
                set_state(f, TrapState::Off);
                let mut info = info;
                info.call = false;
                if info.cond == Cond::Syntax && info.code != 0 {
                    let rc = if info.code >= 1000 { info.code / 1000 } else { info.code };
                    self.st.pools.set(self.st.cur_pool, &Key::Simple(b"RC"), Value::S(crate::num::int_str(rc as i64)));
                }
                f.cond = Some(info);
                Err(Err::Signal { label: t.label })
            }
            TrapState::On => {
                let mut info = info;
                info.call = true;
                f.pending.push(info);
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

    /// Evaluate an expression to its string.
    pub fn eval(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<Val> {
        let v = self.eval_v(f, e, lvl)?;
        self.to_str(f, &v)
    }

    /// The string value of a value (objects: their STRING method).
    pub fn to_str(&mut self, f: &mut Frame, v: &Value) -> R<Val> {
        match v {
            Value::S(s) => Ok(s.clone()),
            Value::O(o) => (self.ops().obj_string)(self, f, o),
        }
    }

    pub fn eval_v(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<Value> {
        match e {
            Expr::Lit(v) => {
                if self.trace_int(f) {
                    self.trace_val(b">L>", v, lvl);
                }
                Ok(Value::S(v.clone()))
            }
            Expr::Var(v) => {
                if let (VarRef::Stem(n), true) = (v, self.oo()) {
                    // Object REXX: a stem is its stem object
                    return Ok((self.ops().stem_object)(self, n));
                }
                let (val, name) = self.get_var_v(v);
                if self.trace_int(f) {
                    if let VarRef::Comp(..) = v {
                        self.trace_val(b">C>", &name, lvl);
                    }
                }
                match val {
                    Some(x) => {
                        if self.trace_int(f) {
                            let s = x.fast_string();
                            self.trace_val(b">V>", &s, lvl);
                        }
                        Ok(x)
                    }
                    None => {
                        if self.trace_int(f) {
                            self.trace_val(b">L>", &name, lvl);
                        }
                        self.novalue(f, &name)?;
                        Ok(Value::S(name))
                    }
                }
            }
            Expr::Call(name, quoted, args) => {
                let mut av = Vec::with_capacity(args.len());
                for a in args {
                    av.push(match a {
                        Some(x) => Some(self.eval_v(f, x, lvl)?),
                        None => None,
                    });
                }
                let r = self.call_routine_v(f, name, *quoted, av, true, lvl)?;
                let r = r.ok_or(Err::new(44))?;
                if self.trace_int(f) {
                    let s = r.fast_string();
                    self.trace_val(b">F>", &s, lvl);
                }
                Ok(r)
            }
            Expr::Un(op, x) => {
                let v = self.eval_v(f, x, lvl)?;
                if let Value::O(_) = &v {
                    if let Some(r) = (self.ops().unary_op)(self, f, *op, &v, lvl)? {
                        return Ok(r);
                    }
                }
                let v = self.to_str(f, &v)?;
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
                Ok(Value::S(r))
            }
            Expr::Bin(op, a, b) => {
                let x = self.eval_v(f, a, lvl)?;
                let y = self.eval_v(f, b, lvl)?;
                let r = match (&x, &y) {
                    (Value::S(x), Value::S(y)) => match binop(*op, x, y, &f.numeric) {
                        Ok(v) => Value::S(v),
                        Err(e) if self.oo() => return Err(self.detail_arith(e, x, y, *op)),
                        Err(e) => return Err(e),
                    },
                    _ => (self.ops().binary_op)(self, f, *op, x, y, lvl)?,
                };
                if self.trace_int(f) {
                    let s = r.fast_string();
                    self.trace_val(b">O>", &s, lvl);
                }
                Ok(r)
            }
            _ => (self.ops().eval_oo)(self, f, e, lvl),
        }
    }

    /// Object REXX: the detailed error of an arithmetic operation.
    pub(crate) fn detail_arith(&mut self, e: Err, x: &[u8], y: &[u8], op: BinOp) -> Err {
        match e {
            Err::Syntax(41) => {
                let bad = if Num::parse(x).is_none() { x } else { y };
                self.err_msg = crate::oo::msg_subst((self.ops().secondary)(41001).unwrap(), &[bad]);
                Err::Syntax(41001)
            }
            Err::Syntax(42) if matches!(op, BinOp::Div | BinOp::IDiv | BinOp::Rem) && Num::parse(y).is_some_and(|n| n.is_zero()) => {
                self.err_msg = (self.ops().secondary)(42003).unwrap().to_vec();
                Err::Syntax(42003)
            }
            Err::Syntax(34) => {
                let bad = if !matches!(x, b"0" | b"1") { x } else { y };
                self.err_msg = crate::oo::msg_subst(b"A logical value must be 0 or 1, not \"&1\"", &[bad]);
                Err::Syntax(34006)
            }
            e => e,
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

    pub(crate) fn eval_top_v(&mut self, f: &mut Frame, e: &Expr, lvl: u32) -> R<Value> {
        let v = self.eval_v(f, e, lvl)?;
        if self.trace_res(f) {
            let s = v.fast_string();
            self.trace_val(b">>>", &s, lvl);
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
        let args = args.into_iter().map(|a| a.map(Value::S)).collect();
        match self.call_routine_v(f, name, quoted, args, func, lvl)? {
            Some(v) => Ok(Some(self.to_str(f, &v)?)),
            None => Ok(None),
        }
    }

    pub fn call_routine_v(&mut self, f: &mut Frame, name: &[u8], quoted: bool, args: Vec<Option<Value>>, func: bool, lvl: u32) -> R<Option<Value>> {
        if !quoted {
            if let Some(&idx) = f.prog.labels.get(name) {
                return self.call_internal(f, idx, args, if func { CallType::Function } else { CallType::Subroutine }, lvl);
            }
        }
        if self.oo() {
            if let Some(r) = (self.ops().call_builtin_oo)(self, f, name, &args) {
                return r;
            }
        }
        // built-in functions take strings
        let mut sargs = Vec::with_capacity(args.len());
        for a in &args {
            sargs.push(match a {
                Some(v) => Some(self.to_str(f, v)?),
                None => None,
            });
        }
        if self.oo() {
            if let Some(r) = (self.ops().oo_bif)(name, &sargs) {
                return r.map(|v| Some(Value::S(v)));
            }
        }
        if let Some(r) = crate::builtins::call(self, f, name, &sargs) {
            return r.map(|v| Some(Value::S(v)));
        }
        if self.oo() {
            if let Some(r) = (self.ops().call_package_routine)(self, f, name, args.clone(), func, lvl) {
                return r;
            }
        }
        let mut st = core::mem::replace(&mut self.st, State::empty());
        let ext = self.host.external(&mut st, name, &sargs, func);
        self.st = st;
        match ext {
            External::Value(v) => Ok(v.map(Value::S)),
            External::NotFound if self.oo() => {
                self.err_msg = crate::oo::msg_subst((self.ops().secondary)(43001).unwrap(), &[name]);
                Err(Err::Syntax(43001))
            }
            External::NotFound => Err(Err::new(43)),
            External::Error(n) => Err(Err::new(n)),
            External::Program { source, path } => {
                if self.depth >= MAX_DEPTH {
                    return Err(Err::new(11));
                }
                let ct = if func { CallType::Function } else { CallType::Subroutine };
                let env = f.addr.0.clone();
                // NUMERIC etc. start fresh; streams and the queue are shared
                match self.run_program_v(&source, &path, ct, args, &env) {
                    Ok(v) => Ok(v),
                    // a condition RAISEd in the caller
                    Err(-0x0fff_ffff) => match self.raised.take() {
                        Some(info) => self.cond_in_caller(f, info, true),
                        None => Ok(None),
                    },
                    // Object REXX: an error the caller may trap
                    Err(c) if c < -0x1000_0000 => Err(Err::Syntax((-(c + 0x1000_0000)) as u32)),
                    Err(c) => Err(Err::Fatal((-c) as u32)),
                }
            }
        }
    }

    pub(crate) fn call_internal(&mut self, f: &mut Frame, idx: usize, args: Vec<Option<Value>>, ct: CallType, lvl: u32) -> R<Option<Value>> {
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
        nf.top = false;
        nf.expose_ok = false;
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
            Err(Err::Cond(info)) => self.cond_in_caller(f, *info, false),
            Err(e) => Err(e),
        }
    }

    /// A condition RAISEd with RETURN or EXIT in a routine or method this
    /// frame called: raised here; the call's result is the RAISE's.
    /// `boundary`: the call was of a program, method or ::ROUTINE (RAISE
    /// EXIT stops there); else of an internal routine.
    pub(crate) fn cond_in_caller(&mut self, f: &mut Frame, mut info: CondInfo, boundary: bool) -> R<Option<Value>> {
        if info.exit && !boundary {
            return Err(Err::Cond(alloc::boxed::Box::new(info)));
        }
        info.exit = false;
        let result = info.result.clone();
        let code = info.code;
        let cond = info.cond;
        let msg = info.msg.clone();
        if self.raise_info(f, info)? {
            return Ok(result);
        }
        if cond == Cond::Syntax {
            self.err_msg = msg;
            return Err(Err::Syntax(code));
        }
        Ok(result)
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
