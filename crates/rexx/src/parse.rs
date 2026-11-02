//! Clauses to instructions. A program is a flat list of instructions;
//! IF/THEN/ELSE, DO/END and SELECT/WHEN/OTHERWISE are linked by index
//! (`link`) so the executor can run it with a program counter (which is
//! what SIGNAL needs). Each instruction keeps its trace text, rebuilt from
//! the tokens as IBM's REXX shows clauses: tokens separated by one blank,
//! keywords capitalised, symbols as first spelt in the program.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::Err;
use crate::lex::{RawClause, Tok, Token, upper};

pub type Name = Vec<u8>;

#[derive(Clone, Debug)]
pub enum Tail {
    Const(Vec<u8>),
    Var(Name),
}

#[derive(Clone, Debug)]
pub enum VarRef {
    Simple(Name),
    Stem(Name),
    Comp(Name, Vec<Tail>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    IDiv,
    Rem,
    Pow,
    Concat,
    Blank,
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    SEq,
    SNe,
    SGt,
    SLt,
    SGe,
    SLe,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Not,
    Neg,
    Plus,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Lit(Vec<u8>),
    Var(VarRef),
    Call(Name, bool, Vec<Option<Expr>>),
    Un(UnOp, alloc::boxed::Box<Expr>),
    Bin(BinOp, alloc::boxed::Box<Expr>, alloc::boxed::Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PosKind {
    Abs,
    Plus,
    Minus,
}

#[derive(Clone, Debug)]
pub enum TplItem {
    /// None is the placeholder `.`
    Target(Option<VarRef>),
    Lit(Vec<u8>),
    VarPat(VarRef),
    Pos(PosKind, Vec<u8>),
    VarPos(PosKind, VarRef),
}

pub type Template = Vec<TplItem>;

#[derive(Clone, Debug)]
pub enum ParseSrc {
    Arg,
    Linein,
    Pull,
    Source,
    Value(Option<Expr>),
    Var(VarRef),
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    Error,
    Failure,
    Halt,
    NoValue,
    NotReady,
    Syntax,
}

impl Cond {
    pub fn name(self) -> &'static [u8] {
        match self {
            Cond::Error => b"ERROR",
            Cond::Failure => b"FAILURE",
            Cond::Halt => b"HALT",
            Cond::NoValue => b"NOVALUE",
            Cond::NotReady => b"NOTREADY",
            Cond::Syntax => b"SYNTAX",
        }
    }
    fn from(s: &[u8]) -> Option<Cond> {
        Some(match s {
            b"ERROR" => Cond::Error,
            b"FAILURE" => Cond::Failure,
            b"HALT" => Cond::Halt,
            b"NOVALUE" => Cond::NoValue,
            b"NOTREADY" => Cond::NotReady,
            b"SYNTAX" => Cond::Syntax,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub enum DoPart {
    To(Expr),
    By(Expr),
    For(Expr),
}

#[derive(Clone, Debug)]
pub struct DoSpec {
    pub var: Option<(VarRef, Expr, Vec<DoPart>)>,
    pub count: Option<Expr>,
    pub forever: bool,
    pub while_: Option<Expr>,
    pub until: Option<Expr>,
    /// Upper-cased control variable name (for END/LEAVE/ITERATE names).
    pub name: Option<Name>,
}

#[derive(Clone, Debug)]
pub enum Instr {
    Nop,
    Label(Name),
    Assign(VarRef, Expr),
    Command(Expr),
    Say(Option<Expr>),
    /// ADDRESS: None toggles; Some((env, Some(cmd))) runs one command;
    /// Some((env, None)) sets the environment.
    Address(Option<(AddrEnv, Option<Expr>)>),
    Call(Name, bool, Vec<Option<Expr>>),
    CallOn(Cond, bool, Option<Name>),
    SignalOn(Cond, bool, Option<Name>),
    Signal(Name),
    SignalValue(Expr),
    Do(alloc::boxed::Box<DoSpec>),
    End(Option<Name>),
    Drop(Vec<DropItem>),
    Exit(Option<Expr>),
    Return(Option<Expr>),
    If(Expr),
    Then,
    Else,
    Select,
    When(Expr),
    Otherwise,
    Interpret(Expr),
    Iterate(Option<Name>),
    Leave(Option<Name>),
    NumDigits(Option<Expr>),
    NumFuzz(Option<Expr>),
    NumForm(NumForm),
    Options(Expr),
    Parse(bool, ParseSrc, Vec<Template>),
    Procedure(Vec<DropItem>),
    Push(Option<Expr>),
    Queue(Option<Expr>),
    Trace(TraceArg),
    /// A clause that could not be tokenized or parsed: raised when reached.
    Error(Err),
}

#[derive(Clone, Debug)]
pub enum AddrEnv {
    Name(Vec<u8>),
    Value(Expr),
}

#[derive(Clone, Debug)]
pub enum NumForm {
    Sci,
    Eng,
    Value(Expr),
}

#[derive(Clone, Debug)]
pub enum TraceArg {
    Lit(Vec<u8>),
    Value(Expr),
}

#[derive(Clone, Debug)]
pub enum DropItem {
    Var(VarRef),
    Indirect(VarRef),
}

#[derive(Clone, Debug)]
pub struct Clause {
    pub instr: Instr,
    pub line: u32,
    pub text: Vec<u8>,
    /// Static nesting level (for trace indentation).
    pub level: u32,
    /// DO: index of its END; END: its DO or SELECT; IF: its ELSE (or 0);
    /// ELSE: the end of its instruction; WHEN: the next WHEN/OTHERWISE/END;
    /// THEN (of a WHEN) and OTHERWISE: the SELECT's END.
    pub link: usize,
    /// The index after this whole instruction.
    pub next: usize,
}

pub struct Program {
    pub clauses: Vec<Clause>,
    pub labels: BTreeMap<Name, usize>,
    pub lines: Vec<Vec<u8>>,
}

/// Parse error for the whole program (structure), with its line.
pub type LoadErr = (Err, u32);

const KEYWORDS: &[&[u8]] = &[
    b"ADDRESS", b"ARG", b"CALL", b"DO", b"DROP", b"ELSE", b"END", b"EXIT", b"IF", b"INTERPRET", b"ITERATE", b"LEAVE", b"NOP",
    b"NUMERIC", b"OPTIONS", b"OTHERWISE", b"PARSE", b"PROCEDURE", b"PULL", b"PUSH", b"QUEUE", b"RETURN", b"SAY", b"SELECT",
    b"SIGNAL", b"THEN", b"TRACE", b"WHEN",
];

struct P<'a> {
    t: &'a [Token],
    pos: usize,
    kw: Vec<bool>,
    fparen: Vec<bool>,
}

type R<T> = Result<T, Err>;

fn var_ref(sym: &[u8]) -> R<VarRef> {
    let u = upper(sym);
    if u[0].is_ascii_digit() || u[0] == b'.' {
        return Err(Err::new(31));
    }
    match u.iter().position(|&c| c == b'.') {
        None => Ok(VarRef::Simple(u)),
        Some(p) if p == u.len() - 1 => Ok(VarRef::Stem(u)),
        Some(p) => {
            let stem = u[..=p].to_vec();
            let tails = u[p + 1..].split(|&c| c == b'.').map(tail_part).collect();
            Ok(VarRef::Comp(stem, tails))
        }
    }
}

fn tail_part(s: &[u8]) -> Tail {
    if s.is_empty() || s[0].is_ascii_digit() {
        Tail::Const(s.to_vec())
    } else {
        Tail::Var(s.to_vec())
    }
}

/// A symbol in an expression: constant or variable.
fn sym_expr(sym: &[u8]) -> Expr {
    let u = upper(sym);
    if u[0].is_ascii_digit() || u[0] == b'.' {
        Expr::Lit(u)
    } else {
        Expr::Var(var_ref(sym).unwrap())
    }
}

impl<'a> P<'a> {
    fn new(t: &'a [Token]) -> P<'a> {
        P { t, pos: 0, kw: alloc::vec![false; t.len()], fparen: alloc::vec![false; t.len()] }
    }
    fn peek(&self) -> Option<&'a Token> {
        self.t.get(self.pos)
    }
    fn at_end(&self) -> bool {
        self.pos >= self.t.len()
    }
    fn peek_kw(&self, k: &[u8]) -> bool {
        self.peek().is_some_and(|t| t.is_kw(k))
    }
    fn take_kw(&mut self, k: &[u8]) -> bool {
        if self.peek_kw(k) {
            self.kw[self.pos] = true;
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_end(&self) -> R<()> {
        if self.at_end() { Ok(()) } else { Err(Err::new(21)) }
    }

    // ---- expressions ----

    fn expr(&mut self, stop: &[&[u8]]) -> R<Option<Expr>> {
        if self.at_end() {
            return Ok(None);
        }
        if let Some(t) = self.peek() {
            if stop.iter().any(|k| t.is_kw(k)) {
                return Ok(None);
            }
            if t.tok == Tok::Comma || t.tok == Tok::RParen {
                return Ok(None);
            }
        }
        let e = self.or_expr(stop)?;
        Ok(Some(e))
    }

    fn need_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        match self.expr(stop)? {
            Some(e) => Ok(e),
            None => Err(Err::new(35)),
        }
    }

    fn at_stop(&self, stop: &[&[u8]]) -> bool {
        match self.peek() {
            None => true,
            Some(t) => match &t.tok {
                Tok::Sym(_) => stop.iter().any(|k| t.is_kw(k)),
                Tok::Comma | Tok::RParen => true,
                _ => false,
            },
        }
    }

    fn or_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.and_expr(stop)?;
        loop {
            let op = match self.peek() {
                Some(t) if t.is_op(b"|") => BinOp::Or,
                Some(t) if t.is_op(b"&&") => BinOp::Xor,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.and_expr(stop)?;
            l = Expr::Bin(op, l.into(), r.into());
        }
    }

    fn and_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.cmp_expr(stop)?;
        while self.peek().is_some_and(|t| t.is_op(b"&")) {
            self.pos += 1;
            let r = self.cmp_expr(stop)?;
            l = Expr::Bin(BinOp::And, l.into(), r.into());
        }
        Ok(l)
    }

    fn cmp_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.cat_expr(stop)?;
        loop {
            let op = match self.peek().map(|t| &t.tok) {
                Some(Tok::Op(o)) => match *o {
                    b"=" => BinOp::Eq,
                    b"\\=" | b"<>" | b"><" => BinOp::Ne,
                    b">" => BinOp::Gt,
                    b"<" => BinOp::Lt,
                    b">=" | b"\\<" => BinOp::Ge,
                    b"<=" | b"\\>" => BinOp::Le,
                    b"==" => BinOp::SEq,
                    b"\\==" => BinOp::SNe,
                    b">>" => BinOp::SGt,
                    b"<<" => BinOp::SLt,
                    b">>=" | b"\\<<" => BinOp::SGe,
                    b"<<=" | b"\\>>" => BinOp::SLe,
                    _ => return Ok(l),
                },
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.cat_expr(stop)?;
            l = Expr::Bin(op, l.into(), r.into());
        }
    }

    fn cat_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.add_expr(stop)?;
        loop {
            let Some(t) = self.peek() else { return Ok(l) };
            if t.is_op(b"||") {
                self.pos += 1;
                let r = self.add_expr(stop)?;
                l = Expr::Bin(BinOp::Concat, l.into(), r.into());
            } else if t.is_term_start() && !self.at_stop(stop) {
                // blank or abuttal concatenation
                let op = if t.blank { BinOp::Blank } else { BinOp::Concat };
                let r = self.add_expr(stop)?;
                l = Expr::Bin(op, l.into(), r.into());
            } else if t.is_op(b"\\") && !self.at_stop(stop) {
                // `a \b` is a blank concatenation of a prefix expression
                let op = if t.blank { BinOp::Blank } else { BinOp::Concat };
                let r = self.add_expr(stop)?;
                l = Expr::Bin(op, l.into(), r.into());
            } else {
                return Ok(l);
            }
        }
    }

    fn add_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.mul_expr(stop)?;
        loop {
            let op = match self.peek() {
                Some(t) if t.is_op(b"+") => BinOp::Add,
                Some(t) if t.is_op(b"-") => BinOp::Sub,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.mul_expr(stop)?;
            l = Expr::Bin(op, l.into(), r.into());
        }
    }

    fn mul_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.pow_expr(stop)?;
        loop {
            let op = match self.peek().map(|t| &t.tok) {
                Some(Tok::Op(b"*")) => BinOp::Mul,
                Some(Tok::Op(b"/")) => BinOp::Div,
                Some(Tok::Op(b"%")) => BinOp::IDiv,
                Some(Tok::Op(b"//")) => BinOp::Rem,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.pow_expr(stop)?;
            l = Expr::Bin(op, l.into(), r.into());
        }
    }

    fn pow_expr(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let mut l = self.prefix(stop)?;
        while self.peek().is_some_and(|t| t.is_op(b"**")) {
            self.pos += 1;
            let r = self.prefix(stop)?;
            l = Expr::Bin(BinOp::Pow, l.into(), r.into());
        }
        Ok(l)
    }

    fn prefix(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let op = match self.peek().map(|t| &t.tok) {
            Some(Tok::Op(b"\\")) => Some(UnOp::Not),
            Some(Tok::Op(b"-")) => Some(UnOp::Neg),
            Some(Tok::Op(b"+")) => Some(UnOp::Plus),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let e = self.prefix(stop)?;
            return Ok(Expr::Un(op, e.into()));
        }
        self.term(stop)
    }

    fn term(&mut self, stop: &[&[u8]]) -> R<Expr> {
        let Some(t) = self.peek() else { return Err(Err::new(35)) };
        match &t.tok {
            Tok::LParen => {
                self.pos += 1;
                let e = self.or_expr(&[])?;
                match self.peek() {
                    Some(t) if t.tok == Tok::RParen => {
                        self.pos += 1;
                        Ok(e)
                    }
                    Some(t) if t.tok == Tok::Comma => Err(Err::new(37)),
                    _ => Err(Err::new(36)),
                }
            }
            Tok::Str { val, .. } => {
                let v = val.clone();
                self.pos += 1;
                if self.peek().is_some_and(|n| n.tok == Tok::LParen && !n.blank) {
                    return self.call(v, true);
                }
                Ok(Expr::Lit(v))
            }
            Tok::Sym(s) => {
                let s = s.clone();
                self.pos += 1;
                if self.peek().is_some_and(|n| n.tok == Tok::LParen && !n.blank) {
                    return self.call(upper(&s), false);
                }
                let _ = stop;
                Ok(sym_expr(&s))
            }
            Tok::RParen | Tok::Comma => Err(Err::new(37)),
            _ => Err(Err::new(35)),
        }
    }

    fn call(&mut self, name: Vec<u8>, quoted: bool) -> R<Expr> {
        self.fparen[self.pos] = true;
        self.pos += 1;
        let mut args = Vec::new();
        if self.peek().is_some_and(|t| t.tok == Tok::RParen) {
            self.fparen[self.pos] = true;
            self.pos += 1;
            return Ok(Expr::Call(name, quoted, args));
        }
        loop {
            let a = if self.peek().is_some_and(|t| t.tok == Tok::Comma || t.tok == Tok::RParen) {
                None
            } else {
                Some(self.or_expr(&[])?)
            };
            args.push(a);
            match self.peek() {
                Some(t) if t.tok == Tok::Comma => {
                    self.pos += 1;
                }
                Some(t) if t.tok == Tok::RParen => {
                    self.fparen[self.pos] = true;
                    self.pos += 1;
                    return Ok(Expr::Call(name, quoted, args));
                }
                _ => return Err(Err::new(36)),
            }
        }
    }

    /// Comma-separated argument expressions (CALL).
    fn arg_list(&mut self) -> R<Vec<Option<Expr>>> {
        let mut args = Vec::new();
        if self.at_end() {
            return Ok(args);
        }
        loop {
            let a = if self.peek().is_some_and(|t| t.tok == Tok::Comma) { None } else { Some(self.or_expr(&[])?) };
            args.push(a);
            match self.peek() {
                None => return Ok(args),
                Some(t) if t.tok == Tok::Comma => {
                    self.pos += 1;
                    if self.at_end() {
                        args.push(None);
                        return Ok(args);
                    }
                }
                Some(t) if t.tok == Tok::RParen => return Err(Err::new(37)),
                _ => return Err(Err::new(35)),
            }
        }
    }

    // ---- templates ----

    fn templates(&mut self) -> R<Vec<Template>> {
        let mut out = Vec::new();
        let mut cur = Vec::new();
        while let Some(t) = self.peek() {
            match &t.tok {
                Tok::Comma => {
                    self.pos += 1;
                    out.push(core::mem::take(&mut cur));
                }
                Tok::Sym(s) => {
                    self.pos += 1;
                    if s.as_slice() == b"." {
                        cur.push(TplItem::Target(None));
                    } else if s[0].is_ascii_digit() {
                        cur.push(TplItem::Pos(PosKind::Abs, upper(s)));
                    } else if s[0] == b'.' {
                        return Err(Err::new(31));
                    } else {
                        cur.push(TplItem::Target(Some(var_ref(s)?)));
                    }
                }
                Tok::Str { val, .. } => {
                    cur.push(TplItem::Lit(val.clone()));
                    self.pos += 1;
                }
                Tok::LParen => {
                    self.pos += 1;
                    let v = self.paren_var()?;
                    cur.push(TplItem::VarPat(v));
                }
                Tok::Op(o) if matches!(*o, b"+" | b"-" | b"=") => {
                    let k = match *o {
                        b"+" => PosKind::Plus,
                        b"-" => PosKind::Minus,
                        _ => PosKind::Abs,
                    };
                    self.pos += 1;
                    match self.peek().map(|t| &t.tok) {
                        Some(Tok::Sym(s)) if s[0].is_ascii_digit() => {
                            cur.push(TplItem::Pos(k, upper(s)));
                            self.pos += 1;
                        }
                        Some(Tok::LParen) => {
                            self.pos += 1;
                            let v = self.paren_var()?;
                            cur.push(TplItem::VarPos(k, v));
                        }
                        _ => return Err(Err::new(38)),
                    }
                }
                _ => return Err(Err::new(38)),
            }
        }
        out.push(cur);
        Ok(out)
    }

    fn paren_var(&mut self) -> R<VarRef> {
        let v = match self.peek().map(|t| &t.tok) {
            Some(Tok::Sym(s)) if !(s[0].is_ascii_digit() || s[0] == b'.') => var_ref(s)?,
            _ => return Err(Err::new(19)),
        };
        self.pos += 1;
        if !self.peek().is_some_and(|t| t.tok == Tok::RParen) {
            return Err(Err::new(38));
        }
        self.pos += 1;
        Ok(v)
    }

    fn name_list(&mut self) -> R<Vec<DropItem>> {
        let mut v = Vec::new();
        while let Some(t) = self.peek() {
            match &t.tok {
                Tok::Sym(s) => {
                    v.push(DropItem::Var(var_ref(s)?));
                    self.pos += 1;
                }
                Tok::LParen => {
                    self.pos += 1;
                    v.push(DropItem::Indirect(self.paren_var()?));
                }
                _ => return Err(Err::new(20)),
            }
        }
        Ok(v)
    }

    fn symbol_taken(&mut self) -> R<Vec<u8>> {
        match self.peek().map(|t| &t.tok) {
            Some(Tok::Sym(s)) => {
                let u = upper(s);
                self.pos += 1;
                Ok(u)
            }
            _ => Err(Err::new(20)),
        }
    }

    fn cond(&mut self, call: bool) -> R<Cond> {
        let u = self.peek().and_then(|t| t.upper()).ok_or(Err::new(25))?;
        let c = Cond::from(&u).ok_or(Err::new(25))?;
        if call && matches!(c, Cond::NoValue | Cond::Syntax) {
            return Err(Err::new(25));
        }
        self.kw[self.pos] = true;
        self.pos += 1;
        Ok(c)
    }
}

/// One parsed piece of a raw clause: an instruction and its token range.
struct Piece {
    instr: Instr,
    from: usize,
    to: usize,
}

fn is_assignment(t: &[Token]) -> bool {
    t.len() >= 2 && t[0].is_sym() && t[1].is_op(b"=")
}

fn is_label(t: &[Token]) -> bool {
    t.len() >= 2 && matches!(t[0].tok, Tok::Sym(_) | Tok::Str { .. }) && t[1].tok == Tok::Colon
}

/// Parse a raw clause into one or more instructions.
fn parse_clause(p: &mut P, out: &mut Vec<Piece>) -> R<()> {
    let start = p.pos;
    let rest = &p.t[p.pos..];
    if rest.is_empty() {
        return Ok(());
    }
    if is_label(rest) {
        let name = match &rest[0].tok {
            Tok::Sym(s) => upper(s),
            Tok::Str { val, .. } => val.clone(),
            _ => unreachable!(),
        };
        p.pos += 2;
        out.push(Piece { instr: Instr::Label(name), from: start, to: p.pos });
        return parse_clause(p, out);
    }
    if is_assignment(rest) {
        let s = rest[0].sym().unwrap();
        let v = var_ref(s).map_err(|_| {
            if s[0].is_ascii_digit() || s[0] == b'.' { Err::new(31) } else { Err::new(31) }
        })?;
        p.pos += 2;
        let e = p.expr(&[])?.unwrap_or(Expr::Lit(Vec::new()));
        if !p.at_end() {
            return Err(Err::new(if p.peek().is_some_and(|t| t.tok == Tok::RParen || t.tok == Tok::Comma) {
                37
            } else {
                35
            }));
        }
        out.push(Piece { instr: Instr::Assign(v, e), from: start, to: p.pos });
        return Ok(());
    }
    let kw = rest[0].upper().filter(|u| KEYWORDS.contains(&u.as_slice()));
    let Some(kw) = kw else {
        // command
        let e = p.need_expr(&[])?;
        if !p.at_end() {
            return Err(Err::new(if p.peek().is_some_and(|t| t.tok == Tok::RParen || t.tok == Tok::Comma) {
                37
            } else {
                35
            }));
        }
        out.push(Piece { instr: Instr::Command(e), from: start, to: p.pos });
        return Ok(());
    };
    p.kw[p.pos] = true;
    p.pos += 1;
    let instr = match kw.as_slice() {
        b"NOP" => {
            p.expect_end()?;
            Instr::Nop
        }
        b"SAY" => {
            let e = p.expr(&[])?;
            p.expect_end().map_err(|_| Err::new(37))?;
            Instr::Say(e)
        }
        b"IF" => {
            let e = p.expr(&[b"THEN"])?.ok_or(Err::new(35))?;
            let to = p.pos;
            out.push(Piece { instr: Instr::If(e), from: start, to });
            if p.at_end() {
                return Ok(());
            }
            if !p.peek_kw(b"THEN") {
                return Err(Err::new(35));
            }
            return parse_clause(p, out);
        }
        b"WHEN" => {
            let e = p.expr(&[b"THEN"])?.ok_or(Err::new(35))?;
            let to = p.pos;
            out.push(Piece { instr: Instr::When(e), from: start, to });
            if p.at_end() {
                return Ok(());
            }
            if !p.peek_kw(b"THEN") {
                return Err(Err::new(35));
            }
            return parse_clause(p, out);
        }
        b"THEN" | b"ELSE" | b"OTHERWISE" => {
            let i = match kw.as_slice() {
                b"THEN" => Instr::Then,
                b"ELSE" => Instr::Else,
                _ => Instr::Otherwise,
            };
            out.push(Piece { instr: i, from: start, to: p.pos });
            return parse_clause(p, out);
        }
        b"SELECT" => {
            p.expect_end()?;
            Instr::Select
        }
        b"END" => {
            let n = if p.at_end() { None } else { Some(p.symbol_taken()?) };
            p.expect_end()?;
            Instr::End(n)
        }
        b"ITERATE" | b"LEAVE" => {
            let n = if p.at_end() { None } else { Some(p.symbol_taken()?) };
            p.expect_end()?;
            if kw.as_slice() == b"ITERATE" { Instr::Iterate(n) } else { Instr::Leave(n) }
        }
        b"EXIT" | b"RETURN" => {
            let e = p.expr(&[])?;
            p.expect_end().map_err(|_| Err::new(37))?;
            if kw.as_slice() == b"EXIT" { Instr::Exit(e) } else { Instr::Return(e) }
        }
        b"INTERPRET" => {
            let e = p.need_expr(&[])?;
            p.expect_end().map_err(|_| Err::new(37))?;
            Instr::Interpret(e)
        }
        b"PUSH" | b"QUEUE" => {
            let e = p.expr(&[])?;
            p.expect_end().map_err(|_| Err::new(37))?;
            if kw.as_slice() == b"PUSH" { Instr::Push(e) } else { Instr::Queue(e) }
        }
        b"OPTIONS" => {
            let e = p.need_expr(&[])?;
            p.expect_end()?;
            Instr::Options(e)
        }
        b"DROP" => {
            let l = p.name_list()?;
            if l.is_empty() {
                return Err(Err::new(20));
            }
            Instr::Drop(l)
        }
        b"PROCEDURE" => {
            let l = if p.take_kw(b"EXPOSE") { p.name_list()? } else { Vec::new() };
            p.expect_end()?;
            Instr::Procedure(l)
        }
        b"ARG" => Instr::Parse(true, ParseSrc::Arg, p.templates()?),
        b"PULL" => Instr::Parse(true, ParseSrc::Pull, p.templates()?),
        b"PARSE" => {
            let up = p.take_kw(b"UPPER");
            let src = if p.take_kw(b"ARG") {
                ParseSrc::Arg
            } else if p.take_kw(b"LINEIN") {
                ParseSrc::Linein
            } else if p.take_kw(b"PULL") {
                ParseSrc::Pull
            } else if p.take_kw(b"SOURCE") {
                ParseSrc::Source
            } else if p.take_kw(b"VERSION") {
                ParseSrc::Version
            } else if p.take_kw(b"VAR") {
                match p.peek().map(|t| &t.tok) {
                    Some(Tok::Sym(s)) => {
                        let v = var_ref(s)?;
                        p.pos += 1;
                        ParseSrc::Var(v)
                    }
                    _ => return Err(Err::new(20)),
                }
            } else if p.take_kw(b"VALUE") {
                let e = p.expr(&[b"WITH"])?;
                if !p.take_kw(b"WITH") {
                    return Err(Err::new(38));
                }
                ParseSrc::Value(e)
            } else {
                return Err(Err::new(25));
            };
            Instr::Parse(up, src, p.templates()?)
        }
        b"NUMERIC" => {
            if p.take_kw(b"DIGITS") {
                let e = p.expr(&[])?;
                p.expect_end()?;
                Instr::NumDigits(e)
            } else if p.take_kw(b"FUZZ") {
                let e = p.expr(&[])?;
                p.expect_end()?;
                Instr::NumFuzz(e)
            } else if p.take_kw(b"FORM") {
                let f = if p.at_end() || p.take_kw(b"SCIENTIFIC") {
                    NumForm::Sci
                } else if p.take_kw(b"ENGINEERING") {
                    NumForm::Eng
                } else {
                    p.take_kw(b"VALUE");
                    NumForm::Value(p.need_expr(&[])?)
                };
                p.expect_end()?;
                Instr::NumForm(f)
            } else {
                return Err(Err::new(25));
            }
        }
        b"ADDRESS" => {
            if p.at_end() {
                Instr::Address(None)
            } else if p.take_kw(b"VALUE") {
                let e = p.need_expr(&[])?;
                p.expect_end()?;
                Instr::Address(Some((AddrEnv::Value(e), None)))
            } else {
                match p.peek().map(|t| &t.tok) {
                    Some(Tok::Sym(s)) => {
                        let env = upper(s);
                        p.pos += 1;
                        let cmd = p.expr(&[])?;
                        p.expect_end()?;
                        Instr::Address(Some((AddrEnv::Name(env), cmd)))
                    }
                    Some(Tok::Str { val, .. }) => {
                        let env = val.clone();
                        p.pos += 1;
                        let cmd = p.expr(&[])?;
                        p.expect_end()?;
                        Instr::Address(Some((AddrEnv::Name(env), cmd)))
                    }
                    _ => {
                        let e = p.need_expr(&[])?;
                        p.expect_end()?;
                        Instr::Address(Some((AddrEnv::Value(e), None)))
                    }
                }
            }
        }
        b"CALL" => {
            if p.peek_kw(b"ON") || p.peek_kw(b"OFF") {
                let on = p.take_kw(b"ON");
                if !on {
                    p.take_kw(b"OFF");
                }
                let c = p.cond(true)?;
                let name = if on && p.take_kw(b"NAME") { Some(p.symbol_or_string()?) } else { None };
                p.expect_end()?;
                Instr::CallOn(c, on, name)
            } else {
                let (name, quoted) = match p.peek().map(|t| &t.tok) {
                    Some(Tok::Sym(s)) => (upper(s), false),
                    Some(Tok::Str { val, .. }) => (val.clone(), true),
                    _ => return Err(Err::new(19)),
                };
                p.pos += 1;
                let args = p.arg_list()?;
                Instr::Call(name, quoted, args)
            }
        }
        b"SIGNAL" => {
            if p.peek_kw(b"ON") || p.peek_kw(b"OFF") {
                let on = p.take_kw(b"ON");
                if !on {
                    p.take_kw(b"OFF");
                }
                let c = p.cond(false)?;
                let name = if on && p.take_kw(b"NAME") { Some(p.symbol_or_string()?) } else { None };
                p.expect_end()?;
                Instr::SignalOn(c, on, name)
            } else if p.take_kw(b"VALUE") {
                let e = p.need_expr(&[])?;
                p.expect_end()?;
                Instr::SignalValue(e)
            } else {
                match p.peek().map(|t| &t.tok) {
                    Some(Tok::Sym(_)) | Some(Tok::Str { .. }) => {
                        let n = p.symbol_or_string()?;
                        p.expect_end()?;
                        Instr::Signal(n)
                    }
                    _ => {
                        let e = p.need_expr(&[])?;
                        p.expect_end()?;
                        Instr::SignalValue(e)
                    }
                }
            }
        }
        b"TRACE" => {
            if p.at_end() {
                Instr::Trace(TraceArg::Lit(b"N".to_vec()))
            } else if p.take_kw(b"VALUE") {
                let e = p.need_expr(&[])?;
                p.expect_end()?;
                Instr::Trace(TraceArg::Value(e))
            } else {
                match p.peek().map(|t| &t.tok) {
                    Some(Tok::Sym(s)) => {
                        let v = upper(s);
                        p.pos += 1;
                        p.expect_end()?;
                        Instr::Trace(TraceArg::Lit(v))
                    }
                    Some(Tok::Str { val, .. }) => {
                        let v = val.clone();
                        p.pos += 1;
                        p.expect_end()?;
                        Instr::Trace(TraceArg::Lit(v))
                    }
                    Some(Tok::Op(b"-")) if p.t.get(p.pos + 1).is_some_and(|t| t.is_sym()) => {
                        let mut v = b"-".to_vec();
                        v.extend(upper(p.t[p.pos + 1].sym().unwrap()));
                        p.pos += 2;
                        p.expect_end()?;
                        Instr::Trace(TraceArg::Lit(v))
                    }
                    _ => {
                        let e = p.need_expr(&[])?;
                        p.expect_end()?;
                        Instr::Trace(TraceArg::Value(e))
                    }
                }
            }
        }
        b"DO" => {
            let spec = parse_do(p)?;
            Instr::Do(spec.into())
        }
        _ => return Err(Err::new(19)),
    };
    out.push(Piece { instr, from: start, to: p.pos });
    Ok(())
}

impl P<'_> {
    fn symbol_or_string(&mut self) -> R<Vec<u8>> {
        match self.peek().map(|t| &t.tok) {
            Some(Tok::Sym(s)) => {
                let u = upper(s);
                self.pos += 1;
                Ok(u)
            }
            Some(Tok::Str { val, .. }) => {
                let v = val.clone();
                self.pos += 1;
                Ok(v)
            }
            _ => Err(Err::new(19)),
        }
    }
}

fn parse_do(p: &mut P) -> R<DoSpec> {
    let mut d = DoSpec { var: None, count: None, forever: false, while_: None, until: None, name: None };
    const STOPS: &[&[u8]] = &[b"TO", b"BY", b"FOR", b"WHILE", b"UNTIL"];
    if p.at_end() {
        return Ok(d);
    }
    let t = &p.t[p.pos..];
    if t.len() >= 2 && t[0].is_sym() && t[1].is_op(b"=") {
        let v = var_ref(t[0].sym().unwrap())?;
        if let VarRef::Simple(n) = &v {
            d.name = Some(n.clone());
        } else if let VarRef::Comp(..) = &v {
            d.name = Some(upper(t[0].sym().unwrap()));
        }
        p.pos += 2;
        let init = p.need_expr(STOPS)?;
        let mut parts = Vec::new();
        let (mut to, mut by, mut fr) = (false, false, false);
        loop {
            if p.take_kw(b"TO") {
                if to {
                    return Err(Err::new(27));
                }
                to = true;
                parts.push(DoPart::To(p.need_expr(STOPS)?));
            } else if p.take_kw(b"BY") {
                if by {
                    return Err(Err::new(27));
                }
                by = true;
                parts.push(DoPart::By(p.need_expr(STOPS)?));
            } else if p.take_kw(b"FOR") {
                if fr {
                    return Err(Err::new(27));
                }
                fr = true;
                parts.push(DoPart::For(p.need_expr(STOPS)?));
            } else {
                break;
            }
        }
        d.var = Some((v, init, parts));
    } else if p.take_kw(b"FOREVER") {
        d.forever = true;
    } else if !(p.peek_kw(b"WHILE") || p.peek_kw(b"UNTIL")) {
        d.count = Some(p.need_expr(&[b"WHILE", b"UNTIL"])?);
    }
    if p.take_kw(b"WHILE") {
        d.while_ = Some(p.need_expr(&[b"UNTIL"])?);
    } else if p.take_kw(b"UNTIL") {
        d.until = Some(p.need_expr(&[b"WHILE"])?);
    }
    if !p.at_end() {
        return Err(Err::new(27));
    }
    Ok(d)
}

/// A compound symbol (a variable with a tail after its stem).
fn is_compound(s: &[u8]) -> bool {
    !(s[0].is_ascii_digit() || s[0] == b'.') && s.iter().position(|&c| c == b'.').is_some_and(|p| p + 1 < s.len())
}

/// Rebuild a clause's text for tracing.
fn render(p: &P, from: usize, to: usize, spell: &BTreeMap<Vec<u8>, Vec<u8>>, out: &mut Vec<u8>) {
    let mut prev: Option<usize> = None;
    for i in from..to {
        let t = &p.t[i];
        if let Some(pi) = prev {
            let pt = &p.t[pi];
            let no_space = match (&pt.tok, &t.tok) {
                (_, Tok::Comma) => true,
                (_, Tok::Colon) => true,
                (_, Tok::LParen) if p.fparen[i] => true,
                (Tok::LParen, _) if p.fparen[pi] => true,
                (_, Tok::RParen) if p.fparen[i] => true,
                (Tok::Str { .. } | Tok::Sym(_) | Tok::RParen, Tok::Str { .. } | Tok::Sym(_) | Tok::LParen) => !t.blank,
                _ => false,
            };
            if !no_space {
                out.push(b' ');
            }
        }
        match &t.tok {
            Tok::Sym(s) => {
                if p.kw[i] {
                    let mut first = true;
                    for &c in s {
                        out.push(if first { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() });
                        first = false;
                    }
                } else if !is_compound(s) {
                    let u = upper(s);
                    out.extend_from_slice(spell.get(&u).map(|v| v.as_slice()).unwrap_or(s));
                } else {
                    // a compound symbol: its stem and tail symbols as first spelt
                    let p = s.iter().position(|&c| c == b'.').unwrap();
                    let stem = &s[..=p];
                    out.extend_from_slice(spell.get(&upper(stem)).map(|v| v.as_slice()).unwrap_or(stem));
                    for (k, part) in s[p + 1..].split(|&c| c == b'.').enumerate() {
                        if k > 0 {
                            out.push(b'.');
                        }
                        if part.is_empty() || part[0].is_ascii_digit() {
                            out.extend_from_slice(part);
                        } else {
                            out.extend_from_slice(spell.get(&upper(part)).map(|v| v.as_slice()).unwrap_or(part));
                        }
                    }
                }
            }
            Tok::Str { raw, kind, .. } => {
                out.push(b'\'');
                out.extend_from_slice(raw);
                out.push(b'\'');
                if *kind != 0 {
                    out.push(kind.to_ascii_lowercase());
                }
            }
            Tok::Op(o) => out.extend_from_slice(o),
            Tok::LParen => out.push(b'('),
            Tok::RParen => out.push(b')'),
            Tok::Comma => out.push(b','),
            Tok::Colon => out.push(b':'),
        }
        prev = Some(i);
    }
}

/// Parse a whole program (or an INTERPRET string).
pub fn program(src: &[u8]) -> Result<Program, LoadErr> {
    let mut spell = BTreeMap::new();
    let raw = crate::lex::clauses(src, &mut spell)?;
    let mut clauses = Vec::new();
    for rc in &raw {
        clauses_of(rc, &spell, &mut clauses);
    }
    let lines = src.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    let mut prog = Program { clauses, labels: BTreeMap::new(), lines };
    link(&mut prog)?;
    for (i, c) in prog.clauses.iter().enumerate() {
        if let Instr::Label(n) = &c.instr {
            prog.labels.entry(n.clone()).or_insert(i);
        }
    }
    Ok(prog)
}

fn clauses_of(rc: &RawClause, spell: &BTreeMap<Vec<u8>, Vec<u8>>, out: &mut Vec<Clause>) {
    let line = rc.toks.first().map_or(rc.line, |t| t.line);
    let mut p = P::new(&rc.toks);
    let mut pieces = Vec::new();
    let r = match &rc.err {
        Some(e) => Err(e.clone()),
        None => parse_clause(&mut p, &mut pieces),
    };
    // the failing part of a clause starts where the last good piece ended
    let err_from = pieces.last().map_or(0, |pc: &Piece| pc.to);
    for pc in pieces {
        let mut text = Vec::new();
        render(&p, pc.from, pc.to, spell, &mut text);
        if !matches!(pc.instr, Instr::Label(_)) {
            text.push(b';');
        }
        let l = rc.toks.get(pc.from).map_or(line, |t| t.line);
        out.push(Clause { instr: pc.instr, line: l, text, level: 0, link: 0, next: 0 });
    }
    if let Err(e) = r {
        let mut text = Vec::new();
        render(&p, err_from, rc.toks.len(), spell, &mut text);
        text.push(b';');
        let l = rc.toks.get(err_from).map_or(line, |t| t.line);
        out.push(Clause { instr: Instr::Error(e), line: l, text, level: 0, link: 0, next: 0 });
    }
}

/// Link the structure: DO/END, IF/THEN/ELSE, SELECT/WHEN/OTHERWISE/END.
fn link(p: &mut Program) -> Result<(), LoadErr> {
    let n = p.clauses.len();
    let mut i = 0;
    while i < n {
        i = link_instr(p, i, 0)?;
    }
    Ok(())
}

fn err_at(p: &Program, code: u32, i: usize) -> LoadErr {
    let line = p.clauses.get(i).or(p.clauses.last()).map_or(1, |c| c.line);
    (Err::new(code), line)
}

/// Skip labels before an instruction (they belong to the flat list).
fn link_instr(p: &mut Program, i: usize, level: u32) -> Result<usize, LoadErr> {
    let n = p.clauses.len();
    if i >= n {
        return Err(err_at(p, 14, n.saturating_sub(1)));
    }
    p.clauses[i].level = level;
    let next = match &p.clauses[i].instr {
        Instr::Do(_) => {
            let mut j = i + 1;
            loop {
                if j >= n {
                    return Err(err_at(p, 14, i));
                }
                if let Instr::End(_) = p.clauses[j].instr {
                    break;
                }
                if matches!(p.clauses[j].instr, Instr::Then | Instr::Else | Instr::When(_) | Instr::Otherwise) {
                    return Err(err_at(p, if matches!(p.clauses[j].instr, Instr::Then | Instr::Else) { 8 } else { 9 }, j));
                }
                j = link_instr(p, j, level + 1)?;
            }
            // END name must match the control variable
            if let (Instr::End(Some(en)), Instr::Do(d)) = (&p.clauses[j].instr, &p.clauses[i].instr) {
                if d.name.as_ref() != Some(en) {
                    return Err(err_at(p, 10, j));
                }
            }
            p.clauses[j].level = level + 1;
            p.clauses[j].link = i;
            p.clauses[j].next = j + 1;
            p.clauses[i].link = j;
            j + 1
        }
        Instr::Select => {
            let mut j = i + 1;
            let mut whens = Vec::new();
            let mut other: Option<usize> = None;
            while j < n && matches!(p.clauses[j].instr, Instr::Label(_)) {
                p.clauses[j].level = level + 1;
                j += 1;
            }
            loop {
                if j >= n {
                    return Err(err_at(p, 14, i));
                }
                match &p.clauses[j].instr {
                    Instr::When(_) if other.is_none() => {
                        p.clauses[j].level = level + 1;
                        let w = j;
                        if w + 1 >= n || !matches!(p.clauses[w + 1].instr, Instr::Then) {
                            return Err(err_at(p, 18, w));
                        }
                        p.clauses[w + 1].level = level + 1;
                        let e = link_instr(p, w + 2, level + 2)?;
                        whens.push(w);
                        p.clauses[w].next = e;
                        j = e;
                    }
                    Instr::Otherwise if other.is_none() => {
                        p.clauses[j].level = level + 1;
                        other = Some(j);
                        j += 1;
                        while j < n && !matches!(p.clauses[j].instr, Instr::End(_)) {
                            j = link_instr(p, j, level + 2)?;
                        }
                    }
                    Instr::End(name) => {
                        if name.is_some() {
                            return Err(err_at(p, 10, j));
                        }
                        if whens.is_empty() {
                            return Err(err_at(p, 7, j));
                        }
                        break;
                    }
                    Instr::Label(_) => {
                        p.clauses[j].level = level + 1;
                        j += 1;
                    }
                    _ => return Err(err_at(p, 7, j)),
                }
            }
            let end = j;
            p.clauses[end].level = if other.is_some() { level + 2 } else { level + 1 };
            p.clauses[end].link = i;
            p.clauses[end].next = end + 1;
            // each WHEN links to the next branch; THENs and OTHERWISE to END
            for (k, &w) in whens.iter().enumerate() {
                p.clauses[w].link = whens.get(k + 1).copied().or(other).unwrap_or(end);
                p.clauses[w + 1].link = end;
            }
            if let Some(o) = other {
                p.clauses[o].link = end;
            }
            p.clauses[i].link = end;
            end + 1
        }
        Instr::If(_) => {
            if i + 1 >= n || !matches!(p.clauses[i + 1].instr, Instr::Then) {
                return Err(err_at(p, 18, i));
            }
            p.clauses[i + 1].level = level;
            let e = link_instr(p, i + 2, level + 1)?;
            if e < n && matches!(p.clauses[e].instr, Instr::Else) {
                p.clauses[e].level = level;
                let e2 = link_instr(p, e + 1, level + 1)?;
                p.clauses[e].link = e2;
                p.clauses[e].next = e2;
                p.clauses[i].link = e;
                e2
            } else {
                p.clauses[i].link = 0;
                e
            }
        }
        Instr::Label(_) => i + 1,
        Instr::Then | Instr::Else => return Err(err_at(p, 8, i)),
        Instr::When(_) | Instr::Otherwise => return Err(err_at(p, 9, i)),
        Instr::End(_) => return Err(err_at(p, 10, i)),
        _ => i + 1,
    };
    p.clauses[i].next = next;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<(u32, alloc::string::String)> {
        let p = program(src.as_bytes()).unwrap();
        p.clauses.iter().map(|c| (c.level, alloc::string::String::from_utf8_lossy(&c.text).into_owned())).collect()
    }

    #[test]
    fn render_like_ibm() {
        let t = texts("X=1+2   /* c */ ;  y   =   'a'\"b\"   'c'||x  ; z = \"it's\" 'x'  X  '41'x\nif x=3 then do; nop; end; else nop\n");
        assert_eq!(t[0].1, "X = 1 + 2;");
        assert_eq!(t[1].1, "y = 'a''b' 'c' || X;");
        assert_eq!(t[2].1, "z = 'it's' 'x' X '41'x;");
        assert_eq!(t[3].1, "If X = 3;");
        assert_eq!(t[4].1, "Then;");
        assert_eq!(t[5], (1, "Do;".into()));
        assert_eq!(t[6], (2, "Nop;".into()));
        assert_eq!(t[7], (2, "End;".into()));
        assert_eq!(t[8].1, "Else;");
        let t = texts("w = -x || \\0 'abc'||(1+2) ab.x.1 sub(3)\nparse   value  'a b c'  with  q r . 1 s +1\ncall   sub   1,  2\n");
        assert_eq!(t[0].1, "w = - x || \\ 0 'abc' || ( 1 + 2 ) ab.x.1 sub(3);");
        assert_eq!(t[1].1, "Parse Value 'a b c' With q r . 1 s + 1;");
        assert_eq!(t[2].1, "Call sub 1, 2;");
    }

    #[test]
    fn structure() {
        let p = program(b"select\nwhen a then nop\nwhen b then do; x=1; end\notherwise\n say 1\n say 2\nend\n").unwrap();
        assert_eq!(p.clauses[0].link, 12);
        assert!(program(b"do i = 1 to 3\nsay i\n").is_err());
        assert!(program(b"do i = 1\nend j\n").is_err());
    }
}
