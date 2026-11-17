//! Module definition (.DEF) files as LINK and LINK386 read them.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppType {
    #[default]
    Unspecified,
    /// WINDOWAPI: a Presentation Manager program.
    Pm,
    /// WINDOWCOMPAT: runs in a PM window.
    Vio,
    /// NOTWINDOWCOMPAT: full screen only.
    NoVio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Exe,
    Dll,
    PhysicalDevice,
    VirtualDevice,
}

/// Segment attributes from CODE, DATA and SEGMENTS (None: not given).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs {
    pub preload: Option<bool>,
    /// Code: EXECUTEONLY (true) or EXECUTEREAD; data: READONLY or READWRITE.
    pub readonly: Option<bool>,
    pub iopl: Option<bool>,
    pub conforming: Option<bool>,
    pub shared: Option<bool>,
    pub discardable: Option<bool>,
    pub moveable: Option<bool>,
    /// DATA NONE / SINGLE / MULTIPLE: 0, 1, 2.
    pub instance: Option<u8>,
    pub alias: bool,
    pub mixed1632: bool,
    pub expanddown: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SegAttrs {
    pub name: String,
    pub class: Option<String>,
    pub attrs: Attrs,
}

#[derive(Debug, Clone, Default)]
pub struct Export {
    pub name: String,
    pub internal: String,
    pub ordinal: Option<u16>,
    pub resident: bool,
    pub noname: bool,
    pub nodata: bool,
    pub private: bool,
    pub pwords: u8,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub internal: String,
    pub module: String,
    pub entry: crate::omf::ImpEntry,
}

#[derive(Debug, Clone, Default)]
pub struct Def {
    pub kind: Kind,
    pub name: Option<String>,
    pub app: AppType,
    pub newfiles: bool,
    /// LIBRARY INITINSTANCE (true) or INITGLOBAL.
    pub init_instance: Option<bool>,
    pub term_instance: Option<bool>,
    pub privatelib: bool,
    pub description: Option<String>,
    pub code: Attrs,
    pub data: Attrs,
    pub segments: Vec<SegAttrs>,
    pub exports: Vec<Export>,
    pub imports: Vec<Import>,
    pub stacksize: Option<u32>,
    pub heapsize: Option<u32>,
    pub heap_maxval: bool,
    pub protmode: bool,
    pub realmode: bool,
    pub stub: Option<String>,
    pub stub_none: bool,
    pub old: Option<String>,
    pub base: Option<u32>,
    pub exetype: Option<String>,
}

/// A problem in the file (L2xxx-style syntax error), with its line number.
#[derive(Debug, Clone)]
pub struct DefError {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    Word(String),
    Quoted(String),
    Eq,
    At,
    Dot,
}

pub(crate) fn tokens(line: &str) -> Vec<Tok> {
    let mut v = Vec::new();
    let b: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == ';' {
            break;
        }
        if c.is_whitespace() || c == '\x1a' {
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            let mut s = String::new();
            i += 1;
            while i < b.len() {
                if b[i] == c {
                    // A doubled quote is a quote.
                    if i + 1 < b.len() && b[i + 1] == c {
                        s.push(c);
                        i += 2;
                        continue;
                    }
                    break;
                }
                s.push(b[i]);
                i += 1;
            }
            i += 1;
            v.push(Tok::Quoted(s));
            continue;
        }
        match c {
            '=' => {
                v.push(Tok::Eq);
                i += 1;
                continue;
            }
            '@' => {
                v.push(Tok::At);
                i += 1;
                continue;
            }
            _ => {}
        }
        let mut s = String::new();
        while i < b.len() && !b[i].is_whitespace() && !matches!(b[i], '=' | '@' | ';' | '\'' | '"') {
            s.push(b[i]);
            i += 1;
        }
        v.push(Tok::Word(s));
    }
    // Split IMPORTS' module.entry later; '.' stays inside words here.
    let _ = Tok::Dot;
    v
}

pub(crate) const STATEMENTS: &[&str] = &[
    "NAME", "LIBRARY", "DESCRIPTION", "CODE", "DATA", "SEGMENTS", "OBJECTS", "EXPORTS", "IMPORTS", "STACKSIZE",
    "HEAPSIZE", "PROTMODE", "REALMODE", "STUB", "OLD", "BASE", "EXETYPE", "PHYSICAL", "VIRTUAL", "APPLOADER",
];

/// Parse a number in C syntax (decimal, 0octal, 0xhex).
pub fn number(s: &str) -> Option<u32> {
    let s = s.trim();
    let (digits, radix) = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (h, 16)
    } else if s.len() > 1 && s.starts_with('0') {
        (&s[1..], 8)
    } else {
        (s, 10)
    };
    u32::from_str_radix(digits, radix).ok()
}

pub(crate) fn word_of(t: &Tok) -> Option<&str> {
    match t {
        Tok::Word(s) | Tok::Quoted(s) => Some(s),
        _ => None,
    }
}

/// Apply one attribute keyword; false if it is not one.
pub(crate) fn attr(a: &mut Attrs, k: &str) -> bool {
    match k {
        "PRELOAD" => a.preload = Some(true),
        "LOADONCALL" => a.preload = Some(false),
        "EXECUTEONLY" | "EXECUTE-ONLY" | "READONLY" => a.readonly = Some(true),
        "EXECUTEREAD" | "READWRITE" => a.readonly = Some(false),
        "IOPL" => a.iopl = Some(true),
        "NOIOPL" => a.iopl = Some(false),
        "CONFORMING" => a.conforming = Some(true),
        "NONCONFORMING" => a.conforming = Some(false),
        "SHARED" => a.shared = Some(true),
        "NONSHARED" => a.shared = Some(false),
        "DISCARDABLE" => a.discardable = Some(true),
        "NONDISCARDABLE" => a.discardable = Some(false),
        "MOVEABLE" | "MOVABLE" => a.moveable = Some(true),
        "FIXED" => a.moveable = Some(false),
        "NONE" => a.instance = Some(0),
        "SINGLE" => a.instance = Some(1),
        "MULTIPLE" => a.instance = Some(2),
        "ALIAS" => a.alias = true,
        "MIXED1632" => a.mixed1632 = true,
        "EXPANDDOWN" => a.expanddown = Some(true),
        "NOEXPANDDOWN" => a.expanddown = Some(false),
        // Accepted and ignored, as IBM's linkers do for OS/2 modules.
        "IMPURE" | "PURE" | "INVALID" | "VALID" | "SWAPPABLE" | "RESIDENT" | "CONTIG" | "NONPERMANENT"
        | "PERMANENT" | "DYNAMIC" | "HUGE" => {}
        _ => return false,
    }
    true
}

