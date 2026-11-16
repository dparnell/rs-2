//! The system keychain (docs/KEYCHAIN.md): secrets such as WiFi
//! passphrases, kept encrypted in `C:\RS2\KEYCHAIN\KEYCHAIN.DAT`
//! (`keychain` crate). Programs cannot open, copy or move that
//! directory ([`protected`], checked in `api::file`); they reach the
//! keychain through RS2API 630-635 (`api::keychain`). The kernel opens it
//! on first use; one with a master password stays locked until
//! [`unlock`].

use crate::sync::Mutex;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use keychain::{Entry, Error, Keychain};

pub const DIR: &str = "C:\\RS2\\KEYCHAIN";
pub const FILE: &str = "C:\\RS2\\KEYCHAIN\\KEYCHAIN.DAT";
const NEW: &str = "KEYCHAIN.NEW";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Not read yet.
    Unknown,
    /// No keychain file yet (an empty keychain is used; saved on the
    /// first change).
    Empty,
    Unlocked,
    /// It has a master password that has not been given.
    Locked,
    /// The file could not be read as a keychain (it is left alone).
    Damaged,
}

struct Kc {
    state: State,
    kc: Option<Keychain>,
}

static KC: Mutex<Kc> = Mutex::new(Kc { state: State::Unknown, kc: None });

/// Whether a program's path is the keychain directory or inside it
/// (refused to programs: `api::file::resolve`).
pub fn protected(p: &crate::fs::path::FullPath) -> bool {
    p.drive == 2 && p.parts.len() >= 2 && p.parts[0].eq_ignore_ascii_case("RS2") && p.parts[1].eq_ignore_ascii_case("KEYCHAIN")
}

/// Whether moving or renaming `p` would take the keychain directory
/// with it (`C:\`, `C:\RS2`).
pub fn contains_protected(p: &crate::fs::path::FullPath) -> bool {
    p.drive == 2 && (p.parts.is_empty() || (p.parts.len() == 1 && p.parts[0].eq_ignore_ascii_case("RS2")))
}

/// Random bytes: RDRAND or virtio-rng, else hashed timer readings.
fn random(buf: &mut [u8]) {
    let n = crate::chardev::random_fill(buf);
    let mut at = n;
    let mut ctr = 0u32;
    while at < buf.len() {
        let mut s = rs2crypto::Sha1::new();
        for _ in 0..8 {
            s.update(&crate::arch::rdtsc().to_le_bytes());
            s.update(&crate::irq::uptime_ms().to_le_bytes());
        }
        s.update(&ctr.to_le_bytes());
        let h = s.finish();
        let k = (buf.len() - at).min(20);
        buf[at..at + k].copy_from_slice(&h[..k]);
        at += k;
        ctr += 1;
    }
}

fn load(k: &mut Kc) {
    if k.state != State::Unknown {
        return;
    }
    let Some(data) = crate::fs::read_file(FILE) else {
        k.state = State::Empty;
        return;
    };
    match Keychain::open(&data, None) {
        Ok(c) => {
            crate::println!("keychain: {} entr{}", c.entries().len(), if c.entries().len() == 1 { "y" } else { "ies" });
            k.kc = Some(c);
            k.state = State::Unlocked;
        }
        Err(Error::NeedsPassword) => {
            crate::println!("keychain: locked (KEYCHAIN UNLOCK gives its master password)");
            k.state = State::Locked;
        }
        Err(e) => {
            crate::println!("keychain: {FILE}: {e}; not used");
            k.state = State::Damaged;
        }
    }
}

/// The open keychain, an empty one if there is no file yet.
fn open(k: &mut Kc) -> Result<&mut Keychain, Error> {
    load(k);
    match k.state {
        State::Locked => return Err(Error::NeedsPassword),
        State::Damaged => return Err(Error::Corrupt),
        State::Empty if k.kc.is_none() => {
            let mut r = [0u8; 48];
            random(&mut r);
            k.kc = Some(Keychain::new(&r));
        }
        _ => {}
    }
    Ok(k.kc.as_mut().unwrap())
}

fn path(p: &str) -> crate::fs::path::FullPath {
    let empty: [String; 26] = Default::default();
    crate::fs::path::canonical(p, 2, &empty).expect("keychain path")
}

/// Write the keychain: KEYCHAIN.NEW first, then it replaces KEYCHAIN.DAT.
fn save(k: &mut Kc) -> Result<(), u32> {
    let Some(c) = k.kc.as_ref() else { return Ok(()) };
    let mut nonce = [0u8; 16];
    random(&mut nonce);
    let data = c.save(&nonce);
    let dir = path(DIR);
    let fs = crate::fs::fs_of(&dir)?;
    if fs.stat("RS2").is_err() {
        fs.mkdir("RS2")?;
    }
    if fs.stat(&dir.rel()).is_err() {
        fs.mkdir(&dir.rel())?;
    }
    let new = alloc::format!("{}\\{NEW}", dir.rel());
    let old = path(FILE).rel();
    crate::fs::write_file(&alloc::format!("{DIR}\\{NEW}"), &data)?;
    let _ = fs.delete(&old);
    fs.rename(&new, &old)?;
    k.state = State::Unlocked;
    Ok(())
}

/// Why a keychain call failed, as an OS/2 error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KcError {
    Locked,
    BadPassword,
    Damaged,
    NotFound,
    NotReadable,
    TooLong,
    Io(u32),
}

impl From<Error> for KcError {
    fn from(e: Error) -> KcError {
        match e {
            Error::NeedsPassword => KcError::Locked,
            Error::BadPassword => KcError::BadPassword,
            Error::TooLong => KcError::TooLong,
            Error::Format | Error::Corrupt => KcError::Damaged,
        }
    }
}

impl core::fmt::Display for KcError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            KcError::Locked => f.write_str("the keychain is locked (KEYCHAIN UNLOCK)"),
            KcError::BadPassword => f.write_str("wrong master password"),
            KcError::Damaged => f.write_str("the keychain file is damaged"),
            KcError::NotFound => f.write_str("no such entry"),
            KcError::NotReadable => f.write_str("that entry is for the system only"),
            KcError::TooLong => f.write_str("a name or secret is too long"),
            KcError::Io(e) => write!(f, "cannot write {FILE} (error {e})"),
        }
    }
}

/// Whether the keychain waits for its master password.
pub fn is_locked() -> bool {
    let mut k = KC.lock();
    load(&mut k);
    k.state == State::Locked
}

/// A secret, for the kernel (any entry).
pub fn get(service: &str, account: &[u8]) -> Option<Vec<u8>> {
    let mut k = KC.lock();
    let c = open(&mut k).ok()?;
    c.get(service, account).map(|e| e.secret.clone())
}

/// A secret for a program: only entries marked readable.
pub fn get_readable(service: &str, account: &[u8]) -> Result<Vec<u8>, KcError> {
    let mut k = KC.lock();
    let c = open(&mut k)?;
    let e = c.get(service, account).ok_or(KcError::NotFound)?;
    if !e.readable {
        return Err(KcError::NotReadable);
    }
    Ok(e.secret.clone())
}

/// Store (or replace) a secret and save the keychain.
pub fn set(service: &str, account: &[u8], secret: &[u8], readable: bool) -> Result<(), KcError> {
    let mut k = KC.lock();
    let c = open(&mut k)?;
    if c.get(service, account).is_some_and(|e| e.secret == secret && e.readable == readable) {
        return Ok(());
    }
    c.set(Entry { service: service.to_string(), account: account.to_vec(), secret: secret.to_vec(), readable })?;
    save(&mut k).map_err(KcError::Io)
}

pub fn remove(service: &str, account: &[u8]) -> Result<(), KcError> {
    let mut k = KC.lock();
    let c = open(&mut k)?;
    if !c.remove(service, account) {
        return Err(KcError::NotFound);
    }
    save(&mut k).map_err(KcError::Io)
}

/// Give the master password of a locked keychain.
pub fn unlock(password: &[u8]) -> Result<(), KcError> {
    {
        let mut k = KC.lock();
        load(&mut k);
        match k.state {
            State::Locked => {
                let data = crate::fs::read_file(FILE).ok_or(KcError::Damaged)?;
                let c = Keychain::open(&data, Some(password))?;
                crate::println!("keychain: unlocked, {} entries", c.entries().len());
                k.kc = Some(c);
                k.state = State::Unlocked;
            }
            State::Unlocked | State::Empty => {
                if !k.kc.as_ref().is_none_or(|c| c.check_password(Some(password))) {
                    return Err(KcError::BadPassword);
                }
            }
            _ => return Err(KcError::Damaged),
        }
    }
    // Networks waiting for their passphrase can be joined now.
    crate::net::wlan::kick();
    Ok(())
}

/// Forget the keys until the next [`unlock`] (only with a master
/// password: without one there would be nothing to unlock with).
pub fn lock() -> Result<(), KcError> {
    let mut k = KC.lock();
    load(&mut k);
    match k.kc.as_ref() {
        Some(c) if c.has_password() => {
            k.kc = None;
            k.state = State::Locked;
            Ok(())
        }
        Some(_) | None if k.state == State::Locked => Ok(()),
        _ => Err(KcError::NotFound),
    }
}

/// Set, change or remove (`new` None) the master password; `old` is the
/// current one, if there is one.
pub fn set_password(old: Option<&[u8]>, new: Option<&[u8]>) -> Result<(), KcError> {
    let mut k = KC.lock();
    load(&mut k);
    if k.state == State::Locked {
        let data = crate::fs::read_file(FILE).ok_or(KcError::Damaged)?;
        k.kc = Some(Keychain::open(&data, old)?);
        k.state = State::Unlocked;
    }
    let c = open(&mut k)?;
    if !c.check_password(if c.has_password() { old.or(Some(b"")) } else { None }) {
        return Err(KcError::BadPassword);
    }
    let mut salt = [0u8; 16];
    random(&mut salt);
    c.set_password(new, &salt);
    save(&mut k).map_err(KcError::Io)
}

/// `key=value` lines for KEYCHAIN.EXE: `state=`, `password=`, then one
/// `entry=service<TAB>account<TAB>readable` per entry (no secrets).
pub fn status_text() -> String {
    let mut k = KC.lock();
    load(&mut k);
    let state = match k.state {
        State::Unknown | State::Empty => "empty",
        State::Unlocked => "unlocked",
        State::Locked => "locked",
        State::Damaged => "damaged",
    };
    let mut t = alloc::format!("state={state}\nfile={FILE}\n");
    if let Some(c) = k.kc.as_ref() {
        t += &alloc::format!("password={}\n", if c.has_password() { "yes" } else { "no" });
        for e in c.entries() {
            let acct: String = e.account.iter().map(|&b| if b < 0x20 { '?' } else { b as char }).collect();
            t += &alloc::format!("entry={}\t{acct}\t{}\n", e.service, if e.readable { 1 } else { 0 });
        }
    } else if k.state == State::Locked {
        t += "password=yes\n";
    }
    t
}
