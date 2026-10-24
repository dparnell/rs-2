//! The console: INT 21h 01h-0Ch, the CON device, line editing.

use alloc::vec::Vec;

use x86emu::flags::ZF;

use super::{Device, LineEdit};
use crate::{Cb, Host, Machine};

impl Machine {
    /// A key from the BIOS buffer as DOS returns characters: extended
    /// keys come as 0, then the scan code.
    pub fn con_key(&mut self) -> Option<u8> {
        if let Some(s) = self.dos.pending_scan.take() {
            return Some(s);
        }
        let k = self.peek_key(true)?;
        self.polls = 0;
        let (sc, ch) = ((k >> 8) as u8, k as u8);
        if ch == 0 || (ch == 0xe0 && sc != 0) {
            self.dos.pending_scan = Some(sc);
            return Some(0);
        }
        Some(ch)
    }

    fn con_avail(&mut self) -> bool {
        self.dos.pending_scan.is_some() || self.peek_key(false).is_some()
    }

    /// Standard input is the console (not redirected).
    fn stdin_is_con(&mut self) -> bool {
        match self.sft_of(0) {
            Some(i) => self.dos.sft[i].as_ref().is_some_and(|s| s.dev == Some(Device::Con)),
            None => true,
        }
    }

    /// One character from standard input; None to wait.
    fn stdin_char<H: Host>(&mut self, host: &mut H) -> Option<u8> {
        if self.stdin_is_con() {
            if let Some(c) = self.dos.con_ahead.first().copied() {
                self.dos.con_ahead.remove(0);
                return Some(c);
            }
            return self.con_key();
        }
        let a = Machine::lin(super::DOSSEG, super::SDA + 0x40);
        match self.read_handle(host, 0, a, 1) {
            Ok(Some(1)) => Some(self.hw.rd(a)),
            // End of file: DOS returns Ctrl+Z.
            _ => Some(0x1a),
        }
    }

    /// Write characters to the console (tabs expanded).
    pub fn con_write<H: Host>(&mut self, host: &mut H, data: &[u8]) {
        let _ = host;
        for &b in data {
            if b == b'\t' {
                loop {
                    self.tty_out(b' ');
                    let p = self.hw.rd(0x462);
                    if self.cursor(p).1 % 8 == 0 {
                        break;
                    }
                }
            } else {
                self.tty_out(b);
            }
        }
    }

    pub(crate) fn con_fn<H: Host>(&mut self, host: &mut H, ah: u8) -> Cb {
        match ah {
            0x01 | 0x07 | 0x08 => {
                let Some(c) = self.stdin_char(host) else { return Cb::Retry };
                if c == 3 && ah != 0x07 {
                    self.con_write(host, b"^C\r\n");
                    return self.ctrl_c(host);
                }
                if ah == 0x01 {
                    self.con_write_std(host, &[c]);
                }
                self.cpu.set_al(c);
            }
            0x03 => self.cpu.set_al(0),
            0x04 | 0x05 => {}
            0x06 => {
                if self.cpu.dl() == 0xff {
                    if self.stdin_is_con() && !self.con_avail() && self.dos.con_ahead.is_empty() {
                        self.cpu.set_al(0);
                        self.set_ret_flag(ZF, true);
                        self.idle_poll();
                    } else {
                        let c = self.stdin_char(host).unwrap_or(0);
                        self.cpu.set_al(c);
                        self.set_ret_flag(ZF, false);
                    }
                } else {
                    let c = self.cpu.dl();
                    self.con_write_std(host, &[c]);
                    self.cpu.set_al(c);
                }
            }
            0x09 => {
                let mut a = self.ds_dx();
                let mut s = Vec::new();
                for _ in 0..65536 {
                    let c = self.hw.rd(a);
                    if c == b'$' {
                        break;
                    }
                    s.push(c);
                    a += 1;
                }
                self.con_write_std(host, &s);
                self.cpu.set_al(b'$');
            }
            0x0a => return self.buffered_input(host),
            0x0b => {
                let ready = if self.stdin_is_con() { !self.dos.con_ahead.is_empty() || self.con_avail() } else { true };
                if !ready {
                    self.idle_poll();
                }
                self.cpu.set_al(if ready { 0xff } else { 0 });
            }
            0x0c => {
                if self.dos.line.is_none() {
                    // Flush the type-ahead once, then do the function.
                    while self.peek_key(true).is_some() {}
                    self.dos.pending_scan = None;
                    self.dos.con_ahead.clear();
                }
                let f = self.cpu.al();
                if matches!(f, 0x01 | 0x06 | 0x07 | 0x08 | 0x0a) {
                    self.cpu.set_ah(f);
                    let r = self.con_fn(host, f);
                    if r == Cb::Retry {
                        // Repeat as 0Ch without another flush.
                        self.cpu.set_ah(0x0c);
                        self.dos.line.get_or_insert_with(LineEdit::default);
                    } else if f != 0x0a {
                        self.dos.line = None;
                    }
                    return r;
                }
                self.cpu.set_al(0);
            }
            _ => {}
        }
        Cb::Done
    }

    /// 0Ah: buffered input into DS:DX.
    fn buffered_input<H: Host>(&mut self, host: &mut H) -> Cb {
        let a = self.ds_dx();
        let max = self.hw.rd(a) as usize;
        if max == 0 {
            return Cb::Done;
        }
        if !self.stdin_is_con() {
            // Redirected: read a line from the file.
            let mut line = Vec::new();
            loop {
                let c = self.stdin_char(host).unwrap_or(0x1a);
                if c == b'\r' || c == 0x1a {
                    break;
                }
                if c == b'\n' {
                    continue;
                }
                if line.len() + 1 < max {
                    line.push(c);
                }
            }
            self.hw.wr(a + 1, line.len() as u8);
            self.hw.write_bytes(a + 2, &line);
            self.hw.wr(a + 2 + line.len() as u32, b'\r');
            return Cb::Done;
        }
        match self.line_edit(host, max) {
            Some(line) => {
                self.hw.wr(a + 1, line.len() as u8);
                self.hw.write_bytes(a + 2, &line);
                self.hw.wr(a + 2 + line.len() as u32, b'\r');
                Cb::Done
            }
            None => Cb::Retry,
        }
    }

    /// Edit a line from the keyboard: the line when Enter was pressed,
    /// or None while it is not finished.
    fn line_edit<H: Host>(&mut self, host: &mut H, max: usize) -> Option<Vec<u8>> {
        let mut le = match self.dos.line.take() {
            Some(l) if l.max != 0 => l,
            _ => {
                let p = self.hw.rd(0x462);
                LineEdit { buf: Vec::new(), max, template: self.dos.template.clone(), start_col: self.cursor(p).1 }
            }
        };
        loop {
            let Some(k) = self.peek_key(true) else {
                self.dos.line = Some(le);
                return None;
            };
            self.polls = 0;
            let (sc, ch) = ((k >> 8) as u8, k as u8);
            match ch {
                b'\r' => {
                    self.con_write(host, b"\r");
                    self.dos.template = le.buf.clone();
                    return Some(le.buf);
                }
                8 => {
                    if le.buf.pop().is_some() {
                        self.con_write(host, b"\x08 \x08");
                    }
                }
                0x1b => {
                    le.buf.clear();
                    self.con_write(host, b"\\\r\n");
                }
                3 => {
                    // Ctrl+C: abandon the line.
                    self.dos.break_pending = true;
                    le.buf.clear();
                    self.dos.line = None;
                    return Some(Vec::new());
                }
                0 | 0xe0 if sc != 0 => match sc {
                    0x3b | 0x4d => {
                        // F1 / Right: one character of the template.
                        if let Some(&c) = le.template.get(le.buf.len()) {
                            if le.buf.len() + 1 < le.max {
                                le.buf.push(c);
                                self.con_write(host, &[c]);
                            }
                        }
                    }
                    0x3d => {
                        // F3: the rest of the template.
                        while let Some(&c) = le.template.get(le.buf.len()) {
                            if le.buf.len() + 1 >= le.max {
                                break;
                            }
                            le.buf.push(c);
                            self.con_write(host, &[c]);
                        }
                    }
                    0x4b => {
                        if le.buf.pop().is_some() {
                            self.con_write(host, b"\x08 \x08");
                        }
                    }
                    _ => {}
                },
                _ => {
                    if le.buf.len() + 1 < le.max {
                        le.buf.push(ch);
                        if ch < 0x20 && ch != b'\t' {
                            self.con_write(host, &[b'^', ch + 0x40]);
                        } else {
                            self.con_write(host, &[ch]);
                        }
                    } else {
                        self.tty_out(7);
                    }
                }
            }
        }
    }

    /// A cooked read from CON through a handle: a whole line is edited,
    /// then returned with CR LF.
    pub(crate) fn con_read_cooked<H: Host>(&mut self, host: &mut H, a: u32, n: usize) -> Result<Option<usize>, u16> {
        if self.dos.con_ahead.is_empty() {
            match self.line_edit(host, 128) {
                Some(mut line) => {
                    self.con_write(host, b"\n");
                    if self.dos.break_pending {
                        line.clear();
                    }
                    line.extend_from_slice(b"\r\n");
                    self.dos.con_ahead = line;
                }
                None => return Ok(None),
            }
        }
        let k = n.min(self.dos.con_ahead.len());
        let data: Vec<u8> = self.dos.con_ahead.drain(..k).collect();
        self.hw.write_bytes(a, &data);
        Ok(Some(k))
    }
}
