//! OS/2 binary profiles (INI files: OS2.INI, OS2SYS.INI and the files
//! programs open with PrfOpenProfile), parsed into and written from a
//! [`Profile`]: a list of applications, each a list of keys with binary
//! values. No OS dependencies; PMSHAPI uses it on real files.
//!
//! # File format
//!
//! All numbers are little-endian; offsets are from the start of the file.
//!
//! Header, 20 bytes:
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0 | 4 | signature `FF FF FF FF` |
//! | 4 | 4 | offset of the first application record (0x14) |
//! | 8 | 4 | file size |
//! | 12 | 4 | reserved (0; IBM's writer sometimes leaves other values) |
//! | 16 | 4 | reserved (0) |
//!
//! Application record, 20 bytes:
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0 | 4 | offset of the next application record (0: last) |
//! | 4 | 4 | offset of the first key record (0: none) |
//! | 8 | 4 | reserved (0) |
//! | 12 | 2 | length of the name, including its NUL |
//! | 14 | 2 | the same length again |
//! | 16 | 4 | offset of the name |
//!
//! Key record, 24 bytes:
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0 | 4 | offset of the next key record (0: last) |
//! | 4 | 4 | reserved (0) |
//! | 8 | 2 | length of the key name, including its NUL |
//! | 10 | 2 | the same length again |
//! | 12 | 4 | offset of the key name |
//! | 16 | 2 | length of the value |
//! | 18 | 2 | the same length again |
//! | 20 | 4 | offset of the value |
//!
//! IBM's PMSHAPI writes each application record, then its name, then for
//! each key the key record, the key name and the value, in list order;
//! [`Profile::to_bytes`] writes exactly that layout (an IBM file written
//! that way re-serialises byte for byte). Files IBM's writer updated in
//! place can hold stale records that nothing links to; readers follow the
//! links only. Values are at most 65535 bytes.
//!
//! Names are compared byte for byte (case-sensitively), as OS/2 does:
//! "There is no case-independent searching" (PM Programming Reference,
//! PrfQueryProfileData).
//!
//! [`parse`] is defensive: every offset and length is bounds-checked, the
//! record chains are cut where they leave the file or loop, and whatever
//! could be read is kept ([`Parsed::damaged`] says that something was
//! dropped).

#![no_std]

extern crate alloc;

pub mod proglist;

use alloc::vec::Vec;

/// The largest value a key can hold.
pub const MAX_DATA: usize = 0xffff;
/// The longest application or key name (the length field includes the NUL).
pub const MAX_NAME: usize = 0xfffe;

const HEADER: usize = 20;
const APP_REC: usize = 20;
const KEY_REC: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: Vec<u8>,
    pub keys: Vec<Key>,
}

impl App {
    pub fn key(&self, name: &[u8]) -> Option<&Key> {
        self.keys.iter().find(|k| k.name == name)
    }
}

/// A profile: applications and their keys, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub apps: Vec<App>,
}

/// The result of [`parse`].
#[derive(Debug)]
pub struct Parsed {
    pub profile: Profile,
    /// The file is not a profile at all (no signature): nothing was read.
    /// An empty file is a valid, empty profile.
    pub foreign: bool,
    /// Records were dropped because offsets or lengths were out of range
    /// or a chain looped.
    pub damaged: bool,
}

/// Why a write was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The value is longer than [`MAX_DATA`].
    TooLarge,
    /// An empty name, or one longer than [`MAX_NAME`].
    BadName,
}

fn u16_at(b: &[u8], off: usize) -> Option<usize> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn u32_at(b: &[u8], off: usize) -> Option<usize> {
    let s = b.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// A name stored at `off` with `len` bytes including the NUL: the bytes
/// before the first NUL.
fn name_at(b: &[u8], off: usize, len: usize) -> Option<Vec<u8>> {
    if len == 0 {
        return None;
    }
    let s = b.get(off..off.checked_add(len)?)?;
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    (n > 0).then(|| s[..n].to_vec())
}

/// Parse a profile file. Never fails: see [`Parsed`].
pub fn parse(b: &[u8]) -> Parsed {
    let mut out = Parsed { profile: Profile::default(), foreign: false, damaged: false };
    if b.is_empty() {
        return out;
    }
    if b.len() < HEADER || b[..4] != [0xff; 4] {
        out.foreign = true;
        return out;
    }
    let mut app_off = u32_at(b, 4).unwrap_or(0);
    // Every record takes at least its own size, so a chain longer than
    // this has a loop.
    let mut apps_left = b.len() / APP_REC;
    let mut keys_left = b.len() / KEY_REC;
    if app_off == b.len() {
        // An empty profile: the first record would start at the end.
        app_off = 0;
    }
    while app_off != 0 {
        if apps_left == 0 || app_off < HEADER || app_off.checked_add(APP_REC).is_none_or(|e| e > b.len()) {
            out.damaged = true;
            break;
        }
        apps_left -= 1;
        let next = u32_at(b, app_off).unwrap();
        let mut key_off = u32_at(b, app_off + 4).unwrap();
        let nlen = u16_at(b, app_off + 12).unwrap();
        let noff = u32_at(b, app_off + 16).unwrap();
        let Some(name) = name_at(b, noff, nlen) else {
            out.damaged = true;
            app_off = next;
            continue;
        };
        let mut app = App { name, keys: Vec::new() };
        while key_off != 0 {
            if keys_left == 0 || key_off < HEADER || key_off.checked_add(KEY_REC).is_none_or(|e| e > b.len()) {
                out.damaged = true;
                break;
            }
            keys_left -= 1;
            let knext = u32_at(b, key_off).unwrap();
            let klen = u16_at(b, key_off + 8).unwrap();
            let koff = u32_at(b, key_off + 12).unwrap();
            let dlen = u16_at(b, key_off + 16).unwrap();
            let doff = u32_at(b, key_off + 20).unwrap();
            let data = if dlen == 0 { Some(&[][..]) } else { doff.checked_add(dlen).and_then(|e| b.get(doff..e)) };
            match (name_at(b, koff, klen), data) {
                (Some(name), Some(data)) => app.keys.push(Key { name, data: data.to_vec() }),
                _ => out.damaged = true,
            }
            key_off = knext;
        }
        out.profile.apps.push(app);
        app_off = next;
    }
    out
}

fn put32(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&(x as u32).to_le_bytes());
}

fn put16x2(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&(x as u16).to_le_bytes());
    v.extend_from_slice(&(x as u16).to_le_bytes());
}

fn valid_name(n: &[u8]) -> bool {
    !n.is_empty() && n.len() <= MAX_NAME && !n.contains(&0)
}

impl Profile {
    /// The file contents in IBM's layout (see the crate documentation).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0xff; 4]);
        put32(&mut v, HEADER);
        put32(&mut v, 0); // file size, below
        put32(&mut v, 0);
        put32(&mut v, 0);
        for (i, a) in self.apps.iter().enumerate() {
            let rec = v.len();
            let name = rec + APP_REC;
            let first_key = if a.keys.is_empty() { 0 } else { name + a.name.len() + 1 };
            put32(&mut v, 0); // next, below
            put32(&mut v, first_key);
            put32(&mut v, 0);
            put16x2(&mut v, a.name.len() + 1);
            put32(&mut v, name);
            v.extend_from_slice(&a.name);
            v.push(0);
            for (j, k) in a.keys.iter().enumerate() {
                let krec = v.len();
                let kname = krec + KEY_REC;
                let data = kname + k.name.len() + 1;
                let next = if j + 1 < a.keys.len() { data + k.data.len() } else { 0 };
                put32(&mut v, next);
                put32(&mut v, 0);
                put16x2(&mut v, k.name.len() + 1);
                put32(&mut v, kname);
                put16x2(&mut v, k.data.len());
                put32(&mut v, data);
                v.extend_from_slice(&k.name);
                v.push(0);
                v.extend_from_slice(&k.data);
            }
            if i + 1 < self.apps.len() {
                let end = v.len();
                v[rec..rec + 4].copy_from_slice(&(end as u32).to_le_bytes());
            }
        }
        let size = v.len() as u32;
        v[8..12].copy_from_slice(&size.to_le_bytes());
        v
    }

    pub fn app(&self, name: &[u8]) -> Option<&App> {
        self.apps.iter().find(|a| a.name == name)
    }

    /// The value of `app`/`key`.
    pub fn get(&self, app: &[u8], key: &[u8]) -> Option<&[u8]> {
        self.app(app)?.key(key).map(|k| k.data.as_slice())
    }

    /// Set `app`/`key` to `data`: the key keeps its place if it exists;
    /// new keys and applications are appended.
    pub fn set(&mut self, app: &[u8], key: &[u8], data: &[u8]) -> Result<(), Error> {
        if !valid_name(app) || !valid_name(key) {
            return Err(Error::BadName);
        }
        if data.len() > MAX_DATA {
            return Err(Error::TooLarge);
        }
        let a = match self.apps.iter().position(|a| a.name == app) {
            Some(i) => &mut self.apps[i],
            None => {
                self.apps.push(App { name: app.to_vec(), keys: Vec::new() });
                self.apps.last_mut().unwrap()
            }
        };
        match a.keys.iter_mut().find(|k| k.name == key) {
            Some(k) => k.data = data.to_vec(),
            None => a.keys.push(Key { name: key.to_vec(), data: data.to_vec() }),
        }
        Ok(())
    }

    /// Delete `app`/`key`; an application whose last key goes is removed.
    /// False if there was no such key.
    pub fn remove_key(&mut self, app: &[u8], key: &[u8]) -> bool {
        let Some(i) = self.apps.iter().position(|a| a.name == app) else { return false };
        let a = &mut self.apps[i];
        let Some(j) = a.keys.iter().position(|k| k.name == key) else { return false };
        a.keys.remove(j);
        if a.keys.is_empty() {
            self.apps.remove(i);
        }
        true
    }

    /// Delete an application and all its keys. False if there was none.
    pub fn remove_app(&mut self, app: &[u8]) -> bool {
        let n = self.apps.len();
        self.apps.retain(|a| a.name != app);
        self.apps.len() != n
    }
}

/// Names as PMSHAPI's enumerations return them: each followed by a NUL,
/// then a final NUL.
pub fn name_list<'a>(names: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut v = Vec::new();
    for n in names {
        v.extend_from_slice(n);
        v.push(0);
    }
    v.push(0);
    v
}

#[cfg(test)]
mod tests;
