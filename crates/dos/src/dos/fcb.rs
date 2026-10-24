//! File control blocks (DOS 1 file calls) and 29h (parse file name).

use alloc::string::String;
use alloc::vec::Vec;

use x86emu::{DS, ES};

use super::{err, fcb_form, matches, Name};
use crate::host::Create;
use crate::{Cb, Host, Machine};

/// Marks an FCB we opened (in its reserved bytes).
const MAGIC: u16 = 0x5246;

impl Machine {
    /// The FCB at DS:DX: (address of the normal part, attribute of an
    /// extended FCB).
    fn fcb(&mut self) -> (u32, u8) {
        let a = self.ds_dx();
        if self.hw.rd(a) == 0xff {
            (a + 7, self.hw.rd(a + 6))
        } else {
            (a, 0)
        }
    }

    /// The drive (0-based) and FCB-form name of an FCB.
    fn fcb_name(&mut self, a: u32) -> (u8, [u8; 11]) {
        let d = self.hw.rd(a);
        let drive = if d == 0 { self.dos.drive } else { d - 1 };
        let mut n = [0u8; 11];
        self.hw.read_bytes(a + 1, &mut n);
        for c in n.iter_mut() {
            *c = self.upcase(*c);
        }
        (drive, n)
    }

    /// The path of an FCB's file ("C:\CUR\DIR\NAME.EXT").
    fn fcb_path(&mut self, a: u32) -> Result<(String, [u8; 11]), u16> {
        let (drive, n) = self.fcb_name(a);
        if drive >= 26 || !self.dos.present[drive as usize] {
            return Err(err::INVALID_DRIVE);
        }
        let base: Vec<u8> = n[..8].iter().copied().take_while(|&c| c != b' ').collect();
        let ext: Vec<u8> = n[8..].iter().copied().take_while(|&c| c != b' ').collect();
        let mut s = String::new();
        s.push((b'A' + drive) as char);
        s.push(':');
        for &c in &base {
            s.push(c as char);
        }
        if !ext.is_empty() {
            s.push('.');
            for &c in &ext {
                s.push(c as char);
            }
        }
        Ok((s, n))
    }

    fn fcb_sft(&mut self, a: u32) -> Option<usize> {
        if self.hw.rd16(a + 0x18) != MAGIC {
            return None;
        }
        let i = self.hw.rd(a + 0x1a) as usize;
        self.dos.sft.get(i).and_then(|s| s.as_ref()).map(|_| i)
    }

    fn rec_size(&mut self, a: u32) -> u32 {
        let r = self.hw.rd16(a + 0x0e) as u32;
        if r == 0 { 128 } else { r }
    }

    /// Read or write one record at `rec` through the DTA; AL code.
    fn fcb_io<H: Host>(&mut self, host: &mut H, a: u32, rec: u32, write: bool, count: u32) -> (u8, u32) {
        let Some(i) = self.fcb_sft(a) else { return (if write { 1 } else { 1 }, 0) };
        let h = self.dos.sft[i].as_ref().unwrap().host;
        let size = self.rec_size(a);
        let (dseg, doff) = self.dta();
        let dta = Machine::lin(dseg, doff);
        if host.seek(h, (rec as u64 * size as u64) as i64, 0).is_err() {
            return (1, 0);
        }
        let total = size * count;
        if write {
            let mut buf = alloc::vec![0u8; total as usize];
            self.hw.read_bytes(dta, &mut buf);
            let n = if total == 0 {
                let pos = host.seek(h, 0, 1).unwrap_or(0);
                let _ = host.set_size(h, pos);
                0
            } else {
                host.write(h, &buf).unwrap_or(0) as u32
            };
            let end = host.seek(h, 0, 2).unwrap_or(0) as u32;
            self.hw.wr32(a + 0x10, end);
            (if n < total { 1 } else { 0 }, n / size.max(1))
        } else {
            let mut buf = alloc::vec![0u8; total as usize];
            let n = host.read(h, &mut buf).unwrap_or(0) as u32;
            if n == 0 {
                return (1, 0);
            }
            // A partial record is padded with zeros.
            for b in buf[n as usize..].iter_mut() {
                *b = 0;
            }
            let recs = n.div_ceil(size);
            self.hw.write_bytes(dta, &buf[..(recs * size) as usize]);
            (if n < total { 3 } else { 0 }, recs)
        }
    }

    fn fcb_seq_pos(&mut self, a: u32) -> u32 {
        self.hw.rd16(a + 0x0c) as u32 * 128 + self.hw.rd(a + 0x20) as u32
    }

    fn fcb_set_seq(&mut self, a: u32, rec: u32) {
        self.hw.wr16(a + 0x0c, (rec / 128) as u16);
        self.hw.wr(a + 0x20, (rec % 128) as u8);
    }

    fn fcb_random(&mut self, a: u32) -> u32 {
        let size = self.rec_size(a);
        let r = self.hw.rd32(a + 0x21);
        if size < 64 { r } else { r & 0x00ff_ffff }
    }

    /// Fill the DTA with a found entry as an (extended) FCB.
    fn fcb_found(&mut self, ext: Option<u8>, drive: u8, e: &crate::DirEntry) {
        let (dseg, doff) = self.dta();
        let mut d = Machine::lin(dseg, doff);
        if let Some(attr) = ext {
            self.hw.wr(d, 0xff);
            for i in 1..6 {
                self.hw.wr(d + i, 0);
            }
            self.hw.wr(d + 6, attr);
            d += 7;
        }
        self.hw.wr(d, drive + 1);
        let f = fcb_form(e.name.as_bytes());
        self.hw.write_bytes(d + 1, &f);
        self.hw.wr(d + 0x0c, e.attr);
        for i in 0x0d..0x16 {
            self.hw.wr(d + i, 0);
        }
        self.hw.wr16(d + 0x16, e.time);
        self.hw.wr16(d + 0x18, e.date);
        self.hw.wr16(d + 0x1a, 0);
        self.hw.wr32(d + 0x1c, e.size);
    }

    fn fcb_dir<H: Host>(&mut self, host: &mut H, drive: u8, pat: &[u8; 11], attr: u8) -> Result<Vec<crate::DirEntry>, u16> {
        let dir = alloc::format!("{}:\\{}", (b'A' + drive) as char, self.dos.cwd[drive as usize]);
        let list = host.list_dir(&dir)?;
        Ok(list
            .into_iter()
            .filter(|e| super::file::is_short(&e.name) && e.name != "." && e.name != "..")
            .map(|e| crate::DirEntry { name: e.name.to_ascii_uppercase(), ..e })
            .filter(|e| matches(pat, e.name.as_bytes()) && e.attr & 0x16 & !attr == 0)
            .collect())
    }

    pub(crate) fn fcb_fn<H: Host>(&mut self, host: &mut H, ah: u8) -> Cb {
        if ah == 0x29 {
            self.parse_name();
            return Cb::Done;
        }
        let (a, attr) = self.fcb();
        let ext = if self.hw.rd(self.ds_dx()) == 0xff { Some(attr) } else { None };
        let al: u8 = match ah {
            0x0f | 0x16 => {
                let Ok((path, _)) = self.fcb_path(a) else { return self.al_done(0xff) };
                let n = match self.resolve(path.as_bytes()) {
                    Ok(n) => n,
                    Err(_) => return self.al_done(0xff),
                };
                let create = if ah == 0x16 { Some((attr, Create::Truncate)) } else { None };
                let r = self.open_name(host, &n, 2, create).or_else(|_| if ah == 0x0f { self.open_name(host, &n, 0, None) } else { Err(err::ACCESS_DENIED) });
                match r {
                    Ok(i) => {
                        let (size, dt) = match &n {
                            Name::File(_) => {
                                let h = self.dos.sft[i].as_ref().unwrap().host;
                                let size = host.seek(h, 0, 2).unwrap_or(0) as u32;
                                let _ = host.seek(h, 0, 0);
                                (size, host.file_time(h).unwrap_or((0, 0)))
                            }
                            Name::Dev(_) => (0, (0, 0)),
                        };
                        if self.hw.rd(a) == 0 {
                            self.hw.wr(a, self.dos.drive + 1);
                        }
                        self.hw.wr16(a + 0x0c, 0);
                        self.hw.wr16(a + 0x0e, 128);
                        self.hw.wr32(a + 0x10, size);
                        self.hw.wr16(a + 0x14, dt.0);
                        self.hw.wr16(a + 0x16, dt.1);
                        self.hw.wr16(a + 0x18, MAGIC);
                        self.hw.wr(a + 0x1a, i as u8);
                        0
                    }
                    Err(_) => 0xff,
                }
            }
            0x10 => match self.fcb_sft(a) {
                Some(i) => {
                    self.release_sft(host, i);
                    self.hw.wr16(a + 0x18, 0);
                    0
                }
                None => 0,
            },
            0x11 | 0x12 => {
                if ah == 0x11 {
                    let (drive, pat) = self.fcb_name(a);
                    if drive >= 26 || !self.dos.present[drive as usize] {
                        return self.al_done(0xff);
                    }
                    let list = match self.fcb_dir(host, drive, &pat, attr) {
                        Ok(l) => l,
                        Err(_) => return self.al_done(0xff),
                    };
                    self.dos.fcb_search = Some((drive, list, 0));
                }
                let Some((drive, list, pos)) = self.dos.fcb_search.take() else { return self.al_done(0xff) };
                match list.get(pos).cloned() {
                    Some(e) => {
                        self.fcb_found(ext, drive, &e);
                        self.dos.fcb_search = Some((drive, list, pos + 1));
                        0
                    }
                    None => 0xff,
                }
            }
            0x13 => {
                let (drive, pat) = self.fcb_name(a);
                if drive >= 26 || !self.dos.present[drive as usize] {
                    return self.al_done(0xff);
                }
                let list = self.fcb_dir(host, drive, &pat, 0).unwrap_or_default();
                let mut any = false;
                for e in list {
                    let p = alloc::format!("{}:\\{}{}{}", (b'A' + drive) as char, self.dos.cwd[drive as usize], if self.dos.cwd[drive as usize].is_empty() { "" } else { "\\" }, e.name);
                    if host.delete(&p).is_ok() {
                        any = true;
                    }
                }
                if any { 0 } else { 0xff }
            }
            0x14 | 0x15 => {
                let rec = self.fcb_seq_pos(a);
                let (r, n) = self.fcb_io(host, a, rec, ah == 0x15, 1);
                if n > 0 {
                    self.fcb_set_seq(a, rec + 1);
                }
                r
            }
            0x17 => {
                let (drive, pat) = self.fcb_name(a);
                let mut new = [0u8; 11];
                self.hw.read_bytes(a + 0x11, &mut new);
                if drive >= 26 || !self.dos.present[drive as usize] {
                    return self.al_done(0xff);
                }
                let list = self.fcb_dir(host, drive, &pat, 0).unwrap_or_default();
                let dir = alloc::format!("{}:\\{}{}", (b'A' + drive) as char, self.dos.cwd[drive as usize], if self.dos.cwd[drive as usize].is_empty() { "" } else { "\\" });
                let mut any = false;
                for e in list {
                    let old = fcb_form(e.name.as_bytes());
                    let mut n = [b' '; 11];
                    for i in 0..11 {
                        n[i] = if new[i] == b'?' { old[i] } else { self.upcase(new[i]) };
                    }
                    let base: String = n[..8].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
                    let ext: String = n[8..].iter().take_while(|&&c| c != b' ').map(|&c| c as char).collect();
                    let to = if ext.is_empty() { alloc::format!("{dir}{base}") } else { alloc::format!("{dir}{base}.{ext}") };
                    if host.rename(&alloc::format!("{dir}{}", e.name), &to).is_ok() {
                        any = true;
                    }
                }
                if any { 0 } else { 0xff }
            }
            0x21 | 0x22 => {
                let rec = self.fcb_random(a);
                self.fcb_set_seq(a, rec);
                self.fcb_io(host, a, rec, ah == 0x22, 1).0
            }
            0x23 => {
                let Ok((path, _)) = self.fcb_path(a) else { return self.al_done(0xff) };
                let size = match self.resolve(path.as_bytes()) {
                    Ok(Name::File(f)) => match host.open(&f, crate::OpenMode(0x40)) {
                        Ok(h) => {
                            let s = host.seek(h, 0, 2).unwrap_or(0);
                            host.close(h);
                            Some(s)
                        }
                        Err(_) => None,
                    },
                    _ => None,
                };
                match size {
                    Some(s) => {
                        let r = self.rec_size(a) as u64;
                        let recs = s.div_ceil(r) as u32;
                        self.hw.wr32(a + 0x21, recs);
                        0
                    }
                    None => 0xff,
                }
            }
            0x24 => {
                let rec = self.fcb_seq_pos(a);
                self.hw.wr32(a + 0x21, rec);
                0
            }
            0x27 | 0x28 => {
                let rec = self.fcb_random(a);
                let count = self.cpu.cx() as u32;
                let (r, n) = self.fcb_io(host, a, rec, ah == 0x28, count);
                self.cpu.set_cx(n as u16);
                self.hw.wr32(a + 0x21, rec + n);
                self.fcb_set_seq(a, rec + n);
                r
            }
            _ => 0xff,
        };
        self.al_done(al)
    }

    fn al_done(&mut self, al: u8) -> Cb {
        self.cpu.set_al(al);
        Cb::Done
    }

    /// 29h: parse a file name at DS:SI into the FCB at ES:DI.
    fn parse_name(&mut self) {
        let flags = self.cpu.al();
        let mut a = Machine::lin(self.cpu.seg[DS].sel, self.cpu.si());
        let f = Machine::lin(self.cpu.seg[ES].sel, self.cpu.di());
        let start = a;
        let seps = b":.;,=+ \t";
        if flags & 1 != 0 {
            // Skip leading separators.
            while seps.contains(&self.hw.rd(a)) && self.hw.rd(a) != b'.' && self.hw.rd(a) != b':' {
                a += 1;
            }
        } else {
            while matches!(self.hw.rd(a), b' ' | b'\t') {
                a += 1;
            }
        }
        let mut drive_bad = false;
        if self.hw.rd(a + 1) == b':' {
            let c = self.hw.rd(a);
            let d = self.upcase(c);
            if d.is_ascii_uppercase() {
                let n = d - b'A';
                drive_bad = !self.dos.present[n as usize];
                self.hw.wr(f, n + 1);
                a += 2;
            }
        } else if flags & 2 == 0 {
            self.hw.wr(f, 0);
        }
        let mut wild = false;
        let term = |c: u8| c <= b' ' || b".\"/\\[]:|<>+=;,".contains(&c);
        let mut field = |m: &mut Machine, a: &mut u32, at: u32, len: u32, keep: bool| {
            let mut out = Vec::new();
            while !term(m.hw.rd(*a)) || (m.hw.rd(*a) == b'*') {
                let c = m.hw.rd(*a);
                *a += 1;
                if c == b'*' {
                    while (out.len() as u32) < len {
                        out.push(b'?');
                    }
                    wild = true;
                    continue;
                }
                if c == b'?' {
                    wild = true;
                }
                if (out.len() as u32) < len {
                    out.push(m.upcase(c));
                }
            }
            if out.is_empty() && keep {
                return;
            }
            for i in 0..len {
                m.hw.wr(at + i, out.get(i as usize).copied().unwrap_or(b' '));
            }
        };
        field(self, &mut a, f + 1, 8, flags & 4 != 0);
        if self.hw.rd(a) == b'.' {
            a += 1;
            field(self, &mut a, f + 9, 3, false);
        } else if flags & 8 == 0 {
            for i in 0..3 {
                self.hw.wr(f + 9 + i, b' ');
            }
        }
        let si = self.cpu.si().wrapping_add((a - start) as u16);
        self.cpu.set_si(si);
        self.cpu.set_al(if drive_bad { 0xff } else if wild { 1 } else { 0 });
    }

    /// FCBs for a program's PSP from its command tail (as COMMAND.COM
    /// fills them).
    pub(crate) fn parse_fcbs(&mut self, tail: &[u8]) -> ([u8; 16], [u8; 16]) {
        let mut fcbs = [[0u8; 16]; 2];
        let words: Vec<&[u8]> = tail.split(|&c| c == b' ' || c == b'\t' || c == b',').filter(|w| !w.is_empty() && !w.starts_with(b"/")).collect();
        for (k, f) in fcbs.iter_mut().enumerate() {
            f[1..12].copy_from_slice(&[b' '; 11]);
            let Some(w) = words.get(k) else { continue };
            let mut w: &[u8] = w;
            if w.len() >= 2 && w[1] == b':' {
                let d = w[0].to_ascii_uppercase();
                if d.is_ascii_uppercase() {
                    f[0] = d - b'A' + 1;
                }
                w = &w[2..];
            }
            let name = w.rsplit(|&c| c == b'\\').next().unwrap_or(w);
            let up: Vec<u8> = name.iter().map(|c| c.to_ascii_uppercase()).collect();
            f[1..12].copy_from_slice(&fcb_form(&up));
        }
        (fcbs[0], fcbs[1])
    }
}
