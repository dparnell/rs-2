//! `PMSEEK /SELFTEST`: the pattern and text matching, and searches of
//! the boot drive.

use alloc::vec::Vec;

use crate::search::{self, Event, Found, Matcher, Spec};

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    let mut t = Vec::new();
    t.extend_from_slice(if ok { b"PASS " } else { b"FAIL " });
    t.extend_from_slice(name.as_bytes());
    crate::trace(&t);
    if !ok {
        unsafe { FAILS += 1 };
    }
}

fn find_all(spec: &Spec) -> Vec<Found> {
    let mut v = Vec::new();
    search::run(spec, &mut |e| {
        if let Event::File(f) = e {
            v.push(f);
        }
        true
    });
    v
}

pub fn run() -> u32 {
    // Patterns.
    let (d, dir, name) = search::split_pattern(b"*.CMD");
    check("split *.CMD", d.is_none() && dir == b"\\" && name == b"*.CMD");
    let (d, dir, name) = search::split_pattern(b"c:\\os2\\*.exe");
    check("split c:\\os2\\*.exe", d == Some(b'C') && dir == b"\\os2\\" && name == b"*.exe");
    let (d, dir, name) = search::split_pattern(b"OS2/HELP/");
    check("split OS2/HELP/", d.is_none() && dir == b"\\OS2\\HELP\\" && name == b"*.*");

    // Text.
    check("contains", search::contains(b"Hello World", b"o W", false));
    check("contains case", !search::contains(b"Hello World", b"world", false) && search::contains(b"Hello World", b"world", true));
    check("contains empty", search::contains(b"abc", b"", false));
    let mut m = Matcher::new(b"needle", true, true);
    m.feed(b"first line\r\nsome NEE");
    m.feed(b"DLE here\r\nnothing\r\nneedle again");
    m.finish();
    check("matcher across chunks", m.found && m.lines.len() == 2 && m.lines[0] == b"some NEEDLE here" && m.lines[1] == b"needle again");
    let mut m = Matcher::new(b"needle", false, true);
    m.feed(b"some NEEDLE here\n");
    m.finish();
    check("matcher case", !m.found);
    let mut m = Matcher::new(b"xyz", false, false);
    let mut long = alloc::vec![b'a'; 4095];
    long.extend_from_slice(b"xy");
    m.feed(&long);
    m.feed(b"z");
    m.finish();
    check("matcher long line", m.found);

    // Lines.
    let f = Found { path: b"C:\\CONFIG.SYS".to_vec(), date: (46 << 9) | (10 << 5) | 24, time: (13 << 11) | (5 << 5), size: 1234, lines: Vec::new() };
    let l = search::entry_line(&f);
    check("entry line", l == b"10-24-26  13:05        1234  C:\\CONFIG.SYS");

    // Searching the boot drive.
    let boot = pmkit::boot_drive();
    let spec = |pattern: &[u8], text: &[u8], subdirs: bool, found: bool| Spec {
        drives: alloc::vec![boot],
        pattern: pattern.to_vec(),
        text: text.to_vec(),
        subdirs,
        ignore_case: true,
        found_text: found,
    };
    let r = find_all(&spec(b"CONFIG.SYS", b"libpath", false, true));
    for f in &r {
        crate::trace(&[&f.path[..], alloc::format!(" {} line(s)", f.lines.len()).as_bytes()].concat());
    }
    let all = find_all(&spec(b"CONFIG.SYS", b"", false, false));
    crate::trace(alloc::format!("{} CONFIG.SYS without text", all.len()).as_bytes());
    let cfg = [boot, b':', b'\\'].iter().copied().chain(b"CONFIG.SYS".iter().copied()).collect::<Vec<u8>>();
    check("CONFIG.SYS with LIBPATH", r.len() == 1 && r[0].path.eq_ignore_ascii_case(&cfg) && r[0].lines.iter().any(|l| search::contains(l, b"LIBPATH", false)));
    let r = find_all(&spec(b"CONFIG.SYS", b"no such text 4711", false, false));
    check("text not there", r.is_empty());
    let r = find_all(&spec(b"PMSEEK.HLP", b"", true, false));
    for f in &r {
        crate::trace(&f.path);
    }
    check("subdirectories (PMSEEK.HLP)", r.iter().any(|f| f.path.ends_with(b"\\OS2\\HELP\\PMSEEK.HLP")));
    let r = find_all(&spec(b"\\OS2\\HELP\\*.HLP", b"", false, false));
    check("directory in pattern", r.len() > 1 && r.iter().all(|f| f.path.len() > 12 && f.path[2..12].eq_ignore_ascii_case(b"\\OS2\\HELP\\")));

    let fails = unsafe { FAILS };
    crate::trace(alloc::format!("pmseek selftest done: {fails} failure(s)").as_bytes());
    fails
}
