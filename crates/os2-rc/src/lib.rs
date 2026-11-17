//! The OS/2 resource compiler (RC.EXE): the `.RC` script language, `.RES`
//! files and binding resources into LX and NE modules.
//!
//! The binary formats are IBM's (the Toolkit's RC and RC16 produce the same
//! bytes): see `docs/UTILITIES.md`, "RC". The crate is `no_std` + `alloc`:
//! `userland/utils` (RC.EXE) and the host tests share it. Files are reached
//! through [`Files`], so include paths and the INCLUDE variable work the
//! same everywhere.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub mod bind;
#[cfg(any(test, feature = "host"))]
pub mod host;
mod compile;
mod expr;
mod lex;
pub mod msg;
mod pp;
pub mod res;

pub use compile::compile;
pub use res::{Name, Resource};

/// Resource types (RT_*).
pub mod rt {
    pub const POINTER: u16 = 1;
    pub const BITMAP: u16 = 2;
    pub const MENU: u16 = 3;
    pub const DIALOG: u16 = 4;
    pub const STRING: u16 = 5;
    pub const FONTDIR: u16 = 6;
    pub const FONT: u16 = 7;
    pub const ACCELTABLE: u16 = 8;
    pub const RCDATA: u16 = 9;
    pub const MESSAGE: u16 = 10;
    pub const DLGINCLUDE: u16 = 11;
    pub const VKEYTBL: u16 = 12;
    pub const KEYTBL: u16 = 13;
    pub const CHARTBL: u16 = 14;
    pub const DISPLAYINFO: u16 = 15;
    pub const FKASHORT: u16 = 16;
    pub const FKALONG: u16 = 17;
    pub const HELPTABLE: u16 = 18;
    pub const HELPSUBTABLE: u16 = 19;
    pub const FDDIR: u16 = 20;
    pub const FD: u16 = 21;
    pub const RESNAMES: u16 = 255;
}

/// Load and memory options of a resource (the `.RES` flags word).
pub mod flags {
    pub const MOVEABLE: u16 = 0x0010;
    pub const PURE: u16 = 0x0020;
    pub const PRELOAD: u16 = 0x0040;
    pub const DISCARDABLE: u16 = 0x1000;
}

/// Where the compiler gets its files from.
pub trait Files {
    /// The contents of the file `path` (OS/2 syntax: `\` separators,
    /// maybe a drive letter), or `None` if it cannot be read.
    fn read(&mut self, path: &str) -> Option<Vec<u8>>;
}

/// Compiler options (the command line's).
#[derive(Clone, Default)]
pub struct Options {
    /// `-d NAME[=VALUE]`, in order.
    pub defines: Vec<(String, String)>,
    /// `-i PATH`, then the INCLUDE variable's directories.
    pub include: Vec<String>,
    /// The code page of the script (`-cp`, or the process's).
    pub codepage: u16,
}

/// How bad a diagnostic is (IBM's numbering: 1000s severe, 2000s errors,
/// 3000s warnings, 4000s information).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

/// A message about the script: IBM's message number, where, and the
/// message's parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub num: u16,
    pub file: String,
    pub line: u32,
    pub arg: String,
}

impl Diag {
    pub fn severity(&self) -> Severity {
        match self.num {
            1000..=1999 => Severity::Fatal,
            2000..=2999 => Severity::Error,
            3000..=3999 => Severity::Warning,
            _ => Severity::Info,
        }
    }

    /// `FILE(LINE) : error R2003: text`, as one line.
    pub fn format(&self) -> String {
        let kind = match self.severity() {
            Severity::Fatal => "fatal error",
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "information",
        };
        let text = msg::expand(self.num, &self.arg);
        if self.file.is_empty() {
            alloc::format!("RC: {kind} R{}: {text}", self.num)
        } else {
            alloc::format!("{}({}) : {kind} R{}: {text}", self.file, self.line, self.num)
        }
    }
}

/// The result of compiling a script.
pub struct Output {
    pub resources: Vec<Resource>,
    pub diags: Vec<Diag>,
    /// DEFAULTICON's file contents (for the `.ICON` extended attribute).
    pub default_icon: Option<Vec<u8>>,
    /// The last ASSOCTABLE (for the `.ASSOC` extended attribute).
    pub assoc: Option<Vec<u8>>,
}

impl Output {
    /// True if an error (or worse) was reported: no output file then.
    pub fn failed(&self) -> bool {
        self.diags.iter().any(|d| d.severity() >= Severity::Error)
    }
}
