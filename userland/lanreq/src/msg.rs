//! NET.MSG output: messages by number with %1.. inserts, the error rule
//! of IBM's NET.EXE (a NERR_ code >= 2100 prints the message of that
//! number; an OS/2 error prints 3502 and the OSO001.MSG text).

use alloc::string::String;
use alloc::vec::Vec;

/// Message `n` of NET.MSG with `ins` inserted (with its line end unless
/// the message ends in %0).
pub fn get(n: u32, ins: &[&str]) -> String {
    let file = b"NET.MSG\0";
    let owned: Vec<Vec<u8>> = ins.iter().map(|s| super::cstr(s)).collect();
    let ptrs: Vec<*const u8> = owned.iter().map(|v| v.as_ptr()).collect();
    let mut buf = alloc::vec![0u8; 1024];
    let mut cb = 0u32;
    let r = unsafe { os2::msg::DosGetMessage(ptrs.as_ptr(), ptrs.len() as u32, buf.as_mut_ptr(), buf.len() as u32, n, file.as_ptr(), &mut cb) };
    if r != 0 && cb == 0 {
        let mut s = alloc::format!("NET{n:04}");
        for i in ins {
            s.push(' ');
            s.push_str(i);
        }
        s.push_str("\r\n");
        return s;
    }
    buf[..cb as usize].iter().map(|&b| b as char).collect()
}

/// The message without its line end (labels, table headers).
pub fn text(n: u32) -> String {
    String::from(get(n, &[]).trim_end_matches(['\r', '\n']))
}

pub fn print(n: u32, ins: &[&str]) {
    os2::write_text(1, &get(n, &ins));
}

pub fn eprint(n: u32, ins: &[&str]) {
    os2::write_text(2, &get(n, &ins));
}

/// Report an error code the way NET.EXE does.
pub fn error(rc: u32) {
    if rc >= 2100 {
        eprint(rc, &[]);
    } else {
        let n = alloc::format!("{rc}");
        eprint(3502, &[&n]);
        os2::write_text(2, &utils::sys_message(rc));
        os2::write_text(2, "\r\n");
    }
}
