//! Tests on made-up sources and, when work/ has them, on the message
//! files of the OS/2 media (decompiled and compiled again, they must come
//! out byte for byte as IBM's MKMSGF wrote them).

use super::*;
use std::path::PathBuf;
use std::vec;

const SAMPLE: &[u8] = b"; a comment\r\n\
MAB\r\n\
MAB0100E: File not found\r\n\
MAB0101?:\r\n\
MAB0102H: Usage: del [drive:][path] filename\r\n\
; another comment\r\n\
MAB0104I: %1 files copied\r\n\
in two lines\r\n\
MAB0108P: Do you wish to apply these patches (Y or N)? %0\r\n\
\x1a";

#[test]
fn source() {
    let s = parse(SAMPLE, &[]).unwrap();
    assert_eq!(&s.component, b"MAB");
    let n: Vec<u16> = s.messages.iter().map(|m| m.number).collect();
    assert_eq!(n, vec![100, 101, 102, 103, 104, 105, 106, 107, 108]);
    assert_eq!(s.messages[0].class, b'E');
    assert_eq!(s.messages[0].text, b"File not found\r\n");
    assert_eq!(s.messages[1].class, b'?');
    assert_eq!(s.messages[1].text, b"\r\n");
    assert_eq!(s.messages[3].class, b'?');
    assert_eq!(s.messages[4].text, b"%1 files copied\r\nin two lines\r\n");
    assert_eq!(s.messages[8].text, b"Do you wish to apply these patches (Y or N)? ");
}

#[test]
fn binary() {
    let opts = Options { code_pages: vec![850, 437], language: (9, 1), name: b"mab.msg".to_vec(), ..Default::default() };
    let d = compile(SAMPLE, &opts).unwrap();
    assert_eq!(&d[..8], MAGIC);
    assert_eq!(d[0x0F], 1);
    let f = MsgFile::parse(&d).unwrap();
    assert_eq!((f.first, f.count, &f.component), (100, 9, b"MAB"));
    assert_eq!(f.get(104), Some((b'I', &b"%1 files copied\r\nin two lines\r\n"[..])));
    // The last message stops at the extended header.
    assert_eq!(f.get(108).unwrap().1, b"Do you wish to apply these patches (Y or N)? ");
    assert_eq!(f.get(99), None);
    assert_eq!(f.get(109), None);
    let o = f.options();
    assert_eq!((o.code_pages, o.language, o.name), (vec![850, 437], (9, 1), b"mab.msg".to_vec()));
    // Decompiled and compiled again: the same file.
    let src = decompile(&d).unwrap();
    assert_eq!(compile(&src, &opts).unwrap(), d);
}

#[test]
fn errors() {
    assert!(parse(b"AB\r\nAB0001E: x\r\n", &[]).is_err());
    let e = parse(b"ABC\r\nABC0002E: x\r\nABC0001E: y\r\n", &[]).unwrap_err();
    assert_eq!(e.line, 3);
    assert!(parse(b"ABC\r\nstray text\r\n", &[]).is_err());
}

#[test]
fn switches() {
    let args: Vec<String> = ["in.txt", "/P850,437", "-L", "9", "/D81", "out.msg", "/v"].iter().map(|s| String::from(*s)).collect();
    let mut o = Options::default();
    let mut v = false;
    let rest = parse_switches(&args, &mut o, &mut v).unwrap();
    assert_eq!(rest, vec!["in.txt", "out.msg"]);
    assert_eq!(o.code_pages, vec![850, 437]);
    assert_eq!(o.language, (9, 1));
    assert_eq!(o.dbcs, vec![(0x81, 0x9F), (0xE0, 0xFC)]);
    assert!(v);
}

/// A trailing `%` byte of a DBCS character is not the `%0` switch.
#[test]
fn dbcs_prompt() {
    let src = b"ABC\r\nABC0001P: \x81%0\r\n";
    assert_eq!(parse(src, &[(0x81, 0x9F)]).unwrap().messages[0].text, b"\x81%0\r\n");
    assert_eq!(parse(src, &[]).unwrap().messages[0].text, b"\x81");
}

fn media_files() -> Vec<PathBuf> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // The repository's work/, or the main checkout's from a worktree
    // (.claude/worktrees/NAME).
    let roots = [here.join("../../work/unpacked"), here.join("../../../../../work/unpacked")];
    let Some(root) = roots.iter().find(|r| r.is_dir()) else { return Vec::new() };
    let mut v = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("msg")) {
                v.push(p);
            }
        }
    }
    v
}

/// IBM's files of the current layout (version 2, the extended header
/// when there are several code pages) come out the same after a round
/// trip through the source; every file at least reads back with the
/// same messages.
#[test]
fn media_round_trip() {
    let mut same = 0;
    for p in media_files() {
        let d = std::fs::read(&p).unwrap();
        // (Some files named .MSG are not message files.)
        let Ok(f) = MsgFile::parse(&d) else { continue };
        let src = decompile(&d).unwrap();
        let o = f.options();
        let again = compile(&src, &o).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let g = MsgFile::parse(&again).unwrap();
        // Old MKMSGFs kept a Ctrl+Z at the end of the last message.
        let strip = |mut m: Vec<Message>| {
            if let Some(l) = m.last_mut() {
                if l.text.ends_with(b"\x1a") {
                    l.text.pop();
                }
            }
            m
        };
        let (a, b) = (strip(f.messages()), strip(g.messages()));
        if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
            panic!("{}: message {i}: {:?} / {:?}", p.display(), a.get(i).map(|m| String::from_utf8_lossy(&m.text)), b.get(i).map(|m| String::from_utf8_lossy(&m.text)));
        }
        let extended = u32::from_le_bytes([d[0x16], d[0x17], d[0x18], d[0x19]]) != 0;
        if f.version == 2 && (extended == (o.code_pages.len() > 1)) && !d.ends_with(b"\x1a") {
            assert_eq!(again, d, "{}", p.display());
            same += 1;
        }
    }
    std::eprintln!("{same} files identical");
}
