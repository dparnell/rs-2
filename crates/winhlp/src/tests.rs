//! Tests on Windows 3.x help files (skipped when work/ does not have
//! them) and on made-up data.

use super::*;
use ipf::layout::{layout, FontMetrics, Metrics, Options as LayoutOptions, RunKind};
use ipf::text::Target;
use ipf::Doc;
use std::path::PathBuf;
use std::string::String;
use std::vec;
use std::vec::Vec;

/// A file under work/win16apps: this checkout's work/, else (in a
/// worktree under .claude/worktrees) the main checkout's, else
/// $RS2_WORK's.
fn media(rel: &str) -> Option<Vec<u8>> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut roots = vec![here.join("work"), here.join("../../../work")];
    if let Ok(w) = std::env::var("RS2_WORK") {
        roots.push(PathBuf::from(w));
    }
    roots.iter().find_map(|r| std::fs::read(r.join("win16apps").join(rel)).ok())
}

const BIN: &str = "delphi/RUNIMAGE/DELPHI/BIN/";

/// Every help file the tests know of.
const FILES: &[&str] = &[
    "delphi/RUNIMAGE/DELPHI/BIN/CWG.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/CWH.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/DELPHI.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/HELPEX.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/HELPREF.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/LOCALSQL.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/MMSYSTEM.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/WINAPI.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/WINSIGHT.HLP",
    "delphi/RUNIMAGE/DELPHI/BIN/WINSPCTR.HLP",
    "delphi_cd/RUNIMAGE/RS_RUN/RS_RUN.HLP",
    "delphi_cd/RUNIMAGE/DBD/DBD.HLP",
    "delphi_cd/RUNIMAGE/DBD/DBDLOCAL.HLP",
    "delphi_cd/RUNIMAGE/RPTSMITH/SBL.HLP",
    "delphi_cd/RUNIMAGE/RPTSMITH/RPTSMITH.HLP",
    "delphi_cd/RUNIMAGE/IDAPI/BDECFG.HLP",
    "delphi_cd/RUNIMAGE/IDAPI/CONNECT.HLP",
    "delphi_cd/RUNIMAGE/IDAPI/SQLD_IB.HLP",
    "delphi_cd/RUNIMAGE/IBLOCAL/BIN/BLINT04.HLP",
    "delphi_cd/RUNIMAGE/IBLOCAL/BIN/SVRMGR.HLP",
    "delphi_cd/RUNIMAGE/IBLOCAL/BIN/WISQL.HLP",
    "delphi_cd/RUNIMAGE/IBLOCAL/BIN/COMDIAG.HLP",
    "delphi_cd/RUNIMAGE/IBLOCAL/BIN/SQLREF.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVACCSS.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVBTRV.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVEXCEL.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVDBASE.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/BIPDX04.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/ODBCINST.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVFOX.HLP",
    "delphi_cd/RUNIMAGE/WINDOWS/SYSTEM/DRVTEXT.HLP",
    "delphi_cd/RUNIMAGE/DELPHI/DEMOS/USERHELP/STEREO.HLP",
    "delphi_cd/RUNIMAGE/DELPHI/DEMOS/DB/MASTAPP/MASTAPP.HLP",
    "dl/pfe/x/PFE.HLP",
    "dl/calmira22/x/CALMIRA.HLP",
    "cica93/x/GANDER.HLP",
    "cica93/x/WIN_NT/SOCK_API/WINSOCK.HLP",
    "cica93/x/WIN_NT/TTINFO/TTF_SPEC.HLP",
    "cica93/apps/WINTREK/wintrek.hlp",
    "cica93/apps/EUCH11/wineuch.hlp",
    "cica93/apps/CRIBBAGE/cribbage.hlp",
    "cica93/apps/PEGIF/pegif.hlp",
    "cica93/apps/WINCODE/wincode.hlp",
    "cica93/apps/CHESS321/chess.hlp",
];

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

/// Panel `res` laid out as text lines (trimmed, empty ones left out),
/// and the targets of its links.
fn render(doc: &Doc<'_>, res: u16) -> (Vec<String>, Vec<Target<'static>>) {
    let toc = doc.panel_by_res(res).unwrap();
    let page = layout(doc, toc, &LayoutOptions { width: 100, keys: b"" }, &mut Grid);
    let mut rows: Vec<Vec<u8>> = Vec::new();
    for r in &page.runs {
        let RunKind::Word(w) = r.kind else { continue };
        let y = r.y.max(0) as usize;
        if rows.len() <= y {
            rows.resize(y + 1, Vec::new());
        }
        let row = &mut rows[y];
        let x = r.x.max(0) as usize;
        if row.len() < x + w.len() {
            row.resize(x + w.len(), b' ');
        }
        row[x..x + w.len()].copy_from_slice(w);
    }
    let lines = rows.iter().map(|r| String::from(ipf::latin1(r).trim())).filter(|l| !l.is_empty()).collect();
    let links = page
        .links
        .iter()
        .filter_map(|l| match l.target {
            Target::Panel(p) => Some(Target::Panel(p)),
            Target::Footnote(p) => Some(Target::Footnote(p)),
            _ => None,
        })
        .collect();
    (lines, links)
}

fn title(doc: &Doc<'_>, toc: u16) -> String {
    ipf::latin1(&doc.toc(toc).unwrap().title)
}

#[test]
fn every_file_converts() {
    for rel in FILES {
        let Some(d) = media(rel) else { continue };
        assert!(is_winhelp(&d), "{rel}");
        let t = std::time::Instant::now();
        let c = convert(&d).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let doc = Doc::parse(&c.ipf).unwrap();
        assert!(doc.is_help());
        for i in 0..doc.header.toc_count {
            doc.toc(i).unwrap();
        }
        for i in 0..doc.header.cell_count {
            doc.cell(i).unwrap();
        }
        assert!(doc.panel_by_res(c.contents_res).is_some(), "{rel}");
        for &(_, r) in &c.contexts {
            assert!(doc.panel_by_res(r).is_some(), "{rel}");
        }
        std::eprintln!("{rel}: {} panels, {} words, {:?}, {:?}", doc.header.toc_count, doc.header.dict_count, t.elapsed(), c.warnings);
    }
}

/// The topic offsets worked out walking `|TOPIC` are those `|TTLBTREE`
/// has, with the same titles.
#[test]
fn topic_offsets_match_titles() {
    for rel in FILES {
        let Some(d) = media(rel) else { continue };
        let h = help::Help::parse(&d).unwrap();
        let ttl = h.ttl();
        assert!(!ttl.is_empty(), "{rel}");
        for (o, t) in &ttl {
            let i = h.list.binary_search_by_key(o, |t| t.offset).unwrap_or_else(|_| panic!("{rel}: {o:x}"));
            assert_eq!(h.list[i].title, *t, "{rel}: {o:x}");
        }
    }
}

/// Windows 3.1's sample help file: contexts by name, map numbers, the
/// text and the bitmap of a topic.
#[test]
fn helpex() {
    let Some(d) = media(&[BIN, "HELPEX.HLP"].concat()) else { return };
    let c = convert(&d).unwrap();
    assert_eq!(c.title, b"Sample Main Window");
    let doc = Doc::parse(&c.ipf).unwrap();
    let r3 = c.context(b"x_topic3").unwrap();
    assert_eq!(c.context(b"X_TOPIC3"), Some(r3));
    assert_eq!(title(&doc, doc.panel_by_res(r3).unwrap()), "Sample Topic x_topic3");
    // [MAP] x_topic3 3.
    assert_eq!(c.map_number(3), Some(r3));
    let (lines, _) = render(&doc, r3);
    assert_eq!(lines[0], "Sample Topic x_topic3");
    assert!(lines.iter().any(|l| l.starts_with("This is a sample pop-up topic. To return")), "{lines:?}");
    // The Close button of a secondary window topic is a bitmap.
    let r = c.context(b"xs_topic1").unwrap();
    let toc = doc.panel_by_res(r).unwrap();
    let page = layout(&doc, toc, &LayoutOptions { width: 100, keys: b"" }, &mut Grid);
    let bm = page.runs.iter().find_map(|r| match r.kind {
        RunKind::Bitmap { offset, .. } => Some(offset),
        _ => None,
    });
    let b = ipf::bitmap::decode(&c.ipf, doc.header.image_start, bm.unwrap(), (0, 0)).unwrap();
    assert_eq!((b.cx, b.cy), (72, 20));
}

/// The Windows API reference: a function's parameter list (a hanging
/// indent with tabs), its jumps, keywords and the contents panel's
/// table of contents.
#[test]
fn winapi() {
    let Some(d) = media(&[BIN, "WINAPI.HLP"].concat()) else { return };
    let c = convert(&d).unwrap();
    let doc = Doc::parse(&c.ipf).unwrap();
    // The contents topic is the first panel, the topics it jumps to its
    // children.
    assert_eq!(doc.panel_by_res(c.contents_res), Some(0));
    let (lines, links) = render(&doc, c.contents_res);
    assert!(lines.iter().any(|l| l == "Alphabetical functions"), "{lines:?}");
    assert!(links.len() >= 10);
    for l in &links {
        let Target::Panel(p) = *l else { panic!("{l:?}") };
        assert_eq!(doc.toc(p).unwrap().level(), 2);
        assert!(!doc.toc(p).unwrap().hidden());
    }
    // A keyword.
    let r = c.keyword(b"createwindow").unwrap()[0];
    let toc = doc.panel_by_res(r).unwrap();
    assert_eq!(title(&doc, toc), "CreateWindow function");
    assert!(doc.toc(toc).unwrap().hidden());
    assert!(c.keywords_with_prefix(b"CreateWindow").len() >= 2);
    let (lines, links) = render(&doc, r);
    assert!(lines.iter().any(|l| l == "CreateWindow (2.x)     (WINPROCS unit)"), "{lines:?}");
    // Parameter and description columns.
    assert!(lines.iter().any(|l| l == "WindowName      Points to a null-terminated string that represents the window name."), "{lines:?}");
    // RegisterClass is a jump.
    assert!(links.iter().any(|l| matches!(l, Target::Panel(p) if title(&doc, *p) == "RegisterClass function")), "{links:?}");
}

/// Delphi's help: popups become footnote links, the index has the K
/// keywords, map numbers lead to topics.
#[test]
fn delphi() {
    let Some(d) = media(&[BIN, "DELPHI.HLP"].concat()) else { return };
    let t = std::time::Instant::now();
    let c = convert(&d).unwrap();
    std::eprintln!("DELPHI.HLP: {:?}", t.elapsed());
    let doc = Doc::parse(&c.ipf).unwrap();
    assert_eq!(title(&doc, 0), "Contents (collapsed)");
    let kw: Vec<&[u8]> = c.keywords_with_prefix(b"tform").iter().map(|k| k.0.as_slice()).collect();
    let r = *c.keyword(b"TForm").unwrap_or_else(|| panic!("{kw:?}")).iter().find(|&&r| title(&doc, doc.panel_by_res(r).unwrap()) == "TForm component").unwrap();
    let (lines, links) = render(&doc, r);
    assert_eq!(lines[0], "TForm Component");
    assert!(lines.iter().any(|l| l.starts_with("The form component is at the center of Delphi applications.")), "{lines:?}");
    // Properties, Methods, Events: popups.
    let fns: Vec<u16> = links.iter().filter_map(|l| if let Target::Footnote(p) = l { Some(*p) } else { None }).collect();
    assert_eq!(fns.len(), 3, "{links:?}");
    assert_eq!(title(&doc, fns[0]), "Properties");
    let (lines, _) = render(&doc, doc.res_of_panel(fns[0]).unwrap());
    assert!(lines.iter().any(|l| l.contains("ActiveControl")), "{lines:?}");
    // ShowModal is a jump.
    assert!(links.iter().any(|l| matches!(l, Target::Panel(p) if title(&doc, *p).starts_with("ShowModal"))), "{links:?}");
    // The index.
    let index = doc.index();
    assert!(index.iter().any(|e| e.text == b"TForm" && doc.res_of_panel(e.toc) == Some(r)));
    assert!(!c.map.is_empty());
    for &(_, r) in &c.map {
        assert!(doc.panel_by_res(r).is_some());
    }
}

/// A Windows 3.0 file: topic numbers in jumps, 2 KB blocks.
#[test]
fn chess_30() {
    let Some(d) = media("cica93/apps/CHESS321/chess.hlp") else { return };
    let c = convert(&d).unwrap();
    let doc = Doc::parse(&c.ipf).unwrap();
    let (lines, links) = render(&doc, c.contents_res);
    assert_eq!(lines[0], "GNU Chess Help Index");
    assert!(lines.iter().any(|l| l.starts_with("To move a piece, place the mouse cursor over the piece")));
    assert!(links.iter().any(|l| matches!(l, Target::Panel(p) if title(&doc, *p) == "GNU Chess menu commands")), "{links:?}");
}

/// Hotspots of a segmented hypergraphic become link areas.
#[test]
fn picture_hotspots() {
    let Some(d) = media(&[BIN, "DELPHI.HLP"].concat()) else { return };
    let c = convert(&d).unwrap();
    let doc = Doc::parse(&c.ipf).unwrap();
    let mut areas = 0;
    for toc in 0..doc.header.toc_count {
        let page = layout(&doc, toc, &LayoutOptions { width: 100, keys: b"" }, &mut Grid);
        areas += page.areas.len();
    }
    assert!(areas > 100, "{areas}");
}

#[test]
fn lz77_and_rle() {
    // "abcabcabcd": 3 literals, a reference 3 back of length 6, a literal.
    let src = [0b0000_1000, b'a', b'b', b'c', 0x02, 0x30, b'd'];
    let mut out = Vec::new();
    hfs::lz77(&src, &mut out, 100);
    assert_eq!(out, b"abcabcabcd");
    let mut out = Vec::new();
    hfs::rle(&[3, b'x', 0x82, b'y', b'z'], &mut out, 100);
    assert_eq!(out, b"xxxyz");
}

#[test]
fn hash_function() {
    assert_eq!(help::hash(b""), 0);
    assert_eq!(help::hash(b"Contents"), help::hash(b"CONTENTS"));
    // hash("ab") = 0x11 * 43 + 0x12.
    assert_eq!(help::hash(b"ab"), 0x11 * 43 + 0x12);
}

#[test]
fn not_winhelp() {
    assert!(!is_winhelp(b"HSP\x10"));
    assert_eq!(convert(b"HSP\x10").unwrap_err(), Error::BadMagic);
    assert!(convert(&[0x3f, 0x5f, 3, 0, 0x10, 0, 0, 0]).is_err());
}
