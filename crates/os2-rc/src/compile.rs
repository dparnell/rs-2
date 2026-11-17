//! The resource statements: from the preprocessed script to resources in
//! IBM's binary formats.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::expr::{self, Eval};
use crate::lex::{Lexer, Tok};
use crate::pp::{Line, Pp};
use crate::res::{Name, Resource};
use crate::{flags, rt, Diag, Files, Options, Output};

// Window classes (WC_* low words).
const WC_FRAME: u16 = 1;
const WC_COMBOBOX: u16 = 2;
const WC_BUTTON: u16 = 3;
const WC_STATIC: u16 = 5;
const WC_ENTRYFIELD: u16 = 6;
const WC_LISTBOX: u16 = 7;
const WC_MLE: u16 = 10;
const WC_SPINBUTTON: u16 = 0x20;
const WC_CONTAINER: u16 = 0x25;
const WC_SLIDER: u16 = 0x26;
const WC_VALUESET: u16 = 0x27;
const WC_NOTEBOOK: u16 = 0x28;

const WS_VISIBLE: u32 = 0x8000_0000;
const WS_CLIPSIBLINGS: u32 = 0x1000_0000;
const WS_SAVEBITS: u32 = 0x0400_0000;
const WS_GROUP: u32 = 0x0001_0000;
const WS_TABSTOP: u32 = 0x0002_0000;
const FS_DLGBORDER: u32 = 0x80;
const FS_BORDER: u32 = 0x100;
const FCF_SIZEBORDER: u32 = 0x8;
const FCF_BORDER: u32 = 0x200;
const FS_SIZEBORDER: u32 = 0x800;

/// MOVEABLE PURE DISCARDABLE: the default of most resources.
const DEFAULT_FLAGS: u16 = flags::MOVEABLE | flags::PURE | flags::DISCARDABLE;

const MIS_TEXT: u16 = 0x0001;
const MIS_SEPARATOR: u16 = 0x0004;
const MIS_SUBMENU: u16 = 0x0010;
const MIS_OWNERDRAW: u16 = 0x0008;

/// Words that start statements or blocks (they end an optional field).
const KEYWORDS: &[&str] = &[
    "BEGIN", "END", "ICON", "POINTER", "BITMAP", "FONT", "DEFAULTICON", "DLGINCLUDE", "ACCELTABLE", "MENU", "STRINGTABLE",
    "MESSAGETABLE", "RCDATA", "RESOURCE", "DLGTEMPLATE", "WINDOWTEMPLATE", "HELPTABLE", "HELPSUBTABLE", "ASSOCTABLE",
    "CODEPAGE", "RCINCLUDE", "DIALOG", "FRAME", "WINDOW", "CONTROL", "PUSHBUTTON", "DEFPUSHBUTTON", "CHECKBOX",
    "AUTOCHECKBOX", "RADIOBUTTON", "AUTORADIOBUTTON", "LTEXT", "CTEXT", "RTEXT", "GROUPBOX", "ENTRYFIELD", "EDITTEXT",
    "COMBOBOX", "MLE", "LISTBOX", "SPINBUTTON", "VALUESET", "NOTEBOOK", "CONTAINER", "SLIDER", "PRESPARAMS", "CTLDATA",
    "MENUITEM", "SUBMENU", "HELPITEM", "HELPSUBITEM", "SUBITEMSIZE", "PRELOAD", "LOADONCALL", "FIXED", "MOVEABLE",
    "DISCARDABLE", "PURE", "IMPURE", "SEPARATOR",
];

/// A class: a WC_* ordinal or a name.
#[derive(Clone, Debug)]
enum Class {
    Ord(u16),
    Name(Vec<u8>),
}

/// Window text: a string or a resource ordinal (`0xFF id`).
#[derive(Clone, Debug)]
enum Text {
    Str(Vec<u8>),
    Ord(u16),
}

#[derive(Clone, Debug)]
struct Win {
    class: Class,
    text: Text,
    id: u16,
    x: u16,
    y: u16,
    cx: u16,
    cy: u16,
    style: u32,
    ctldata: Option<Vec<u8>>,
    pres: Vec<(u32, Vec<u8>)>,
    children: Vec<Win>,
}

struct MenuItem {
    style: u16,
    attr: u16,
    id: u16,
    text: Vec<u8>,
    sub: Option<Menu>,
}

struct Menu {
    pres: Vec<(u32, Vec<u8>)>,
    items: Vec<MenuItem>,
}

struct Str {
    /// Definition order.
    seq: usize,
    text: Vec<u8>,
    flags: u16,
    cp: u16,
}

struct C<'a, 'p, 'f, F: Files> {
    lx: Lexer<'a>,
    pp: &'p mut Pp<'f, F>,
    names: Vec<String>,
    diags: Vec<Diag>,
    res: Vec<Resource>,
    strings: BTreeMap<u16, Str>,
    messages: BTreeMap<u16, Str>,
    cp: u16,
    default_icon: Option<Vec<u8>>,
    assoc: Option<Vec<u8>>,
    /// A statement failed: skip to the next one.
    bad: bool,
    /// DISCARDABLE was among the last options.
    discard_given: bool,
}

/// Compiles the script `name` (read with `files`).
pub fn compile<F: Files>(files: &mut F, name: &str, opts: &Options) -> Output {
    let cp = if opts.codepage == 0 { 850 } else { opts.codepage };
    let Some(data) = files.read(name) else {
        return Output {
            resources: Vec::new(),
            diags: vec![Diag { num: 1002, file: String::new(), line: 0, arg: String::from(name) }],
            default_icon: None,
            assoc: None,
        };
    };
    let mut pp = Pp::new(files, opts.include.clone(), &opts.defines);
    pp.run(name, &data, true);
    let lines: Vec<Line> = core::mem::take(&mut pp.out);
    let mut diags = core::mem::take(&mut pp.diags);
    let names = pp.names.clone();
    // The lexer borrows the lines; the compiler borrows the preprocessor
    // (for resource file lookups).
    let out = {
        let mut c = C {
            lx: Lexer::new(&lines),
            pp: &mut pp,
            names,
            diags: Vec::new(),
            res: Vec::new(),
            strings: BTreeMap::new(),
            messages: BTreeMap::new(),
            cp,
            default_icon: None,
            assoc: None,
            bad: false,
            discard_given: false,
        };
        c.run();
        diags.append(&mut c.diags);
        Output { resources: c.res, diags: Vec::new(), default_icon: c.default_icon, assoc: c.assoc }
    };
    let mut out = out;
    out.diags = diags;
    out
}

fn le16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn le32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn lstr(s: &[u8]) -> String {
    s.iter().map(|&b| b as char).collect()
}

impl<F: Files> C<'_, '_, '_, F> {
    fn diag(&mut self, num: u16, arg: &str) {
        let (f, l) = self.lx.place();
        let file = self.names.get(f as usize).cloned().unwrap_or_default();
        self.diags.push(Diag { num, file, line: l, arg: String::from(arg) });
        if (2000..3000).contains(&num) || num < 2000 {
            self.bad = true;
        }
    }

    fn peek(&mut self) -> Tok {
        self.lx.peek().clone()
    }

    fn next(&mut self) -> Tok {
        self.lx.next()
    }

    fn kw(&mut self, k: &str) -> bool {
        if self.lx.peek().is_kw(k) {
            self.lx.next();
            true
        } else {
            false
        }
    }

    fn op(&mut self, o: &str) -> bool {
        if self.lx.peek().is_op(o) {
            self.lx.next();
            true
        } else {
            false
        }
    }

    fn begin(&mut self) -> bool {
        if self.kw("BEGIN") || self.op("{") {
            true
        } else {
            let t = self.peek().describe();
            self.diag(2003, &alloc::format!("BEGIN expected, found {t}"));
            false
        }
    }

    fn at_end(&mut self) -> bool {
        let t = self.peek();
        t.is_kw("END") || t.is_op("}") || t == Tok::Eof
    }

    fn end(&mut self) {
        if !(self.kw("END") || self.op("}")) {
            let t = self.peek().describe();
            self.diag(2003, &alloc::format!("END expected, found {t}"));
        }
    }

    /// Skips to the END of a block that went wrong.
    fn skip_block(&mut self) {
        let mut depth = 1;
        loop {
            let t = self.next();
            if t == Tok::Eof {
                return;
            }
            if t.is_kw("BEGIN") || t.is_op("{") {
                depth += 1;
            } else if t.is_kw("END") || t.is_op("}") {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
        }
    }

    /// A constant expression; `num`: the message if it is not one.
    fn value(&mut self, num: u16) -> Option<i64> {
        self.value_long(num).map(|v| v.0)
    }

    /// A constant expression and whether it had a long (L) number.
    fn value_long(&mut self, num: u16) -> Option<(i64, bool)> {
        let mut e = Eval { lx: &mut self.lx, ids_zero: false, cast: false, long: false };
        let r = e.expr();
        let long = e.long;
        match r {
            Ok(v) => Some((v, long)),
            Err(expr::Err::Unknown(s)) => {
                self.diag(num, &lstr(&s));
                None
            }
            Err(expr::Err::DivZero) => {
                self.diag(2059, "division by zero");
                None
            }
            Err(expr::Err::Syntax) => {
                let t = self.peek().describe();
                self.diag(num, &t);
                None
            }
        }
    }

    fn comma(&mut self) -> bool {
        self.op(",")
    }

    /// The next token can start a value (not a statement keyword, BEGIN,
    /// END or punctuation that ends a statement).
    fn starts_value(&mut self) -> bool {
        match self.peek() {
            Tok::Num(..) | Tok::Str(_) => true,
            Tok::Op(o) => matches!(o, "(" | "-" | "+" | "~" | "!"),
            Tok::Id(w) => !KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k.as_bytes())) || w.eq_ignore_ascii_case(b"NOT"),
            Tok::Eof => false,
        }
    }

    /// An optional field: after a comma, or (IBM's RC lets commas out)
    /// when a value follows.
    fn more(&mut self) -> bool {
        if self.comma() {
            return true;
        }
        self.starts_value() && !matches!(self.peek(), Tok::Str(_))
    }

    /// A style: terms ORed together, `NOT term` clearing bits of the
    /// default. Returns (bits set, bits cleared).
    fn style(&mut self, num: u16) -> Option<(u32, u32)> {
        let (mut set, mut clr) = (0u32, 0u32);
        loop {
            if self.kw("NOT") {
                let mut e = Eval { lx: &mut self.lx, ids_zero: false, cast: false, long: false };
                match e.binary(4) {
                    Ok(v) => clr |= v as u32,
                    Err(_) => {
                        let t = self.peek().describe();
                        self.diag(num, &t);
                        return None;
                    }
                }
            } else {
                let mut e = Eval { lx: &mut self.lx, ids_zero: false, cast: false, long: false };
                match e.binary(4) {
                    Ok(v) => set |= v as u32,
                    Err(expr::Err::Unknown(s)) => {
                        self.diag(num, &lstr(&s));
                        return None;
                    }
                    Err(_) => {
                        let t = self.peek().describe();
                        self.diag(num, &t);
                        return None;
                    }
                }
            }
            // A trailing | before the next statement is let pass; a value
            // right after another (no |) is ORed in too, as IBM's RC does.
            let bar = self.op("|");
            if !self.starts_value() || (!bar && matches!(self.peek(), Tok::Str(_) | Tok::Op(_))) {
                return Some((set, clr));
            }
        }
    }

    /// A string: one or more adjacent literals.
    fn string(&mut self) -> Option<Vec<u8>> {
        let mut s = self.raw_string()?;
        // A C string: it ends at an embedded NUL ("\0"). Carriage returns
        // are dropped (IBM's RC reads the script as text).
        if let Some(n) = s.iter().position(|&c| c == 0) {
            s.truncate(n);
        }
        s.retain(|&c| c != b'\r');
        Some(s)
    }

    /// Adjacent string literals with any NULs they hold.
    fn raw_string(&mut self) -> Option<Vec<u8>> {
        let Tok::Str(mut s) = self.peek() else { return None };
        self.next();
        while let Tok::Str(more) = self.peek() {
            self.next();
            s.extend_from_slice(&more);
        }
        Some(s)
    }

    /// A resource name: a number or a quoted string.
    fn res_name(&mut self) -> Option<Name> {
        if let Tok::Str(_) = self.peek() {
            // Names are kept in upper case.
            let s = self.string()?;
            return Some(Name::Str(s.to_ascii_uppercase()));
        }
        let v = self.value(2088)?;
        Some(Name::Id(v as u16))
    }

    /// Load and memory options and a code page after a resource name.
    fn options(&mut self, mut f: u16) -> (u16, u16) {
        let mut cp = self.cp;
        self.discard_given = false;
        loop {
            let t = self.peek();
            let Tok::Id(w) = &t else {
                return (f, cp);
            };
            let w = w.to_ascii_uppercase();
            match &w[..] {
                b"PRELOAD" => f |= flags::PRELOAD,
                b"LOADONCALL" => f &= !flags::PRELOAD,
                b"FIXED" => f &= !(flags::MOVEABLE | flags::DISCARDABLE),
                b"MOVEABLE" => f |= flags::MOVEABLE,
                b"DISCARDABLE" => {
                    f |= flags::DISCARDABLE | flags::MOVEABLE;
                    self.discard_given = true;
                }
                b"PURE" => f |= flags::PURE,
                b"IMPURE" => f &= !flags::PURE,
                _ => return (f, cp),
            }
            self.next();
            if let Tok::Num(n, _) = self.peek() {
                // A code page in the options.
                if n >= 100 && self.lx.same_line() {
                    self.next();
                    cp = n as u16;
                }
            }
        }
    }

    fn add(&mut self, ty: u16, name: Name, flags: u16, data: Vec<u8>) {
        if self.res.iter().any(|r| r.ty == ty && r.name == name) {
            // A name given twice is a warning (the first counts), as in
            // IBM's RC; a number twice is an error.
            if let Name::Str(_) = name {
                self.diag(3030, &alloc::format!("{ty}, ID {}", name.describe()));
            } else {
                self.diag(2034, &alloc::format!("{ty}, ID {}", name.describe()));
            }
            return;
        }
        self.res.push(Resource { ty, name, flags, data });
    }

    fn run(&mut self) {
        loop {
            self.bad = false;
            let t = self.next();
            let Tok::Id(w) = &t else {
                if t == Tok::Eof {
                    break;
                }
                self.diag(2003, &t.describe());
                self.recover();
                continue;
            };
            let w = w.to_ascii_uppercase();
            match &w[..] {
                b"ICON" => self.file_res(rt::POINTER),
                b"POINTER" => self.file_res(rt::POINTER),
                b"BITMAP" => self.file_res(rt::BITMAP),
                b"FONT" => self.font(),
                b"DEFAULTICON" => self.default_icon(),
                b"DLGINCLUDE" => self.dlginclude(),
                b"ACCELTABLE" => self.acceltable(),
                b"MENU" => self.menu_res(),
                b"STRINGTABLE" => self.stringtable(false),
                b"MESSAGETABLE" => self.stringtable(true),
                b"RCDATA" => self.rcdata(),
                b"RESOURCE" => self.resource(),
                b"DLGTEMPLATE" | b"WINDOWTEMPLATE" => self.template(),
                b"HELPTABLE" => self.helptable(),
                b"HELPSUBTABLE" => self.helpsubtable(),
                b"ASSOCTABLE" => self.assoctable(),
                b"CODEPAGE" => {
                    if let Some(v) = self.value(2091) {
                        self.cp = v as u16;
                    }
                }
                _ => {
                    self.diag(2003, &t.describe());
                    self.recover();
                }
            }
        }
        self.flush_strings();
    }

    /// After an error at the top level: skip to a statement keyword at
    /// the start of a line, or past a block.
    fn recover(&mut self) {
        loop {
            let t = self.peek();
            if t == Tok::Eof {
                return;
            }
            if t.is_kw("BEGIN") || t.is_op("{") {
                self.next();
                self.skip_block();
                return;
            }
            const TOP: [&str; 16] = [
                "ICON", "POINTER", "BITMAP", "FONT", "DEFAULTICON", "DLGINCLUDE", "ACCELTABLE", "MENU", "STRINGTABLE",
                "MESSAGETABLE", "RCDATA", "RESOURCE", "DLGTEMPLATE", "WINDOWTEMPLATE", "HELPTABLE", "HELPSUBTABLE",
            ];
            if !self.lx.same_line() && TOP.iter().any(|k| t.is_kw(k)) {
                return;
            }
            self.next();
        }
    }

    fn read_file(&mut self, name: &str) -> Option<Vec<u8>> {
        match self.pp.find_resource_file(name) {
            Some((_, d)) => Some(d),
            None => {
                self.diag(1002, name);
                None
            }
        }
    }

    /// ICON, POINTER, BITMAP: `KW id [options] file`.
    fn file_res(&mut self, ty: u16) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        let Some(file) = self.lx.filename() else {
            self.diag(1002, "");
            return;
        };
        if let Some(d) = self.read_file(&file) {
            // IBM's RC leaves bitmaps over 4K, and bitmaps with a quoted name
            // or file name, not discardable unless DISCARDABLE is given.
            let f = if ty == rt::BITMAP && (d.len() > 4096 || self.lx.quoted || matches!(name, Name::Str(_))) && !self.discard_given { f & !flags::DISCARDABLE } else { f };
            // Icon 1 is the program's default icon: a type 22 resource too
            // (and the .ICON extended attribute).
            let default = ty == rt::POINTER && name == Name::Id(1) && self.default_icon.is_none();
            self.add(ty, name, f, d.clone());
            if default {
                self.add(22, Name::Id(22), DEFAULT_FLAGS, d.clone());
                self.default_icon = Some(d);
            }
        }
    }

    fn default_icon(&mut self) {
        let Some(file) = self.lx.filename() else { return };
        if let Some(d) = self.read_file(&file) {
            if self.default_icon.is_none() {
                // Also a resource (type and ID 22) for the binder.
                self.add(22, Name::Id(22), DEFAULT_FLAGS, d.clone());
                self.default_icon = Some(d);
            } else {
                self.diag(3033, "");
            }
        }
    }

    /// FONT id [options] file: the font (RT_FONT) and a font directory
    /// entry (RT_FONTDIR, all fonts in one resource of ID 1).
    fn font(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        let Some(file) = self.lx.filename() else { return };
        if let Some(d) = self.read_file(&file) {
            self.add(rt::FONT, name, f, d);
        }
    }

    fn dlginclude(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let Some(file) = self.lx.filename() else {
            self.diag(2153, "");
            return;
        };
        let mut d = file.into_bytes();
        d.push(0);
        self.add(rt::DLGINCLUDE, name, DEFAULT_FLAGS, d);
    }

    // ---- ACCELTABLE ----

    fn acceltable(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, cp) = self.options(flags::MOVEABLE | flags::PURE);
        if !self.begin() {
            return self.recover();
        }
        let mut items: Vec<(u16, u16, u16)> = Vec::new();
        while !self.at_end() {
            match self.accel() {
                Some(a) => items.push(a),
                None => {
                    // Skip the rest of the line.
                    while self.lx.same_line() && !self.at_end() {
                        self.next();
                    }
                }
            }
        }
        self.end();
        if items.is_empty() {
            self.diag(2147, "");
            return;
        }
        let mut d = Vec::new();
        le16(&mut d, items.len() as u16);
        le16(&mut d, cp);
        for (fs, key, cmd) in items {
            le16(&mut d, fs);
            le16(&mut d, key);
            le16(&mut d, cmd);
        }
        self.add(rt::ACCELTABLE, name, f, d);
    }

    fn accel(&mut self) -> Option<(u16, u16, u16)> {
        let (key, is_str, ctl) = match self.peek() {
            Tok::Str(s) => {
                self.next();
                if s.first() == Some(&b'^') && s.len() == 2 {
                    let c = s[1].to_ascii_uppercase();
                    if !c.is_ascii_uppercase() {
                        self.diag(2140, "");
                        return None;
                    }
                    (c as u16, true, true)
                } else if s.len() == 1 {
                    (s[0] as u16, true, false)
                } else {
                    self.diag(if s.is_empty() { 2138 } else { 2139 }, &lstr(&s));
                    return None;
                }
            }
            _ => {
                let v = self.value(2142)?;
                (v as u16, false, false)
            }
        };
        if !self.comma() {
            { let t = self.peek().describe(); self.diag(2003, &t); }
            return None;
        }
        let cmd = self.value(2143)? as u16;
        let mut fs: u16 = 0;
        while self.comma() {
            let t = self.peek();
            let bit = if t.is_kw("VIRTUALKEY") {
                0x0002
            } else if t.is_kw("SCANCODE") {
                0x0004
            } else if t.is_kw("CHAR") {
                0x0001
            } else if t.is_kw("SHIFT") {
                0x0008
            } else if t.is_kw("CONTROL") {
                0x0010
            } else if t.is_kw("ALT") {
                0x0020
            } else if t.is_kw("LONEKEY") {
                0x0040
            } else if t.is_kw("SYSCOMMAND") {
                0x0100
            } else if t.is_kw("HELP") {
                0x0200
            } else {
                let v = self.value(2145)?;
                fs |= v as u16;
                continue;
            };
            self.next();
            fs |= bit;
        }
        if fs & 0x0007 == 0 {
            // A numeric key needs its kind (IBM's RC insists).
            if !is_str {
                self.diag(2146, "");
                return None;
            }
            fs |= 0x0001;
        }
        if ctl {
            fs |= 0x0010;
        }
        Some((fs, key, cmd))
    }

    // ---- MENU ----

    fn menu_res(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, cp) = self.options(DEFAULT_FLAGS);
        let Some(m) = self.menu_body() else { return };
        if m.items.is_empty() && m.pres.is_empty() {
            self.diag(2150, "");
        }
        let d = menu_template(&m, cp);
        self.add(rt::MENU, name, f, d);
    }

    /// BEGIN [PRESPARAMS...] items END
    fn menu_body(&mut self) -> Option<Menu> {
        if !self.begin() {
            self.recover();
            return None;
        }
        let mut m = Menu { pres: Vec::new(), items: Vec::new() };
        while !self.at_end() {
            if self.kw("PRESPARAMS") {
                if let Some(p) = self.presparam() {
                    m.pres.push(p);
                }
                continue;
            }
            let t = self.next();
            if t.is_kw("MENUITEM") {
                if let Some(i) = self.menuitem(false) {
                    m.items.push(i);
                }
            } else if t.is_kw("SUBMENU") {
                if let Some(i) = self.menuitem(true) {
                    m.items.push(i);
                }
            } else {
                self.diag(2003, &t.describe());
                while self.lx.same_line() && !self.at_end() {
                    self.next();
                }
            }
        }
        self.end();
        Some(m)
    }

    fn menuitem(&mut self, submenu: bool) -> Option<MenuItem> {
        if !submenu && self.kw("SEPARATOR") {
            return Some(MenuItem { style: MIS_SEPARATOR, attr: 0x4000, id: 0xffff, text: Vec::new(), sub: None });
        }
        let text = match self.string() {
            Some(s) => s,
            None => {
                { let t = self.peek().describe(); self.diag(2126, &t); }
                return None;
            }
        };
        if !self.comma() {
            { let t = self.peek().describe(); self.diag(2003, &t); }
            return None;
        }
        let id = self.value(2087)? as u16;
        let mut style = MIS_TEXT;
        let mut style_given = false;
        let mut attr = 0;
        if self.more() {
            // `id,` with nothing after the comma: style 0.
            style = 0;
            if self.starts_value() {
                style = self.style(2094)?.0 as u16;
                style_given = true;
            }
            if self.more() && self.starts_value() {
                attr = self.style(2149)?.0 as u16;
            }
        }
        let _ = style_given;
        if submenu {
            style |= MIS_SUBMENU;
        }
        let mut sub = None;
        if style & MIS_SUBMENU != 0 {
            sub = Some(self.menu_body()?);
        }
        Some(MenuItem { style, attr, id, text, sub })
    }

    fn presparam(&mut self) -> Option<(u32, Vec<u8>)> {
        let id = self.value(2151)? as u32;
        let mut data = Vec::new();
        // Commas between the values may be left out on the same line.
        while self.comma() || (self.lx.same_line() && self.starts_value()) {
            if let Tok::Str(_) = self.peek() {
                let s = self.string()?;
                data.extend_from_slice(&s);
                data.push(0);
            } else {
                let v = self.value(2151)?;
                le32(&mut data, v as u32);
            }
        }
        Some((id, data))
    }

    // ---- STRINGTABLE / MESSAGETABLE ----

    fn stringtable(&mut self, messages: bool) {
        let (f, cp) = self.options(DEFAULT_FLAGS);
        if !self.begin() {
            return self.recover();
        }
        while !self.at_end() {
            let Some(id) = self.value(if messages { 2155 } else { 2154 }) else {
                while self.lx.same_line() && !self.at_end() {
                    self.next();
                }
                continue;
            };
            self.comma();
            let Some(s) = self.string() else {
                let t = self.peek().describe();
                self.diag(if messages { 2155 } else { 2154 }, &t);
                continue;
            };
            let id = id as u16;
            let table = if messages { &mut self.messages } else { &mut self.strings };
            if table.contains_key(&id) {
                self.diag(if messages { 2137 } else { 2136 }, &alloc::format!("{id}"));
                continue;
            }
            let seq = self.strings.len() + self.messages.len();
            let table = if messages { &mut self.messages } else { &mut self.strings };
            table.insert(id, Str { seq, text: s, flags: f, cp });
        }
        self.end();
    }

    fn flush_strings(&mut self) {
        for messages in [false, true] {
            let table = core::mem::take(if messages { &mut self.messages } else { &mut self.strings });
            // Bundles in the order their first string was defined.
            let mut bundles: Vec<(u16, Vec<(u16, Str)>)> = Vec::new();
            let mut order: Vec<(usize, u16, Str)> = table.into_iter().map(|(id, s)| (s.seq, id, s)).collect();
            order.sort_by_key(|x| x.0);
            for (_, id, s) in order {
                match bundles.iter_mut().find(|b| b.0 == id / 16) {
                    Some(b) => b.1.push((id, s)),
                    None => bundles.push((id / 16, vec![(id, s)])),
                }
            }
            for (b, strs) in bundles {
                let (f, cp) = (strs[0].1.flags, strs[0].1.cp);
                let mut d = Vec::new();
                le16(&mut d, cp);
                for i in 0..16 {
                    let id = b * 16 + i;
                    match strs.iter().find(|(x, _)| *x == id) {
                        Some((_, s)) => {
                            let t = &s.text[..s.text.len().min(254)];
                            d.push(t.len() as u8 + 1);
                            d.extend_from_slice(t);
                            d.push(0);
                        }
                        None => d.extend_from_slice(&[1, 0]),
                    }
                }
                self.add(if messages { rt::MESSAGE } else { rt::STRING }, Name::Id(b + 1), f, d);
            }
        }
    }

    // ---- RCDATA / RESOURCE ----

    /// Data items: strings (NUL added if `nul`) and numbers (2 bytes, or
    /// 4 with an L suffix).
    fn data_block(&mut self, nul: bool, num: u16) -> Option<Vec<u8>> {
        if !self.begin() {
            self.recover();
            return None;
        }
        let mut d = Vec::new();
        while !self.at_end() {
            match self.peek() {
                Tok::Str(_) => {
                    // RCDATA: C strings with their NUL; RESOURCE: the bytes.
                    if nul {
                        let s = self.string()?;
                        d.extend_from_slice(&s);
                        d.push(0);
                    } else {
                        let s = self.raw_string()?;
                        d.extend_from_slice(&s);
                    }
                }
                _ => {
                    let (v, long) = self.value_long(num)?;
                    if long {
                        le32(&mut d, v as u32);
                    } else {
                        le16(&mut d, v as u16);
                    }
                }
            }
            self.comma();
        }
        self.end();
        Some(d)
    }

    /// True if the number about to be read has an L suffix.
    #[allow(dead_code)]
    fn value_is_long(&mut self) -> bool {
        matches!(self.peek(), Tok::Num(_, true))
    }

    fn rcdata(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(flags::MOVEABLE | flags::PURE);
        let Some(d) = self.data_block(true, 2158) else { return };
        self.add(rt::RCDATA, name, f, d);
    }

    fn resource(&mut self) {
        let Some(ty) = self.value(2108) else { return self.recover() };
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        let t = self.peek();
        if t.is_kw("BEGIN") || t.is_op("{") {
            let Some(d) = self.data_block(false, 2159) else { return };
            self.add(ty as u16, name, f, d);
            return;
        }
        let Some(file) = self.lx.filename() else {
            self.diag(2172, "");
            return;
        };
        if let Some(d) = self.read_file(&file) {
            self.add(ty as u16, name, f, d);
        }
    }

    // ---- HELPTABLE / HELPSUBTABLE ----

    fn helptable(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        if !self.begin() {
            return self.recover();
        }
        let mut d = Vec::new();
        while !self.at_end() {
            if !self.kw("HELPITEM") {
                let t = self.next().describe();
                self.diag(2003, &t);
                continue;
            }
            let Some(a) = self.value(2127) else { continue };
            self.comma();
            let Some(b) = self.value(2127) else { continue };
            self.comma();
            let Some(c) = self.value(2127) else { continue };
            le16(&mut d, a as u16);
            le16(&mut d, b as u16);
            le16(&mut d, 0xffff);
            le16(&mut d, c as u16);
        }
        self.end();
        le16(&mut d, 0);
        self.add(rt::HELPTABLE, name, f, d);
    }

    fn helpsubtable(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        let mut size = 2u16;
        if self.kw("SUBITEMSIZE") {
            match self.value(2113) {
                Some(v) if v >= 2 => size = v as u16,
                Some(_) => self.diag(2114, ""),
                None => {}
            }
        }
        if !self.begin() {
            return self.recover();
        }
        let mut d = Vec::new();
        le16(&mut d, size);
        while !self.at_end() {
            if !self.kw("HELPSUBITEM") {
                let t = self.next().describe();
                self.diag(2003, &t);
                continue;
            }
            let mut vals = Vec::new();
            loop {
                let Some(v) = self.value(2127) else { break };
                vals.push(v as u16);
                if !self.comma() || !self.starts_value() {
                    break;
                }
            }
            // Every item has SUBITEMSIZE values (IBM's RC insists).
            if vals.len() != size as usize {
                self.diag(2162, "");
            }
            vals.resize(size as usize, 0);
            for v in vals {
                le16(&mut d, v);
            }
        }
        self.end();
        le16(&mut d, 0);
        self.add(rt::HELPSUBTABLE, name, f, d);
    }

    // ---- ASSOCTABLE ----

    fn assoctable(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, _) = self.options(DEFAULT_FLAGS);
        if !self.begin() {
            return self.recover();
        }
        let mut d = Vec::new();
        let mut n = 0u16;
        let mut entries = Vec::new();
        while !self.at_end() {
            let Some(aname) = self.string() else {
                self.diag(2156, "");
                self.next();
                continue;
            };
            self.comma();
            let Some(m) = self.string() else {
                self.diag(2157, "");
                continue;
            };
            let mut eaf = 0u16;
            let mut icon = Vec::new();
            if self.comma() {
                if !matches!(self.peek(), Tok::Str(_)) && self.lx.same_line() {
                    if let Tok::Num(..) | Tok::Id(_) | Tok::Op("(") = self.peek() {
                        if let Some((s, _)) = self.style(2111) {
                            eaf = s as u16;
                        }
                    }
                }
                if self.comma() || self.lx.same_line() {
                    if let Some(file) = self.lx.filename() {
                        icon = self.read_file(&file).unwrap_or_default();
                    }
                }
            }
            entries.push((aname, m, eaf, icon));
            n += 1;
        }
        self.end();
        le16(&mut d, n);
        for (a, m, eaf, icon) in entries {
            d.extend_from_slice(&a);
            d.push(0);
            d.extend_from_slice(&m);
            d.push(0);
            le16(&mut d, eaf);
            le32(&mut d, icon.len() as u32);
            d.extend_from_slice(&icon);
        }
        self.assoc = Some(d.clone());
        self.add(rt::DISPLAYINFO, name, f, d);
    }

    // ---- DLGTEMPLATE / WINDOWTEMPLATE ----

    fn template(&mut self) {
        let Some(name) = self.res_name() else { return self.recover() };
        let (f, cp) = self.options(DEFAULT_FLAGS);
        if !self.begin() {
            return self.recover();
        }
        let mut wins = Vec::new();
        self.window_list(&mut wins);
        self.end();
        if wins.is_empty() {
            self.diag(2135, "");
            return;
        }
        if wins.len() > 1 {
            self.diag(2134, "");
        }
        let d = dialog_template(&wins[0], cp);
        self.add(rt::DIALOG, name, f, d);
    }

    /// Window statements up to END; PRESPARAMS and CTLDATA go to the
    /// window before them.
    fn window_list(&mut self, out: &mut Vec<Win>) {
        while !self.at_end() {
            let t = self.next();
            let Tok::Id(w) = &t else {
                self.diag(2003, &t.describe());
                continue;
            };
            let w = w.to_ascii_uppercase();
            if w == b"PRESPARAMS" || w == b"CTLDATA" {
                let pres = w == b"PRESPARAMS";
                let r = if pres { self.presparam().map(|p| (Some(p), None)) } else { self.ctldata().map(|c| (None, Some(c))) };
                if let (Some((p, c)), Some(last)) = (r, out.last_mut()) {
                    if let Some(p) = p {
                        last.pres.push(p);
                    }
                    if let Some(c) = c {
                        last.ctldata = Some(c);
                    }
                }
                continue;
            }
            self.bad = false;
            match self.window(&w) {
                Some(win) => out.push(win),
                None => {
                    while self.lx.same_line() && !self.at_end() {
                        self.next();
                    }
                }
            }
        }
    }

    fn ctldata(&mut self) -> Option<Vec<u8>> {
        if self.kw("MENU") {
            let m = self.menu_body()?;
            return Some(menu_template(&m, self.cp));
        }
        let mut d = Vec::new();
        loop {
            if let Tok::Str(_) = self.peek() {
                let s = self.string()?;
                d.extend_from_slice(&s);
                d.push(0);
            } else {
                let v = self.value(2152)?;
                le16(&mut d, v as u16);
            }
            if !self.comma() {
                break;
            }
            if self.at_end() {
                self.diag(3017, "");
                break;
            }
        }
        Some(d)
    }

    /// Text of a control: a string, or a number (a resource ordinal).
    fn text(&mut self) -> Option<Text> {
        if let Tok::Str(_) = self.peek() {
            return Some(Text::Str(self.string()?));
        }
        let v = self.value(2126)?;
        Some(Text::Ord(v as u16))
    }

    fn coords(&mut self) -> Option<[u16; 4]> {
        let x = self.value(2101)?;
        self.comma();
        let y = self.value(2103)?;
        self.comma();
        let cx = self.value(2099)?;
        self.comma();
        let cy = self.value(2097)?;
        Some([x as u16, y as u16, cx as u16, cy as u16])
    }

    fn class(&mut self) -> Option<Class> {
        if let Tok::Str(_) = self.peek() {
            return Some(Class::Name(self.string()?));
        }
        let v = self.value(2090)? as u32;
        if v >> 16 == 0xffff || v < 0x10000 {
            Some(Class::Ord(v as u16))
        } else {
            self.diag(2090, &alloc::format!("{v:#x}"));
            None
        }
    }

    fn window(&mut self, kw: &[u8]) -> Option<Win> {
        // (class, default style, has text, id only)
        let (class, def, text_first): (Option<u16>, u32, bool) = match kw {
            b"DIALOG" | b"FRAME" => (Some(WC_FRAME), 0, true),
            b"WINDOW" | b"CONTROL" => (None, 0, true),
            b"PUSHBUTTON" => (Some(WC_BUTTON), WS_TABSTOP | WS_VISIBLE, true),
            b"DEFPUSHBUTTON" => (Some(WC_BUTTON), 0x0400 | WS_TABSTOP | WS_VISIBLE, true),
            b"CHECKBOX" => (Some(WC_BUTTON), 1 | WS_TABSTOP | WS_VISIBLE, true),
            b"AUTOCHECKBOX" => (Some(WC_BUTTON), 2 | WS_TABSTOP | WS_VISIBLE, true),
            b"RADIOBUTTON" => (Some(WC_BUTTON), 3 | WS_VISIBLE, true),
            b"AUTORADIOBUTTON" => (Some(WC_BUTTON), 4 | WS_VISIBLE, true),
            b"LTEXT" => (Some(WC_STATIC), 1 | WS_GROUP | WS_VISIBLE, true),
            b"CTEXT" => (Some(WC_STATIC), 1 | 0x100 | WS_GROUP | WS_VISIBLE, true),
            b"RTEXT" => (Some(WC_STATIC), 1 | 0x200 | WS_GROUP | WS_VISIBLE, true),
            b"GROUPBOX" => (Some(WC_STATIC), 2 | WS_GROUP | WS_VISIBLE, true),
            b"ENTRYFIELD" | b"EDITTEXT" => (Some(WC_ENTRYFIELD), 4 | WS_TABSTOP | WS_VISIBLE, true),
            b"COMBOBOX" => (Some(WC_COMBOBOX), 1 | WS_TABSTOP | WS_VISIBLE, true),
            b"MLE" => (Some(WC_MLE), 0x2 | WS_TABSTOP | WS_VISIBLE, true),
            b"ICON" => (Some(WC_STATIC), 3 | WS_VISIBLE, true),
            b"LISTBOX" => (Some(WC_LISTBOX), WS_TABSTOP | WS_VISIBLE, false),
            b"SPINBUTTON" => (Some(WC_SPINBUTTON), WS_TABSTOP | WS_VISIBLE, false),
            b"VALUESET" => (Some(WC_VALUESET), WS_TABSTOP | WS_VISIBLE, false),
            b"NOTEBOOK" => (Some(WC_NOTEBOOK), WS_TABSTOP | WS_VISIBLE, false),
            b"CONTAINER" => (Some(WC_CONTAINER), WS_TABSTOP | WS_VISIBLE, false),
            b"SLIDER" => (Some(WC_SLIDER), WS_TABSTOP | WS_VISIBLE, false),
            _ => {
                self.diag(2003, &lstr(kw));
                return None;
            }
        };
        let text = if text_first {
            let t = if kw == b"ICON" { Text::Ord(self.value(2126)? as u16) } else { self.text()? };
            self.comma();
            t
        } else {
            Text::Str(Vec::new())
        };
        let id = self.value(2087)? as u16;
        self.comma();
        let [x, y, cx, cy] = self.coords()?;
        let class = match class {
            Some(c) => Class::Ord(c),
            None => {
                if kw == b"WINDOW" && !self.lx.peek().is_op(",") {
                    // WINDOW needs a class.
                }
                self.comma();
                self.class()?
            }
        };
        let mut style = if kw == b"DIALOG" { WS_SAVEBITS | WS_CLIPSIBLINGS } else { def };
        let mut clear = 0;
        let mut framectl: Option<u32> = None;
        if self.more() {
            if self.starts_value() {
                let (set, clr) = self.style(match class {
                    Class::Ord(WC_BUTTON) => 2079,
                    Class::Ord(WC_STATIC) => 2085,
                    Class::Ord(WC_FRAME) => 2083,
                    _ => 2081,
                })?;
                // A combo box's style replaces CBS_SIMPLE.
                if kw == b"COMBOBOX" {
                    style = WS_TABSTOP | WS_VISIBLE;
                }
                style = (style & !clr) | set;
                clear = clr;
            }
            if matches!(kw, b"DIALOG" | b"FRAME" | b"WINDOW") && self.more() && self.starts_value() {
                let (set, _) = self.style(2089)?;
                framectl = Some(set);
            }
        }
        match kw {
            b"FRAME" => style |= WS_VISIBLE,
            b"DIALOG" => {
                let fcf = framectl.unwrap_or(0);
                if style & (FS_DLGBORDER | FS_BORDER | FS_SIZEBORDER) == 0 && fcf & (FCF_BORDER | FCF_SIZEBORDER) == 0 && clear & FS_DLGBORDER == 0 {
                    style |= FS_DLGBORDER;
                }
            }
            _ => {}
        }
        let mut win = Win {
            class,
            text,
            id,
            x,
            y,
            cx,
            cy,
            style,
            // DIALOG and FRAME always have their frame creation flags as
            // control data; WINDOW's are not kept.
            ctldata: matches!(kw, b"DIALOG" | b"FRAME").then(|| framectl.unwrap_or(0).to_le_bytes().to_vec()),
            pres: Vec::new(),
            children: Vec::new(),
        };
        // Data definitions, then the children.
        loop {
            if self.kw("PRESPARAMS") {
                if let Some(p) = self.presparam() {
                    win.pres.push(p);
                }
            } else if self.kw("CTLDATA") {
                if let Some(c) = self.ctldata() {
                    win.ctldata = Some(c);
                }
            } else {
                break;
            }
        }
        let t = self.peek();
        if t.is_kw("BEGIN") || t.is_op("{") {
            self.next();
            self.window_list(&mut win.children);
            self.end();
        }
        Some(win)
    }
}

/// A menu template: cb, version, code page, offset of the items (from
/// the code page word), count, [presentation parameters], items.
fn menu_template(m: &Menu, cp: u16) -> Vec<u8> {
    let mut d = Vec::new();
    let mut pps: Vec<(usize, Vec<u8>)> = Vec::new();
    menu_rec(m, cp, &mut d, &mut pps);
    // Presentation parameters of the menu and its submenus follow the
    // whole template, each pointed to by its template's last header word.
    for (at, p) in pps {
        let off = d.len() as u16;
        d[at..at + 2].copy_from_slice(&off.to_le_bytes());
        d.extend_from_slice(&p);
    }
    let n = d.len() as u16;
    d[0..2].copy_from_slice(&n.to_le_bytes());
    d
}

/// One menu template (size, version, code page, offset of the items,
/// count, [offset of the presentation parameters]) and its items.
fn menu_rec(m: &Menu, cp: u16, d: &mut Vec<u8>, pps: &mut Vec<(usize, Vec<u8>)>) {
    let start = d.len();
    let pres = !m.pres.is_empty();
    le16(d, 0);
    le16(d, pres as u16);
    le16(d, cp);
    le16(d, if pres { 12 } else { 4 });
    le16(d, m.items.len() as u16);
    if pres {
        pps.push((d.len(), presparams(&m.pres)));
        le16(d, 0);
    }
    for i in &m.items {
        le16(d, i.style);
        le16(d, i.attr);
        le16(d, i.id);
        // Separators and owner-drawn items without MIS_TEXT have no text.
        if i.style & (MIS_SEPARATOR | MIS_OWNERDRAW) == 0 || i.style & 0x0003 != 0 {
            d.extend_from_slice(&i.text);
            d.push(0);
        }
        if let Some(s) = &i.sub {
            menu_rec(s, cp, d, pps);
        }
    }
    let n = (d.len() - start) as u16;
    d[start..start + 2].copy_from_slice(&n.to_le_bytes());
}

/// PRESPARAMS: total size, then (id, size, data) per parameter.
fn presparams(p: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut d = Vec::new();
    let total: usize = p.iter().map(|(_, v)| 8 + v.len()).sum();
    le32(&mut d, total as u32);
    for (id, v) in p {
        le32(&mut d, *id);
        le32(&mut d, v.len() as u32);
        d.extend_from_slice(v);
    }
    d
}

/// A dialog template: the header, every window's 30-byte item (each
/// followed by its children), then the strings and data they point to.
fn dialog_template(top: &Win, cp: u16) -> Vec<u8> {
    fn count(w: &Win) -> usize {
        1 + w.children.iter().map(count).sum::<usize>()
    }
    let n = count(top);
    let mut d = Vec::new();
    le16(&mut d, 0);
    le16(&mut d, 0);
    le16(&mut d, cp);
    le16(&mut d, 14);
    le16(&mut d, 1);
    le16(&mut d, 0xffff);
    le16(&mut d, 0);
    d.resize(14 + 30 * n, 0);
    let mut idx = 0;
    fn put(w: &Win, d: &mut Vec<u8>, idx: &mut usize) {
        let at = 14 + 30 * *idx;
        *idx += 1;
        let mut item = Vec::with_capacity(30);
        le16(&mut item, 0);
        le16(&mut item, w.children.len() as u16);
        // Class.
        match &w.class {
            Class::Ord(c) => {
                le16(&mut item, 0);
                le16(&mut item, *c);
            }
            Class::Name(s) => {
                le16(&mut item, s.len() as u16);
                le16(&mut item, d.len() as u16);
                d.extend_from_slice(s);
                d.push(0);
            }
        }
        // Text.
        match &w.text {
            Text::Str(s) => {
                le16(&mut item, s.len() as u16);
                le16(&mut item, d.len() as u16);
                d.extend_from_slice(s);
                d.push(0);
            }
            Text::Ord(o) => {
                le16(&mut item, 3);
                le16(&mut item, d.len() as u16);
                d.push(0xff);
                d.extend_from_slice(&o.to_le_bytes());
            }
        }
        le32(&mut item, w.style);
        le16(&mut item, w.x);
        le16(&mut item, w.y);
        le16(&mut item, w.cx);
        le16(&mut item, w.cy);
        le16(&mut item, w.id);
        // The control data goes before the presentation parameters.
        let ctl_at = match &w.ctldata {
            None => 0xffff,
            Some(c) => {
                let at = d.len() as u16;
                d.extend_from_slice(c);
                at
            }
        };
        if w.pres.is_empty() {
            le16(&mut item, 0xffff);
        } else {
            le16(&mut item, d.len() as u16);
            let p = presparams(&w.pres);
            d.extend_from_slice(&p);
        }
        le16(&mut item, ctl_at);
        d[at..at + 30].copy_from_slice(&item);
        for c in &w.children {
            put(c, d, idx);
        }
    }
    put(top, &mut d, &mut idx);
    let n = d.len() as u16;
    d[0..2].copy_from_slice(&n.to_le_bytes());
    d
}
