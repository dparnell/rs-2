//! LINK.EXE and LINK386.EXE: thin OS/2 wrappers around crates/omflink.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

struct Os2Host;

impl omflink::Host for Os2Host {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        utils::files::read_file(path).ok()
    }
    fn write(&mut self, path: &str, data: &[u8]) -> bool {
        utils::files::write_file(path, data).is_ok()
    }
    fn delete(&mut self, path: &str) {
        let c = cstr(path);
        unsafe { DosDelete(c.as_ptr()) };
    }
    fn env(&mut self, name: &str) -> Option<String> {
        let c = cstr(name);
        let mut v: *const u8 = core::ptr::null();
        if unsafe { DosScanEnv(c.as_ptr(), &mut v) } != 0 || v.is_null() {
            return None;
        }
        let mut s = String::new();
        let mut p = v;
        unsafe {
            while *p != 0 {
                s.push(*p as char);
                p = p.add(1);
            }
        }
        Some(s)
    }
    fn out(&mut self, s: &str) {
        os2::print(s);
    }
    fn prompt(&mut self) -> Option<String> {
        let mut line = Vec::new();
        if utils::stdin_is_console() {
            let mut buf = [0u8; 256];
            let mut n = 0;
            if unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
                return None;
            }
            line.extend_from_slice(&buf[..n as usize]);
        } else {
            // A file or pipe: one line of it, shown as if typed.
            loop {
                let mut b = [0u8; 1];
                let mut got = 0;
                if unsafe { DosRead(0, b.as_mut_ptr(), 1, &mut got) } != 0 || got == 0 {
                    if line.is_empty() {
                        return None;
                    }
                    break;
                }
                line.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
            os2::write(&line);
        }
        let s: String = line.iter().map(|&b| b as char).collect();
        Some(String::from(s.trim_end_matches(['\r', '\n'])))
    }
}

/// The program's main: the command line goes to the linker as typed.
pub fn main(s: &Startup, prog: omflink::Prog) -> u32 {
    let args: String = s.args().iter().map(|&b| b as char).collect();
    omflink::run(&mut Os2Host, prog, &args) as u32
}
