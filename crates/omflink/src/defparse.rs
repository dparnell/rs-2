//! The .DEF parser proper. Statements are free-form: line ends separate
//! nothing (NAME's application type may be on the next line, as in
//! Borland's THUNK example).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::def::*;
use crate::omf::ImpEntry;

impl Def {
    pub fn parse(text: &[u8]) -> Result<Def, DefError> {
        let text: String = text.iter().map(|&c| c as char).collect();
        let mut toks: Vec<(Tok, usize)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            for t in tokens(line) {
                toks.push((t, n + 1));
            }
        }
        let mut d = Def::default();
        let err = |line: usize, t: &str| DefError { line, text: t.to_string() };
        let up = |t: &Tok| word_of(t).map(|s| s.to_ascii_uppercase()).unwrap_or_default();
        let is_stmt = |t: &Tok| matches!(t, Tok::Word(w) if STATEMENTS.contains(&w.to_ascii_uppercase().as_str()));
        let mut i = 0;
        while i < toks.len() {
            let (t, ln) = (&toks[i].0, toks[i].1);
            if !is_stmt(t) {
                return Err(err(ln, "unknown statement"));
            }
            let stmt = up(t);
            i += 1;
            // The statement's operands: up to the next statement keyword.
            let mut j = i;
            while j < toks.len() && !is_stmt(&toks[j].0) {
                j += 1;
            }
            let ops = &toks[i..j];
            let opl = |k: usize| ops.get(k).map(|x| x.1).unwrap_or(ln);
            let first = || ops.first().and_then(|x| word_of(&x.0)).map(String::from);
            match stmt.as_str() {
                "NAME" | "LIBRARY" => {
                    d.kind = if stmt == "NAME" { Kind::Exe } else { Kind::Dll };
                    for (k, (t, l)) in ops.iter().enumerate() {
                        match up(t).as_str() {
                            "WINDOWAPI" => d.app = AppType::Pm,
                            "WINDOWCOMPAT" => d.app = AppType::Vio,
                            "NOTWINDOWCOMPAT" => d.app = AppType::NoVio,
                            "NEWFILES" | "LONGNAMES" => d.newfiles = true,
                            "INITINSTANCE" => d.init_instance = Some(true),
                            "INITGLOBAL" => d.init_instance = Some(false),
                            "TERMINSTANCE" => d.term_instance = Some(true),
                            "TERMGLOBAL" => d.term_instance = Some(false),
                            "PRIVATELIB" => d.privatelib = true,
                            _ if k == 0 => d.name = word_of(t).map(String::from),
                            _ => return Err(err(*l, "unknown keyword after NAME or LIBRARY")),
                        }
                    }
                }
                "PHYSICAL" | "VIRTUAL" => {
                    if ops.first().map(|x| up(&x.0)).as_deref() != Some("DEVICE") {
                        return Err(err(ln, "DEVICE expected"));
                    }
                    d.kind = if stmt == "PHYSICAL" { Kind::PhysicalDevice } else { Kind::VirtualDevice };
                    d.name = ops.get(1).and_then(|x| word_of(&x.0)).map(String::from);
                }
                "DESCRIPTION" => d.description = first(),
                "CODE" | "DATA" => {
                    for (t, l) in ops {
                        let a = if stmt == "CODE" { &mut d.code } else { &mut d.data };
                        if !attr(a, &up(t)) {
                            return Err(err(*l, "unknown segment attribute"));
                        }
                    }
                }
                "STACKSIZE" | "HEAPSIZE" => {
                    let w = first().unwrap_or_default();
                    if stmt == "HEAPSIZE" && w.eq_ignore_ascii_case("MAXVAL") {
                        d.heap_maxval = true;
                    } else {
                        let v = number(&w).ok_or_else(|| err(opl(0), "invalid number"))?;
                        if stmt == "STACKSIZE" { d.stacksize = Some(v) } else { d.heapsize = Some(v) }
                    }
                }
                "PROTMODE" => d.protmode = true,
                "REALMODE" => d.realmode = true,
                "STUB" => match ops.first() {
                    Some((Tok::Word(w), _)) if w.eq_ignore_ascii_case("NONE") => d.stub_none = true,
                    Some((t, _)) => d.stub = word_of(t).map(String::from),
                    None => return Err(err(ln, "file name expected")),
                },
                "OLD" => d.old = first(),
                "BASE" => {
                    let w = ops.iter().filter_map(|x| word_of(&x.0)).next().unwrap_or("");
                    d.base = Some(number(w).ok_or_else(|| err(opl(0), "invalid number"))?);
                }
                "EXETYPE" => d.exetype = first().map(|s| s.to_ascii_uppercase()),
                "APPLOADER" => {}
                "SEGMENTS" | "OBJECTS" => {
                    let mut k = 0;
                    while k < ops.len() {
                        let (t, l) = (&ops[k].0, ops[k].1);
                        let name = word_of(t).ok_or_else(|| err(l, "segment name expected"))?;
                        let mut sa = SegAttrs { name: String::from(name), class: None, attrs: Attrs::default() };
                        k += 1;
                        while k < ops.len() {
                            let u = up(&ops[k].0);
                            if u == "CLASS" {
                                sa.class = ops.get(k + 1).and_then(|x| word_of(&x.0)).map(String::from);
                                k += 2;
                                continue;
                            }
                            if matches!(ops[k].0, Tok::Quoted(_)) || !attr(&mut sa.attrs, &u) {
                                break;
                            }
                            k += 1;
                        }
                        d.segments.push(sa);
                    }
                }
                "EXPORTS" => {
                    let mut k = 0;
                    while k < ops.len() {
                        let (t, l) = (&ops[k].0, ops[k].1);
                        let name = word_of(t).ok_or_else(|| err(l, "export name expected"))?;
                        let mut e = Export { name: String::from(name), internal: String::from(name), ..Default::default() };
                        k += 1;
                        if ops.get(k).map(|x| &x.0) == Some(&Tok::Eq) {
                            e.internal = ops.get(k + 1).and_then(|x| word_of(&x.0)).ok_or_else(|| err(l, "internal name expected"))?.into();
                            k += 2;
                        }
                        while k < ops.len() {
                            match &ops[k].0 {
                                Tok::At => {
                                    let w = ops.get(k + 1).and_then(|x| word_of(&x.0)).unwrap_or("");
                                    let o = number(w).filter(|&o| o > 0 && o < 0x10000);
                                    e.ordinal = Some(o.ok_or_else(|| err(ops[k].1, "invalid ordinal"))? as u16);
                                    k += 2;
                                    continue;
                                }
                                Tok::Word(w) => match w.to_ascii_uppercase().as_str() {
                                    "RESIDENTNAME" => e.resident = true,
                                    "NONAME" => e.noname = true,
                                    "NODATA" => e.nodata = true,
                                    "PRIVATE" => e.private = true,
                                    _ => match number(w) {
                                        // The IOPL parameter word count.
                                        Some(v) if w.starts_with(|c: char| c.is_ascii_digit()) => e.pwords = v as u8,
                                        _ => break,
                                    },
                                },
                                _ => break,
                            }
                            k += 1;
                        }
                        d.exports.push(e);
                    }
                }
                "IMPORTS" => {
                    let mut k = 0;
                    while k < ops.len() {
                        let l = ops[k].1;
                        let (internal, spec) = if ops.get(k + 1).map(|x| &x.0) == Some(&Tok::Eq) {
                            let r = (ops.get(k).and_then(|x| word_of(&x.0)).map(String::from), ops.get(k + 2).and_then(|x| word_of(&x.0)));
                            k += 3;
                            r
                        } else {
                            let r = (None, ops.get(k).and_then(|x| word_of(&x.0)));
                            k += 1;
                            r
                        };
                        let spec = spec.ok_or_else(|| err(l, "import expected"))?;
                        let (module, entry) = spec.split_once('.').ok_or_else(|| err(l, "module.entry expected"))?;
                        let entry = match entry.chars().next() {
                            Some(c) if c.is_ascii_digit() => {
                                ImpEntry::Ordinal(number(entry).ok_or_else(|| err(l, "invalid ordinal"))? as u16)
                            }
                            _ => ImpEntry::Name(String::from(entry)),
                        };
                        let internal = match (&internal, &entry) {
                            (Some(i), _) => i.clone(),
                            (None, ImpEntry::Name(n)) => n.clone(),
                            (None, _) => return Err(err(l, "an import by ordinal needs an internal name")),
                        };
                        d.imports.push(Import { internal, module: String::from(module), entry });
                    }
                }
                _ => {}
            }
            i = j;
        }
        Ok(d)
    }
}
