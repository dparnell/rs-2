//! The clause loop and the instructions.

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::exec::*;
use crate::num::{Num, int_str};
use crate::parse::*;
use crate::vars::Key;
use crate::{CallType, Err};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Seq,
    Again,
    Leave,
}

pub(crate) fn cond_index(c: Cond) -> usize {
    match c {
        Cond::Error => 0,
        Cond::Failure => 1,
        Cond::Halt => 2,
        Cond::NoValue => 3,
        Cond::NotReady => 4,
        Cond::Syntax => 5,
    }
}

impl Interp<'_> {
    pub(crate) fn run(&mut self, f: &mut Frame, code: &Rc<Program>, mut pc: usize, extra: u32) -> R<Flow> {
        let mut ctl: Vec<Ctl> = Vec::new();
        let main = Rc::ptr_eq(code, &f.prog);
        let n = code.clauses.len();
        let mut mode = Mode::Seq;
        while pc < n {
            let lvl = f.base + extra + code.clauses[pc].level;
            let m = core::mem::replace(&mut mode, Mode::Seq);
            let mut r = self.step(f, code, pc, &mut ctl, lvl, extra, m);
            if r.is_ok() && !f.pending.is_empty() {
                if let Err(e) = self.run_pending(f, code.clauses[pc].line, lvl) {
                    r = Err(e);
                }
            }
            match r {
                Ok(Next::Go) => pc += 1,
                Ok(Next::Jump(t)) => pc = t,
                Ok(Next::Again(t)) => {
                    mode = Mode::Again;
                    pc = t;
                }
                Ok(Next::LeaveDo(t)) => {
                    mode = Mode::Leave;
                    pc = t;
                }
                Ok(Next::Flow(Flow::Leave(name))) => match find_loop(&ctl, code, &name) {
                    Some((k, idx)) => {
                        ctl.truncate(k + 1);
                        mode = Mode::Leave;
                        pc = idx;
                    }
                    None if !main => return Ok(Flow::Leave(name)),
                    None => return Err(self.fatal(f, code, pc, lvl, 28)),
                },
                Ok(Next::Flow(Flow::Iterate(name))) => match find_loop(&ctl, code, &name) {
                    Some((k, idx)) => {
                        ctl.truncate(k + 1);
                        mode = Mode::Again;
                        pc = idx;
                    }
                    None if !main => return Ok(Flow::Iterate(name)),
                    None => return Err(self.fatal(f, code, pc, lvl, 28)),
                },
                Ok(Next::Flow(fl)) => return Ok(fl),
                Err(Err::Syntax(no)) => {
                    let t = cond_index(Cond::Syntax);
                    if f.traps[t].state == TrapState::On {
                        let label = f.traps[t].label.clone();
                        f.traps[t].state = TrapState::Off;
                        f.cond = Some(CondInfo { cond: Cond::Syntax, desc: Vec::new(), call: false });
                        self.set_simple(b"RC", int_str(no as i64));
                        self.set_simple(b"SIGL", int_str(code.clauses[pc].line as i64));
                        if !main {
                            return Err(Err::Goto { label });
                        }
                        ctl.clear();
                        pc = self.label_index(f, code, pc, lvl, &label)?;
                    } else {
                        return Err(self.fatal(f, code, pc, lvl, no));
                    }
                }
                Err(Err::Signal { label }) => {
                    self.set_simple(b"SIGL", int_str(code.clauses[pc].line as i64));
                    if !main {
                        return Err(Err::Goto { label });
                    }
                    ctl.clear();
                    pc = self.label_index(f, code, pc, lvl, &label)?;
                }
                Err(Err::Goto { label }) => {
                    if !main {
                        return Err(Err::Goto { label });
                    }
                    ctl.clear();
                    pc = self.label_index(f, code, pc, lvl, &label)?;
                }
                Err(Err::Fatal(no)) => {
                    self.traceback(&code.clauses[pc], lvl);
                    return Err(Err::Fatal(no));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::Normal)
    }

    fn traceback(&mut self, c: &Clause, lvl: u32) {
        let mut s = alloc::format!("{:>6} +++", c.line).into_bytes();
        for _ in 0..(1 + 2 * lvl) {
            s.push(b' ');
        }
        s.extend_from_slice(&c.text);
        self.trace_line(&s);
    }

    fn label_index(&mut self, f: &mut Frame, code: &Rc<Program>, pc: usize, lvl: u32, label: &[u8]) -> R<usize> {
        match f.prog.labels.get(label) {
            Some(&i) => Ok(i),
            None => Err(self.fatal(f, code, pc, lvl, 16)),
        }
    }

    /// Report an untrapped error at clause `pc`: the clause, then the
    /// message; the program ends.
    fn fatal(&mut self, f: &mut Frame, code: &Rc<Program>, pc: usize, lvl: u32, n: u32) -> Err {
        let c = &code.clauses[pc.min(code.clauses.len() - 1)];
        self.traceback(c, lvl);
        let name = f.name.clone();
        let mut st = core::mem::replace(&mut self.st, State::empty());
        self.host.report_error(&mut st, n, &name, c.line);
        self.st = st;
        Err::Fatal(n)
    }

    /// Run the CALL ON handlers of conditions raised in the last clause.
    fn run_pending(&mut self, f: &mut Frame, line: u32, lvl: u32) -> R<()> {
        while !f.pending.is_empty() {
            let (c, desc) = f.pending.remove(0);
            let t = cond_index(c);
            let label = f.traps[t].label.clone();
            let Some(&idx) = f.prog.labels.get(&label) else { return Err(Err::new(16)) };
            f.traps[t].state = TrapState::Delay;
            let saved = f.cond.replace(CondInfo { cond: c, desc, call: true });
            self.set_simple(b"SIGL", int_str(line as i64));
            let r = self.call_internal(f, idx, Vec::new(), CallType::Subroutine, lvl);
            f.cond = saved;
            if f.traps[t].state == TrapState::Delay {
                f.traps[t].state = TrapState::On;
            }
            r?;
        }
        Ok(())
    }

    fn halted(&mut self) -> bool {
        let mut st = core::mem::replace(&mut self.st, State::empty());
        let h = self.host.halt(&mut st);
        self.st = st;
        h
    }

    #[allow(clippy::too_many_arguments)]
    fn step(&mut self, f: &mut Frame, code: &Rc<Program>, pc: usize, ctl: &mut Vec<Ctl>, lvl: u32, extra: u32, mode: Mode) -> R<Next> {
        let c = &code.clauses[pc];
        if self.halted() && !self.raise(f, Cond::Halt, Vec::new())? {
            return Err(Err::new(4));
        }
        if let Some(t) = self.host.trace_request() {
            f.trace = if t {
                TraceSet { opt: b'R', interactive: true, inhibit: false }
            } else {
                TraceSet { opt: b'N', interactive: false, inhibit: false }
            };
        }
        // trace the clause
        let mut traced = false;
        match &c.instr {
            Instr::Label(_) => {
                if !self.quiet && matches!(f.trace.opt, b'A' | b'R' | b'I' | b'L') {
                    self.trace_clause(c, lvl);
                    traced = true;
                }
            }
            Instr::Command(_) => {
                if !self.quiet && matches!(f.trace.opt, b'A' | b'R' | b'I' | b'C') {
                    self.trace_clause(c, lvl);
                    traced = true;
                }
            }
            Instr::Do(_) if mode != Mode::Seq => {
                if self.tracing_all(f) {
                    self.trace_clause(c, lvl + 1);
                    traced = true;
                }
            }
            _ => {
                if self.tracing_all(f) {
                    self.trace_clause(c, lvl);
                    traced = true;
                }
            }
        }
        if !matches!(c.instr, Instr::Label(_) | Instr::Procedure(_)) {
            f.proc_ok = false;
        }
        let r = self.exec(f, code, pc, ctl, lvl, extra, mode, traced);
        if traced && f.trace.interactive && !self.quiet && r.is_ok() && !matches!(c.instr, Instr::Trace(_)) {
            if f.skip > 0 {
                f.skip -= 1;
            } else if let Some(true) = self.pause(f, lvl)? {
                return Ok(Next::Jump(pc));
            }
        }
        r
    }

    /// Interactive trace pause: run lines the user types until an empty
    /// one; "=" re-runs the clause (Some(true)).
    fn pause(&mut self, f: &mut Frame, lvl: u32) -> R<Option<bool>> {
        loop {
            let line = {
                let mut st = core::mem::replace(&mut self.st, State::empty());
                let l = self.host.trace_in(&mut st);
                self.st = st;
                l
            };
            let Some(line) = line else { return Ok(None) };
            if line.is_empty() {
                return Ok(None);
            }
            if line.as_slice() == b"=" {
                return Ok(Some(true));
            }
            let prog = match crate::parse::program(&line) {
                Ok(p) => Rc::new(p),
                Err((Err::Syntax(n), _)) => {
                    self.interactive_error(n);
                    continue;
                }
                Err(_) => continue,
            };
            let old_quiet = self.quiet;
            self.quiet = true;
            let r = self.run_interactive(f, &prog, lvl);
            self.quiet = old_quiet;
            match r {
                Ok(()) => {}
                Err(Err::Syntax(n)) => self.interactive_error(n),
                Err(e) => return Err(e),
            }
            if !f.trace.interactive || f.trace.opt == b'O' {
                return Ok(None);
            }
        }
    }

    /// Run a line typed at an interactive-trace pause: errors in it are
    /// reported and do not end the program.
    fn run_interactive(&mut self, f: &mut Frame, prog: &Rc<Program>, lvl: u32) -> R<()> {
        // the line runs as INTERPRET code, without the SYNTAX trap
        let saved = f.traps[cond_index(Cond::Syntax)].state;
        f.traps[cond_index(Cond::Syntax)].state = TrapState::Off;
        let mut ctl = Vec::new();
        let mut pc = 0;
        let mut r = Ok(());
        while pc < prog.clauses.len() {
            match self.exec(f, prog, pc, &mut ctl, lvl, lvl, Mode::Seq, false) {
                Ok(Next::Go) => pc += 1,
                Ok(Next::Jump(t)) => pc = t,
                Ok(_) => break,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        f.traps[cond_index(Cond::Syntax)].state = saved;
        r
    }

    fn interactive_error(&mut self, n: u32) {
        let text = self.host.message(n).unwrap_or_default();
        let mut s = alloc::format!("Error {n} in interactive trace: ").into_bytes();
        s.extend_from_slice(&text);
        self.trace_line(&s);
    }

    #[allow(clippy::too_many_arguments)]
    fn exec(&mut self, f: &mut Frame, code: &Rc<Program>, pc: usize, ctl: &mut Vec<Ctl>, lvl: u32, extra: u32, mode: Mode, traced: bool) -> R<Next> {
        let c = &code.clauses[pc];
        match &c.instr {
            Instr::Nop | Instr::Label(_) | Instr::Then => Ok(Next::Go),
            Instr::Error(e) => Err(e.clone()),
            Instr::Say(e) => {
                let v = match e {
                    Some(e) => self.eval_top(f, e, lvl)?,
                    None => Vec::new(),
                };
                let mut st = core::mem::replace(&mut self.st, State::empty());
                self.host.say(&mut st, &v);
                self.st = st;
                Ok(Next::Go)
            }
            Instr::Assign(v, e) => {
                let val = self.eval_top(f, e, lvl)?;
                self.set_var(v, val);
                Ok(Next::Go)
            }
            Instr::Command(e) => {
                let cmd = self.eval(f, e, lvl)?;
                let env = f.addr.0.clone();
                self.command(f, c, &env, cmd, lvl, traced)?;
                Ok(Next::Go)
            }
            Instr::Address(a) => {
                match a {
                    None => {
                        let (x, y) = core::mem::take(&mut f.addr);
                        f.addr = (y, x);
                    }
                    Some((env, None)) => {
                        let e = match env {
                            AddrEnv::Name(n) => n.clone(),
                            AddrEnv::Value(e) => self.eval_top(f, e, lvl)?,
                        };
                        let old = core::mem::replace(&mut f.addr.0, e);
                        f.addr.1 = old;
                    }
                    Some((env, Some(cmd))) => {
                        let e = match env {
                            AddrEnv::Name(n) => n.clone(),
                            AddrEnv::Value(e) => self.eval(f, e, lvl)?,
                        };
                        let cmd = self.eval(f, cmd, lvl)?;
                        self.command(f, c, &e, cmd, lvl, traced)?;
                    }
                }
                Ok(Next::Go)
            }
            Instr::Call(name, quoted, args) => {
                let mut av = Vec::with_capacity(args.len());
                for a in args {
                    av.push(match a {
                        Some(x) => Some(self.eval(f, x, lvl)?),
                        None => None,
                    });
                }
                self.set_simple(b"SIGL", int_str(c.line as i64));
                let r = self.call_routine(f, name, *quoted, av, false, lvl)?;
                match r {
                    Some(v) => {
                        if self.trace_res(f) {
                            self.trace_val(b">>>", &v, lvl);
                        }
                        self.set_simple(b"RESULT", v);
                    }
                    None => self.st.pools.drop_var(self.st.cur_pool, &Key::Simple(b"RESULT")),
                }
                Ok(Next::Go)
            }
            Instr::CallOn(cond, on, name) | Instr::SignalOn(cond, on, name) => {
                let call = matches!(c.instr, Instr::CallOn(..));
                let t = &mut f.traps[cond_index(*cond)];
                if *on {
                    t.state = TrapState::On;
                    t.call = call;
                    t.label = name.clone().unwrap_or_else(|| cond.name().to_vec());
                } else {
                    t.state = TrapState::Off;
                }
                Ok(Next::Go)
            }
            Instr::Signal(label) => {
                self.set_simple(b"SIGL", int_str(c.line as i64));
                Err(Err::Goto { label: label.clone() })
            }
            Instr::SignalValue(e) => {
                let label = self.eval_top(f, e, lvl)?;
                self.set_simple(b"SIGL", int_str(c.line as i64));
                Err(Err::Goto { label })
            }
            Instr::Do(spec) => self.exec_do(f, code, pc, spec, ctl, lvl, mode),
            Instr::End(_) => match ctl.last_mut() {
                Some(Ctl::Do { idx, simple, .. }) if *idx == c.link => {
                    if *simple {
                        ctl.pop();
                        Ok(Next::Go)
                    } else {
                        Ok(Next::Again(c.link))
                    }
                }
                Some(Ctl::Select { idx, taken }) if *idx == c.link => {
                    if !*taken {
                        return Err(Err::new(7));
                    }
                    ctl.pop();
                    Ok(Next::Go)
                }
                _ => Err(Err::new(10)),
            },
            Instr::Drop(items) => {
                for it in items {
                    match it {
                        DropItem::Var(v) => self.drop_ref(v),
                        DropItem::Indirect(v) => {
                            let (val, name) = self.get_var(v);
                            let list = match val {
                                Some(x) => x,
                                None => {
                                    self.novalue(f, &name)?;
                                    name
                                }
                            };
                            self.drop_ref(v);
                            for w in words(&list) {
                                self.st.var_drop(w, true).map_err(|_| Err::new(20))?;
                            }
                        }
                    }
                }
                Ok(Next::Go)
            }
            Instr::Exit(e) => {
                let v = match e {
                    Some(e) => Some(self.eval_top(f, e, lvl)?),
                    None => None,
                };
                Err(Err::Exit(v))
            }
            Instr::Return(e) => {
                let v = match e {
                    Some(e) => Some(self.eval_top(f, e, lvl)?),
                    None => None,
                };
                Ok(Next::Flow(Flow::Return(v)))
            }
            Instr::If(e) => {
                if self.eval_bool(f, e, lvl)? {
                    Ok(Next::Go)
                } else if c.link != 0 {
                    let el = &code.clauses[c.link];
                    if self.tracing_all(f) {
                        self.trace_clause(el, f.base + extra + el.level);
                    }
                    Ok(Next::Jump(c.link + 1))
                } else {
                    Ok(Next::Jump(c.next))
                }
            }
            Instr::Else => Ok(Next::Jump(c.link)),
            Instr::Select => {
                ctl.push(Ctl::Select { idx: pc, taken: false });
                Ok(Next::Go)
            }
            Instr::When(e) => {
                let (sel, taken) = match ctl.last() {
                    Some(Ctl::Select { idx, taken }) => (*idx, *taken),
                    _ => return Err(Err::new(9)),
                };
                if taken {
                    return Ok(Next::Jump(code.clauses[sel].link));
                }
                if self.eval_bool(f, e, lvl)? {
                    if let Some(Ctl::Select { taken, .. }) = ctl.last_mut() {
                        *taken = true;
                    }
                    Ok(Next::Go)
                } else {
                    Ok(Next::Jump(c.link))
                }
            }
            Instr::Otherwise => {
                let (sel, taken) = match ctl.last() {
                    Some(Ctl::Select { idx, taken }) => (*idx, *taken),
                    _ => return Err(Err::new(9)),
                };
                if taken {
                    return Ok(Next::Jump(code.clauses[sel].link));
                }
                if let Some(Ctl::Select { taken, .. }) = ctl.last_mut() {
                    *taken = true;
                }
                Ok(Next::Go)
            }
            Instr::Interpret(e) => {
                let s = self.eval_top(f, e, lvl)?;
                let prog = match crate::parse::program(&s) {
                    Ok(p) => Rc::new(p),
                    Err((e, _)) => return Err(e),
                };
                let r = self.run(f, &prog, 0, extra + c.level + 1)?;
                match r {
                    Flow::Normal => Ok(Next::Go),
                    fl => Ok(Next::Flow(fl)),
                }
            }
            Instr::Iterate(name) | Instr::Leave(name) => {
                let leave = matches!(c.instr, Instr::Leave(_));
                match find_loop(ctl, code, name) {
                    Some((k, idx)) => {
                        ctl.truncate(k + 1);
                        Ok(if leave { Next::LeaveDo(idx) } else { Next::Again(idx) })
                    }
                    None => {
                        if Rc::ptr_eq(code, &f.prog) {
                            Err(Err::new(28))
                        } else if leave {
                            Ok(Next::Flow(Flow::Leave(name.clone())))
                        } else {
                            Ok(Next::Flow(Flow::Iterate(name.clone())))
                        }
                    }
                }
            }
            Instr::NumDigits(e) => {
                let d = match e {
                    Some(e) => {
                        let v = self.eval_top(f, e, lvl)?;
                        whole(&v, f.numeric.digits)?
                    }
                    None => 9,
                };
                if d < 1 || d as usize <= f.numeric.fuzz {
                    return Err(Err::new(33));
                }
                f.numeric.digits = d as usize;
                Ok(Next::Go)
            }
            Instr::NumFuzz(e) => {
                let d = match e {
                    Some(e) => {
                        let v = self.eval_top(f, e, lvl)?;
                        whole(&v, f.numeric.digits)?
                    }
                    None => 0,
                };
                if d < 0 || d as usize >= f.numeric.digits {
                    return Err(Err::new(33));
                }
                f.numeric.fuzz = d as usize;
                Ok(Next::Go)
            }
            Instr::NumForm(form) => {
                f.numeric.engineering = match form {
                    NumForm::Sci => false,
                    NumForm::Eng => true,
                    NumForm::Value(e) => {
                        let v = self.eval_top(f, e, lvl)?;
                        match v.as_slice() {
                            b"SCIENTIFIC" => false,
                            b"ENGINEERING" => true,
                            _ => return Err(Err::new(33)),
                        }
                    }
                };
                Ok(Next::Go)
            }
            Instr::Options(e) => {
                self.eval_top(f, e, lvl)?;
                Ok(Next::Go)
            }
            Instr::Parse(up, src, tpls) => {
                self.exec_parse(f, *up, src, tpls, lvl)?;
                Ok(Next::Go)
            }
            Instr::Procedure(items) => {
                if !f.proc_ok || !f.internal {
                    return Err(Err::new(17));
                }
                f.proc_ok = false;
                let parent = self.st.cur_pool;
                let np = self.st.pools.push(parent);
                self.st.cur_pool = np;
                for it in items {
                    match it {
                        DropItem::Var(v) => {
                            let name = self.ref_name(v);
                            self.st.pools.pools[np].exposed.insert(name);
                        }
                        DropItem::Indirect(v) => {
                            let name = self.ref_name(v);
                            self.st.pools.pools[np].exposed.insert(name);
                            let (val, name) = self.get_var(v);
                            let list = val.unwrap_or(name);
                            for w in words(&list) {
                                let n = self.st.expose_name(w).ok_or(Err::new(20))?;
                                self.st.pools.pools[np].exposed.insert(n);
                            }
                        }
                    }
                }
                Ok(Next::Go)
            }
            Instr::Push(e) | Instr::Queue(e) => {
                let v = match e {
                    Some(e) => self.eval_top(f, e, lvl)?,
                    None => Vec::new(),
                };
                let lifo = matches!(c.instr, Instr::Push(_));
                let mut st = core::mem::replace(&mut self.st, State::empty());
                self.host.queue_add(&mut st, &v, lifo);
                self.st = st;
                Ok(Next::Go)
            }
            Instr::Trace(arg) => {
                let v = match arg {
                    TraceArg::Lit(v) => v.clone(),
                    TraceArg::Value(e) => self.eval_top(f, e, lvl)?,
                };
                self.set_trace(f, &v)?;
                Ok(Next::Go)
            }
        }
    }

    /// The name of a variable reference (tails substituted), for EXPOSE.
    fn ref_name(&self, v: &VarRef) -> Val {
        match v {
            VarRef::Simple(n) | VarRef::Stem(n) => n.clone(),
            VarRef::Comp(s, t) => {
                let mut n = s.clone();
                n.extend_from_slice(&self.tail_of(t));
                n
            }
        }
    }

    /// The TRACE() string of a frame.
    pub fn trace_str(f: &Frame) -> Val {
        let mut v = Vec::new();
        if f.trace.interactive {
            v.push(b'?');
        }
        if f.trace.inhibit {
            v.push(b'!');
        }
        v.push(f.trace.opt);
        v
    }

    /// TRACE setting: [?|!]...[letter] or a number.
    pub fn set_trace(&mut self, f: &mut Frame, v: &[u8]) -> R<()> {
        let s = strip_blanks(v);
        if s.is_empty() {
            f.trace.opt = b'N';
            f.trace.interactive = false;
            return Ok(());
        }
        if let Some(n) = Num::parse(s) {
            let n = n.to_whole(f.numeric.digits).ok_or(Err::new(26))?;
            f.skip = n.abs();
            return Ok(());
        }
        let mut i = 0;
        while i < s.len() && (s[i] == b'?' || s[i] == b'!') {
            if s[i] == b'?' {
                f.trace.interactive = !f.trace.interactive;
                if f.trace.interactive && !self.quiet {
                    let m = self.host.message(114).unwrap_or_default();
                    let mut line = b"       +++ ".to_vec();
                    line.extend_from_slice(&m);
                    self.trace_line(&line);
                }
            } else {
                f.trace.inhibit = !f.trace.inhibit;
            }
            i += 1;
        }
        if i < s.len() {
            let o = s[i].to_ascii_uppercase();
            if !b"ACEFILNORS".contains(&o) {
                return Err(Err::new(24));
            }
            f.trace.opt = o;
            if o == b'O' {
                f.trace.interactive = false;
                f.trace.inhibit = false;
            }
        }
        Ok(())
    }

    fn command(&mut self, f: &mut Frame, c: &Clause, env: &[u8], cmd: Val, lvl: u32, traced: bool) -> R<()> {
        if !self.quiet && matches!(f.trace.opt, b'A' | b'R' | b'I' | b'C') {
            let mut s = alloc::format!("{:>6} >>>", c.line).into_bytes();
            for _ in 0..(3 + 2 * lvl) {
                s.push(b' ');
            }
            s.push(b'"');
            s.extend_from_slice(&cmd);
            s.push(b'"');
            self.trace_line(&s);
        }
        if f.trace.inhibit {
            self.set_simple(b"RC", b"0".to_vec());
            return Ok(());
        }
        let r = {
            let mut st = core::mem::replace(&mut self.st, State::empty());
            let r = self.host.command(&mut st, env, &cmd);
            self.st = st;
            r
        };
        self.set_simple(b"RC", r.rc.clone());
        if r.cond != 0 {
            let show = !self.quiet
                && match f.trace.opt {
                    b'A' | b'R' | b'I' | b'C' | b'E' => true,
                    b'F' | b'N' => r.cond == 2,
                    _ => false,
                };
            if show {
                if !traced {
                    self.trace_clause(c, lvl);
                }
                let mut s = b"       +++".to_vec();
                for _ in 0..(3 + 2 * lvl) {
                    s.push(b' ');
                }
                s.extend_from_slice(b"RC(");
                s.extend_from_slice(&r.rc);
                s.push(b')');
                self.trace_line(&s);
            }
            let cond = if r.cond == 2 && f.traps[cond_index(Cond::Failure)].state != TrapState::Off {
                Cond::Failure
            } else {
                Cond::Error
            };
            self.raise(f, cond, cmd)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_do(&mut self, f: &mut Frame, code: &Rc<Program>, pc: usize, spec: &DoSpec, ctl: &mut Vec<Ctl>, lvl: u32, mode: Mode) -> R<Next> {
        let end = code.clauses[pc].link;
        let simple = spec.var.is_none() && spec.count.is_none() && !spec.forever && spec.while_.is_none() && spec.until.is_none();
        let elvl = lvl + 1;
        match mode {
            Mode::Leave => {
                ctl.pop();
                Ok(Next::Jump(end + 1))
            }
            Mode::Again => {
                if let Some(u) = &spec.until {
                    if self.eval_bool(f, u, elvl)? {
                        ctl.pop();
                        return Ok(Next::Jump(end + 1));
                    }
                }
                if let Some(Ctl::Do { var: Some(v), by, .. }) = ctl.last() {
                    let v = v.clone();
                    let by = by.clone();
                    let (cur, name) = self.get_var(&v);
                    let cur = match cur {
                        Some(x) => x,
                        None => {
                            self.novalue(f, &name)?;
                            name
                        }
                    };
                    let n = Num::parse(&cur).ok_or(Err::new(41))?;
                    let nv = Num::add(&n, &by, &f.numeric).format(&f.numeric);
                    self.set_var(&v, nv);
                }
                if self.do_check(f, ctl, spec, elvl)? {
                    Ok(Next::Go)
                } else {
                    ctl.pop();
                    Ok(Next::Jump(end + 1))
                }
            }
            Mode::Seq => {
                if simple {
                    ctl.push(Ctl::Do { idx: pc, var: None, to: None, by: Num::zero(), for_left: None, count: None, simple: true });
                    return Ok(Next::Go);
                }
                let mut var = None;
                let mut to = None;
                let mut by = Num::from_i64(1);
                let mut for_left = None;
                let mut count = None;
                if let Some((v, init, parts)) = &spec.var {
                    let iv = self.eval_top(f, init, elvl)?;
                    let n = Num::parse(&iv).ok_or(Err::new(41))?;
                    for p in parts {
                        match p {
                            DoPart::To(e) => {
                                let x = self.eval_top(f, e, elvl)?;
                                to = Some(Num::parse(&x).ok_or(Err::new(41))?.operand(f.numeric.digits));
                            }
                            DoPart::By(e) => {
                                let x = self.eval_top(f, e, elvl)?;
                                by = Num::parse(&x).ok_or(Err::new(41))?.operand(f.numeric.digits);
                            }
                            DoPart::For(e) => {
                                let x = self.eval_top(f, e, elvl)?;
                                let k = whole(&x, f.numeric.digits)?;
                                if k < 0 {
                                    return Err(Err::new(26));
                                }
                                for_left = Some(k);
                            }
                        }
                    }
                    let init = Num::add(&n, &Num::zero(), &f.numeric).format(&f.numeric);
                    self.set_var(v, init);
                    var = Some(v.clone());
                }
                if let Some(e) = &spec.count {
                    let x = self.eval_top(f, e, elvl)?;
                    let k = whole(&x, f.numeric.digits)?;
                    if k < 0 {
                        return Err(Err::new(26));
                    }
                    count = Some(k);
                }
                ctl.push(Ctl::Do { idx: pc, var, to, by, for_left, count, simple: false });
                if self.do_check(f, ctl, spec, elvl)? {
                    Ok(Next::Go)
                } else {
                    ctl.pop();
                    Ok(Next::Jump(end + 1))
                }
            }
        }
    }

    /// Whether another iteration runs (count, TO, FOR, WHILE).
    fn do_check(&mut self, f: &mut Frame, ctl: &mut [Ctl], spec: &DoSpec, lvl: u32) -> R<bool> {
        let Some(Ctl::Do { var, to, by, for_left, count, .. }) = ctl.last_mut() else { return Ok(false) };
        if let Some(k) = count {
            if *k <= 0 {
                return Ok(false);
            }
            *k -= 1;
        }
        if let (Some(v), Some(t)) = (var.as_ref(), to.as_ref()) {
            let (cur, _) = self.get_var(v);
            let cur = Num::parse(&cur.unwrap_or_default()).ok_or(Err::new(41))?;
            let ord = Num::compare(&cur, t, &f.numeric);
            let past = if by.neg { ord == core::cmp::Ordering::Less } else { ord == core::cmp::Ordering::Greater };
            if past {
                return Ok(false);
            }
        }
        if let Some(k) = for_left {
            if *k <= 0 {
                return Ok(false);
            }
            *k -= 1;
        }
        if let Some(w) = &spec.while_ {
            if !self.eval_bool(f, w, lvl)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn exec_parse(&mut self, f: &mut Frame, up: bool, src: &ParseSrc, tpls: &[Template], lvl: u32) -> R<()> {
        let mut strings: Vec<Val> = Vec::new();
        match src {
            ParseSrc::Arg => {
                for a in self.st.args.clone() {
                    strings.push(a.unwrap_or_default());
                }
            }
            ParseSrc::Linein => {
                let l = crate::stream::linein_default(self, f)?;
                strings.push(l);
            }
            ParseSrc::Pull => {
                let l = self.pull_line();
                strings.push(l);
            }
            ParseSrc::Source => strings.push(self.st.source.clone()),
            ParseSrc::Version => strings.push(crate::VERSION.to_vec()),
            ParseSrc::Value(e) => {
                let v = match e {
                    Some(e) => self.eval_top(f, e, lvl)?,
                    None => Vec::new(),
                };
                strings.push(v);
            }
            ParseSrc::Var(v) => {
                let (val, name) = self.get_var(v);
                let val = match val {
                    Some(x) => x,
                    None => {
                        self.novalue(f, &name)?;
                        name
                    }
                };
                if self.trace_res(f) {
                    self.trace_val(b">>>", &val, lvl);
                }
                strings.push(val);
            }
        }
        for (i, t) in tpls.iter().enumerate() {
            let mut s = strings.get(i).cloned().unwrap_or_default();
            if up {
                s.make_ascii_uppercase();
            }
            self.parse_template(f, t, &s, lvl)?;
        }
        Ok(())
    }

    /// PULL: the queue, else the terminal.
    pub fn pull_line(&mut self) -> Val {
        let mut st = core::mem::replace(&mut self.st, State::empty());
        let l = match self.host.queue_pull(&mut st) {
            Some(l) => l,
            None => self.host.pull_terminal(&mut st).unwrap_or_default(),
        };
        self.st = st;
        l
    }

    fn assign_section(&mut self, f: &mut Frame, targets: &[Option<VarRef>], sec: &[u8], lvl: u32) {
        if targets.is_empty() {
            return;
        }
        let mut rest = sec;
        for (k, t) in targets.iter().enumerate() {
            let val: Val = if k == targets.len() - 1 {
                if targets.len() == 1 {
                    rest.to_vec()
                } else {
                    let a = rest.iter().position(|&c| c != b' ').unwrap_or(rest.len());
                    rest[a..].to_vec()
                }
            } else {
                let a = rest.iter().position(|&c| c != b' ').unwrap_or(rest.len());
                let r2 = &rest[a..];
                let b = r2.iter().position(|&c| c == b' ').unwrap_or(r2.len());
                let w = r2[..b].to_vec();
                rest = if b < r2.len() { &r2[b + 1..] } else { &r2[b..] };
                w
            };
            match t {
                Some(v) => {
                    if self.trace_res(f) {
                        self.trace_val(b">>>", &val, lvl);
                    }
                    self.set_var(v, val);
                }
                None => {
                    if self.trace_res(f) {
                        self.trace_val(b">.>", &val, lvl);
                    }
                }
            }
        }
    }

    fn pattern_value(&mut self, f: &mut Frame, v: &VarRef) -> R<Val> {
        let (val, name) = self.get_var(v);
        match val {
            Some(x) => Ok(x),
            None => {
                self.novalue(f, &name)?;
                Ok(name)
            }
        }
    }

    fn parse_template(&mut self, f: &mut Frame, tpl: &[TplItem], s: &[u8], lvl: u32) -> R<()> {
        let len = s.len();
        let mut cur = 0usize;
        let mut anchor = 0usize;
        let mut targets: Vec<Option<VarRef>> = Vec::new();
        for it in tpl {
            match it {
                TplItem::Target(t) => targets.push(t.clone()),
                TplItem::Lit(_) | TplItem::VarPat(_) => {
                    let pat = match it {
                        TplItem::Lit(p) => p.clone(),
                        TplItem::VarPat(v) => self.pattern_value(f, v)?,
                        _ => unreachable!(),
                    };
                    let c0 = cur.min(len);
                    let found = if pat.is_empty() { None } else { find(&s[c0..], &pat).map(|k| k + c0) };
                    let (end, next) = match found {
                        Some(k) => (k, k + pat.len()),
                        None => (len, len),
                    };
                    let tg = core::mem::take(&mut targets);
                    self.assign_section(f, &tg, &s[c0..end.max(c0)], lvl);
                    anchor = end;
                    cur = next;
                }
                TplItem::Pos(k, _) | TplItem::VarPos(k, _) => {
                    let nv = match it {
                        TplItem::Pos(_, n) => n.clone(),
                        TplItem::VarPos(_, v) => self.pattern_value(f, v)?,
                        _ => unreachable!(),
                    };
                    let n = whole(&nv, f.numeric.digits)?;
                    let t: i64 = match k {
                        PosKind::Abs => n - 1,
                        PosKind::Plus => anchor as i64 + n,
                        PosKind::Minus => anchor as i64 - n,
                    };
                    let t = t.clamp(0, len as i64) as usize;
                    let c0 = cur.min(len);
                    let sec = if t > c0 { &s[c0..t] } else { &s[c0..] };
                    let tg = core::mem::take(&mut targets);
                    self.assign_section(f, &tg, sec, lvl);
                    cur = t;
                    anchor = t;
                }
            }
        }
        let c0 = cur.min(len);
        let tg = core::mem::take(&mut targets);
        self.assign_section(f, &tg, &s[c0..], lvl);
        Ok(())
    }
}

/// The innermost repetitive DO (named `name` if given): (stack index,
/// clause index).
pub(crate) fn find_loop(ctl: &[Ctl], code: &Program, name: &Option<Name>) -> Option<(usize, usize)> {
    for (k, c) in ctl.iter().enumerate().rev() {
        if let Ctl::Do { idx, simple, .. } = c {
            if *simple {
                continue;
            }
            match name {
                None => return Some((k, *idx)),
                Some(n) => {
                    if let Instr::Do(d) = &code.clauses[*idx].instr {
                        if d.name.as_ref() == Some(n) {
                            return Some((k, *idx));
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Blank-delimited words.
pub fn words(s: &[u8]) -> Vec<&[u8]> {
    s.split(|&c| c == b' ' || c == b'\t').filter(|w| !w.is_empty()).collect()
}
