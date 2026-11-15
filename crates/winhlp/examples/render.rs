//! Convert a help file and show panels as text: `render FILE.HLP
//! [res|context|title-substring ...]` (no panel: the contents panel).

use ipf::layout::{layout, FontMetrics, Metrics, Options, RunKind};

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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = std::fs::read(&args[0]).unwrap();
    let c = winhlp::convert(&data).unwrap();
    let doc = ipf::Doc::parse(&c.ipf).unwrap();
    let want: Vec<String> = if args.len() > 1 { args[1..].to_vec() } else { vec![c.contents_res.to_string()] };
    for w in want {
        let res = w.parse::<u16>().ok().or_else(|| c.context(w.as_bytes())).or_else(|| {
            (0..doc.header.toc_count).find(|&i| ipf::latin1(&doc.toc(i).unwrap().title).contains(&w)).and_then(|i| doc.res_of_panel(i))
        });
        let Some(res) = res else {
            println!("{w}: not found");
            continue;
        };
        let toc = doc.panel_by_res(res).unwrap();
        let e = doc.toc(toc).unwrap();
        println!("=== res {res} toc {toc} {:?} ({} cells)", ipf::latin1(&e.title), e.cells.len());
        let page = layout(&doc, toc, &Options { width: 100, keys: b"" }, &mut Grid);
        let mut rows: Vec<Vec<u8>> = Vec::new();
        for r in &page.runs {
            let (y, text): (usize, Vec<u8>) = match r.kind {
                RunKind::Word(w) => {
                    let mut t = w.to_vec();
                    if let Some(l) = r.link {
                        if page.links.get(l as usize).is_some() {
                            t = t.iter().map(|&c| if c == b' ' { b'_' } else { c }).collect();
                        }
                    }
                    (r.y.max(0) as usize, t)
                }
                RunKind::Bitmap { offset, .. } | RunKind::Image { handle: offset, .. } => (r.y.max(0) as usize, format!("[bitmap {offset:x}]").into_bytes()),
            };
            if rows.len() <= y {
                rows.resize(y + 1, Vec::new());
            }
            let row = &mut rows[y];
            let x = r.x.max(0) as usize;
            if row.len() < x + text.len() {
                row.resize(x + text.len(), b' ');
            }
            row[x..x + text.len()].copy_from_slice(&text);
        }
        for r in rows {
            println!("{}", ipf::latin1(&r).trim_end());
        }
        for l in &page.links {
            print!("{:?} ", l.target);
        }
        println!();
        for (_, a) in &page.areas {
            print!("area {:?} {:?}; ", a.target, a.rect);
        }
        println!();
    }
}
