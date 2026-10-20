//! Tests on the IPF files of the OS/2 media (skipped when work/ does not
//! have them) and on made-up data.

use super::*;
use crate::layout::{layout, FontMetrics, Metrics, Options, RunKind};
use crate::text::{decode, Item, Target};
use std::path::PathBuf;
use std::string::String;
use std::vec;
use std::vec::Vec;

fn media(rel: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work").join(rel);
    std::fs::read(p).ok()
}

const EHXHP: &str = "unpacked/disk_2/OS2/HELP/EHXHP.HLP";
const REXX: &str = "unpacked/disk_17/OS2/BOOK/REXX.INF";
const CMDREF: &str = "unpacked/disk_18/OS2/BOOK/CMDREF.INF";

/// One character cell per character: lays panels out as text.
struct Grid;

impl Metrics for Grid {
    fn system(&mut self) -> FontMetrics {
        FontMetrics { height: 1, space: 1, descent: 0 }
    }
    fn font(&mut self, _: u8, _: u8) -> FontMetrics {
        FontMetrics { height: 1, space: 1, descent: 0 }
    }
    fn width(&mut self, _: u8, _: u8, w: &[u8]) -> i32 {
        w.len() as i32
    }
}

fn render(doc: &Doc<'_>, toc: u16, width: i32) -> Vec<String> {
    let page = layout(doc, toc, &Options { width, keys: b"" }, &mut Grid);
    let mut rows: Vec<Vec<u8>> = Vec::new();
    for r in &page.runs {
        let (y, text): (usize, &[u8]) = match r.kind {
            RunKind::Word(w) => (r.y as usize, w),
            RunKind::Bitmap { .. } | RunKind::Image { .. } => (r.y as usize, b"[bitmap]"),
        };
        if rows.len() <= y {
            rows.resize(y + 1, Vec::new());
        }
        let row = &mut rows[y];
        let x = r.x.max(0) as usize;
        if row.len() < x + text.len() {
            row.resize(x + text.len(), b' ');
        }
        row[x..x + text.len()].copy_from_slice(text);
    }
    rows.iter().map(|r| latin1(r).trim_end().into()).collect()
}

#[test]
fn ehxhp_structures() {
    let Some(d) = media(EHXHP) else { return };
    let doc = Doc::parse(&d).unwrap();
    assert!(doc.is_help());
    assert_eq!(doc.title(), b"System Editor Help");
    assert_eq!(doc.header.toc_count, 114);
    for i in 0..doc.header.toc_count {
        doc.toc(i).unwrap();
    }
    for i in 0..doc.header.cell_count {
        doc.cell(i).unwrap();
    }
    // Resource ids from the spec's validation run.
    assert_eq!(doc.panel_by_res(1091), Some(12));
    assert_eq!(doc.panel_by_res(4144), Some(0));
    assert_eq!(doc.res_of_panel(12), Some(1091));
    assert_eq!(doc.databases(), [b"\\OS2\\HELP\\WPINDEX.HLP".to_vec()]);
    assert_eq!(doc.fonts()[0].face, b"System Proportional");
    assert!(!doc.index().is_empty());
}

#[test]
fn ehxhp_first_panel_text() {
    let Some(d) = media(EHXHP) else { return };
    let doc = Doc::parse(&d).unwrap();
    let toc = doc.panel_by_res(4144).unwrap();
    let rows = render(&doc, toc, 64);
    let text: Vec<&str> = rows.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
    assert_eq!(text[0], "  Use the OS/2* System Editor as a text editor to create and");
    assert_eq!(text[1], "  edit your CONFIG.SYS and AUTOEXEC.BAT system files.  You can");
    // The OS/2 link covers the glued words.
    let page = layout(&doc, toc, &Options { width: 64, keys: b"" }, &mut Grid);
    assert_eq!(page.links[0].target, Target::Panel(1));
    let linked: Vec<_> = page.runs.iter().filter(|r| r.link == Some(0)).collect();
    assert_eq!(linked.len(), 4);
}

#[test]
fn rexx_example_and_note() {
    let Some(d) = media(REXX) else { return };
    let doc = Doc::parse(&d).unwrap();
    // The panel holding cell 18 (SELECT).
    let toc = (0..doc.header.toc_count).find(|&t| doc.toc(t).unwrap().cells.contains(&18)).unwrap();
    let rows = render(&doc, toc, 64).join("\n");
    assert!(rows.contains("\n  SELECT\n     WHEN expression1\n        THEN instruction1"), "{rows}");
    assert!(rows.contains("  Note:    An IF-THEN instruction cannot be used with a SELECT"), "{rows}");
    assert!(rows.contains("[bitmap]"));
}

#[test]
fn every_bitmap_decodes() {
    for f in [REXX, CMDREF] {
        let Some(d) = media(f) else { continue };
        let doc = Doc::parse(&d).unwrap();
        let mut n = 0;
        for c in 0..doc.header.cell_count {
            let cell = doc.cell(c).unwrap();
            for it in decode(&doc, &cell) {
                if let Item::Bitmap { offset, .. } = it {
                    let b = bitmap::decode(&d, doc.header.image_start, offset, (0, 0)).unwrap();
                    assert_eq!(b.bits.len(), b.row_bytes() * b.cy as usize);
                    n += 1;
                }
            }
        }
        assert!(n > 0, "{f}");
    }
}

#[test]
fn rexx_bitmap_has_content() {
    let Some(d) = media(REXX) else { return };
    let doc = Doc::parse(&d).unwrap();
    let b = bitmap::decode(&d, doc.header.image_start, 0, (0, 0)).unwrap();
    assert_eq!((b.cx, b.cy, b.bpp), (91, 35, 4));
    // Not a blank picture: several colours are used.
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..b.cy {
        for x in 0..b.cx {
            seen.insert(b.pixel(x, y));
        }
    }
    assert!(seen.len() >= 2);
}

#[test]
fn search_table_matches_panels() {
    let Some(d) = media(EHXHP) else { return };
    let doc = Doc::parse(&d).unwrap();
    // "Editor" is on the first panel.
    let w = doc.find_word(b"editor").unwrap();
    let panels = doc.panels_with_word(w).unwrap();
    assert!(panels.contains(&doc.panel_by_res(4144).unwrap()));
    for p in panels {
        let t = latin1(&text::plain(&doc, p)).to_ascii_lowercase();
        assert!(t.contains("editor"), "panel {p}");
    }
}

#[test]
fn panel_names() {
    let Some(d) = media(CMDREF) else { return };
    let doc = Doc::parse(&d).unwrap();
    assert!(!doc.is_help());
    for e in doc.index().iter().take(50) {
        doc.toc(e.toc).unwrap();
    }
}

#[test]
fn lzw_literals_and_repeats() {
    // Codes 9 bits MSB first: 'a', 'b', 258 ("ab"), 260 ("ba"? no: next is
    // 259 = "ba", 260 = "aba"), end.
    let codes = [b'a' as u16, b'b' as u16, 258, 257];
    let mut bits = Vec::new();
    let (mut acc, mut n) = (0u32, 0);
    for c in codes {
        acc = acc << 9 | c as u32;
        n += 9;
        while n >= 8 {
            bits.push((acc >> (n - 8)) as u8);
            n -= 8;
        }
    }
    bits.push((acc << (8 - n)) as u8);
    let mut out = [0u8; 8];
    let n = bitmap::lzw(&bits, &mut out);
    assert_eq!(&out[..n], b"abab");
}

#[test]
fn rle_forms() {
    let src = [0x02, 7, 0x81, 1, 2, 0x80, 0x01, 2, 0, 0x80, 0x02, 9, 8];
    let mut out = [0u8; 32];
    let n = bitmap::rle(&src, &mut out);
    assert_eq!(&out[..n], [7, 7, 7, 1, 2, 0xff, 0xff, 0xff, 9, 8, 9, 8, 9, 8]);
}

#[test]
fn bad_files() {
    assert_eq!(Doc::parse(b"MZ").err(), Some(Error::BadMagic));
    let mut h = std::vec![0u8; 0x9b];
    h[..3].copy_from_slice(b"HSP");
    assert!(Doc::parse(&h).is_err());
}

/// Every INF/HLP file on the media: every panel lays out and every
/// bitmap decodes.
#[test]
fn whole_media() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work");
    let mut stack = std::vec![root.join("unpacked"), root.join("media/boot/toolkit/os2tk45/book")];
    let (mut files, mut panels, mut bitmaps) = (0, 0, 0);
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().map(|x| x.to_ascii_lowercase());
            if ext.as_deref() != Some("inf".as_ref()) && ext.as_deref() != Some("hlp".as_ref()) {
                continue;
            }
            let d = std::fs::read(&p).unwrap();
            let Ok(doc) = Doc::parse(&d) else { continue };
            files += 1;
            for t in 0..doc.header.toc_count {
                let page = layout(&doc, t, &Options { width: 400, keys: b"" }, &mut Grid);
                panels += 1;
                for r in &page.runs {
                    if let RunKind::Bitmap { offset, .. } = r.kind {
                        bitmap::decode(&d, doc.header.image_start, offset, (0, 0))
                            .unwrap_or_else(|e| panic!("{}: bitmap {offset:#x}: {e:?}", p.display()));
                        bitmaps += 1;
                    }
                }
            }
            doc.index();
        }
    }
    std::eprintln!("{files} files, {panels} panels, {bitmaps} bitmaps");
}

#[test]
fn compile_round_trip() {
    let src = b":userdoc.\n:title.Test Help\n.* a comment\n:h1 res=4144 id=main.Main panel\n:i1.Main things\n:p.Use :hp2.Save:ehp2., then quit&period.\n:ul.\n:li.One\n:li.Two :link reftype=hd refid=second.second:elink.\n:eul.\n:note.Careful.\n:h2 res=0x1360 id=second.Second\n:p.See :link reftype=hd res=4144.main:elink.&colon.\n:dl tsize=8.\n:dt.Key\n:dd.Does it\n:edl.\n:euserdoc.\n";
    let f = crate::compile::compile(src, true).unwrap();
    let doc = Doc::parse(&f).unwrap();
    assert!(doc.is_help());
    assert_eq!(doc.title(), b"Test Help");
    assert_eq!(doc.panel_by_res(4144), Some(0));
    assert_eq!(doc.panel_by_res(0x1360), Some(1));
    let t = doc.toc(0).unwrap();
    assert_eq!(t.title, b"Main panel");
    assert!(t.has_children());
    assert_eq!(doc.toc(1).unwrap().level(), 2);
    let text = latin1(&crate::text::plain(&doc, 0));
    assert!(text.contains("Use Save, then quit."), "{text:?}");
    assert!(text.contains("\x07One"), "{text:?}");
    let items = decode(&doc, &doc.cell(0).unwrap());
    assert!(items.contains(&Item::Link { target: Target::Panel(1), data: None }));
    assert_eq!(doc.index()[0].text, b"Main things");
    let w = doc.find_word(b"careful").unwrap();
    assert_eq!(doc.panels_with_word(w), Some(std::vec![0]));
    let page = render(&doc, 1, 40);
    assert!(page.iter().any(|l| l.trim_start().starts_with("Key     Does it")), "{page:?}");
    assert!(page.iter().any(|l| l.contains("See main:")), "{page:?}");
}

#[test]
fn compile_errors() {
    let e = crate::compile::compile(b":h1.A\n:p.:link reftype=hd res=9.x:elink.\n", true).unwrap_err();
    assert_eq!(e.line, 2);
    assert!(crate::compile::compile(b":h1.A\n:frob.\n", true).is_err());
}

/// Dynamic data as the Ddf* functions write it: words as 0x66 escapes,
/// one space per 0xFE, no autospacing; a link by name and a font.
#[test]
fn ddf_buffer() {
    use crate::text::{decode_ddf, expand_ddf};
    let word = |w: &[u8]| {
        let mut v = vec![0xff, w.len() as u8 + 2, 0x66];
        v.extend_from_slice(w);
        v
    };
    let mut b = Vec::new();
    b.extend(word(b"Hello"));
    b.extend(word(b","));
    b.push(0xfe);
    b.extend(word(b"world"));
    b.push(0xfa);
    b.extend([0xff, 0x08, 0x65]);
    b.extend_from_slice(b"there\0");
    b.extend(word(b"link"));
    b.extend([0xff, 0x02, 0x08]);
    b.extend([0xff, 12, 0x67, 10, 0, 12, 0, 5]);
    b.extend_from_slice(b"Times");
    b.extend([0xff, 0x14, 0x69]);
    b.extend([0u8; 16]);
    b.extend(7u32.to_le_bytes());
    b.extend(word(b"end"));
    let items = decode_ddf(&b);
    assert_eq!(items[0], Item::Word(b"Hello"));
    assert_eq!(items[1], Item::Word(b","));
    assert_eq!(items[2], Item::Space);
    assert_eq!(items[4], Item::Para);
    assert_eq!(items[5], Item::Link { target: Target::Name(b"there"), data: None });
    assert_eq!(items[8], Item::FaceFont { face: b"Times", width: 10, height: 12 });
    assert_eq!(items[9], Item::Metafile { handle: 7, rect: [0; 4] });
    assert_eq!(items[10], Item::Word(b"end"));

    // Laid out: no space after "Hello", one before "world".
    let data = crate::compile::compile(b":userdoc.\n:h1 res=1.A\n:p.x\n:euserdoc.\n", true).unwrap();
    let doc = Doc::parse(&data).unwrap();
    let mut cells = vec![vec![Item::Word(b"Text"), Item::Ddf(5), Item::Word(b"after")]];
    expand_ddf(&mut cells, |r| (r == 5).then_some(&b[..]));
    let page = crate::layout::layout_items(&doc, &cells, &Options { width: 60, keys: b"" }, &mut Grid);
    let words: Vec<(i32, &[u8])> = page
        .runs
        .iter()
        .filter_map(|r| match r.kind {
            RunKind::Word(w) => Some((r.x, w)),
            _ => None,
        })
        .collect();
    let x = |w: &[u8]| words.iter().find(|(_, t)| *t == w).unwrap().0;
    assert_eq!(x(b","), x(b"Hello") + 5);
    assert_eq!(x(b"world"), x(b",") + 2);
    assert_eq!(page.links.len(), 1);
}

/// The compiler writes panel names (`id=`) and fonts (`:font.`).
#[test]
fn compile_names_and_fonts() {
    let src = b":userdoc.\n:h1 res=1.A\n:p.:font facename=Helv size=14x10.x\n:h1 res=2 id=Second.B\n:p.y\n:euserdoc.\n";
    let data = crate::compile::compile(src, true).unwrap();
    let doc = Doc::parse(&data).unwrap();
    assert_eq!(doc.panel_by_name(b"second"), Some(1));
    assert_eq!(doc.panel_by_name(b"x"), None);
    let f = doc.fonts();
    assert_eq!(f.len(), 2);
    assert_eq!((f[1].face.as_slice(), f[1].height, f[1].width), (&b"Helv"[..], 14, 10));
}

/// Window data of links and headings, compiled and read back.
#[test]
fn compile_window_data() {
    let src = b":userdoc.\n:h1 res=1.A\n:p.:link reftype=hd res=2 vpx=right vpy=top vpcx=40% vpcy=50% group=3 dependent.w:elink.\n:h1 res=2 x=left y=bottom width=50% height=20c group=7.B\n:p.y\n:euserdoc.\n";
    let data = crate::compile::compile(src, true).unwrap();
    let doc = Doc::parse(&data).unwrap();
    let items = crate::text::panel(&doc, 0);
    let d = items.iter().find_map(|i| match i {
        Some(Item::Link { data, .. }) => *data,
        _ => None,
    });
    let d = d.unwrap();
    assert_eq!(d.flags, 0x603);
    assert_eq!(d.origin, Some((0x44, 2, 4)));
    assert_eq!(d.size, Some((0x11, 40, 50)));
    assert_eq!(d.group, Some(3));
    let t = doc.toc(1).unwrap();
    assert_eq!(t.ext, 0x403);
    assert_eq!(t.origin, Some(WinCoord { units: 0x44, x: 1, y: 8 }));
    assert_eq!(t.size, Some(WinCoord { units: 0x10, x: 50, y: 20 }));
    assert_eq!(t.group, Some(7));
    assert_eq!(t.title, b"B");
}

/// `:acviewport.` as IBM's compiler wrote it in the Toolkit's IPF sample,
/// and as rs2's writes it.
#[test]
fn acviewport() {
    let ibm = [0xff, 0x1f, 0x21, 0x00, 0x0d, 0x01, 0x00, 0x08, b'I', b'P', b'F', b'M', b'a', b'i', b'n', 0x04, b'i', b'p', b'f', 0x01, 0xc3, 0x00, 0x44, 0x02, 0x00, 0x04, 0x00, 0x11, 0x37, 0x00, 0x64, 0x00];
    let items = crate::text::decode_ddf(&ibm);
    let Some(Item::AcViewport(a)) = items.first() else { panic!("{items:?}") };
    assert_eq!((a.objid, a.name, a.dll, a.info), (1, &b"IPFMain"[..], &b"ipf"[..], &b""[..]));
    let d = a.data.unwrap();
    assert_eq!((d.flags, d.origin, d.size), (0xc3, Some((0x44, 2, 4)), Some((0x11, 55, 100))));
    let src = b":userdoc.\n:h1 res=1.A\n:acviewport dll='ipf' objectname='IPFMain' objectid=1 vpx=right vpy=top vpcx=55% vpcy=100%.\n:euserdoc.\n";
    let data = crate::compile::compile(src, true).unwrap();
    let doc = Doc::parse(&data).unwrap();
    let items = crate::text::panel(&doc, 0);
    let a = items.iter().find_map(|i| match i {
        Some(Item::AcViewport(a)) => Some(a.clone()),
        _ => None,
    });
    let a = a.unwrap();
    assert_eq!((a.objid, a.name, a.dll), (1, &b"IPFMain"[..], &b"ipf"[..]));
    assert_eq!(a.data.unwrap().size, Some((0x11, 55, 100)));
}
