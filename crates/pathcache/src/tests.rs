extern crate std;

use super::*;
use std::collections::BTreeSet;
use std::format;
use std::string::ToString;
use std::vec::Vec;

const NOT_FOUND: u32 = 2;
const PATH_NOT_FOUND: u32 = 3;

/// A file system of directories and files, case-insensitive.
#[derive(Default)]
struct Model {
    dirs: BTreeSet<String>,
    files: BTreeSet<String>,
}

fn up(p: &str) -> String {
    p.to_ascii_uppercase()
}

impl Model {
    fn stat(&self, p: &str) -> Result<u32, u32> {
        let k = up(p);
        if k.is_empty() || self.dirs.contains(&k) {
            return Ok(1);
        }
        if self.files.contains(&k) {
            return Err(100); // a file: the cache does not keep these
        }
        match k.rsplit_once('\\') {
            Some((d, _)) if !self.dirs.contains(d) => Err(PATH_NOT_FOUND),
            _ => Err(NOT_FOUND),
        }
    }

    fn parent_ok(&self, p: &str) -> bool {
        p.rsplit_once('\\').is_none_or(|(d, _)| self.dirs.contains(&up(d)))
    }

    fn free(&self, p: &str) -> bool {
        !self.dirs.contains(&up(p)) && !self.files.contains(&up(p)) && self.parent_ok(p)
    }

    fn rename(&mut self, from: &str, to: &str) {
        let (f, t) = (up(from), up(to));
        if !self.free(to) || t.starts_with(&(f.clone() + "\\")) {
            return;
        }
        if self.files.remove(&f) {
            self.files.insert(t);
        } else if self.dirs.contains(&f) {
            let below = f.clone() + "\\";
            let moved = |s: &BTreeSet<String>| -> Vec<String> { s.iter().filter(|x| **x == f || x.starts_with(&below)).cloned().collect() };
            for x in moved(&self.dirs) {
                self.dirs.remove(&x);
                self.dirs.insert(format!("{t}{}", &x[f.len()..]));
            }
            for x in moved(&self.files) {
                self.files.remove(&x);
                self.files.insert(format!("{t}{}", &x[f.len()..]));
            }
        }
    }
}

/// `stat` as the kernel's wrapper does it.
fn cached(c: &mut PathCache<u32>, m: &Model, p: &str) -> Result<u32, u32> {
    let Some(k) = c.key(p) else { return m.stat(p) };
    if let Some(r) = c.get(&k) {
        return r;
    }
    let g = c.generation();
    let r = m.stat(p);
    if r != Err(100) {
        c.put(k, g, r);
    }
    r
}

#[test]
fn stays_true() {
    let mut seed = 99u32;
    let mut rnd = move |n: u32| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) % n
    };
    let names = ["a", "B", "cc", "Dir", "e.txt", "\u{e9}x", "f."];
    let mut path = |depth: u32| -> String {
        let mut p = String::new();
        for i in 0..=depth {
            if i > 0 {
                p.push('\\');
            }
            let n = names[rnd(names.len() as u32) as usize];
            p.push_str(&if rnd(2) == 0 { n.to_ascii_uppercase() } else { n.to_ascii_lowercase() });
        }
        p
    };
    let (mut m, mut c) = (Model::default(), PathCache::<u32>::new(false));
    let mut depth = 0;
    for step in 0..200_000 {
        depth = (depth + 1) % 4;
        let (p, q) = (path(depth), path((depth + 1) % 4));
        match step % 7 {
            0 => {
                if m.free(&p) {
                    m.dirs.insert(up(&p));
                }
                c.changed(&p);
            }
            1 => {
                if m.free(&p) {
                    m.files.insert(up(&p));
                }
                c.changed(&p);
            }
            2 => {
                m.files.remove(&up(&p));
                c.changed(&p);
            }
            3 => {
                let below = up(&p) + "\\";
                if !m.dirs.iter().chain(&m.files).any(|x| x.starts_with(&below)) {
                    m.dirs.remove(&up(&p));
                }
                c.changed(&p);
            }
            4 => {
                m.rename(&p, &q);
                c.changed(&p);
                c.changed(&q);
            }
            _ => {}
        }
        for x in [&p, &q] {
            for s in [x.clone(), x.to_ascii_lowercase()] {
                let _ = cached(&mut c, &m, &s);
                assert_eq!(cached(&mut c, &m, &s), m.stat(&s), "step {step}: {s}");
            }
        }
    }
}

#[test]
fn an_overtaken_answer_is_dropped() {
    let mut c = PathCache::<u32>::new(false);
    let g = c.generation();
    c.changed("X");
    c.put("X".to_string(), g, Err(NOT_FOUND));
    assert!(c.get("X").is_none());
}

#[test]
fn other_spellings_are_not_kept() {
    assert_eq!(key("a/b\\C.txt", false).as_deref(), Some("A\\B\\C.TXT"));
    assert!(key("a.", false).is_none());
    assert!(key("a \\b", false).is_none());
    assert!(key("\u{e9}", false).is_none());
    assert!(key("..\\a", false).is_none());
    assert!(key("longer-name.txt", true).is_none());
    assert!(key("a.b.c", true).is_none());
    assert!(key("FILE.TXT", true).is_some());
}

#[test]
fn a_change_by_another_spelling_forgets_everything() {
    let mut c = PathCache::<u32>::new(true);
    c.put("LONGNAME".to_string(), 0, Err(NOT_FOUND));
    c.changed("longnamexx");
    assert!(c.get("LONGNAME").is_none());
}
