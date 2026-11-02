//! Variable pools. Each PROCEDURE gets a new pool; names it exposes are
//! looked up in the caller's pool. Stems keep a default value (from
//! `stem. = value`) and their tails; a tail dropped while the stem has a
//! default is kept as "dropped" so it reads as its own name again.
//!
//! Pools and stems are shared (`Rc`): an Object REXX method's pool has the
//! object's variable pool as its parent (EXPOSE), and a stem can be passed
//! by reference (USE ARG) or held by a .Stem object.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::value::Value;

#[derive(Default)]
pub struct Stem {
    pub default: Option<Value>,
    pub tails: BTreeMap<Vec<u8>, Option<Value>>,
}

pub type StemRef = Rc<RefCell<Stem>>;

#[derive(Default)]
pub struct Pool {
    pub vars: BTreeMap<Vec<u8>, Value>,
    pub stems: BTreeMap<Vec<u8>, StemRef>,
    /// Exposed names: simple names, stems ("A.") and compound names.
    pub exposed: BTreeSet<Vec<u8>>,
    pub parent: Option<usize>,
}

pub type PoolRef = Rc<RefCell<Pool>>;

/// A fully resolved variable name.
pub enum Key<'a> {
    Simple(&'a [u8]),
    Stem(&'a [u8]),
    /// stem (with the period) and the substituted tail
    Comp(&'a [u8], &'a [u8]),
}

pub struct Pools {
    pub pools: Vec<PoolRef>,
}

impl Pools {
    pub fn new() -> Pools {
        Pools { pools: alloc::vec![Rc::new(RefCell::new(Pool::default()))] }
    }

    /// The pool that holds `key` when seen from pool `p`.
    fn owner(&self, mut p: usize, key: &Key) -> usize {
        loop {
            let pool = self.pools[p].borrow();
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

    pub fn get(&self, p: usize, key: &Key) -> Option<Value> {
        let o = self.owner(p, key);
        let pool = self.pools[o].borrow();
        match key {
            Key::Simple(n) => pool.vars.get(*n).cloned(),
            Key::Stem(n) => pool.stems.get(*n).and_then(|s| s.borrow().default.clone()),
            Key::Comp(stem, tail) => {
                let s = pool.stems.get(*stem)?.borrow();
                match s.tails.get(*tail) {
                    Some(v) => v.clone(),
                    None => s.default.clone(),
                }
            }
        }
    }

    pub fn set(&self, p: usize, key: &Key, val: Value) {
        let o = self.owner(p, key);
        let mut pool = self.pools[o].borrow_mut();
        match key {
            Key::Simple(n) => {
                pool.vars.insert(n.to_vec(), val);
            }
            Key::Stem(n) => {
                let s = pool.stems.entry(n.to_vec()).or_default().clone();
                let mut s = s.borrow_mut();
                s.tails.clear();
                s.default = Some(val);
            }
            Key::Comp(stem, tail) => {
                let s = pool.stems.entry(stem.to_vec()).or_default().clone();
                s.borrow_mut().tails.insert(tail.to_vec(), Some(val));
            }
        }
    }

    pub fn drop_var(&self, p: usize, key: &Key) {
        let o = self.owner(p, key);
        let mut pool = self.pools[o].borrow_mut();
        match key {
            Key::Simple(n) => {
                pool.vars.remove(*n);
            }
            Key::Stem(n) => {
                pool.stems.remove(*n);
            }
            Key::Comp(stem, tail) => {
                if let Some(s) = pool.stems.get(*stem) {
                    let mut s = s.borrow_mut();
                    if s.default.is_some() {
                        s.tails.insert(tail.to_vec(), None);
                    } else {
                        s.tails.remove(*tail);
                    }
                }
            }
        }
    }

    /// The stem object behind a stem name (made if it does not exist).
    pub fn stem(&self, p: usize, name: &[u8]) -> StemRef {
        let o = self.owner(p, &Key::Stem(name));
        let mut pool = self.pools[o].borrow_mut();
        pool.stems.entry(name.to_vec()).or_default().clone()
    }

    /// Whether the stem exists (has been assigned or used).
    pub fn has_stem(&self, p: usize, name: &[u8]) -> bool {
        let o = self.owner(p, &Key::Stem(name));
        self.pools[o].borrow().stems.contains_key(name)
    }

    /// Make a stem name refer to a stem object (USE ARG, `a. = b.`).
    pub fn bind_stem(&self, p: usize, name: &[u8], s: StemRef) {
        let o = self.owner(p, &Key::Stem(name));
        self.pools[o].borrow_mut().stems.insert(name.to_vec(), s);
    }

    /// Start a procedure's pool.
    pub fn push(&mut self, parent: usize) -> usize {
        self.pools.push(Rc::new(RefCell::new(Pool { parent: Some(parent), ..Default::default() })));
        self.pools.len() - 1
    }

    /// Put an existing pool (an object's variables) on the stack.
    pub fn push_pool(&mut self, p: PoolRef) -> usize {
        self.pools.push(p);
        self.pools.len() - 1
    }

    pub fn pop_to(&mut self, len: usize) {
        self.pools.truncate(len);
    }

    /// All variables visible in pool `p` (own ones), for RXSHV_NEXTV:
    /// (name, value).
    pub fn list(&self, p: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        let pool = self.pools[p].borrow();
        for (k, v) in &pool.vars {
            out.push((k.clone(), v.fast_string()));
        }
        for (stem, s) in &pool.stems {
            let s = s.borrow();
            if let Some(d) = &s.default {
                out.push((stem.clone(), d.fast_string()));
            }
            for (t, v) in &s.tails {
                if let Some(v) = v {
                    let mut n = stem.clone();
                    n.extend_from_slice(t);
                    out.push((n, v.fast_string()));
                }
            }
        }
        // exposed variables live in the parent
        if let Some(parent) = pool.parent {
            for name in &pool.exposed {
                if name.ends_with(b".") {
                    let o = self.owner(parent, &Key::Stem(name));
                    if let Some(s) = self.pools[o].borrow().stems.get(name) {
                        let s = s.borrow();
                        if let Some(d) = &s.default {
                            out.push((name.clone(), d.fast_string()));
                        }
                        for (t, v) in &s.tails {
                            if let Some(v) = v {
                                let mut n = name.clone();
                                n.extend_from_slice(t);
                                out.push((n, v.fast_string()));
                            }
                        }
                    }
                } else if let Some(v) = self.get(parent, &Key::Simple(name)) {
                    out.push((name.clone(), v.fast_string()));
                }
            }
        }
        out
    }
}
