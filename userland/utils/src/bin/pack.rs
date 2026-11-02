//! PACK / PACK2: compress files into an FTCOMP packed file (the format of
//! the OS/2 installation media, which UNPACK and UNPACK2 expand).
//! PACK source [packedfile] [/H:path\][name] [/D:mm-dd-yyyy] [/T:hh.mm] [/C] [/A] [/R]
//! PACK listfile [packedfile] /L [...]
//!
//! As the Toolkit's PACK: the stored name is the source's (or /H's name),
//! the stored path /H's path or, with /C, the current directory; /D and
//! /T set the stored date and time; /A adds to an existing packed file,
//! /R removes the entry with the given stored path from it; /L reads the
//! sources (one per line, each with its own /H /D /T /C; `;` starts a
//! comment) from a list file. Without a packed file name the source's
//! name is used with its last character replaced by `@`. EAs are packed
//! along. PACK and PACK2 are the same program here; both write PACK2's
//! format.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use utils::files::*;
use utils::msgs::err;
use utils::*;
os2::entry!(main);

#[derive(Clone, Default)]
struct Item {
    source: String,
    header: Option<String>,
    date: Option<u16>,
    time: Option<u16>,
    current: bool,
}

/// Apply one option to an item; false if it is not one of the per-file
/// options.
fn item_opt(it: &mut Item, a: &str) -> Result<bool, ()> {
    let u = a.to_ascii_uppercase();
    if let Some(h) = u.strip_prefix("/H:") {
        let _ = h;
        it.header = Some(String::from(&a[3..]));
    } else if let Some(d) = u.strip_prefix("/D:") {
        let p: Vec<u32> = d.split(['-', '/', '.']).map(|x| x.parse().map_err(|_| ())).collect::<Result<_, _>>()?;
        if p.len() != 3 {
            return Err(());
        }
        let y = if p[2] < 80 { p[2] + 2000 } else if p[2] < 100 { p[2] + 1900 } else { p[2] };
        if !(1..=12).contains(&p[0]) || !(1..=31).contains(&p[1]) || y < 1980 {
            return Err(());
        }
        it.date = Some((((y - 1980) << 9) | (p[0] << 5) | p[1]) as u16);
    } else if let Some(t) = u.strip_prefix("/T:") {
        let p: Vec<u32> = t.split(['.', ':']).map(|x| x.parse().map_err(|_| ())).collect::<Result<_, _>>()?;
        if p.len() != 2 || p[0] > 23 || p[1] > 59 {
            return Err(());
        }
        it.time = Some(((p[0] << 11) | (p[1] << 5)) as u16);
    } else if u == "/C" {
        it.current = true;
    } else {
        return Ok(false);
    }
    Ok(true)
}

fn current_dir() -> String {
    let mut buf = [0u8; 260];
    let mut cb = buf.len() as u32;
    unsafe { DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut cb) };
    let d: String = buf.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    alloc::format!("\\{d}")
}

/// The path stored for a file.
fn stored_path(it: &Item, name: &str) -> String {
    match &it.header {
        Some(h) if h.ends_with('\\') => alloc::format!("{h}{name}"),
        Some(h) => String::from(h.as_str()),
        None if it.current => {
            let c = current_dir();
            join(&c, name)
        }
        None => String::from(name),
    }
}

fn default_target(src: &str) -> String {
    let mut t = String::from(base_name(src));
    if !t.contains('.') {
        t.push('.');
    }
    t.pop();
    t.push('@');
    t
}

fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (mut list, mut append, mut remove) = (false, false, false);
    let mut common = Item::default();
    let mut names: Vec<String> = Vec::new();
    for x in &a {
        match x.to_ascii_uppercase().as_str() {
            "/L" => list = true,
            "/A" => append = true,
            "/R" => remove = true,
            _ if x.starts_with('/') => match item_opt(&mut common, x) {
                Ok(true) => {}
                _ => {
                    err(1003, &[], "The command syntax is not correct.\r\n");
                    return 1;
                }
            },
            _ => names.push(x.clone()),
        }
    }
    if names.is_empty() || names.len() > 2 || (common.current && common.header.as_deref().is_some_and(|h| h.contains('\\'))) {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1;
    }
    let target = names.get(1).cloned().unwrap_or_else(|| default_target(&names[0]));
    let mut file: Vec<u8> = Vec::new();
    let mut count = 0u32;
    let mut last = None;
    if append || remove {
        if let Ok(old) = read_file(&target) {
            let Ok(entries) = os2_pack::entries(&old) else {
                outln(&alloc::format!("{target} is not a packed file."));
                return 1;
            };
            // Rebuild: keep the old entries (raw), relinked.
            for e in &entries {
                let stored: String = e.path.iter().map(|&c| c as char).collect();
                if remove && stored.eq_ignore_ascii_case(&names[0]) {
                    continue;
                }
                let start = e.offset;
                let end = entries.iter().map(|x| x.offset).filter(|&o| o > start).min().unwrap_or(old.len() - 4);
                let at = file.len();
                let mut raw = old[start..end].to_vec();
                raw[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
                let ea = u32::from_le_bytes([raw[0x0c], raw[0x0d], raw[0x0e], raw[0x0f]]);
                if ea != 0 {
                    let moved = ea as usize - start + at;
                    raw[0x0c..0x10].copy_from_slice(&(moved as u32).to_le_bytes());
                }
                if let Some(p) = last {
                    let v = (at as u32).to_le_bytes();
                    let f: &mut Vec<u8> = &mut file;
                    f[p + 0x14..p + 0x18].copy_from_slice(&v);
                }
                file.extend_from_slice(&raw);
                last = Some(at);
                count += 1;
            }
            if remove && count as usize == entries.len() {
                outln("The file to remove was not found in the packed file.");
                return 1;
            }
        } else if remove {
            err(2, &[], "The file cannot be found.\r\n");
            return 2;
        }
    }
    if !remove {
        let mut items = Vec::new();
        if list {
            let Ok(l) = read_file(&names[0]) else {
                err(2, &[], "The file cannot be found.\r\n");
                return 2;
            };
            let mut cur: Option<Item> = None;
            for line in lines(&l) {
                let line = line.trim();
                if line.is_empty() || line.starts_with(';') {
                    continue;
                }
                for w in split(line) {
                    if w.starts_with('/') {
                        let Some(it) = cur.as_mut() else { continue };
                        if !matches!(item_opt(it, &w), Ok(true)) {
                            outln(&alloc::format!("{w} is not a correct option in the list file."));
                            return 1;
                        }
                    } else {
                        if let Some(it) = cur.take() {
                            items.push(it);
                        }
                        cur = Some(Item { source: w, ..Item::default() });
                    }
                }
            }
            items.extend(cur);
        } else {
            let dir = dir_of(&names[0]);
            for f in find_all(&names[0], FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED) {
                if f.attr & FILE_DIRECTORY == 0 {
                    items.push(Item { source: alloc::format!("{dir}{}", f.name), ..common.clone() });
                }
            }
        }
        if items.is_empty() {
            err(2, &[], "The file cannot be found.\r\n");
            return 2;
        }
        for it in &items {
            if full_path(&it.source).eq_ignore_ascii_case(&full_path(&target)) {
                continue;
            }
            let data = match read_file(&it.source) {
                Ok(d) => d,
                Err(e) => {
                    error(e);
                    return e;
                }
            };
            let st = status(&it.source).unwrap_or_default();
            let eas = os2_backup::fea2list(&read_eas(&it.source).unwrap_or_default());
            let name = base_name(&it.source).to_ascii_uppercase();
            let path = bytes(&stored_path(it, &name));
            let src = os2_pack::Source {
                path: &path,
                date: it.date.unwrap_or(st.fdate_write),
                time: it.time.unwrap_or(st.ftime_write),
                attributes: st.attr & 0x27,
                data: &data,
                eas: if eas.len() > 4 { &eas } else { &[] },
            };
            last = Some(os2_pack::add_entry(&mut file, last, &src));
            outln(&alloc::format!("{} -> {}", full_path(&it.source), stored_path(it, &name)));
            count += 1;
        }
    }
    file.extend_from_slice(&count.to_le_bytes());
    if count == 0 {
        let c = os2::path::cstr(&target);
        unsafe { DosDelete(c.as_ptr()) };
        return 0;
    }
    if let Err(e) = write_file(&target, &file) {
        error(e);
        return e;
    }
    0
}

