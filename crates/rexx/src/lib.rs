//! A classic REXX interpreter: the language of IBM's OS/2 REXX.DLL
//! (SAA Procedures Language 2.0, `REXXSAA 4.00`). The crate has no OS
//! dependencies: everything that touches the system (output, commands,
//! queues, files, external routines, the clock) goes through [`Host`].
//! REXX.DLL implements the host on OS/2 calls; the tests use a host on
//! memory.
//!
//! Strings are byte strings (one character per byte, as on OS/2).

#![no_std]
extern crate alloc;

pub mod builtins;
pub mod exec;
pub mod lex;
pub mod num;
pub mod parse;
mod run;
pub mod stream;
pub mod vars;

#[cfg(test)]
mod tests;

use alloc::vec::Vec;

pub use exec::{Interp, State};

/// The program's version string (PARSE VERSION), IBM's.
pub const VERSION: &[u8] = b"REXXSAA 4.00 3 Feb 1999";

/// An error or a transfer of control that leaves a clause.
#[derive(Clone, Debug, PartialEq)]
pub enum Err {
    /// A REXX error (syntax condition) with its number.
    Syntax(u32),
    /// A condition trapped by SIGNAL ON: transfer to `label`.
    Signal { label: Vec<u8> },
    /// SIGNAL label (or SIGNAL VALUE) leaving INTERPRET code.
    Goto { label: Vec<u8> },
    /// An error already reported; the program ends with it.
    Fatal(u32),
    /// EXIT: the program ends with this result.
    Exit(Option<Vec<u8>>),
}

impl Err {
    pub fn new(code: u32) -> Err {
        Err::Syntax(code)
    }
}

/// How a program was called (PARSE SOURCE's second word).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CallType {
    Command,
    Function,
    Subroutine,
}

impl CallType {
    pub fn word(self) -> &'static [u8] {
        match self {
            CallType::Command => b"COMMAND",
            CallType::Function => b"FUNCTION",
            CallType::Subroutine => b"SUBROUTINE",
        }
    }
}

/// The result of a command.
pub struct CmdResult {
    pub rc: Vec<u8>,
    /// 0 none, 1 ERROR, 2 FAILURE.
    pub cond: u8,
}

/// The result of looking for an external routine.
pub enum External {
    /// A function package or exit answered (None: no result).
    Value(Option<Vec<u8>>),
    /// A REXX program to run (from a file or the macro space).
    Program { source: Vec<u8>, path: Vec<u8> },
    NotFound,
    /// The routine failed with this REXX error number (40...).
    Error(u32),
}

/// Local date and time.
#[derive(Clone, Copy, Default)]
pub struct Now {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub micros: u32,
}

/// How a stream is opened.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OpenMode {
    Read,
    Write,
    Both,
}

/// Information about a file for STREAM's QUERY commands.
pub struct FileInfo {
    pub size: u64,
    /// mm-dd-yy hh:mm:ss
    pub datetime: Vec<u8>,
}

/// The system the interpreter runs on.
pub trait Host {
    /// SAY output (one line, without the line end).
    fn say(&mut self, st: &mut State, line: &[u8]);
    /// A trace or error-report line.
    fn trace_out(&mut self, st: &mut State, line: &[u8]);
    /// Read a line from the terminal (PULL with an empty queue).
    fn pull_terminal(&mut self, st: &mut State) -> Option<Vec<u8>>;
    /// Read interactive-trace input.
    fn trace_in(&mut self, st: &mut State) -> Option<Vec<u8>>;
    /// Run a command in an environment.
    fn command(&mut self, st: &mut State, env: &[u8], cmd: &[u8]) -> CmdResult;
    fn queued(&mut self, st: &mut State) -> usize;
    fn queue_add(&mut self, st: &mut State, line: &[u8], lifo: bool);
    fn queue_pull(&mut self, st: &mut State) -> Option<Vec<u8>>;
    /// RXQUEUE(op, name): Ok(result) or Err(REXX error number).
    fn rxqueue(&mut self, st: &mut State, op: &[u8], name: Option<&[u8]>) -> Result<Vec<u8>, u32>;
    /// An external routine (function package, macro, program file).
    fn external(&mut self, st: &mut State, name: &[u8], args: &[Option<Vec<u8>>], func: bool) -> External;
    /// RXFUNCADD/RXFUNCDROP/RXFUNCQUERY ('A', 'D', 'Q').
    fn rxfunc(&mut self, op: u8, name: &[u8], module: &[u8], proc_: &[u8]) -> u32;
    fn env_get(&mut self, name: &[u8]) -> Option<Vec<u8>>;
    fn env_set(&mut self, name: &[u8], val: Option<&[u8]>);
    /// DIRECTORY(): change (if given) and return the current directory.
    fn directory(&mut self, new: Option<&[u8]>) -> Option<Vec<u8>>;
    fn setlocal(&mut self) -> bool;
    fn endlocal(&mut self) -> bool;
    fn now(&mut self) -> Now;
    /// A halt was requested (RexxSetHalt, the halt exit, Ctrl+Break).
    fn halt(&mut self, st: &mut State) -> bool;
    /// A trace request from outside (RexxSetTrace: Some(true),
    /// RexxResetTrace: Some(false)).
    fn trace_request(&mut self) -> Option<bool> {
        None
    }
    fn beep(&mut self, freq: u32, ms: u32);
    /// A message from REX.MSG (text only, no prefix), for ERRORTEXT.
    fn message(&mut self, num: u32) -> Option<Vec<u8>>;
    /// Report an error ending the program: `REXnnnn: Error n running
    /// FILE, line L: text`.
    fn report_error(&mut self, st: &mut State, code: u32, file: &[u8], line: u32);
    /// Called before the first clause and after the last (exits RXINI,
    /// RXTER).
    fn init(&mut self, _st: &mut State) {}
    fn term(&mut self, _st: &mut State) {}

    // ---- files (stream I/O) ----
    /// Open a file or device; handles 0, 1 and 2 are the standard streams.
    fn f_open(&mut self, name: &[u8], mode: OpenMode) -> Result<u32, u32>;
    fn f_read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u32>;
    fn f_write(&mut self, h: u32, data: &[u8]) -> Result<usize, u32>;
    /// Seek to an absolute position.
    fn f_seek(&mut self, h: u32, pos: u64) -> Result<(), u32>;
    /// Size of a file (None for devices and pipes).
    fn f_size(&mut self, h: u32) -> Option<u64>;
    fn f_close(&mut self, h: u32);
    /// The fully qualified name of an existing file.
    fn f_exists(&mut self, name: &[u8]) -> Option<Vec<u8>>;
    fn f_info(&mut self, name: &[u8]) -> Option<FileInfo>;
    /// Whether a handle is a character device (LINES and CHARS report 1
    /// for devices without reading ahead).
    fn f_is_device(&mut self, _h: u32) -> bool {
        false
    }
}
