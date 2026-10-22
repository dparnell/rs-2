//! The media control interface (MCI) of OS/2 multimedia, the parts that
//! do not touch the system: string commands split into words and read
//! keyword by keyword, the device types and their names, mode names,
//! error texts, and the memory playlist processor. MDM.DLL builds on it.
//!
//! A string command is `COMMAND object [keywords] [WAIT | NOTIFY]`; the
//! parser is case-insensitive and quoted strings keep blanks (file names).

#![no_std]

extern crate alloc;

pub mod cd;
pub mod errors;
pub mod playlist;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub use errors::*;

pub const MCIERR_SUCCESS: u32 = 0;
pub const MCI_NOTIFY: u32 = 0x1;
pub const MCI_WAIT: u32 = 0x2;

/// Device types (MCI_DEVTYPE_*), with their string names.
pub const DEVTYPES: &[(u16, &str)] = &[
    (1, "videotape"),
    (2, "videodisc"),
    (3, "cdaudio"),
    (4, "dat"),
    (5, "audiotape"),
    (6, "other"),
    (7, "waveaudio"),
    (8, "sequencer"),
    (9, "ampmix"),
    (10, "overlay"),
    (11, "animation"),
    (12, "digitalvideo"),
    (13, "speaker"),
    (14, "headphone"),
    (15, "microphone"),
    (16, "monitor"),
    (17, "cdxa"),
    (18, "filter"),
    (19, "texttospeech"),
];

pub const DEVTYPE_CD_AUDIO: u16 = 3;
pub const DEVTYPE_WAVEFORM_AUDIO: u16 = 7;
pub const DEVTYPE_SEQUENCER: u16 = 8;
pub const DEVTYPE_DIGITAL_VIDEO: u16 = 12;

pub fn devtype_name(t: u16) -> Option<&'static str> {
    DEVTYPES.iter().find(|d| d.0 == t).map(|d| d.1)
}

/// A device name: a type with an optional two-digit ordinal
/// ("waveaudio", "waveaudio01", "Digitalvideo02"): (type, ordinal), the
/// ordinal 0 for the default device.
pub fn parse_device_name(s: &str) -> Option<(u16, u16)> {
    let lower = s.to_ascii_lowercase();
    let digits = lower.bytes().rev().take_while(u8::is_ascii_digit).count();
    let (name, num) = lower.split_at(lower.len() - digits);
    let t = DEVTYPES.iter().find(|d| d.1 == name)?.0;
    let n = if num.is_empty() { 0 } else { num.parse().ok()? };
    Some((t, n))
}

/// A device name as MDM reports it: "Waveaudio01".
pub fn device_name(t: u16, ordinal: u16) -> String {
    let n = devtype_name(t).unwrap_or("other");
    let mut s = String::new();
    s.push(n.as_bytes()[0].to_ascii_uppercase() as char);
    s.push_str(&n[1..]);
    s.push_str(&alloc::format!("{:02}", ordinal.max(1)));
    s
}

/// The device type a file is played on, by its extension (as MMPM/2's
/// MciInstallExt table maps them).
pub fn devtype_for_file(name: &str) -> Option<u16> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "wav" | "aif" | "aiff" | "au" | "snd" | "voc" | "_au" => Some(DEVTYPE_WAVEFORM_AUDIO),
        "mid" | "rmi" | "_mi" => Some(DEVTYPE_SEQUENCER),
        "flc" | "fli" | "avi" | "avs" | "mpg" | "vbs" => Some(DEVTYPE_DIGITAL_VIDEO),
        _ => None,
    }
}

/// MCI_MODE_* as strings (STATUS ... MODE).
pub fn mode_name(mode: u32) -> &'static str {
    match mode {
        1 => "not ready",
        2 => "paused",
        3 => "playing",
        4 => "stopped",
        5 => "recording",
        6 => "seeking",
        _ => "unknown",
    }
}

/// The text of an MCI error (mciGetErrorString).
pub fn error_text(code: u32) -> Option<&'static str> {
    ERRORS.iter().find(|e| e.0 == code).map(|e| e.2)
}

/// A string command, split into its verb, its object and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command, lower case ("open", "play").
    pub verb: String,
    /// The object: a device type or name, a file name or an alias (as
    /// written). Empty for MASTERAUDIO.
    pub object: String,
    /// WAIT and NOTIFY (MCI_WAIT, MCI_NOTIFY).
    pub flags: u32,
    pub args: Args,
}

/// The keywords after the object: words are taken off as the command's
/// handler recognises them; whatever is left is an unknown keyword.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    words: Vec<String>,
}

/// Split a command into words: blanks separate them, double quotes keep
/// them together (the quotes are dropped).
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parse a string command. WAIT and NOTIFY may stand anywhere after the
/// object. Errors: MCIERR_MISSING_COMMAND_STRING, MCIERR_MISSING_PARAMETER
/// (no object), MCIERR_FLAGS_NOT_COMPATIBLE (WAIT and NOTIFY).
pub fn parse(s: &str) -> Result<Command, u32> {
    let mut w = words(s);
    if w.is_empty() {
        return Err(MCIERR_MISSING_COMMAND_STRING);
    }
    let verb = w.remove(0).to_ascii_lowercase();
    let object = if verb == "masteraudio" {
        String::new()
    } else if w.is_empty() {
        return Err(MCIERR_MISSING_PARAMETER);
    } else {
        w.remove(0)
    };
    let mut flags = 0;
    w.retain(|x| match x.to_ascii_lowercase().as_str() {
        "wait" => {
            flags |= MCI_WAIT;
            false
        }
        "notify" => {
            flags |= MCI_NOTIFY;
            false
        }
        _ => true,
    });
    if flags == MCI_WAIT | MCI_NOTIFY {
        return Err(MCIERR_FLAGS_NOT_COMPATIBLE);
    }
    Ok(Command { verb, object, flags, args: Args { words: w } })
}

impl Args {
    pub fn new(words: Vec<String>) -> Args {
        Args { words }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The first word left (for an error message or a handler that reads
    /// in order).
    pub fn peek(&self) -> Option<&str> {
        self.words.first().map(String::as_str)
    }

    fn find(&self, seq: &[&str]) -> Option<usize> {
        if seq.is_empty() || seq.len() > self.words.len() {
            return None;
        }
        (0..=self.words.len() - seq.len()).find(|&i| seq.iter().enumerate().all(|(j, k)| self.words[i + j].eq_ignore_ascii_case(k)))
    }

    /// Take a keyword (or a phrase: "to start", "can play") if present.
    pub fn flag(&mut self, phrase: &str) -> bool {
        let seq: Vec<&str> = phrase.split(' ').collect();
        match self.find(&seq) {
            Some(i) => {
                self.words.drain(i..i + seq.len());
                true
            }
            None => false,
        }
    }

    /// Take a keyword and the word after it ("alias movie"):
    /// Ok(None) when absent, Err(MCIERR_MISSING_STRING_ARGUMENT) when the
    /// value is missing.
    pub fn value(&mut self, phrase: &str) -> Result<Option<String>, u32> {
        let seq: Vec<&str> = phrase.split(' ').collect();
        let Some(i) = self.find(&seq) else { return Ok(None) };
        if i + seq.len() >= self.words.len() {
            return Err(MCIERR_MISSING_STRING_ARGUMENT);
        }
        let v = self.words.remove(i + seq.len());
        self.words.drain(i..i + seq.len());
        Ok(Some(v))
    }

    /// Like `value`, as a number (decimal, or 0x hex).
    pub fn number(&mut self, phrase: &str) -> Result<Option<u32>, u32> {
        match self.value(phrase)? {
            None => Ok(None),
            Some(v) => parse_number(&v).map(Some).ok_or(MCIERR_BAD_INTEGER),
        }
    }

    /// Take `n` numbers after a keyword ("destination at 0 0 320 200").
    pub fn numbers(&mut self, phrase: &str, n: usize) -> Result<Option<Vec<i32>>, u32> {
        let seq: Vec<&str> = phrase.split(' ').collect();
        let Some(i) = self.find(&seq) else { return Ok(None) };
        let start = i + seq.len();
        if start + n > self.words.len() {
            return Err(MCIERR_MISSING_PARAMETER);
        }
        let mut v = Vec::new();
        for w in &self.words[start..start + n] {
            v.push(parse_number(w).ok_or(MCIERR_BAD_INTEGER)? as i32);
        }
        self.words.drain(i..start + n);
        Ok(Some(v))
    }

    /// Take the next word whatever it is.
    pub fn next(&mut self) -> Option<String> {
        (!self.words.is_empty()).then(|| self.words.remove(0))
    }

    /// Ok if nothing is left, else MCIERR_UNRECOGNIZED_KEYWORD.
    pub fn done(&self) -> Result<(), u32> {
        if self.words.is_empty() { Ok(()) } else { Err(MCIERR_UNRECOGNIZED_KEYWORD) }
    }
}

/// A number in a command: decimal (possibly negative) or 0x hex.
pub fn parse_number(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(h, 16).ok();
    }
    if let Some(n) = s.strip_prefix('-') {
        return n.parse::<u32>().ok().map(|v| v.wrapping_neg());
    }
    s.parse().ok()
}

/// TRUE/FALSE as string commands return them.
pub fn bool_text(b: bool) -> String {
    if b { "TRUE" } else { "FALSE" }.to_string()
}

#[cfg(test)]
mod tests;
