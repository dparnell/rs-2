//! OS/2 message files (`.MSG`), as IBM's MKMSGF writes them and
//! DosGetMessage reads them: a compiler from MKMSGF's `.TXT` source, a
//! reader and a decompiler. rs2 builds its own system message files
//! (OSO001.MSG, OSO001H.MSG, ...) with it; `tools/mkmsgf` is the command
//! line tool. No OS dependencies.
//!
//! # Source (`.TXT`)
//!
//! ```text
//! ; comment lines start with a semicolon in column 1
//! ABC
//! ABC0001E: The first message, an error.
//! It can go on over several lines.
//! ABC0002?:
//! ABC0003P: A prompt keeps the cursor on its line (Y/N)? %0
//! ```
//!
//! The first line that is not a comment is the component id (3
//! characters). Each message starts in column 1 with the component id,
//! a four-digit number, its class (`E` error, `W` warning, `I`
//! information, `H` help, `P` prompt, `?` unused number), a colon and one
//! blank; its text runs to the next message header. Every line of it
//! ends with CR LF in the file, unless the text ends with `%0`. `%1` to
//! `%9` are insertion points for DosGetMessage. Numbers must ascend; a
//! gap is filled with `?` entries, as IBM requires them to be written.
//!
//! # Binary (`.MSG`)
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0x00 | 8 | `FF "MKMSGF" 00` |
//! | 0x08 | 3 | component id |
//! | 0x0B | 2 | number of messages |
//! | 0x0D | 2 | number of the first message |
//! | 0x0F | 1 | 1: the index holds 16-bit offsets, 0: 32-bit |
//! | 0x10 | 2 | version (2; 0 in old files without the country block) |
//! | 0x12 | 2 | offset of the index (0x1F) |
//! | 0x14 | 2 | offset of the country block (version 2) |
//! | 0x16 | 4 | offset of the extended header at the end (0: none) |
//! | 0x1A | 5 | reserved |
//! | 0x1F | | index: the offset of each message |
//!
//! The country block (0x12E bytes): bytes per character (1, 2 for DBCS
//! files), u16 country, u16 language family, u16 sublanguage, u16 number
//! of code pages, 16 u16 code pages, the file's name (as given to
//! MKMSGF). Each message is its class character and its text. Files with
//! more than one code page end with the extended header: u16 size of the
//! country block, u16 number of further blocks (0).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
mod tests;

/// File signature.
pub const MAGIC: &[u8; 8] = b"\xffMKMSGF\x00";
/// Size of the header (and offset of the index).
pub const HEADER_SIZE: usize = 0x1F;
/// Size of the country block.
pub const COUNTRY_SIZE: usize = 0x12E;
/// Offset of the file name in the country block.
const COUNTRY_NAME: usize = 0x29;
/// Most code pages a file records.
pub const MAX_CODE_PAGES: usize = 16;

/// One message of a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub number: u16,
    /// `E`, `W`, `I`, `H`, `P` or `?`.
    pub class: u8,
    /// The text as DosGetMessage returns it (lines end with CR LF).
    pub text: Vec<u8>,
}

/// A parsed source file.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub component: [u8; 3],
    pub messages: Vec<Message>,
}

/// What MKMSGF's switches set.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// `/P`: code pages of the text (at most 16).
    pub code_pages: Vec<u16>,
    /// `/L`: language family and sublanguage.
    pub language: (u16, u16),
    /// `/D`: DBCS lead byte ranges (a DBCS file).
    pub dbcs: Vec<(u8, u8)>,
    /// The file name recorded in the country block (MKMSGF records the
    /// output name as it was given).
    pub name: Vec<u8>,
}

/// An error with the source line it was found on (0: none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub text: String,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.text)
        } else {
            f.write_str(&self.text)
        }
    }
}

fn error(line: usize, text: String) -> Error {
    Error { line, text }
}

/// Is `b` a message class character? (A blank too: files of IBM's C
/// runtime, LIBC.MSG, have it.)
fn is_class(b: u8) -> bool {
    matches!(b, b'E' | b'W' | b'I' | b'H' | b'P' | b'?' | b' ')
}

/// The DBCS lead byte ranges of a country (MKMSGF's `/D` with a single
/// number).
pub fn dbcs_country(country: u16) -> Option<Vec<(u8, u8)>> {
    Some(match country {
        81 => alloc::vec![(0x81, 0x9F), (0xE0, 0xFC)],
        82 | 86 | 88 => alloc::vec![(0x81, 0xFE)],
        _ => return None,
    })
}

/// Does `text` end with `%0` (the last `%` not being the second byte of a
/// DBCS character)?
fn ends_with_pct0(text: &[u8], dbcs: &[(u8, u8)]) -> bool {
    if !text.ends_with(b"%0") {
        return false;
    }
    if dbcs.is_empty() {
        return true;
    }
    let lead = |b: u8| dbcs.iter().any(|&(lo, hi)| b >= lo && b <= hi);
    let mut i = 0;
    let n = text.len() - 2;
    while i < n {
        i += if lead(text[i]) { 2 } else { 1 };
    }
    i == n
}

/// Split source text into lines (CR LF or LF; a Ctrl+Z at the end of the
/// file is dropped).
fn lines(src: &[u8]) -> Vec<&[u8]> {
    let src = src.strip_suffix(b"\x1a").unwrap_or(src);
    let mut v: Vec<&[u8]> = src.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// The header of a message line: number and class.
fn header(line: &[u8], comp: &[u8; 3]) -> Option<(u16, u8)> {
    if line.len() < 9 || &line[..3] != comp || line[8] != b':' || !is_class(line[7]) {
        return None;
    }
    let digits = &line[3..7];
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = digits.iter().fold(0u16, |a, &d| a * 10 + (d - b'0') as u16);
    Some((n, line[7]))
}

/// Parse MKMSGF source.
pub fn parse(src: &[u8], dbcs: &[(u8, u8)]) -> Result<Source, Error> {
    let lines = lines(src);
    let mut it = lines.iter().enumerate().peekable();
    let mut component = [0u8; 3];
    loop {
        let Some((i, l)) = it.next() else { return Err(error(0, "no component identifier".into())) };
        if l.starts_with(b";") || l.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let id: Vec<u8> = l.iter().copied().take_while(|b| !b.is_ascii_whitespace()).collect();
        if id.len() != 3 {
            return Err(error(i + 1, "the component identifier must have 3 characters".into()));
        }
        component.copy_from_slice(&id);
        break;
    }
    // (number, class, text lines, line number)
    let mut raw: Vec<(u16, u8, Vec<&[u8]>, usize)> = Vec::new();
    for (i, l) in it {
        if let Some((n, class)) = header(l, &component) {
            let mut rest = &l[9..];
            if rest.first() == Some(&b' ') {
                rest = &rest[1..];
            }
            raw.push((n, class, alloc::vec![rest], i + 1));
        } else if l.starts_with(b";") {
            continue;
        } else if let Some(m) = raw.last_mut() {
            m.2.push(l);
        } else if !l.iter().all(|b| b.is_ascii_whitespace()) {
            return Err(error(i + 1, "text before the first message".into()));
        }
    }
    let mut messages: Vec<Message> = Vec::new();
    for (n, class, text_lines, line) in raw {
        if let Some(prev) = messages.last() {
            if n <= prev.number {
                return Err(error(line, format!("message {n:04} is out of order (after {:04})", prev.number)));
            }
            for gap in prev.number + 1..n {
                messages.push(Message { number: gap, class: b'?', text: b"\r\n".to_vec() });
            }
        }
        let mut text = Vec::new();
        for (j, t) in text_lines.iter().enumerate() {
            if j > 0 {
                text.extend_from_slice(b"\r\n");
            }
            text.extend_from_slice(t);
        }
        if ends_with_pct0(&text, dbcs) {
            text.truncate(text.len() - 2);
        } else {
            text.extend_from_slice(b"\r\n");
        }
        messages.push(Message { number: n, class, text });
    }
    if messages.is_empty() {
        return Err(error(0, "no messages".into()));
    }
    Ok(Source { component, messages })
}

/// Write a message file.
pub fn build(src: &Source, opts: &Options) -> Result<Vec<u8>, Error> {
    if opts.code_pages.len() > MAX_CODE_PAGES {
        return Err(error(0, format!("at most {MAX_CODE_PAGES} code pages")));
    }
    if src.messages.len() > 0xFFFF {
        return Err(error(0, "too many messages".into()));
    }
    let count = src.messages.len();
    let body: usize = src.messages.iter().map(|m| 1 + m.text.len()).sum();
    let start16 = HEADER_SIZE + count * 2 + COUNTRY_SIZE;
    // 16-bit offsets when every message starts below 64K.
    let last_len = src.messages.last().map_or(0, |m| 1 + m.text.len());
    let off16 = start16 + body - last_len <= 0xFFFF;
    let width = if off16 { 2 } else { 4 };
    let country = HEADER_SIZE + count * width;
    let start = country + COUNTRY_SIZE;
    let ext = if opts.code_pages.len() > 1 { start + body } else { 0 };
    let mut out = Vec::with_capacity(start + body + 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&src.component);
    out.extend_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&src.messages[0].number.to_le_bytes());
    out.push(off16 as u8);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&(country as u16).to_le_bytes());
    out.extend_from_slice(&(ext as u32).to_le_bytes());
    out.extend_from_slice(&[0; 5]);
    let mut o = start;
    for m in &src.messages {
        if off16 {
            out.extend_from_slice(&(o as u16).to_le_bytes());
        } else {
            out.extend_from_slice(&(o as u32).to_le_bytes());
        }
        o += 1 + m.text.len();
    }
    let mut cb = [0u8; COUNTRY_SIZE];
    cb[0] = if opts.dbcs.is_empty() { 1 } else { 2 };
    cb[3..5].copy_from_slice(&opts.language.0.to_le_bytes());
    cb[5..7].copy_from_slice(&opts.language.1.to_le_bytes());
    cb[7..9].copy_from_slice(&(opts.code_pages.len() as u16).to_le_bytes());
    for (i, cp) in opts.code_pages.iter().enumerate() {
        cb[9 + i * 2..11 + i * 2].copy_from_slice(&cp.to_le_bytes());
    }
    let name = &opts.name[..opts.name.len().min(COUNTRY_SIZE - COUNTRY_NAME - 1)];
    cb[COUNTRY_NAME..COUNTRY_NAME + name.len()].copy_from_slice(name);
    out.extend_from_slice(&cb);
    for m in &src.messages {
        out.push(m.class);
        out.extend_from_slice(&m.text);
    }
    if ext != 0 {
        out.extend_from_slice(&(COUNTRY_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(out)
}

/// Compile MKMSGF source to a message file.
pub fn compile(src: &[u8], opts: &Options) -> Result<Vec<u8>, Error> {
    build(&parse(src, &opts.dbcs)?, opts)
}

/// A message file being read.
pub struct MsgFile<'a> {
    pub data: &'a [u8],
    pub component: [u8; 3],
    pub first: u16,
    pub count: u16,
    pub version: u16,
    offsets: Vec<u32>,
    /// Where the last message ends.
    end: usize,
    country: Option<usize>,
}

impl<'a> MsgFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < HEADER_SIZE || &data[..8] != MAGIC {
            return Err(error(0, "not a message file".into()));
        }
        let r16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let r32 = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let count = r16(0x0B);
        let first = r16(0x0D);
        let width = if data[0x0F] != 0 { 2 } else { 4 };
        let version = r16(0x10);
        // Old (version 0) files leave the index offset 0.
        let idx = match r16(0x12) as usize {
            0 => HEADER_SIZE,
            i => i,
        };
        if idx + count as usize * width > data.len() {
            return Err(error(0, "index beyond the end of the file".into()));
        }
        let offsets: Vec<u32> = (0..count as usize)
            .map(|i| if width == 2 { r16(idx + i * 2) as u32 } else { r32(idx + i * 4) })
            .collect();
        let country = (version == 2).then(|| r16(0x14) as usize).filter(|&c| c + COUNTRY_SIZE <= data.len());
        let ext = r32(0x16) as usize;
        let last = offsets.last().copied().unwrap_or(0) as usize;
        let end = if ext > last && ext <= data.len() { ext } else { data.len() };
        let mut component = [0u8; 3];
        component.copy_from_slice(&data[8..11]);
        Ok(MsgFile { data, component, first, count, version, offsets, end, country })
    }

    /// (class, text) of message `n`.
    pub fn get(&self, n: u16) -> Option<(u8, &'a [u8])> {
        let i = n.checked_sub(self.first)? as usize;
        let start = *self.offsets.get(i)? as usize;
        let end = self.offsets.get(i + 1).map_or(self.end, |&o| o as usize);
        if start >= end || end > self.data.len() {
            return None;
        }
        Some((self.data[start], &self.data[start + 1..end]))
    }

    /// Every message.
    pub fn messages(&self) -> Vec<Message> {
        (0..self.count)
            .filter_map(|i| {
                let n = self.first.checked_add(i)?;
                let (class, text) = self.get(n)?;
                Some(Message { number: n, class, text: text.to_vec() })
            })
            .collect()
    }

    /// The options the file was made with (version 2 files).
    pub fn options(&self) -> Options {
        let Some(c) = self.country else { return Options::default() };
        let d = &self.data[c..c + COUNTRY_SIZE];
        let r16 = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
        let n = (r16(7) as usize).min(MAX_CODE_PAGES);
        let name = &d[COUNTRY_NAME..];
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        Options {
            code_pages: (0..n).map(|i| r16(9 + i * 2)).collect(),
            language: (r16(3), r16(5)),
            dbcs: if d[0] == 2 { alloc::vec![(0x81, 0xFE)] } else { Vec::new() },
            name: name.to_vec(),
        }
    }
}

/// Source text for a message file (lines end with LF; MKMSGF and
/// [`compile`] read it back to the same messages).
pub fn decompile(data: &[u8]) -> Result<Vec<u8>, Error> {
    let f = MsgFile::parse(data)?;
    let o = f.options();
    let mut out = Vec::new();
    out.extend_from_slice(b"; ");
    out.extend_from_slice(&o.name);
    if !o.code_pages.is_empty() {
        let cps: Vec<String> = o.code_pages.iter().map(|c| format!("{c}")).collect();
        out.extend_from_slice(format!(" /P{}", cps.join(",")).as_bytes());
    }
    if o.language != (0, 0) {
        out.extend_from_slice(format!(" /L{},{}", o.language.0, o.language.1).as_bytes());
    }
    out.push(b'\n');
    out.extend_from_slice(&f.component);
    out.push(b'\n');
    for m in f.messages() {
        out.extend_from_slice(&f.component);
        out.extend_from_slice(format!("{:04}", m.number).as_bytes());
        out.push(m.class);
        out.extend_from_slice(b": ");
        let (text, prompt) = match m.text.strip_suffix(b"\r\n") {
            Some(t) => (t, false),
            None => (&m.text[..], true),
        };
        let mut first = true;
        let mut rest = text;
        loop {
            if !first {
                out.push(b'\n');
            }
            first = false;
            let cut = rest.windows(2).position(|w| w == b"\r\n");
            out.extend_from_slice(&rest[..cut.unwrap_or(rest.len())]);
            match cut {
                Some(c) => rest = &rest[c + 2..],
                None => break,
            }
        }
        if prompt {
            out.extend_from_slice(b"%0");
        }
        out.push(b'\n');
    }
    Ok(out)
}

/// Parse MKMSGF's switches (`/P850,437`, `/P 850`, `/L9,1`, `/D81`,
/// `/D129,159,224,252`; `-` works as `/`). Returns the arguments that are
/// not switches.
pub fn parse_switches(args: &[String], opts: &mut Options, verbose: &mut bool) -> Result<Vec<String>, Error> {
    let mut rest = Vec::new();
    let mut i = 0;
    let nums = |s: &str| -> Result<Vec<u16>, Error> {
        s.split([',', ' ']).filter(|t| !t.is_empty()).map(|t| t.trim().parse::<u16>().map_err(|_| error(0, format!("bad number {t}")))).collect()
    };
    while i < args.len() {
        let a = &args[i];
        // (An absolute Unix path is not a switch.)
        let sw = a.strip_prefix('/').filter(|s| !s.contains(['/', '\\'])).or_else(|| a.strip_prefix('-'));
        let Some(sw) = sw.filter(|s| !s.is_empty()) else {
            rest.push(a.clone());
            i += 1;
            continue;
        };
        let letter = sw.as_bytes()[0].to_ascii_uppercase();
        let mut value = String::from(&sw[1..]);
        if value.is_empty() && matches!(letter, b'P' | b'L' | b'D') {
            i += 1;
            value = args.get(i).cloned().ok_or_else(|| error(0, format!("/{} needs a value", letter as char)))?;
        }
        match letter {
            b'P' => opts.code_pages.extend(nums(&value)?),
            b'L' => {
                let v = nums(&value)?;
                let family = *v.first().ok_or_else(|| error(0, "/L needs a language id".into()))?;
                opts.language = (family, v.get(1).copied().filter(|&s| s != 0).unwrap_or(1));
            }
            b'D' => {
                let v = nums(&value)?;
                if v.len() == 1 {
                    opts.dbcs = dbcs_country(v[0]).ok_or_else(|| error(0, format!("{} is not a DBCS country", v[0])))?;
                } else if v.len() % 2 == 0 && v.iter().all(|&b| b <= 0xFF) {
                    opts.dbcs = v.chunks(2).map(|c| (c[0] as u8, c[1] as u8)).collect();
                } else {
                    return Err(error(0, "bad DBCS range".into()));
                }
            }
            b'V' => *verbose = true,
            _ => return Err(error(0, format!("unknown switch {a}"))),
        }
        i += 1;
    }
    Ok(rest)
}
