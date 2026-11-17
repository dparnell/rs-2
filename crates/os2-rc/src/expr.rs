//! Constant expressions: C's operators and precedence, `NOT` as `~`, type
//! casts such as `(PSZ)` skipped.

use alloc::vec::Vec;

use crate::lex::{Lexer, Tok};
use crate::pp::Line;

#[derive(Debug, PartialEq, Eq)]
pub enum Err {
    /// Not an expression here (the token is left in place).
    Syntax,
    DivZero,
    /// An identifier that is not a number (statement expressions).
    Unknown(Vec<u8>),
}

pub struct Eval<'l, 'a> {
    pub lx: &'l mut Lexer<'a>,
    /// Identifiers count as 0 (#if) instead of being errors.
    pub ids_zero: bool,
    /// A cast was skipped.
    pub cast: bool,
    /// A number with an L suffix was read.
    pub long: bool,
}

type R = Result<i64, Err>;

/// Value of an #if expression (already macro-expanded). Err(true) on a
/// division by zero.
pub fn eval_pp(text: &[u8]) -> Result<i64, bool> {
    let lines = [Line { text: text.to_vec(), file: 0, line: 0 }];
    let mut lx = Lexer::new(&lines);
    let mut e = Eval { lx: &mut lx, ids_zero: true, cast: false, long: false };
    match e.expr() {
        Ok(v) if *e.lx.peek() == Tok::Eof => Ok(v),
        Err(Err::DivZero) => Err(true),
        _ => Err(false),
    }
}

impl Eval<'_, '_> {
    pub fn expr(&mut self) -> R {
        let c = self.binary(0)?;
        if self.lx.peek().is_op("?") {
            self.lx.next();
            let a = self.expr()?;
            if !self.lx.peek().is_op(":") {
                return Err(Err::Syntax);
            }
            self.lx.next();
            let b = self.expr()?;
            return Ok(if c != 0 { a } else { b });
        }
        Ok(c)
    }

    /// Binary operators from precedence level `min` up (C's order).
    pub fn binary(&mut self, min: u8) -> R {
        let mut l = self.unary()?;
        loop {
            let (op, prec) = match self.lx.peek() {
                Tok::Op(o) => match prec(o) {
                    Some(p) if p >= min => (*o, p),
                    _ => return Ok(l),
                },
                _ => return Ok(l),
            };
            self.lx.next();
            let r = self.binary(prec + 1)?;
            l = apply(op, l, r)?;
        }
    }

    pub fn unary(&mut self) -> R {
        let t = self.lx.peek().clone();
        match t {
            Tok::Op("-") => {
                self.lx.next();
                Ok(self.unary()?.wrapping_neg())
            }
            Tok::Op("+") => {
                self.lx.next();
                self.unary()
            }
            Tok::Op("~") => {
                self.lx.next();
                Ok(!self.unary()? & 0xffff_ffff)
            }
            Tok::Op("!") => {
                self.lx.next();
                Ok((self.unary()? == 0) as i64)
            }
            Tok::Id(ref s) if s.eq_ignore_ascii_case(b"NOT") => {
                self.lx.next();
                Ok(!self.unary()? & 0xffff_ffff)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> R {
        match self.lx.peek().clone() {
            Tok::Num(v, l) => {
                self.lx.next();
                self.long |= l;
                Ok(v as i64)
            }
            Tok::Op("(") => {
                self.lx.next();
                // (TYPE) or (unsigned long) and the like: a cast, skipped.
                if let Tok::Id(s) = self.lx.peek().clone() {
                    if !s.eq_ignore_ascii_case(b"NOT") && !self.ids_zero && self.try_cast()? {
                        self.cast = true;
                        return self.unary();
                    }
                }
                let v = self.expr()?;
                if !self.lx.peek().is_op(")") {
                    return Err(Err::Syntax);
                }
                self.lx.next();
                Ok(v)
            }
            Tok::Id(s) => {
                if self.ids_zero {
                    self.lx.next();
                    Ok(0)
                } else {
                    Err(Err::Unknown(s))
                }
            }
            _ => Err(Err::Syntax),
        }
    }

    /// After `(` with an identifier next: consumes `IDS... )`, a cast.
    /// Identifiers left after macro expansion are type names (`PSZ`,
    /// `unsigned long`, `CHAR *`).
    fn try_cast(&mut self) -> Result<bool, Err> {
        let mut n = 0;
        loop {
            match self.lx.peek().clone() {
                Tok::Id(_) | Tok::Op("*") => {
                    self.lx.next();
                    n += 1;
                }
                Tok::Op(")") if n > 0 => {
                    self.lx.next();
                    return Ok(true);
                }
                _ => return Err(Err::Syntax),
            }
        }
    }
}

fn prec(o: &str) -> Option<u8> {
    Some(match o {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" | "=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

fn apply(op: &str, l: i64, r: i64) -> R {
    Ok(match op {
        "||" => (l != 0 || r != 0) as i64,
        "&&" => (l != 0 && r != 0) as i64,
        "|" => l | r,
        "^" => l ^ r,
        "&" => l & r,
        "==" | "=" => (l == r) as i64,
        "!=" => (l != r) as i64,
        "<" => (l < r) as i64,
        ">" => (l > r) as i64,
        "<=" => (l <= r) as i64,
        ">=" => (l >= r) as i64,
        "<<" => l.wrapping_shl(r as u32),
        ">>" => l.wrapping_shr(r as u32),
        "+" => l.wrapping_add(r),
        "-" => l.wrapping_sub(r),
        "*" => l.wrapping_mul(r),
        "/" => {
            if r == 0 {
                return Err(Err::DivZero);
            }
            l.wrapping_div(r)
        }
        "%" => {
            if r == 0 {
                return Err(Err::DivZero);
            }
            l.wrapping_rem(r)
        }
        _ => return Err(Err::Syntax),
    })
}
