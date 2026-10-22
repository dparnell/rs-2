//! Shadows: objects that stand for another object (a file or folder, a
//! program object, a program group, a drive...) in some folder, as the
//! Workplace Shell's are. They live in OS2.INI, application
//! `RS2_Shadows`: key `S` + 8 hex digits, value `FOLDER` TAB `TARGET`,
//! where FOLDER is the folder id they are in (`DESKTOP`, `G` + group
//! handle, `DIR:` + directory) and TARGET a full path or an object id
//! (`P`/`G` + handle, `DRIVES`, `DRIVE:C`, `MINIMIZED`, `WINLIST`,
//! `SHUTDOWN`). A shadow whose target is gone is broken. The shell
//! follows its own renames, moves and deletes of targets. (Free of the
//! shell's windows: DESKTEST includes this file.)

#![allow(dead_code)]

use alloc::vec::Vec;

use crate::fileops;
use crate::objects;
use crate::pm::*;

pub const APP_SHADOWS: &[u8] = b"RS2_Shadows\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Path(Vec<u8>),
    Program(u32),
    Group(u32),
    Drives,
    Drive(u8),
    Minimized,
    WindowList,
    Shutdown,
}

impl Target {
    pub fn parse(t: &[u8]) -> Target {
        if t.len() >= 2 && t[1] == b':' {
            return Target::Path(t.to_vec());
        }
        match t {
            b"DRIVES" => Target::Drives,
            b"MINIMIZED" => Target::Minimized,
            b"WINLIST" => Target::WindowList,
            b"SHUTDOWN" => Target::Shutdown,
            _ => {
                if let Some(c) = t.strip_prefix(b"DRIVE:") {
                    return Target::Drive(c.first().copied().unwrap_or(b'C'));
                }
                match (t.first(), objects::parse_hex(t.get(1..).unwrap_or(&[]))) {
                    (Some(b'P'), Some(h)) => Target::Program(h),
                    (Some(b'G'), Some(h)) => Target::Group(h),
                    _ => Target::Path(t.to_vec()),
                }
            }
        }
    }

    pub fn id(&self) -> Vec<u8> {
        match self {
            Target::Path(p) => p.clone(),
            Target::Program(h) => [&b"P"[..], &objects::hex(*h)].concat(),
            Target::Group(h) => [&b"G"[..], &objects::hex(*h)].concat(),
            Target::Drives => b"DRIVES".to_vec(),
            Target::Drive(c) => alloc::vec![b'D', b'R', b'I', b'V', b'E', b':', *c],
            Target::Minimized => b"MINIMIZED".to_vec(),
            Target::WindowList => b"WINLIST".to_vec(),
            Target::Shutdown => b"SHUTDOWN".to_vec(),
        }
    }

    /// Whether the target is there.
    pub fn exists(&self) -> bool {
        match self {
            Target::Path(p) => fileops::exists(p) || fileops::is_dir(p) && p.len() <= 3,
            Target::Program(h) => objects::definition(*h).is_some(),
            Target::Group(h) => objects::group_exists(*h),
            Target::Drive(c) => {
                let (mut cur, mut map) = (0u32, 0u32);
                unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
                c.is_ascii_alphabetic() && map & (1 << (c.to_ascii_uppercase() - b'A')) != 0
            }
            _ => true,
        }
    }

    /// The target's title (a shadow shows it).
    pub fn title(&self) -> Vec<u8> {
        match self {
            Target::Path(p) => {
                let n = fileops::name_of(p);
                if n.is_empty() || n.len() <= 3 && n.get(1) == Some(&b':') { fileops::strip(p).to_vec() } else { n.to_vec() }
            }
            Target::Program(h) => objects::definition(*h).map_or(b"Program".to_vec(), |p| p.title),
            Target::Group(h) => objects::titles(SGH_ROOT).into_iter().find(|g| g.0 == *h).map_or(b"Folder".to_vec(), |g| g.3),
            Target::Drives => b"Drives".to_vec(),
            Target::Drive(c) => alloc::vec![b'D', b'r', b'i', b'v', b'e', b' ', *c],
            Target::Minimized => b"Minimized\nWindow Viewer".to_vec(),
            Target::WindowList => b"Window List".to_vec(),
            Target::Shutdown => b"Shutdown".to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadow {
    pub id: u32,
    pub folder: Vec<u8>,
    pub target: Target,
}

pub fn key(id: u32) -> Vec<u8> {
    [&b"S"[..], &objects::hex(id)].concat()
}

/// Every shadow.
pub fn all() -> Vec<Shadow> {
    let mut out = Vec::new();
    for k in objects::keys(APP_SHADOWS) {
        let Some(id) = k.strip_prefix(b"S").and_then(objects::parse_hex) else { continue };
        let Some(v) = objects::query(APP_SHADOWS, &k) else { continue };
        let Some(tab) = v.iter().position(|&c| c == b'\t') else { continue };
        out.push(Shadow { id, folder: v[..tab].to_vec(), target: Target::parse(&v[tab + 1..]) });
    }
    out.sort_by_key(|s| s.id);
    out
}

/// The shadows in a folder.
pub fn in_folder(folder: &[u8]) -> Vec<Shadow> {
    all().into_iter().filter(|s| s.folder.eq_ignore_ascii_case(folder)).collect()
}

pub fn get(id: u32) -> Option<Shadow> {
    all().into_iter().find(|s| s.id == id)
}

fn write(s: &Shadow) -> bool {
    let mut v = s.folder.clone();
    v.push(b'\t');
    v.extend_from_slice(&s.target.id());
    objects::write(APP_SHADOWS, &key(s.id), Some(&v))
}

/// A new shadow of `target` in `folder`; its id.
pub fn create(folder: &[u8], target: Target) -> u32 {
    let next = objects::query_u32(objects::APP_DESKTOP, b"NextShadow").unwrap_or(1).max(all().iter().map(|s| s.id + 1).max().unwrap_or(1));
    objects::write(objects::APP_DESKTOP, b"NextShadow", Some(&objects::hex(next + 1)));
    let s = Shadow { id: next, folder: folder.to_vec(), target };
    write(&s);
    next
}

pub fn remove(id: u32) -> bool {
    objects::write(APP_SHADOWS, &key(id), None)
}

pub fn set_folder(id: u32, folder: &[u8]) -> bool {
    match get(id) {
        Some(mut s) => {
            s.folder = folder.to_vec();
            write(&s)
        }
        None => false,
    }
}

/// A file or directory moved or renamed: shadows of it (and of what was
/// in it) follow, and so do shadows kept in a directory folder that
/// moved. Returns how many changed.
pub fn path_moved(old: &[u8], new: &[u8]) -> u32 {
    let (o, n) = (fileops::strip(old), fileops::strip(new));
    let mut changed = 0;
    for mut s in all() {
        let mut dirty = false;
        if let Target::Path(p) = &s.target {
            if fileops::is_within(p, o) {
                let rest = &fileops::strip(p)[o.len()..];
                s.target = Target::Path([n, rest].concat());
                dirty = true;
            }
        }
        if let Some(d) = s.folder.strip_prefix(b"DIR:") {
            if fileops::is_within(d, o) {
                let rest = &fileops::strip(d)[o.len()..];
                let mut f = [&b"DIR:"[..], n, rest].concat();
                if !f.ends_with(b"\\") {
                    f.push(b'\\');
                }
                s.folder = f;
                dirty = true;
            }
        }
        if dirty {
            write(&s);
            changed += 1;
        }
    }
    changed
}

/// An object deleted (through the shell): its shadows go too, as do the
/// shadows kept in a deleted directory or group folder.
pub fn target_deleted(t: &Target) -> u32 {
    let mut n = 0;
    for s in all() {
        let gone = match (&s.target, t) {
            (Target::Path(p), Target::Path(d)) => fileops::is_within(p, d),
            (a, b) => a == b,
        };
        let folder_gone = match t {
            Target::Path(d) => s.folder.strip_prefix(b"DIR:").is_some_and(|f| fileops::is_within(f, d)),
            Target::Group(h) => s.folder == [&b"G"[..], &objects::hex(*h)].concat(),
            _ => false,
        };
        if gone || folder_gone {
            remove(s.id);
            n += 1;
        }
    }
    n
}

/// A group got a new handle (renaming makes a new group): its shadows,
/// and the shadows kept in it, follow.
pub fn group_renamed(old: u32, new: u32) {
    let (of, nf) = ([&b"G"[..], &objects::hex(old)].concat(), [&b"G"[..], &objects::hex(new)].concat());
    for mut s in all() {
        let mut dirty = false;
        if s.target == Target::Group(old) {
            s.target = Target::Group(new);
            dirty = true;
        }
        if s.folder == of {
            s.folder = nf.clone();
            dirty = true;
        }
        if dirty {
            write(&s);
        }
    }
}

/// A program object got a new handle (moved to another group).
pub fn program_moved(old: u32, new: u32) {
    for mut s in all() {
        if s.target == Target::Program(old) {
            s.target = Target::Program(new);
            write(&s);
        }
    }
}
