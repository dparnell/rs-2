//! The search itself: walk the drives' directories for files matching a
//! name pattern (DosFindFirst does the matching, as for IBM's PMSEEK),
//! optionally containing a text, line by line.

use alloc::vec::Vec;

/// What to look for.
pub struct Spec {
    /// The drives to search (letters).
    pub drives: Vec<u8>,
    /// The file name pattern, possibly with a directory (`\OS2\*.EXE`)
    /// or a drive (`C:\*.SYS`, which searches only that drive).
    pub pattern: Vec<u8>,
    /// The text to find in the files (empty: any file).
    pub text: Vec<u8>,
    pub subdirs: bool,
    pub ignore_case: bool,
    /// Collect the lines holding the text.
    pub found_text: bool,
}

/// One file found.
pub struct Found {
    pub path: Vec<u8>,
    pub date: u16,
    pub time: u16,
    pub size: u32,
    /// The lines holding the text (with "Display found text").
    pub lines: Vec<Vec<u8>>,
}

/// What the search reports as it goes; returning false stops it.
pub enum Event<'a> {
    Dir(&'a [u8]),
    File(Found),
    Error(&'a [u8], u32),
}

/// At most this many lines of found text per file.
pub const MAX_LINES: usize = 50;
/// Found text is cut to this length.
pub const MAX_LINE: usize = 160;

/// Split a pattern into (drive, directory with a trailing backslash,
/// name pattern). `*.CMD` -> (None, "\", "*.CMD").
pub fn split_pattern(p: &[u8]) -> (Option<u8>, Vec<u8>, Vec<u8>) {
    let mut p = p;
    let mut drive = None;
    if p.len() >= 2 && p[1] == b':' && p[0].is_ascii_alphabetic() {
        drive = Some(p[0].to_ascii_uppercase());
        p = &p[2..];
    }
    let (dir, name) = match p.iter().rposition(|&c| c == b'\\' || c == b'/') {
        Some(i) => (&p[..=i], &p[i + 1..]),
        None => (&b""[..], p),
    };
    let mut d: Vec<u8> = dir.iter().map(|&c| if c == b'/' { b'\\' } else { c }).collect();
    if d.first() != Some(&b'\\') {
        d.insert(0, b'\\');
    }
    let name = if name.is_empty() { b"*.*".to_vec() } else { name.to_vec() };
    (drive, d, name)
}

fn upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

/// Does `hay` contain `needle`?
pub fn contains(hay: &[u8], needle: &[u8], ignore_case: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.len() < needle.len() {
        return false;
    }
    'outer: for i in 0..=hay.len() - needle.len() {
        for j in 0..needle.len() {
            let (a, b) = (hay[i + j], needle[j]);
            if a != b && !(ignore_case && upper(a) == upper(b)) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

/// Line-by-line matcher fed in chunks (a match never spans lines).
pub struct Matcher<'a> {
    text: &'a [u8],
    ignore_case: bool,
    all: bool,
    line: Vec<u8>,
    pub found: bool,
    pub lines: Vec<Vec<u8>>,
}

impl<'a> Matcher<'a> {
    pub fn new(text: &'a [u8], ignore_case: bool, all: bool) -> Self {
        Matcher { text, ignore_case, all, line: Vec::new(), found: false, lines: Vec::new() }
    }

    /// Done (no need to read more)?
    pub fn done(&self) -> bool {
        self.found && (!self.all || self.lines.len() >= MAX_LINES)
    }

    fn end_line(&mut self) {
        if contains(&self.line, self.text, self.ignore_case) {
            self.found = true;
            if self.all && self.lines.len() < MAX_LINES {
                let l: Vec<u8> = self.line.iter().take(MAX_LINE).map(|&c| if c < 0x20 { b' ' } else { c }).collect();
                self.lines.push(l);
            }
        }
        self.line.clear();
    }

    pub fn feed(&mut self, data: &[u8]) {
        for &c in data {
            if self.done() {
                return;
            }
            match c {
                b'\n' | b'\r' | 0 | 0x1a => self.end_line(),
                _ => {
                    self.line.push(c);
                    // A very long "line" (a binary file): look at it, and
                    // keep its tail in case the text starts there.
                    if self.line.len() >= 4096 {
                        let keep = self.text.len().saturating_sub(1).min(self.line.len());
                        let tail = self.line[self.line.len() - keep..].to_vec();
                        if contains(&self.line, self.text, self.ignore_case) {
                            self.end_line();
                        }
                        self.line = tail;
                    }
                }
            }
        }
    }

    pub fn finish(&mut self) {
        if !self.line.is_empty() && !self.done() {
            self.end_line();
        }
    }
}

/// Look for the text in a file.
fn scan_file(path: &[u8], spec: &Spec) -> Option<Vec<Vec<u8>>> {
    let p = pmkit::z(path);
    let (mut h, mut action) = (0u32, 0u32);
    // OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, OPEN_FLAGS_FAIL_ON_ERROR.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x2040, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut m = Matcher::new(&spec.text, spec.ignore_case, spec.found_text);
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let mut got = 0;
        if unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        m.feed(&buf[..got as usize]);
        if m.done() {
            break;
        }
    }
    unsafe { os2::DosClose(h) };
    m.finish();
    m.found.then_some(m.lines)
}

/// The entries of `spec` (a path with wildcards): (name, attr, date,
/// time, size).
fn find(spec: &[u8], attr: u32) -> Vec<(Vec<u8>, u32, u16, u16, u32)> {
    let mut out = Vec::new();
    let s = pmkit::z(spec);
    let mut hdir: u32 = 0xffff_ffff; // HDIR_CREATE
    let mut buf = alloc::vec![0u8; 8192];
    let mut count: u32 = 64;
    let mut rc = unsafe { os2::DosFindFirst(s.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    while rc == 0 && count > 0 {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = pmkit::rd32(e, 0) as usize;
            // FILEFINDBUF3: the write date and time at 12, the size at
            // 16, the attributes at 24, the name's length and the name.
            let date = pmkit::rd16(e, 12);
            let time = pmkit::rd16(e, 14);
            let size = pmkit::rd32(e, 16);
            let a = pmkit::rd32(e, 24);
            let n = e[28] as usize;
            out.push((e[29..29 + n].to_vec(), a, date, time, size));
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(hdir) };
    out
}

/// Run a search; the number of files found.
pub fn run(spec: &Spec, report: &mut dyn FnMut(Event) -> bool) -> u32 {
    let (only, dir, name) = split_pattern(&spec.pattern);
    let drives: Vec<u8> = match only {
        Some(d) => alloc::vec![d],
        None => spec.drives.clone(),
    };
    let mut n = 0;
    for d in drives {
        let mut stack: Vec<Vec<u8>> = alloc::vec![[&[d, b':'][..], &dir].concat()];
        let mut first = true;
        while let Some(path) = stack.pop() {
            if !report(Event::Dir(&path)) {
                return n;
            }
            // Normal, read-only, hidden and system files.
            let files = find(&[&path[..], &name].concat(), 0x07);
            if first && files.is_empty() {
                // Is the drive there at all?
                let probe = pmkit::z(&[d, b':', b'\\']);
                let mut info = [0u8; 64];
                let rc = unsafe { os2::DosQueryPathInfo(probe.as_ptr(), 1, info.as_mut_ptr(), info.len() as u32) };
                if rc != 0 && rc != 87 && !report(Event::Error(&path, rc)) {
                    return n;
                }
            }
            first = false;
            for (fname, attr, date, time, size) in files {
                if attr & os2::FILE_DIRECTORY != 0 {
                    continue;
                }
                let full = [&path[..], &fname].concat();
                let lines = if spec.text.is_empty() {
                    Vec::new()
                } else {
                    match scan_file(&full, spec) {
                        Some(l) => l,
                        None => continue,
                    }
                };
                n += 1;
                if !report(Event::File(Found { path: full, date, time, size, lines })) {
                    return n;
                }
            }
            if spec.subdirs {
                // MUST_HAVE_DIRECTORY plus hidden and system ones.
                let mut dirs: Vec<Vec<u8>> = find(&[&path[..], b"*"].concat(), 0x1016).into_iter().filter(|e| e.1 & os2::FILE_DIRECTORY != 0 && e.0 != b"." && e.0 != b"..").map(|e| e.0).collect();
                // Popped from the end: push in reverse to walk in order.
                dirs.reverse();
                for sub in dirs {
                    let mut p = [&path[..], &sub].concat();
                    p.push(b'\\');
                    stack.push(p);
                }
            }
        }
    }
    n
}

/// A listbox line for a file: date, time, size and path.
pub fn entry_line(f: &Found) -> Vec<u8> {
    let (day, month, year) = (f.date & 31, (f.date >> 5) & 15, 1980 + (f.date >> 9) as u32);
    let (min, hour) = ((f.time >> 5) & 63, f.time >> 11);
    let mut l = alloc::format!("{:02}-{:02}-{:02}  {:02}:{:02}  {:>10}  ", month, day, year % 100, hour, min, f.size).into_bytes();
    l.extend_from_slice(&f.path);
    l
}

/// The prefix of a line of found text.
pub const FOUND_PREFIX: &[u8] = b"-------> ";
