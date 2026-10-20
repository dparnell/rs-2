//! Tests on the IPF files of the OS/2 media (skipped when work/ does not
//! have them) and on made-up data.

use super::*;
use crate::layout::{layout, FontMetrics, Metrics, Options, RunKind};
use crate::text::{decode, Item, Target};
use std::path::PathBuf;
use std::string::String;
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
            RunKind::Bitmap { .. } => (r.y as usize, b"[bitmap]"),
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
