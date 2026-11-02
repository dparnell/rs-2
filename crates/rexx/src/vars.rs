//! Variable pools. Each PROCEDURE gets a new pool; names it exposes are
//! looked up in the caller's pool. Stems keep a default value (from
//! `stem. = value`) and their tails; a tail dropped while the stem has a
//! default is kept as "dropped" so it reads as its own name again.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

#[derive(Default)]
pub struct Stem {
    pub default: Option<Vec<u8>>,
    pub tails: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Pool {
    pub vars: BTreeMap<Vec<u8>, Vec<u8>>,
    pub stems: BTreeMap<Vec<u8>, Stem>,
    /// Exposed names: simple names, stems ("A.") and compound names.
    pub exposed: BTreeSet<Vec<u8>>,
    pub parent: Option<usize>,
}

/// A fully resolved variable name.
pub enum Key<'a> {
    Simple(&'a [u8]),
    Stem(&'a [u8]),
    /// stem (with the period) and the substituted tail
    Comp(&'a [u8], &'a [u8]),
}

pub struct Pools {
    pub pools: Vec<Pool>,
}

impl Pools {
    pub fn new() -> Pools {
        Pools { pools: alloc::vec![Pool::default()] }
    }

    /// The pool that holds `key` when seen from pool `p`.
    fn owner(&self, mut p: usize, key: &Key) -> usize {
        loop {
            let pool = &self.pools[p];
            let Some(parent) = pool.parent else { return p };
            let exposed = match key {
                Key::Simple(n) | Key::Stem(n) => pool.exposed.contains(*n),
                Key::Comp(stem, tail) => {
                    pool.exposed.contains(*stem) || {
                        let mut full = stem.to_vec();
                        full.extend_from_slice(tail);
                        pool.exposed.contains(&full)
                    }
                }
            };
            if !exposed {
                return p;
            }
            p = parent;
        }
    }

    pub fn get(&self, p: usize, key: &Key) -> Option<Vec<u8>> {
        let o = self.owner(p, key);
        let pool = &self.pools[o];
        match key {
            Key::Simple(n) => pool.vars.get(*n).cloned(),
            Key::Stem(n) => pool.stems.get(*n).and_then(|s| s.default.clone()),
            Key::Comp(stem, tail) => {
                let s = pool.stems.get(*stem)?;
                match s.tails.get(*tail) {
                    Some(v) => v.clone(),
                    None => s.default.clone(),
                }
            }
        }
    }

    pub fn set(&mut self, p: usize, key: &Key, val: Vec<u8>) {
        let o = self.owner(p, key);
        let pool = &mut self.pools[o];
        match key {
            Key::Simple(n) => {
                pool.vars.insert(n.to_vec(), val);
            }
            Key::Stem(n) => {
                let s = pool.stems.entry(n.to_vec()).or_default();
                s.tails.clear();
                s.default = Some(val);
            }
            Key::Comp(stem, tail) => {
                pool.stems.entry(stem.to_vec()).or_default().tails.insert(tail.to_vec(), Some(val));
            }
        }
    }

    pub fn drop_var(&mut self, p: usize, key: &Key) {
        let o = self.owner(p, key);
        let pool = &mut self.pools[o];
        match key {
            Key::Simple(n) => {
                pool.vars.remove(*n);
            }
            Key::Stem(n) => {
                pool.stems.remove(*n);
            }
            Key::Comp(stem, tail) => {
                if let Some(s) = pool.stems.get_mut(*stem) {
                    if s.default.is_some() {
                        s.tails.insert(tail.to_vec(), None);
                    } else {
                        s.tails.remove(*tail);
                    }
                }
            }
        }
    }

    /// Start a procedure's pool.
    pub fn push(&mut self, parent: usize) -> usize {
        self.pools.push(Pool { parent: Some(parent), ..Default::default() });
        self.pools.len() - 1
    }

    pub fn pop_to(&mut self, len: usize) {
        self.pools.truncate(len);
    }

    /// All variables visible in pool `p` (own ones), for RXSHV_NEXTV:
    /// (name, value).
    pub fn list(&self, p: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        let pool = &self.pools[p];
        for (k, v) in &pool.vars {
            out.push((k.clone(), v.clone()));
        }
        for (stem, s) in &pool.stems {
            if let Some(d) = &s.default {
                out.push((stem.clone(), d.clone()));
            }
            for (t, v) in &s.tails {
                if let Some(v) = v {
                    let mut n = stem.clone();
                    n.extend_from_slice(t);
                    out.push((n, v.clone()));
                }
            }
        }
        // exposed variables live in the parent
        if let Some(parent) = pool.parent {
            for name in &pool.exposed {
                if name.ends_with(b".") {
                    if let Some(s) = self.pools[self.owner(parent, &Key::Stem(name))].stems.get(name) {
                        if let Some(d) = &s.default {
                            out.push((name.clone(), d.clone()));
                        }
                        for (t, v) in &s.tails {
                            if let Some(v) = v {
                                let mut n = name.clone();
                                n.extend_from_slice(t);
                                out.push((n, v.clone()));
                            }
                        }
                    }
                } else if let Some(v) = self.get(parent, &Key::Simple(name)) {
                    out.push((name.clone(), v));
                }
            }
        }
        out
    }
}
