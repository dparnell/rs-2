//! Paths, devices, handles and the file and directory calls.

use alloc::string::String;
use alloc::vec::Vec;

use x86emu::flags::CF;
use x86emu::{DS, ES};

use super::{err, Search, Sft, SFT_COUNT};
use crate::host::{Create, OpenMode};
use crate::{Cb, Host, Machine};

/// Character devices DOS knows by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Con,
    Nul,
    Aux,
    Prn,
    Clock,
    Emm,
    Xms,
}

impl Device {
    fn from_name(n: &[u8]) -> Option<Device> {
        Some(match n {
            b"CON" => Device::Con,
            b"NUL" => Device::Nul,
            b"AUX" | b"COM1" | b"COM2" | b"COM3" | b"COM4" => Device::Aux,
            b"PRN" | b"LPT1" | b"LPT2" | b"LPT3" => Device::Prn,
            b"CLOCK$" => Device::Clock,
            b"EMMXXXX0" => Device::Emm,
            b"XMSXXXX0" => Device::Xms,
            _ => return None,
        })
    }

    /// The IOCTL device information word.
    pub fn info(self) -> u16 {
        match self {
            Device::Con => 0x80d3,
            Device::Nul => 0x80c4,
            Device::Clock => 0x80c8,
            Device::Aux => 0x80c0,
            Device::Prn => 0xa0c0,
            Device::Emm | Device::Xms => 0xc0c0,
        }
    }
}

/// A resolved name.
#[derive(Clone, Debug)]
pub enum Name {
    File(String),
    Dev(Device),
}

/// The 11-byte FCB form of a name (`NAME    EXT`), wildcards expanded.
pub fn fcb_form(name: &[u8]) -> [u8; 11] {
    let mut f = [b' '; 11];
    let (base, ext) = match name.iter().rposition(|&c| c == b'.') {
        Some(i) if name != b"." && name != b".." => (&name[..i], &name[i + 1..]),
        _ => (name, &b""[..]),
    };
    let fill = |dst: &mut [u8], src: &[u8]| {
        let mut i = 0;
        for &c in src {
            if i >= dst.len() {
                break;
            }
            if c == b'*' {
                while i < dst.len() {
                    dst[i] = b'?';
                    i += 1;
                }
                break;
            }
            dst[i] = c;
            i += 1;
        }
    };
    fill(&mut f[..8], base);
    fill(&mut f[8..], ext);
    f
}

/// Does `name` (a directory entry) match the FCB-form `pattern`?
pub fn matches(pattern: &[u8; 11], name: &[u8]) -> bool {
    let f = fcb_form(name);
    pattern.iter().zip(f.iter()).all(|(p, c)| *p == b'?' || p == c)
}

/// Is `name` a valid 8.3 name DOS programs can see?
pub fn is_short(name: &str) -> bool {
    if name == "." || name == ".." {
        return true;
    }
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 12 {
        return false;
    }
    let (base, ext) = match name.find('.') {
        Some(i) => (&b[..i], &b[i + 1..]),
        None => (b, &b""[..]),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 || ext.contains(&b'.') {
        return false;
    }
    b.iter().all(|&c| c > b' ' && !b"\"*+,/:;<=>?[\\]|".contains(&c))
}

impl Machine {
    pub(crate) fn drive_valid<H: Host>(&mut self, host: &mut H, d: u8) -> bool {
        let _ = host;
        d < 26 && self.dos.present[d as usize]
    }

    /// Resolve a DOS path to a full name or a device.
    pub fn resolve(&mut self, raw: &[u8]) -> Result<Name, u16> {
        let mut p: Vec<u8> = raw.iter().copied().take_while(|&c| c != 0).collect();
        while p.last() == Some(&b' ') {
            p.pop();
        }
        if p.is_empty() {
            return Err(err::PATH_NOT_FOUND);
        }
        let mut drive = self.dos.drive;
        let mut rest: &[u8] = &p;
        if p.len() >= 2 && p[1] == b':' {
            let d = self.upcase(p[0]);
            if !d.is_ascii_uppercase() {
                return Err(err::INVALID_DRIVE);
            }
            drive = d - b'A';
            rest = &p[2..];
        }
        if !self.dos.present[drive as usize] {
            return Err(err::INVALID_DRIVE);
        }
        let mut parts: Vec<Vec<u8>> = Vec::new();
        if !rest.starts_with(b"\\") && !rest.starts_with(b"/") {
            for c in self.dos.cwd[drive as usize].split('\\').filter(|c| !c.is_empty()) {
                parts.push(c.as_bytes().to_vec());
            }
        }
        for comp in rest.split(|&c| c == b'\\' || c == b'/') {
            if comp.is_empty() || comp == b"." {
                continue;
            }
            if comp == b".." {
                if parts.pop().is_none() {
                    return Err(err::PATH_NOT_FOUND);
                }
                continue;
            }
            if comp.iter().all(|&c| c == b'.') {
                // "..." and beyond climb further in DOS: not supported.
                return Err(err::PATH_NOT_FOUND);
            }
            let up: Vec<u8> = comp.iter().map(|&c| self.upcase(c)).collect();
            // Truncate to 8.3 as DOS does.
            let (base, ext) = match up.iter().position(|&c| c == b'.') {
                Some(i) => (&up[..i], Some(&up[i + 1..])),
                None => (&up[..], None),
            };
            let mut n: Vec<u8> = base.iter().copied().take(8).collect();
            if let Some(e) = ext {
                let e: Vec<u8> = e.iter().copied().filter(|&c| c != b'.').take(3).collect();
                if !e.is_empty() {
                    n.push(b'.');
                    n.extend_from_slice(&e);
                }
            }
            parts.push(n);
        }
        // Devices anywhere, with or without an extension.
        if let Some(last) = parts.last() {
            let base: Vec<u8> = last.iter().copied().take_while(|&c| c != b'.').collect();
            if let Some(d) = Device::from_name(&base) {
                let enabled = match d {
                    Device::Emm => self.cfg.ems_kb > 0,
                    Device::Xms => self.cfg.xms_kb > 0,
                    _ => true,
                };
                if enabled {
                    return Ok(Name::Dev(d));
                }
            }
        }
        let mut s = String::new();
        s.push((b'A' + drive) as char);
        s.push_str(":\\");
        for (i, c) in parts.iter().enumerate() {
            if i > 0 {
                s.push('\\');
            }
            for &b in c {
                s.push(b as char);
            }
        }
        Ok(Name::File(s))
    }

    /// A path at DS:DX resolved to a file name (devices refused).
    fn file_at(&mut self, a: u32) -> Result<String, u16> {
        let raw = self.hw.cstr(a, 128);
        match self.resolve(&raw)? {
            Name::File(f) => Ok(f),
            Name::Dev(_) => Err(err::ACCESS_DENIED),
        }
    }

    // ---- Handles ----

    /// The PSP's job file table: (far address, size).
    fn jft(&mut self, psp: u16) -> (u32, u16) {
        let a = Machine::lin(psp, 0);
        let n = self.hw.rd16(a + 0x32);
        let off = self.hw.rd16(a + 0x34);
        let seg = self.hw.rd16(a + 0x36);
        (Machine::lin(seg, off), n)
    }

    /// The SFT index of handle `h` of the current process.
    pub fn sft_of(&mut self, h: u16) -> Option<usize> {
        let psp = self.dos.psp;
        let (t, n) = self.jft(psp);
        if h >= n {
            return None;
        }
        let i = self.hw.rd(t + h as u32);
        if i == 0xff || self.dos.sft.get(i as usize).is_none_or(|s| s.is_none()) {
            return None;
        }
        Some(i as usize)
    }

    pub fn new_sft(&mut self, s: Sft) -> Result<usize, u16> {
        for (i, e) in self.dos.sft.iter_mut().enumerate() {
            if e.is_none() {
                *e = Some(s);
                return Ok(i);
            }
        }
        if self.dos.sft.len() < SFT_COUNT as usize {
            self.dos.sft.push(Some(s));
            return Ok(self.dos.sft.len() - 1);
        }
        Err(err::TOO_MANY_FILES)
    }

    /// Give SFT entry `i` a handle in the current process.
    fn new_handle(&mut self, i: usize) -> Result<u16, u16> {
        let psp = self.dos.psp;
        let (t, n) = self.jft(psp);
        for h in 0..n {
            if self.hw.rd(t + h as u32) == 0xff {
                self.hw.wr(t + h as u32, i as u8);
                return Ok(h);
            }
        }
        Err(err::TOO_MANY_FILES)
    }

    /// Drop one reference to SFT entry `i`.
    pub fn release_sft<H: Host>(&mut self, host: &mut H, i: usize) {
        let Some(Some(s)) = self.dos.sft.get_mut(i) else { return };
        s.refs = s.refs.saturating_sub(1);
        if s.refs == 0 {
            if s.dev.is_none() {
                host.close(s.host);
            }
            self.dos.sft[i] = None;
        }
    }

    pub fn close_handle<H: Host>(&mut self, host: &mut H, psp: u16, h: u16) -> Result<(), u16> {
        let (t, n) = self.jft(psp);
        if h >= n {
            return Err(err::INVALID_HANDLE);
        }
        let i = self.hw.rd(t + h as u32);
        if i == 0xff {
            return Err(err::INVALID_HANDLE);
        }
        self.hw.wr(t + h as u32, 0xff);
        self.release_sft(host, i as usize);
        Ok(())
    }

    /// Open a resolved name.
    pub fn open_name<H: Host>(&mut self, host: &mut H, name: &Name, mode: u8, create: Option<(u8, Create)>) -> Result<usize, u16> {
        let psp = self.dos.psp;
        let s = match name {
            Name::Dev(d) => Sft { refs: 1, dev: Some(*d), host: 0, path: String::new(), mode, info: d.info(), owner: psp },
            Name::File(f) => {
                let h = match create {
                    Some((attr, how)) => host.create(f, attr, how)?,
                    None => host.open(f, OpenMode(mode))?,
                };
                let drive = (f.as_bytes()[0] - b'A') as u16;
                Sft { refs: 1, dev: None, host: h, path: f.clone(), mode, info: drive | 0x40, owner: psp }
            }
        };
        let host_h = s.host;
        let is_dev = s.dev.is_some();
        match self.new_sft(s) {
            Ok(i) => Ok(i),
            Err(e) => {
                if !is_dev {
                    host.close(host_h);
                }
                Err(e)
            }
        }
    }

    fn open_handle<H: Host>(&mut self, host: &mut H, name: &Name, mode: u8, create: Option<(u8, Create)>) -> Result<u16, u16> {
        let i = self.open_name(host, name, mode, create)?;
        match self.new_handle(i) {
            Ok(h) => Ok(h),
            Err(e) => {
                self.release_sft(host, i);
                Err(e)
            }
        }
    }

    /// Read from handle `h` into guest memory at `a`.
    pub fn read_handle<H: Host>(&mut self, host: &mut H, h: u16, a: u32, n: usize) -> Result<Option<usize>, u16> {
        let i = self.sft_of(h).ok_or(err::INVALID_HANDLE)?;
        let s = self.dos.sft[i].clone().unwrap();
        match s.dev {
            Some(Device::Con) => {
                if s.info & 0x20 != 0 {
                    // Raw: one key at a time, no echo.
                    let mut got = 0;
                    while got < n {
                        match self.con_key() {
                            Some(c) => {
                                self.hw.wr(a + got as u32, c);
                                got += 1;
                            }
                            None => break,
                        }
                    }
                    if got == 0 {
                        return Ok(None);
                    }
                    Ok(Some(got))
                } else {
                    self.con_read_cooked(host, a, n)
                }
            }
            Some(Device::Clock) => {
                let t = host.now();
                let data = [0u8, 0, t.minute, t.hour, t.hundredths, t.second];
                let k = n.min(6);
                for (j, b) in data.iter().take(k).enumerate() {
                    self.hw.wr(a + j as u32, *b);
                }
                Ok(Some(k))
            }
            Some(_) => Ok(Some(0)),
            None => {
                if s.mode & 7 == 1 {
                    return Err(err::ACCESS_DENIED);
                }
                let mut buf = alloc::vec![0u8; n.min(0x10000)];
                let k = host.read(s.host, &mut buf)?;
                self.hw.write_bytes(a, &buf[..k]);
                Ok(Some(k))
            }
        }
    }

    pub fn write_handle<H: Host>(&mut self, host: &mut H, h: u16, data: &[u8]) -> Result<usize, u16> {
        let i = self.sft_of(h).ok_or(err::INVALID_HANDLE)?;
        let s = self.dos.sft[i].clone().unwrap();
        match s.dev {
            Some(Device::Con) => {
                self.con_write(host, data);
                Ok(data.len())
            }
            Some(_) => Ok(data.len()),
            None => {
                if s.mode & 7 == 0 {
                    return Err(err::ACCESS_DENIED);
                }
                if data.is_empty() {
                    let pos = host.seek(s.host, 0, 1)?;
                    host.set_size(s.host, pos)?;
                    return Ok(0);
                }
                let k = host.write(s.host, data)?;
                if let Some(Some(e)) = self.dos.sft.get_mut(i) {
                    e.info &= !0x40;
                }
                Ok(k)
            }
        }
    }

    /// Write to standard output (handle 1), as 02h/09h do.
    pub fn con_write_std<H: Host>(&mut self, host: &mut H, data: &[u8]) {
        if self.write_handle(host, 1, data).is_err() {
            self.con_write(host, data);
        }
    }

    // ---- The calls ----

    pub(crate) fn file_fn<H: Host>(&mut self, host: &mut H, ah: u8) -> Cb {
        let r: Result<(), u16> = (|| -> Result<(), u16> {
            match ah {
                0x39 | 0x3a => {
                    let f = self.file_at(self.ds_dx())?;
                    if ah == 0x39 {
                        host.mkdir(&f)
                    } else {
                        let cur = alloc::format!("{}:\\{}", &f[..1], self.dos.cwd[(f.as_bytes()[0] - b'A') as usize]);
                        if cur == f {
                            return Err(err::CURRENT_DIRECTORY);
                        }
                        host.rmdir(&f)
                    }
                }
                0x3b => {
                    let f = self.file_at(self.ds_dx())?;
                    if f.len() > 3 {
                        let a = host.get_attr(&f).map_err(|_| err::PATH_NOT_FOUND)?;
                        if a & 0x10 == 0 {
                            return Err(err::PATH_NOT_FOUND);
                        }
                    }
                    let d = (f.as_bytes()[0] - b'A') as usize;
                    if f.len() > 66 {
                        return Err(err::PATH_NOT_FOUND);
                    }
                    self.dos.cwd[d] = String::from(&f[3..]);
                    self.write_cds(host, d as u8);
                    Ok(())
                }
                0x3c | 0x5b => {
                    let raw = self.hw.cstr(self.ds_dx(), 128);
                    let n = self.resolve(&raw)?;
                    let attr = self.cpu.cx() as u8;
                    let how = if ah == 0x3c { Create::Truncate } else { Create::New };
                    let h = self.open_handle(host, &n, 2, Some((attr, how)))?;
                    self.cpu.set_ax(h);
                    Ok(())
                }
                0x3d => {
                    let raw = self.hw.cstr(self.ds_dx(), 128);
                    let n = self.resolve(&raw)?;
                    let mode = self.cpu.al();
                    if mode & 7 > 2 {
                        return Err(err::INVALID_ACCESS);
                    }
                    let h = self.open_handle(host, &n, mode, None)?;
                    self.cpu.set_ax(h);
                    Ok(())
                }
                0x3e => {
                    let psp = self.dos.psp;
                    let h = self.cpu.bx();
                    self.close_handle(host, psp, h)
                }
                0x3f => {
                    let (h, n, a) = (self.cpu.bx(), self.cpu.cx() as usize, self.ds_dx());
                    match self.read_handle(host, h, a, n)? {
                        Some(k) => {
                            self.cpu.set_ax(k as u16);
                            Ok(())
                        }
                        None => Err(0xffff),
                    }
                }
                0x40 => {
                    let (h, n, a) = (self.cpu.bx(), self.cpu.cx() as usize, self.ds_dx());
                    let mut buf = alloc::vec![0u8; n];
                    self.hw.read_bytes(a, &mut buf);
                    let k = self.write_handle(host, h, &buf)?;
                    self.cpu.set_ax(k as u16);
                    Ok(())
                }
                0x41 => {
                    let f = self.file_at(self.ds_dx())?;
                    if f.contains(['?', '*']) {
                        return Err(err::FILE_NOT_FOUND);
                    }
                    host.delete(&f)
                }
                0x42 => {
                    let i = self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                    let s = self.dos.sft[i].clone().unwrap();
                    let off = ((self.cpu.cx() as u32) << 16 | self.cpu.dx() as u32) as i32 as i64;
                    let whence = self.cpu.al();
                    if whence > 2 {
                        return Err(err::INVALID_FUNCTION);
                    }
                    let pos = if s.dev.is_some() { 0 } else {
                        // Offsets from the start are unsigned.
                        let off = if whence == 0 { off as u32 as i64 } else { off };
                        host.seek(s.host, off, whence)?
                    };
                    self.cpu.set_ax(pos as u16);
                    self.cpu.set_dx((pos >> 16) as u16);
                    Ok(())
                }
                0x43 => {
                    let f = self.file_at(self.ds_dx())?;
                    match self.cpu.al() {
                        0 => {
                            let a = host.get_attr(&f)?;
                            self.cpu.set_cx(a as u16);
                            self.cpu.set_ax(a as u16);
                            Ok(())
                        }
                        1 => host.set_attr(&f, self.cpu.cx() as u8 & 0x27),
                        _ => Err(err::INVALID_FUNCTION),
                    }
                }
                0x44 => self.ioctl(host),
                0x45 => {
                    let i = self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                    let h = self.new_handle(i)?;
                    if let Some(Some(s)) = self.dos.sft.get_mut(i) {
                        s.refs += 1;
                    }
                    self.cpu.set_ax(h);
                    Ok(())
                }
                0x46 => {
                    let i = self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                    let to = self.cpu.cx();
                    let psp = self.dos.psp;
                    let (t, n) = self.jft(psp);
                    if to >= n {
                        return Err(err::INVALID_HANDLE);
                    }
                    if self.hw.rd(t + to as u32) != 0xff {
                        let _ = self.close_handle(host, psp, to);
                    }
                    self.hw.wr(t + to as u32, i as u8);
                    if let Some(Some(s)) = self.dos.sft.get_mut(i) {
                        s.refs += 1;
                    }
                    Ok(())
                }
                0x47 => {
                    let d = if self.cpu.dl() == 0 { self.dos.drive } else { self.cpu.dl() - 1 };
                    if d >= 26 || !self.dos.present[d as usize] {
                        return Err(err::INVALID_DRIVE);
                    }
                    let a = self.seg_lin(DS, self.cpu.si());
                    let cwd = self.dos.cwd[d as usize].clone();
                    self.hw.write_bytes(a, cwd.as_bytes());
                    self.hw.wr(a + cwd.len() as u32, 0);
                    self.cpu.set_ax(0x0100);
                    Ok(())
                }
                0x4e => {
                    let raw = self.hw.cstr(self.ds_dx(), 128);
                    let attr = self.cpu.cx() as u8;
                    self.find_first(host, &raw, attr)
                }
                0x4f => self.find_next(),
                0x56 => {
                    let from = self.file_at(self.ds_dx())?;
                    let to = self.file_at(self.es_di())?;
                    if from[..1] != to[..1] {
                        return Err(err::NOT_SAME_DEVICE);
                    }
                    host.rename(&from, &to)
                }
                0x57 => {
                    let i = self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                    let s = self.dos.sft[i].clone().unwrap();
                    match self.cpu.al() {
                        0 => {
                            let (d, t) = if s.dev.is_some() {
                                let n = host.now();
                                crate::dos::proc::pack_dt(&n)
                            } else {
                                host.file_time(s.host)?
                            };
                            self.cpu.set_cx(t);
                            self.cpu.set_dx(d);
                            Ok(())
                        }
                        1 => {
                            if s.dev.is_none() {
                                host.set_file_time(s.host, self.cpu.dx(), self.cpu.cx())?;
                            }
                            Ok(())
                        }
                        _ => {
                            self.cpu.set_cx(0);
                            self.cpu.set_dx(0);
                            Ok(())
                        }
                    }
                }
                0x5a => {
                    let a = self.ds_dx();
                    let mut dir = self.hw.cstr(a, 128);
                    if !dir.is_empty() && !dir.ends_with(b"\\") && !dir.ends_with(b":") {
                        dir.push(b'\\');
                    }
                    let mut seed = (self.hw.us_now as u32) ^ 0x5a5a;
                    for _ in 0..100 {
                        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                        let name = alloc::format!("{:08X}", seed);
                        let mut full = dir.clone();
                        full.extend_from_slice(name.as_bytes());
                        let n = self.resolve(&full)?;
                        if let Name::File(f) = &n {
                            if host.get_attr(f).is_ok() {
                                continue;
                            }
                        }
                        let h = self.open_handle(host, &n, 2, Some((self.cpu.cx() as u8, Create::New)))?;
                        self.hw.write_bytes(a + dir.len() as u32, name.as_bytes());
                        self.hw.wr(a + dir.len() as u32 + 8, 0);
                        self.cpu.set_ax(h);
                        return Ok(());
                    }
                    Err(err::ACCESS_DENIED)
                }
                0x5c => Ok(()),
                0x60 => {
                    let raw = self.hw.cstr(self.seg_lin(DS, self.cpu.si()), 128);
                    let s = match self.resolve(&raw)? {
                        Name::File(f) => f,
                        Name::Dev(d) => alloc::format!("{}:/{:?}", (b'A' + self.dos.drive) as char, d).to_ascii_uppercase(),
                    };
                    let a = self.es_di();
                    self.hw.write_bytes(a, s.as_bytes());
                    self.hw.wr(a + s.len() as u32, 0);
                    Ok(())
                }
                0x67 => self.set_handle_count(host),
                0x68 | 0x6a => {
                    self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                    Ok(())
                }
                0x6c => {
                    let raw = self.hw.cstr(self.seg_lin(DS, self.cpu.si()), 128);
                    let n = self.resolve(&raw)?;
                    let mode = self.cpu.bx() as u8;
                    let attr = self.cpu.cx() as u8;
                    let action = self.cpu.dx();
                    let exists = match &n {
                        Name::Dev(_) => true,
                        Name::File(f) => host.get_attr(f).is_ok(),
                    };
                    let (h, how) = if exists {
                        match action & 0xf {
                            1 => (self.open_handle(host, &n, mode, None)?, 1),
                            2 => (self.open_handle(host, &n, mode, Some((attr, Create::Truncate)))?, 3),
                            _ => return Err(err::FILE_EXISTS),
                        }
                    } else if action & 0xf0 == 0x10 {
                        (self.open_handle(host, &n, mode, Some((attr, Create::New)))?, 2)
                    } else {
                        return Err(err::FILE_NOT_FOUND);
                    };
                    self.cpu.set_ax(h);
                    self.cpu.set_cx(how);
                    Ok(())
                }
                _ => Err(err::INVALID_FUNCTION),
            }
        })();
        match r {
            Ok(()) => self.ok(),
            Err(0xffff) => return Cb::Retry,
            Err(e) => self.fail(e),
        }
        Cb::Done
    }

    fn ioctl<H: Host>(&mut self, host: &mut H) -> Result<(), u16> {
        let al = self.cpu.al();
        match al {
            0x00 | 0x01 | 0x02 | 0x03 | 0x06 | 0x07 | 0x0a | 0x0c | 0x10 => {
                let i = self.sft_of(self.cpu.bx()).ok_or(err::INVALID_HANDLE)?;
                let s = self.dos.sft[i].clone().unwrap();
                match al {
                    0x00 => {
                        self.cpu.set_dx(s.info);
                        self.cpu.set_ax(s.info);
                    }
                    0x01 => {
                        if s.dev.is_some() {
                            let v = (s.info & 0xff00) | (self.cpu.dl() as u16 & 0xff);
                            self.dos.sft[i].as_mut().unwrap().info = v | 0x80;
                        }
                    }
                    0x02 | 0x03 => {
                        if s.dev == Some(Device::Emm) && self.cpu.cx() >= 2 {
                            // EMM386-style "get status": a word.
                            let a = self.ds_dx();
                            self.hw.wr16(a, 0x0025);
                            self.cpu.set_ax(2);
                        } else if s.dev.is_some() {
                            self.cpu.set_ax(0);
                        } else {
                            return Err(err::INVALID_FUNCTION);
                        }
                    }
                    0x06 => {
                        let ready = match s.dev {
                            Some(Device::Con) => !self.dos.con_ahead.is_empty() || self.peek_key(false).is_some(),
                            Some(Device::Nul) => false,
                            Some(_) => true,
                            None => {
                                let pos = host.seek(s.host, 0, 1)?;
                                let end = host.seek(s.host, 0, 2)?;
                                host.seek(s.host, pos as i64, 0)?;
                                pos < end
                            }
                        };
                        if s.dev == Some(Device::Con) && !ready {
                            self.idle_poll();
                        }
                        self.cpu.set_al(if ready { 0xff } else { 0 });
                    }
                    0x07 => self.cpu.set_al(0xff),
                    0x0a => self.cpu.set_dx(0),
                    _ => return Err(err::INVALID_FUNCTION),
                }
                Ok(())
            }
            0x08 => {
                let d = if self.cpu.bl() == 0 { self.dos.drive } else { self.cpu.bl() - 1 };
                if d >= 26 || !self.dos.present[d as usize] {
                    return Err(err::INVALID_DRIVE);
                }
                self.cpu.set_ax(if d < 2 { 0 } else { 1 });
                Ok(())
            }
            0x09 => {
                let d = if self.cpu.bl() == 0 { self.dos.drive } else { self.cpu.bl() - 1 };
                if d >= 26 || !self.dos.present[d as usize] {
                    return Err(err::INVALID_DRIVE);
                }
                self.cpu.set_dx(0);
                Ok(())
            }
            0x0b => Ok(()),
            0x0e => {
                self.cpu.set_al(0);
                Ok(())
            }
            0x0f => Ok(()),
            0x0d => {
                // Generic block IOCTL: only "get media ID" (0866h).
                if self.cpu.cx() == 0x0866 {
                    let a = self.ds_dx();
                    self.hw.wr16(a, 0);
                    self.hw.wr32(a + 2, 0x1234_5678);
                    self.hw.write_bytes(a + 6, b"NO NAME    FAT16   ");
                    return Ok(());
                }
                Err(err::INVALID_FUNCTION)
            }
            _ => Err(err::INVALID_FUNCTION),
        }
    }

    fn set_handle_count<H: Host>(&mut self, host: &mut H) -> Result<(), u16> {
        let want = self.cpu.bx().max(20);
        let psp = self.dos.psp;
        let (t, n) = self.jft(psp);
        if want <= n {
            return Ok(());
        }
        let paras = (want as u32).div_ceil(16) as u16;
        let seg = self.alloc_mem(host, paras, psp)?;
        let new = Machine::lin(seg, 0);
        for h in 0..want as u32 {
            let v = if h < n as u32 { self.hw.rd(t + h) } else { 0xff };
            self.hw.wr(new + h, v);
        }
        let a = Machine::lin(psp, 0);
        self.hw.wr16(a + 0x32, want);
        self.hw.wr16(a + 0x34, 0);
        self.hw.wr16(a + 0x36, seg);
        Ok(())
    }

    // ---- Find first/next ----

    fn find_first<H: Host>(&mut self, host: &mut H, raw: &[u8], attr: u8) -> Result<(), u16> {
        let full = match self.resolve(raw)? {
            Name::File(f) => f,
            Name::Dev(d) => {
                // A device name finds itself.
                let name = alloc::format!("{:?}", d).to_ascii_uppercase();
                let e = crate::DirEntry { name, attr: 0x40, size: 0, date: 0, time: 0 };
                let id = self.new_search(alloc::vec![e]);
                return self.find_fill(id, attr, &[b'?'; 11]);
            }
        };
        let (dir, pat) = match full.rfind('\\') {
            Some(i) => (&full[..i.max(2)], &full[i + 1..]),
            None => return Err(err::PATH_NOT_FOUND),
        };
        let dir = if dir.len() == 2 { alloc::format!("{dir}\\") } else { String::from(dir) };
        let pattern = fcb_form(pat.as_bytes());
        let mut entries = Vec::new();
        if attr & 0x08 != 0 && attr & 0x10 == 0 {
            if let Some(l) = host.volume_label(full.as_bytes()[0] - b'A') {
                if attr == 0x08 || dir.len() == 3 {
                    entries.push(crate::DirEntry { name: l, attr: 0x08, size: 0, date: 0, time: 0 });
                }
            }
        }
        if attr != 0x08 {
            let list = host.list_dir(&dir).map_err(|e| if e == err::FILE_NOT_FOUND { err::PATH_NOT_FOUND } else { e })?;
            for e in list {
                if !is_short(&e.name) {
                    continue;
                }
                if dir.len() == 3 && (e.name == "." || e.name == "..") {
                    continue;
                }
                let name = e.name.to_ascii_uppercase();
                if !matches(&pattern, name.as_bytes()) {
                    continue;
                }
                // Hidden, system and directories only when asked for.
                if e.attr & 0x16 & !attr != 0 {
                    continue;
                }
                entries.push(crate::DirEntry { name, ..e });
            }
        }
        if entries.is_empty() {
            return Err(err::NO_MORE_FILES);
        }
        let id = self.new_search(entries);
        self.find_fill(id, attr, &pattern)
    }

    fn new_search(&mut self, entries: Vec<crate::DirEntry>) -> u32 {
        let id = self.dos.next_search;
        self.dos.next_search = self.dos.next_search.wrapping_add(1).max(1);
        self.dos.searches.insert(id, Search { entries, pos: 0 });
        // Searches never closed by the program: keep the last 64.
        while self.dos.searches.len() > 64 {
            let first = *self.dos.searches.keys().next().unwrap();
            self.dos.searches.remove(&first);
        }
        id
    }

    /// Put the search's next entry into the DTA.
    fn find_fill(&mut self, id: u32, attr: u8, pattern: &[u8; 11]) -> Result<(), u16> {
        let a = self.dta_lin();
        let Some(s) = self.dos.searches.get_mut(&id) else { return Err(err::NO_MORE_FILES) };
        let Some(e) = s.entries.get(s.pos).cloned() else {
            self.dos.searches.remove(&id);
            return Err(err::NO_MORE_FILES);
        };
        s.pos += 1;
        self.hw.wr(a, self.dos.drive + 1);
        self.hw.write_bytes(a + 1, pattern);
        self.hw.wr(a + 0x0c, attr);
        self.hw.wr32(a + 0x0d, id);
        self.hw.wr32(a + 0x11, 0x5253_3244);
        self.hw.wr(a + 0x15, e.attr);
        self.hw.wr16(a + 0x16, e.time);
        self.hw.wr16(a + 0x18, e.date);
        self.hw.wr32(a + 0x1a, e.size);
        let n = e.name.as_bytes();
        for i in 0..13u32 {
            self.hw.wr(a + 0x1e + i, n.get(i as usize).copied().unwrap_or(0));
        }
        self.cpu.set_ax(0);
        Ok(())
    }

    fn find_next(&mut self) -> Result<(), u16> {
        let a = self.dta_lin();
        if self.hw.rd32(a + 0x11) != 0x5253_3244 {
            return Err(err::NO_MORE_FILES);
        }
        let id = self.hw.rd32(a + 0x0d);
        let attr = self.hw.rd(a + 0x0c);
        let mut pat = [0u8; 11];
        self.hw.read_bytes(a + 1, &mut pat);
        self.find_fill(id, attr, &pat)
    }

    /// The handle calls fail with CF set and AX = code (used by FCB code
    /// and the shell).
    pub fn set_cf(&mut self, on: bool) {
        self.set_ret_flag(CF, on);
    }

    pub fn es_seg(&self) -> u16 {
        self.cpu.seg[ES].sel
    }
}
