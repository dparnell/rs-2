//! Separator pages: a queue's separator file (PRQINFO3 pszSepFile, the
//! printer object's Print options page) is printed before each of its
//! jobs, as IBM's spooler does (`PrintSep`, `DoPrintSep`). The file is
//! LAN Manager's separator format, which OS/2's SAMPLE.SEP (text) and
//! PSCRIPT.SEP (a PostScript page) use: its first character is the
//! escape character, and escape sequences make the output:
//!
//! | Escape | Prints |
//! |--------|--------|
//! | `N` | the job's owner (user name) |
//! | `I` | the job id |
//! | `J` | the job's document name |
//! | `Q` | the queue's name |
//! | `D`, `T` | the date and the time the separator is printed |
//! | `L` | the text that follows, up to the next escape character |
//! | `F` path | the file's contents |
//! | `H`nn | the byte with hexadecimal value nn (printer control codes) |
//! | `W`nn | the page width (default 80 columns): longer lines are cut |
//! | `0`-`9` | a new line, then that many empty lines |
//! | `E` | a form feed (eject) |
//! | `B`, `S`, `M`, `U` | block characters (`B` then `S` single, `M` double width), `U` normal text |
//!
//! Other text outside `L` is not printed (line ends in the file only lay
//! it out). rs2 prints block-character text as normal text.

use alloc::vec::Vec;

use crate::util::{dec, read_file};

/// What a separator says of the job.
pub struct SepJob<'a> {
    pub user: &'a [u8],
    pub id: u32,
    pub document: &'a [u8],
    pub queue: &'a [u8],
}

/// Output text, cutting lines at the page width.
fn put(out: &mut Vec<u8>, col: &mut usize, b: &[u8], width: usize) {
    for &c in b {
        match c {
            b'\r' | b'\n' | b'\x0c' => {
                *col = 0;
                out.push(c);
            }
            _ => {
                if *col < width {
                    out.push(c);
                }
                *col += 1;
            }
        }
    }
}

fn two(n: u32) -> Vec<u8> {
    dec(n, 2)
}

/// The separator file's output for a job.
pub fn render(file: &[u8], j: &SepJob) -> Vec<u8> {
    let mut out = Vec::new();
    let Some(&esc) = file.first() else { return out };
    let mut width = 80usize;
    // The current output line's length (for @W).
    let mut col = 0usize;
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let mut i = 1;
    while i < file.len() {
        // Skip to the next escape.
        let Some(k) = file[i..].iter().position(|&c| c == esc) else { break };
        i += k + 1;
        let Some(&code) = file.get(i) else { break };
        i += 1;
        match code.to_ascii_uppercase() {
            c @ b'0'..=b'9' => {
                put(&mut out, &mut col, b"\r\n", width);
                for _ in 0..(c - b'0') {
                    put(&mut out, &mut col, b"\r\n", width);
                }
            }
            b'N' => put(&mut out, &mut col, j.user, width),
            b'I' => put(&mut out, &mut col, &dec(j.id, 1), width),
            b'J' => put(&mut out, &mut col, j.document, width),
            b'Q' => put(&mut out, &mut col, j.queue, width),
            b'D' => {
                let d = [&two(dt.month as u32)[..], b"-", &two(dt.day as u32), b"-", &dec(dt.year as u32, 4)].concat();
                put(&mut out, &mut col, &d, width);
            }
            b'T' => {
                let t = [&two(dt.hours as u32)[..], b":", &two(dt.minutes as u32), b":", &two(dt.seconds as u32)].concat();
                put(&mut out, &mut col, &t, width);
            }
            b'E' => put(&mut out, &mut col, b"\x0c", width),
            b'L' => {
                let end = file[i..].iter().position(|&c| c == esc).map_or(file.len(), |k| i + k);
                put(&mut out, &mut col, &file[i..end], width);
                i = end;
            }
            b'F' => {
                let end = file[i..].iter().position(|&c| c == esc || c == b'\r' || c == b'\n').map_or(file.len(), |k| i + k);
                let path: Vec<u8> = file[i..end].iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
                if let Some(d) = read_file(&path) {
                    out.extend_from_slice(&d);
                    col = 0;
                }
                i = end;
            }
            b'H' => {
                let hex = |c: u8| (c as char).to_digit(16);
                if let (Some(a), Some(b)) = (file.get(i).and_then(|&c| hex(c)), file.get(i + 1).and_then(|&c| hex(c))) {
                    out.push((a * 16 + b) as u8);
                    i += 2;
                }
            }
            b'W' => {
                let mut n = 0usize;
                while let Some(&c) = file.get(i).filter(|c| c.is_ascii_digit()) {
                    n = n * 10 + (c - b'0') as usize;
                    i += 1;
                }
                if n > 0 {
                    width = n.min(256);
                }
            }
            // Block characters on and off: rs2 prints the text as it is.
            b'B' | b'S' | b'M' | b'U' => {}
            _ => {}
        }
    }
    out
}
