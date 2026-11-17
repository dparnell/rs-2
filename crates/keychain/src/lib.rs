//! rs2's system keychain file, `C:\RS2\KEYCHAIN\KEYCHAIN.DAT`, without
//! the kernel: the format, its encryption, the entries. The kernel's
//! `keychain` module keeps the one open keychain, saves it, and keeps
//! programs from opening the file (docs/KEYCHAIN.md).
//!
//! An entry is a secret (a WiFi passphrase, a password) named by a
//! service (`wifi`) and an account (the SSID). Entries are either
//! readable by programs (Rs2KeyGet) or for the system only: programs may
//! store, replace and delete those, but only the kernel reads them back.
//!
//! The file:
//!
//! | Offset | Size | |
//! |--------|------|-|
//! | 0  | 8  | `RS2KEYC1` |
//! | 8  | 1  | version 1 |
//! | 9  | 1  | flags: bit 0, a master password protects the keychain |
//! | 10 | 2  | reserved, 0 |
//! | 12 | 4  | PBKDF2 iterations (LE) |
//! | 16 | 16 | salt |
//! | 32 | 40 | the 32-byte master key, AES key wrapped (RFC 3394) with the KEK |
//! | 72 | 16 | CTR nonce of this save |
//! | 88 | n  | the entries, AES-128-CTR with master key bytes 0..16 |
//! | 88+n | 20 | HMAC-SHA1 (master key bytes 16..32) of everything before it |
//!
//! The KEK is PBKDF2-HMAC-SHA1(password, salt, iterations, 16) when there
//! is a master password. Without one it is derived from the salt alone,
//! which only keeps the secrets from being read as plain text: whoever
//! has the file has them. A master password makes the file useless
//! without it (and the keychain stays locked at boot until it is given).
//!
//! The entries (plain text): count u32, then each: flags u8 (bit 0
//! readable by programs), service (u8 length + bytes), account (u8 length
//! + bytes), secret (u16 length + bytes); all little-endian.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use rs2crypto::{aes_key_unwrap, aes_key_wrap, aes128_ctr, ct_eq, hmac_sha1, pbkdf2_sha1, sha1};

pub const MAGIC: &[u8; 8] = b"RS2KEYC1";
pub const VERSION: u8 = 1;
/// PBKDF2 iterations for a master password.
pub const ITERATIONS: u32 = 20_000;
const HEADER: usize = 88;
const MAC: usize = 20;
const FLAG_PASSWORD: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub service: String,
    pub account: Vec<u8>,
    pub secret: Vec<u8>,
    /// Programs may read it (Rs2KeyGet); otherwise only the kernel.
    pub readable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a keychain file, or a version this code does not know.
    Format,
    /// A master password protects it and none was given.
    NeedsPassword,
    /// The master password is wrong.
    BadPassword,
    /// The file was changed or damaged (the MAC does not match).
    Corrupt,
    /// A service or account over 255 bytes, a secret over 65535.
    TooLong,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(match self {
            Error::Format => "not a keychain file",
            Error::NeedsPassword => "the keychain is locked (it has a master password)",
            Error::BadPassword => "wrong master password",
            Error::Corrupt => "the keychain file is damaged",
            Error::TooLong => "a name or secret is too long",
        })
    }
}

/// Whether the file needs a master password to open.
pub fn has_password(file: &[u8]) -> bool {
    file.len() > 9 && &file[..8] == MAGIC && file[9] & FLAG_PASSWORD != 0
}

/// An open keychain: its entries in memory and the master key.
pub struct Keychain {
    entries: Vec<Entry>,
    master: [u8; 32],
    password: bool,
    salt: [u8; 16],
    iterations: u32,
    /// The wrapped master key, as stored.
    wrapped: Vec<u8>,
}

impl Drop for Keychain {
    fn drop(&mut self) {
        self.master = [0; 32];
        for e in &mut self.entries {
            e.secret.iter_mut().for_each(|b| *b = 0);
        }
    }
}

fn kek(password: Option<&[u8]>, salt: &[u8; 16], iterations: u32) -> [u8; 16] {
    let mut k = [0u8; 16];
    match password {
        Some(p) => pbkdf2_sha1(p, salt, iterations, &mut k),
        None => {
            let mut d = Vec::from(&b"rs2 keychain without a master password"[..]);
            d.extend_from_slice(salt);
            k.copy_from_slice(&sha1(&d)[..16]);
        }
    }
    k
}

impl Keychain {
    /// An empty keychain without a master password. `random`: 48 random
    /// bytes (the master key and the salt).
    pub fn new(random: &[u8; 48]) -> Keychain {
        let mut master = [0u8; 32];
        master.copy_from_slice(&random[..32]);
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&random[32..]);
        let wrapped = aes_key_wrap(&kek(None, &salt, 0), &master);
        Keychain { entries: Vec::new(), master, password: false, salt, iterations: 0, wrapped }
    }

    /// Open a saved keychain; `password` is needed when [`has_password`].
    pub fn open(file: &[u8], password: Option<&[u8]>) -> Result<Keychain, Error> {
        if file.len() < HEADER + 4 + MAC || &file[..8] != MAGIC || file[8] != VERSION {
            return Err(Error::Format);
        }
        let has = file[9] & FLAG_PASSWORD != 0;
        if has && password.is_none() {
            return Err(Error::NeedsPassword);
        }
        let iterations = u32::from_le_bytes(file[12..16].try_into().unwrap());
        let salt: [u8; 16] = file[16..32].try_into().unwrap();
        let wrapped = &file[32..72];
        let k = kek(if has { password } else { None }, &salt, iterations);
        let Some(m) = aes_key_unwrap(&k, wrapped) else {
            return Err(if has { Error::BadPassword } else { Error::Corrupt });
        };
        let mut master = [0u8; 32];
        master.copy_from_slice(&m);
        let body = file.len() - MAC;
        if !ct_eq(&hmac_sha1(&master[16..], &[&file[..body]]), &file[body..]) {
            return Err(Error::Corrupt);
        }
        let nonce: [u8; 16] = file[72..88].try_into().unwrap();
        let mut plain = file[HEADER..body].to_vec();
        aes128_ctr(master[..16].try_into().unwrap(), &nonce, &mut plain);
        let entries = parse(&plain);
        plain.iter_mut().for_each(|b| *b = 0);
        let entries = entries.ok_or(Error::Corrupt)?;
        Ok(Keychain { entries, master, password: has, salt, iterations, wrapped: wrapped.to_vec() })
    }

    /// The file to store. `nonce`: 16 fresh random bytes.
    pub fn save(&self, nonce: &[u8; 16]) -> Vec<u8> {
        let mut f = Vec::with_capacity(HEADER + 64 * self.entries.len() + MAC);
        f.extend_from_slice(MAGIC);
        f.push(VERSION);
        f.push(if self.password { FLAG_PASSWORD } else { 0 });
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(&self.iterations.to_le_bytes());
        f.extend_from_slice(&self.salt);
        f.extend_from_slice(&self.wrapped);
        f.extend_from_slice(nonce);
        let mut plain = serialize(&self.entries);
        aes128_ctr(self.master[..16].try_into().unwrap(), nonce, &mut plain);
        f.extend_from_slice(&plain);
        let mac = hmac_sha1(&self.master[16..], &[&f]);
        f.extend_from_slice(&mac);
        f
    }

    pub fn has_password(&self) -> bool {
        self.password
    }

    /// Set, change (`Some`) or remove (`None`) the master password; the
    /// master key stays, so only the header changes. `salt`: 16 fresh
    /// random bytes.
    pub fn set_password(&mut self, password: Option<&[u8]>, salt: &[u8; 16]) {
        self.salt = *salt;
        self.password = password.is_some();
        self.iterations = if self.password { ITERATIONS } else { 0 };
        self.wrapped = aes_key_wrap(&kek(password, &self.salt, self.iterations), &self.master);
    }

    /// Whether `password` is the master password (a keychain without one
    /// takes none).
    pub fn check_password(&self, password: Option<&[u8]>) -> bool {
        match (self.password, password) {
            (false, None) => true,
            (true, Some(p)) => aes_key_unwrap(&kek(Some(p), &self.salt, self.iterations), &self.wrapped).is_some_and(|m| ct_eq(&m, &self.master)),
            _ => false,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, service: &str, account: &[u8]) -> Option<&Entry> {
        self.entries.iter().find(|e| e.service.eq_ignore_ascii_case(service) && e.account == account)
    }

    /// Add or replace (same service and account) an entry.
    pub fn set(&mut self, e: Entry) -> Result<(), Error> {
        if e.service.len() > 255 || e.account.len() > 255 || e.secret.len() > 0xffff {
            return Err(Error::TooLong);
        }
        self.remove(&e.service, &e.account);
        self.entries.push(e);
        Ok(())
    }

    pub fn remove(&mut self, service: &str, account: &[u8]) -> bool {
        let n = self.entries.len();
        self.entries.retain_mut(|e| {
            let hit = e.service.eq_ignore_ascii_case(service) && e.account == account;
            if hit {
                e.secret.iter_mut().for_each(|b| *b = 0);
            }
            !hit
        });
        self.entries.len() != n
    }
}

fn serialize(es: &[Entry]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(es.len() as u32).to_le_bytes());
    for e in es {
        v.push(e.readable as u8);
        v.push(e.service.len() as u8);
        v.extend_from_slice(e.service.as_bytes());
        v.push(e.account.len() as u8);
        v.extend_from_slice(&e.account);
        v.extend_from_slice(&(e.secret.len() as u16).to_le_bytes());
        v.extend_from_slice(&e.secret);
    }
    v
}

fn parse(b: &[u8]) -> Option<Vec<Entry>> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let n = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut es = Vec::new();
    for _ in 0..n {
        let flags = take(1)?[0];
        let l = take(1)?[0] as usize;
        let service = String::from(core::str::from_utf8(take(l)?).ok()?);
        let l = take(1)?[0] as usize;
        let account = take(l)?.to_vec();
        let l = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let secret = take(l)?.to_vec();
        es.push(Entry { service, account, secret, readable: flags & 1 != 0 });
    }
    Some(es)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn entry(s: &str, a: &str, k: &str, r: bool) -> Entry {
        Entry { service: s.into(), account: a.as_bytes().to_vec(), secret: k.as_bytes().to_vec(), readable: r }
    }

    fn sample() -> Keychain {
        let mut k = Keychain::new(&[7; 48]);
        k.set(entry("wifi", "Home Network", "correct horse battery staple", false)).unwrap();
        k.set(entry("mail", "me@example.org", "hunter22", true)).unwrap();
        k
    }

    #[test]
    fn round_trip_without_password() {
        let f = sample().save(&[1; 16]);
        assert!(!has_password(&f));
        // Not stored as plain text.
        assert!(!f.windows(8).any(|w| w == b"correct "));
        let k = Keychain::open(&f, None).unwrap();
        assert_eq!(k.entries().len(), 2);
        assert_eq!(k.get("WIFI", b"Home Network").unwrap().secret, b"correct horse battery staple");
        assert!(k.get("mail", b"me@example.org").unwrap().readable);
        assert!(k.get("wifi", b"Cafe").is_none());
    }

    #[test]
    fn password() {
        let mut k = sample();
        k.set_password(Some(b"open sesame"), &[3; 16]);
        let f = k.save(&[2; 16]);
        assert!(has_password(&f));
        assert_eq!(Keychain::open(&f, None).err(), Some(Error::NeedsPassword));
        assert_eq!(Keychain::open(&f, Some(b"open sesam")).err(), Some(Error::BadPassword));
        let mut k = Keychain::open(&f, Some(b"open sesame")).unwrap();
        assert!(k.check_password(Some(b"open sesame")) && !k.check_password(Some(b"x")) && !k.check_password(None));
        assert_eq!(k.get("wifi", b"Home Network").unwrap().secret, b"correct horse battery staple");
        // Removing it.
        k.set_password(None, &[4; 16]);
        let f = k.save(&[5; 16]);
        assert!(Keychain::open(&f, None).is_ok());
    }

    #[test]
    fn tampering_is_found() {
        let f = sample().save(&[1; 16]);
        for at in [9usize, 40, 80, 100, f.len() - 1] {
            let mut g = f.clone();
            g[at] ^= 1;
            assert!(Keychain::open(&g, None).is_err(), "byte {at}");
        }
        assert_eq!(Keychain::open(b"RS2KEYC1", None).err(), Some(Error::Format));
    }

    #[test]
    fn set_replaces_and_remove() {
        let mut k = sample();
        k.set(entry("wifi", "Home Network", "new passphrase", false)).unwrap();
        assert_eq!(k.entries().len(), 2);
        assert_eq!(k.get("wifi", b"Home Network").unwrap().secret, b"new passphrase");
        assert!(k.remove("wifi", b"Home Network"));
        assert!(!k.remove("wifi", b"Home Network"));
        assert_eq!(k.entries().len(), 1);
        let long = Entry { service: "x".into(), account: vec![0; 256], secret: vec![], readable: false };
        assert_eq!(k.set(long), Err(Error::TooLong));
    }
}
