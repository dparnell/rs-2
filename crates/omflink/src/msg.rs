//! The linkers' messages: `WHERE : error Lnnnn: text`, with IBM's numbers
//! (L1xxx fatal, L2xxx errors, L4xxx warnings) and rs2's own wording.

use alloc::format;
use alloc::string::String;

use crate::Host;

pub struct Msgs {
    /// Program name shown when no file is involved ("LINK", "LINK386").
    pub prog: &'static str,
    pub errors: u32,
    pub warnings: u32,
    pub fatal: bool,
    /// /WARNFIXUP and similar: show L4xxx warnings at all.
    pub quiet_warnings: bool,
}

impl Msgs {
    pub fn new(prog: &'static str) -> Self {
        Msgs { prog, errors: 0, warnings: 0, fatal: false, quiet_warnings: false }
    }

    fn show(&self, host: &mut dyn Host, file: &str, kind: &str, num: u32, text: &str) {
        let at = if file.is_empty() { self.prog } else { file };
        let s = format!("{at} : {kind} L{num:04}: {text}\r\n");
        host.out(&s);
    }

    pub fn fatal(&mut self, host: &mut dyn Host, file: &str, num: u32, text: &str) {
        self.show(host, file, "fatal error", num, text);
        self.fatal = true;
        self.errors += 1;
    }

    pub fn error(&mut self, host: &mut dyn Host, file: &str, num: u32, text: &str) {
        self.show(host, file, "error", num, text);
        self.errors += 1;
    }

    pub fn warn(&mut self, host: &mut dyn Host, file: &str, num: u32, text: &str) {
        if self.quiet_warnings {
            return;
        }
        self.show(host, file, "warning", num, text);
        self.warnings += 1;
    }

    /// The closing count, as IBM's linkers print it when there were errors.
    pub fn summary(&self) -> Option<String> {
        match self.errors {
            0 => None,
            1 => Some(String::from("The link found 1 error.\r\n")),
            n => Some(format!("The link found {n} errors.\r\n")),
        }
    }
}
