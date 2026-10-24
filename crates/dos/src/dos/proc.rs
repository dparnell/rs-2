//! Processes: the PSP, the environment, loading .COM and .EXE programs
//! (4Bh), ending them (00h, 4Ch, 31h, INT 20h, INT 27h).

use alloc::string::String;
use alloc::vec::Vec;

use x86emu::flags::{CF, IF};
use x86emu::{Seg, CS, DS, EAX, ES, ESP, SS};

use super::{err, ExecFrame, Name, Sft};
use crate::{Host, Machine};

/// Pack a date and time as DOS stores them: (date, time).
pub fn pack_dt(n: &crate::DateTime) -> (u16, u16) {
    let d = ((n.year.saturating_sub(1980)) << 9) | (n.month as u16) << 5 | n.day as u16;
    let t = (n.hour as u16) << 11 | (n.minute as u16) << 5 | (n.second as u16 / 2);
    (d, t)
}

impl Machine {
    /// Build a PSP at `seg`: memory top `end`, parent `parent`,
    /// environment `env`.
    pub fn init_psp(&mut self, seg: u16, end: u16, parent: u16, env: u16) {
        let a = Machine::lin(seg, 0);
        for i in 0..256 {
            self.hw.wr(a + i, 0);
        }
        self.hw.wr16(a, 0x20cd);
        self.hw.wr16(a + 2, end);
        // CP/M entry: far call to DOS.
        self.hw.wr(a + 5, 0x9a);
        self.hw.wr16(a + 6, 0xfef0);
        self.hw.wr16(a + 8, 0xf01d);
        // INT 22h-24h at the time.
        for (i, v) in [0x22u8, 0x23, 0x24].iter().enumerate() {
            let (s, o) = self.vector(*v);
            self.hw.wr16(a + 0x0a + i as u32 * 4, o);
            self.hw.wr16(a + 0x0c + i as u32 * 4, s);
        }
        self.hw.wr16(a + 0x16, parent);
        for i in 0..20 {
            self.hw.wr(a + 0x18 + i, 0xff);
        }
        self.hw.wr16(a + 0x2c, env);
        self.hw.wr16(a + 0x32, 20);
        self.hw.wr16(a + 0x34, 0x18);
        self.hw.wr16(a + 0x36, seg);
        self.hw.wr32(a + 0x38, 0xffff_ffff);
        let (maj, min) = self.cfg.version;
        self.hw.wr(a + 0x40, maj);
        self.hw.wr(a + 0x41, min);
        // INT 21h; RETF
        self.hw.write_bytes(a + 0x50, &[0xcd, 0x21, 0xcb]);
        // Blank FCBs.
        self.hw.write_bytes(a + 0x5d, &[b' '; 11]);
        self.hw.write_bytes(a + 0x6d, &[b' '; 11]);
        self.hw.wr(a + 0x81, 0x0d);
    }

    /// 26h/55h: copy the current PSP to `to`.
    pub(crate) fn copy_psp(&mut self, from: u16, to: u16, end: u16) {
        let a = Machine::lin(from, 0);
        let b = Machine::lin(to, 0);
        for i in 0..256 {
            let v = self.hw.rd(a + i);
            self.hw.wr(b + i, v);
        }
        self.hw.wr16(b + 2, end);
        self.hw.wr16(b + 0x34, 0x18);
        self.hw.wr16(b + 0x36, to);
    }

    /// Give the child copies of the parent's inheritable handles.
    pub(crate) fn inherit_handles(&mut self, parent: u16, child: u16) {
        let pa = Machine::lin(parent, 0);
        let n = self.hw.rd16(pa + 0x32).min(20);
        let poff = self.hw.rd16(pa + 0x34);
        let pseg = self.hw.rd16(pa + 0x36);
        let pt = Machine::lin(pseg, poff);
        let ct = Machine::lin(child, 0x18);
        for h in 0..20u32 {
            let i = if h < n as u32 { self.hw.rd(pt + h) } else { 0xff };
            let keep = i != 0xff && self.dos.sft.get(i as usize).and_then(|s| s.as_ref()).is_some_and(|s| s.mode & 0x80 == 0);
            if keep {
                self.dos.sft[i as usize].as_mut().unwrap().refs += 1;
                self.hw.wr(ct + h, i);
            } else {
                self.hw.wr(ct + h, 0xff);
            }
        }
    }

    /// Close every handle of `psp`.
    fn close_all<H: Host>(&mut self, host: &mut H, psp: u16) {
        let a = Machine::lin(psp, 0);
        let n = self.hw.rd16(a + 0x32);
        for h in 0..n {
            let _ = self.close_handle(host, psp, h);
        }
    }

    /// The environment of `psp` as strings, and the size of its block.
    pub fn env_strings(&mut self, env: u16) -> Vec<Vec<u8>> {
        let mut v = Vec::new();
        if env == 0 {
            return v;
        }
        let mut a = Machine::lin(env, 0);
        for _ in 0..1024 {
            let s = self.hw.cstr(a, 32768);
            if s.is_empty() {
                break;
            }
            a += s.len() as u32 + 1;
            v.push(s);
        }
        v
    }

    /// Value of an environment variable of the current process.
    pub fn getenv(&mut self, name: &str) -> Option<String> {
        let psp = self.dos.psp;
        let env = self.hw.rd16(Machine::lin(psp, 0x2c));
        for s in self.env_strings(env) {
            if let Some(i) = s.iter().position(|&c| c == b'=') {
                if s[..i].eq_ignore_ascii_case(name.as_bytes()) {
                    return Some(s[i + 1..].iter().map(|&b| b as char).collect());
                }
            }
        }
        None
    }

    /// Make an environment block: `vars`, then the program's path.
    fn make_env<H: Host>(&mut self, host: &mut H, vars: &[Vec<u8>], prog: &str, owner: u16) -> Result<u16, u16> {
        let mut data = Vec::new();
        for v in vars {
            data.extend_from_slice(v);
            data.push(0);
        }
        if vars.is_empty() {
            data.push(0);
        }
        data.push(0);
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(prog.as_bytes());
        data.push(0);
        // Room for the program to add variables, as COMMAND.COM leaves.
        let paras = (data.len() as u32 + 160).div_ceil(16) as u16;
        let seg = self.alloc_mem(host, paras, owner)?;
        self.hw.write_bytes(Machine::lin(seg, 0), &data);
        Ok(seg)
    }

    /// Read a whole file through the host.
    pub fn read_file<H: Host>(&mut self, host: &mut H, path: &str) -> Result<Vec<u8>, u16> {
        let h = host.open(path, crate::OpenMode(0x40))?;
        let mut v = Vec::new();
        let mut buf = alloc::vec![0u8; 32768];
        loop {
            match host.read(h, &mut buf) {
                Ok(0) => break,
                Ok(n) => v.extend_from_slice(&buf[..n]),
                Err(e) => {
                    host.close(h);
                    return Err(e);
                }
            }
            if v.len() > 0x200_0000 {
                break;
            }
        }
        host.close(h);
        Ok(v)
    }

    /// Load a program: (psp, cs, ip, ss, sp). `tail` is the command
    /// tail (length byte first, CR at the end), `env` the environment
    /// segment to copy (0: the parent's).
    #[allow(clippy::too_many_arguments)]
    pub fn load_program<H: Host>(
        &mut self,
        host: &mut H,
        path: &str,
        tail: &[u8],
        env_src: u16,
        fcb1: Option<[u8; 16]>,
        fcb2: Option<[u8; 16]>,
    ) -> Result<(u16, u16, u16, u16, u16), u16> {
        let image = self.read_file(host, path)?;
        let parent = self.dos.psp;
        let pa = Machine::lin(parent, 0);
        let vars: Vec<Vec<u8>> = {
            let env = if env_src != 0 { env_src } else { self.hw.rd16(pa + 0x2c) };
            self.env_strings(env)
        };
        let is_exe = image.len() >= 0x1c && (image[0..2] == *b"MZ" || image[0..2] == *b"ZM");
        let env = self.make_env(host, &vars, path, parent)?;
        // The program gets the largest block (EXE: bounded by its header).
        let largest = self.largest_free();
        let (need, want) = if is_exe {
            let h = |o: usize| u16::from_le_bytes([image[o], image[o + 1]]) as u32;
            let pages = h(4);
            let last = h(2);
            let size = if last == 0 { pages * 512 } else { (pages - 1) * 512 + last };
            let hdr = h(8) * 16;
            let load = size.saturating_sub(hdr);
            let min = h(0x0a);
            let max = h(0x0c);
            let need = (load.div_ceil(16) + 0x10 + min) as u16;
            let want = if max == 0 { need } else { (load.div_ceil(16) + 0x10).saturating_add(max).min(0xffff) as u16 };
            (need, want.max(need))
        } else {
            (((image.len() as u32 + 0x100 + 0x100).div_ceil(16)).min(0x1000) as u16, 0xffff)
        };
        if largest < need {
            let _ = self.free_mem(env);
            return Err(err::NO_MEMORY);
        }
        let size = want.min(largest);
        let psp = match self.alloc_mem(host, size, 0) {
            Ok(s) => s,
            Err(e) => {
                let _ = self.free_mem(env);
                return Err(e);
            }
        };
        // Owners: the new process.
        self.hw.wr16(Machine::lin(psp - 1, 1), psp);
        self.hw.wr16(Machine::lin(env - 1, 1), psp);
        let base = path.rsplit('\\').next().unwrap_or(path);
        let stem: Vec<u8> = base.bytes().take_while(|&c| c != b'.').take(8).collect();
        self.mcb_name(psp - 1, &stem);
        self.mcb_name(env - 1, &stem);
        self.init_psp(psp, psp + size, parent, env);
        self.inherit_handles(parent, psp);
        let a = Machine::lin(psp, 0);
        let n = tail.len().min(128);
        self.hw.write_bytes(a + 0x80, &tail[..n]);
        if let Some(f) = fcb1 {
            self.hw.write_bytes(a + 0x5c, &f);
        }
        if let Some(f) = fcb2 {
            self.hw.write_bytes(a + 0x6c, &f[..16]);
        }
        let r = if is_exe {
            let h = |o: usize| u16::from_le_bytes([image[o], image[o + 1]]);
            let hdr = h(8) as usize * 16;
            let pages = h(4) as usize;
            let last = h(2) as usize;
            let size = if last == 0 { pages * 512 } else { (pages - 1) * 512 + last };
            let end = size.min(image.len());
            let load = psp + 0x10;
            let body = &image[hdr.min(end)..end];
            self.hw.write_bytes(Machine::lin(load, 0), body);
            let nrel = h(6) as usize;
            let rel = h(0x18) as usize;
            for i in 0..nrel {
                let o = rel + i * 4;
                if o + 4 > image.len() {
                    break;
                }
                let off = h(o);
                let seg = h(o + 2);
                let at = Machine::lin(load.wrapping_add(seg), off);
                let v = self.hw.rd16(at);
                self.hw.wr16(at, v.wrapping_add(load));
            }
            (psp, load.wrapping_add(h(0x16)), h(0x14), load.wrapping_add(h(0x0e)), h(0x10))
        } else {
            let n = image.len().min(0xff00);
            self.hw.write_bytes(Machine::lin(psp, 0x100), &image[..n]);
            let top = ((size as u32) * 16).min(0x10000);
            let sp = (top - 2) as u16;
            self.hw.wr16(Machine::lin(psp, sp), 0);
            (psp, psp, 0x100, psp, sp)
        };
        Ok(r)
    }

    /// Start the first program of the session.
    pub(crate) fn dos_start<H: Host>(&mut self, host: &mut H, path: &str, args: &str) -> Result<(), u16> {
        // Standard handles for the system PSP: CON x3, AUX, PRN.
        let con = self.new_sft(Sft { refs: 0, dev: Some(super::Device::Con), host: 0, path: String::new(), mode: 2, info: super::Device::Con.info(), owner: 0 })?;
        let aux = self.new_sft(Sft { refs: 0, dev: Some(super::Device::Aux), host: 0, path: String::new(), mode: 2, info: super::Device::Aux.info(), owner: 0 })?;
        let prn = self.new_sft(Sft { refs: 0, dev: Some(super::Device::Prn), host: 0, path: String::new(), mode: 1, info: super::Device::Prn.info(), owner: 0 })?;
        let t = Machine::lin(super::SYS_PSP, 0x18);
        for (h, i) in [con, con, con, aux, prn].iter().enumerate() {
            self.hw.wr(t + h as u32, *i as u8);
            self.dos.sft[*i].as_mut().unwrap().refs += 1;
        }
        // The first environment.
        let vars: Vec<Vec<u8>> = self.cfg.env.iter().map(|s| s.as_bytes().to_vec()).collect();
        let mut data = Vec::new();
        for v in &vars {
            data.extend_from_slice(v);
            data.push(0);
        }
        data.push(0);
        data.extend_from_slice(&0u16.to_le_bytes());
        let env = self.alloc_mem(host, (data.len() as u32 + 512).div_ceil(16) as u16, super::SYS_PSP)?;
        self.hw.write_bytes(Machine::lin(env, 0), &data);
        self.mcb_name(env - 1, b"SYSTEM");
        self.hw.wr16(Machine::lin(super::SYS_PSP, 0x2c), env);
        self.set_psp(super::SYS_PSP);
        // The program.
        let full = match self.resolve(path.as_bytes())? {
            Name::File(f) => f,
            Name::Dev(_) => return Err(err::FILE_NOT_FOUND),
        };
        let mut tail = Vec::new();
        let args = args.trim_end();
        let a: Vec<u8> = if args.is_empty() || args.starts_with(' ') { args.bytes().collect() } else { core::iter::once(b' ').chain(args.bytes()).collect() };
        tail.push(a.len().min(126) as u8);
        tail.extend_from_slice(&a[..a.len().min(126)]);
        tail.push(0x0d);
        let (f1, f2) = self.parse_fcbs(&a);
        let (psp, cs, ip, ss, sp) = self.load_program(host, &full, &tail, 0, Some(f1), Some(f2))?;
        self.dos.root_psp = psp;
        self.hw.wr16(Machine::lin(psp, 0x16), psp);
        // Its terminate address: the termination callback.
        self.hw.wr16(Machine::lin(psp, 0x0a), crate::bios::TERM_CODE);
        self.hw.wr16(Machine::lin(psp, 0x0c), crate::bios::ROM);
        self.set_psp(psp);
        self.set_dta(psp, 0x80);
        self.enter(psp, cs, ip, ss, sp);
        Ok(())
    }

    /// Transfer to a loaded program.
    fn enter(&mut self, psp: u16, cs: u16, ip: u16, ss: u16, sp: u16) {
        self.cpu.cr0 &= !1;
        self.cpu.seg[CS] = Seg::real_code(cs);
        self.cpu.seg[DS] = Seg::real(psp);
        self.cpu.seg[ES] = Seg::real(psp);
        self.cpu.seg[SS] = Seg::real(ss);
        self.cpu.seg[x86emu::FS] = Seg::real(0);
        self.cpu.seg[x86emu::GS] = Seg::real(0);
        self.cpu.eip = ip as u32;
        self.cpu.regs = [0; 8];
        self.cpu.regs[ESP] = sp as u32;
        self.cpu.flags = 0x0202;
        self.cpu.halted = false;
        // AL/AH: whether the FCB drives are valid; BX:CX ignored; DX=PSP.
        let fcb_ok = |m: &mut Machine, off: u32| {
            let d = m.hw.rd(Machine::lin(psp, 0) + off);
            if d == 0 || (d <= 26 && m.dos.present[d as usize - 1]) { 0 } else { 0xff }
        };
        let al = fcb_ok(self, 0x5c);
        let ah = fcb_ok(self, 0x6c);
        self.cpu.regs[EAX] = (ah as u32) << 8 | al as u32;
        self.cpu.set_dx(psp);
        self.cpu.set_si(ip);
        self.cpu.set_di(sp);
        self.cpu.regs[x86emu::EBP] = 0x091e;
    }

    /// 4Bh: load and execute (00h), load only (01h), overlay (03h).
    pub(crate) fn exec_fn<H: Host>(&mut self, host: &mut H) {
        let al = self.cpu.al();
        let name = self.hw.cstr(self.ds_dx(), 128);
        let full = match self.resolve(&name) {
            Ok(Name::File(f)) => f,
            Ok(Name::Dev(_)) => return self.fail(err::FILE_NOT_FOUND),
            Err(e) => return self.fail(e),
        };
        let pb = Machine::lin(self.cpu.seg[ES].sel, self.cpu.bx());
        match al {
            0x00 | 0x01 => {
                let env = self.hw.rd16(pb);
                let tail_ptr = Machine::lin(self.hw.rd16(pb + 4), self.hw.rd16(pb + 2));
                let f1p = Machine::lin(self.hw.rd16(pb + 8), self.hw.rd16(pb + 6));
                let f2p = Machine::lin(self.hw.rd16(pb + 12), self.hw.rd16(pb + 10));
                let len = (self.hw.rd(tail_ptr) as usize).min(126);
                let mut tail = alloc::vec![0u8; len + 2];
                self.hw.read_bytes(tail_ptr, &mut tail);
                tail[len + 1] = 0x0d;
                let mut f1 = [0u8; 16];
                let mut f2 = [0u8; 16];
                self.hw.read_bytes(f1p, &mut f1);
                self.hw.read_bytes(f2p, &mut f2);
                let (psp, cs, ip, ss, sp) = match self.load_program(host, &full, &tail, env, Some(f1), Some(f2)) {
                    Ok(r) => r,
                    Err(e) => return self.fail(e),
                };
                // The child returns where the parent called from.
                let (rip, rcs) = (self.ret_ip(), self.ret_cs());
                self.hw.wr16(Machine::lin(psp, 0x0a), rip);
                self.hw.wr16(Machine::lin(psp, 0x0c), rcs);
                // The parent's stack (as DOS keeps it in the PSP).
                let parent = self.dos.psp;
                let sp_now = self.cpu.sp();
                let ss_now = self.cpu.seg[SS].sel;
                self.hw.wr16(Machine::lin(parent, 0x2e), sp_now);
                self.hw.wr16(Machine::lin(parent, 0x30), ss_now);
                if al == 0x01 {
                    // Load only: SS:SP and CS:IP into the parameter block.
                    let sp2 = sp.wrapping_sub(2);
                    let ax = self.hw.rd16(Machine::lin(ss, sp));
                    let _ = ax;
                    self.hw.wr16(pb + 0x0e, sp2);
                    self.hw.wr16(pb + 0x10, ss);
                    self.hw.wr16(pb + 0x12, ip);
                    self.hw.wr16(pb + 0x14, cs);
                    self.hw.wr16(Machine::lin(ss, sp2), 0);
                    self.set_psp(psp);
                    self.set_dta(psp, 0x80);
                    self.ok();
                    return;
                }
                let mut segs = [0u16; 6];
                for (i, s) in segs.iter_mut().enumerate() {
                    *s = self.cpu.seg[i].sel;
                }
                self.dos.exec_stack.push(ExecFrame { child: psp, regs: self.cpu.regs, segs, flags: self.cpu.flags });
                self.set_psp(psp);
                self.set_dta(psp, 0x80);
                self.enter(psp, cs, ip, ss, sp);
            }
            0x03 => {
                // Overlay: load at the segment given, relocated by the factor.
                let seg = self.hw.rd16(pb);
                let factor = self.hw.rd16(pb + 2);
                let image = match self.read_file(host, &full) {
                    Ok(i) => i,
                    Err(e) => return self.fail(e),
                };
                if image.len() >= 0x1c && (image[0..2] == *b"MZ" || image[0..2] == *b"ZM") {
                    let h = |o: usize| u16::from_le_bytes([image[o], image[o + 1]]);
                    let hdr = h(8) as usize * 16;
                    let pages = h(4) as usize;
                    let last = h(2) as usize;
                    let size = if last == 0 { pages * 512 } else { (pages - 1) * 512 + last };
                    let end = size.min(image.len());
                    self.hw.write_bytes(Machine::lin(seg, 0), &image[hdr.min(end)..end]);
                    for i in 0..h(6) as usize {
                        let o = h(0x18) as usize + i * 4;
                        if o + 4 > image.len() {
                            break;
                        }
                        let at = Machine::lin(seg.wrapping_add(h(o + 2)), h(o));
                        let v = self.hw.rd16(at);
                        self.hw.wr16(at, v.wrapping_add(factor));
                    }
                } else {
                    self.hw.write_bytes(Machine::lin(seg, 0), &image);
                }
                self.ok();
            }
            _ => self.fail(err::INVALID_FUNCTION),
        }
    }

    /// End the current program (00h, 4Ch, INT 20h): exit code `code`,
    /// termination type `kind` (0 normal, 1 Ctrl+C, 2 critical error).
    pub fn dos_exit<H: Host>(&mut self, host: &mut H, code: u8, kind: u8) {
        let psp = self.dos.psp;
        self.dpmi_process_ended(psp);
        self.close_all(host, psp);
        self.free_owned(psp);
        self.mouse_process_ended(psp);
        self.xms_process_ended(psp);
        self.ems_process_ended(psp);
        self.finish(host, psp, code, kind);
    }

    /// Terminate and stay resident with `paras` of the PSP's block.
    pub(crate) fn dos_tsr<H: Host>(&mut self, host: &mut H, paras: u16, code: u8) {
        let psp = self.dos.psp;
        let _ = self.resize_mem(psp, paras);
        self.finish(host, psp, code, 3);
    }

    fn finish<H: Host>(&mut self, host: &mut H, psp: u16, code: u8, kind: u8) {
        self.dos.return_code = (kind as u16) << 8 | code as u16;
        let a = Machine::lin(psp, 0);
        // Restore INT 22h-24h from the PSP.
        let term_off = self.hw.rd16(a + 0x0a);
        let term_seg = self.hw.rd16(a + 0x0c);
        for (i, v) in [0x22u8, 0x23, 0x24].iter().enumerate() {
            let o = self.hw.rd16(a + 0x0a + i as u32 * 4);
            let s = self.hw.rd16(a + 0x0c + i as u32 * 4);
            self.set_vector(*v, s, o);
        }
        if psp == self.dos.root_psp || psp == super::SYS_PSP {
            self.exited = Some(code);
            return;
        }
        let parent = self.hw.rd16(a + 0x16);
        self.set_psp(parent);
        self.shell_child_ended(host, psp);
        if let Some(i) = self.dos.exec_stack.iter().rposition(|f| f.child == psp) {
            let f = self.dos.exec_stack[i].clone();
            self.dos.exec_stack.truncate(i);
            self.cpu.cr0 &= !1;
            self.cpu.regs = f.regs;
            for (i, s) in f.segs.iter().enumerate() {
                self.cpu.seg[i] = if i == CS { Seg::real_code(*s) } else { Seg::real(*s) };
            }
            // Pop the INT 21h frame: back to the caller with CF clear.
            let flags = {
                let sa = self.cpu.lin(SS, self.cpu.stack_ptr().wrapping_add(4));
                self.hw.rd16(sa) as u32
            };
            let sp = self.cpu.sp().wrapping_add(6);
            self.cpu.set16(ESP, sp);
            self.cpu.flags = (flags & !CF) | 2;
            self.cpu.seg[CS] = Seg::real_code(term_seg);
            self.cpu.eip = term_off as u32;
        } else {
            // Loaded with 4B01h: to the terminate address on the
            // parent's saved stack.
            let pa = Machine::lin(parent, 0);
            let sp = self.hw.rd16(pa + 0x2e);
            let ss = self.hw.rd16(pa + 0x30);
            self.cpu.cr0 &= !1;
            self.cpu.seg[SS] = Seg::real(ss);
            self.cpu.set16(ESP, sp);
            self.cpu.seg[DS] = Seg::real(parent);
            self.cpu.seg[ES] = Seg::real(parent);
            self.cpu.seg[CS] = Seg::real_code(term_seg);
            self.cpu.eip = term_off as u32;
            self.cpu.flags = 0x0202 | IF;
        }
        self.set_dta(parent, 0x80);
    }

    /// The termination callback (the first program's return address).
    pub(crate) fn dos_terminated<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        if self.exited.is_none() {
            self.exited = Some(self.dos.return_code as u8);
        }
    }
}
