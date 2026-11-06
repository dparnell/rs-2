//! What the kernel remembers of path lookups, above the file systems.
//!
//! kLIBC checks every component of every path, so the same directories are
//! asked for over and over. The kernel keeps the answers that only change
//! through the file system's own path operations: what a directory is, and
//! that a name does not exist. A file's size and times also change through
//! its handles, so files are left to the file system.
//!
//! Names can have other spellings: case, and on FAT the 8.3 forms. A key folds
//! ASCII case and nothing else, so a path that could have another spelling
//! is not kept at all, and a change through such a path forgets everything:
//! the worst it costs is a lookup.

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;

/// Answers kept; at this many it starts over.
const KEPT: usize = 8192;

/// The key of a path, in upper case with `\`, or None if the path may have
/// another spelling: not ASCII, a component ending in a dot or a space, or
/// with `short` (8.3 file systems) one that does not fit.
pub fn key(path: &str, short: bool) -> Option<String> {
    let mut k = String::with_capacity(path.len());
    for c in path.split(['\\', '/']).filter(|c| !c.is_empty()) {
        let b = c.as_bytes();
        if !c.is_ascii() || c == "." || c == ".." || matches!(b.last(), Some(b'.' | b' ')) {
            return None;
        }
        if short {
            let (name, ext) = c.split_once('.').unwrap_or((c, ""));
            if name.len() > 8 || ext.len() > 3 || ext.contains('.') {
                return None;
            }
        }
        if !k.is_empty() {
            k.push('\\');
        }
        k.extend(c.chars().map(|x| x.to_ascii_uppercase()));
    }
    Some(k)
}

/// The answers of one volume.
pub struct PathCache<V> {
    map: BTreeMap<String, Result<V, u32>>,
    generation: u64,
    short: bool,
}

impl<V: Clone> PathCache<V> {
    pub const fn new(short: bool) -> Self {
        PathCache { map: BTreeMap::new(), generation: 0, short }
    }

    pub fn key(&self, path: &str) -> Option<String> {
        key(path, self.short)
    }

    pub fn get(&self, key: &str) -> Option<Result<V, u32>> {
        self.map.get(key).cloned()
    }

    /// Take this before asking the file system, and hand it to `put`: an
    /// answer that a change has overtaken is not kept.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn put(&mut self, key: String, generation: u64, value: Result<V, u32>) {
        if generation != self.generation {
            return;
        }
        if self.map.len() >= KEPT {
            self.map.clear();
        }
        self.map.insert(key, value);
    }

    /// `path` has changed, or was removed or made: forget it, what is below
    /// it, and its directory. Call it after the change, even a failed one.
    pub fn changed(&mut self, path: &str) {
        self.generation += 1;
        let Some(k) = self.key(path) else {
            self.map.clear();
            return;
        };
        self.map.remove(&k);
        let below = k.clone() + "\\";
        let gone: alloc::vec::Vec<String> = self.map.range(below.clone()..).take_while(|(x, _)| x.starts_with(&below)).map(|(x, _)| x.clone()).collect();
        for x in gone {
            self.map.remove(&x);
        }
        let parent = k.rsplit_once('\\').map_or("", |p| p.0);
        self.map.remove(parent);
    }

    pub fn clear(&mut self) {
        self.generation += 1;
        self.map.clear();
    }
}

#[cfg(test)]
mod tests;
