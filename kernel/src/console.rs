//! The console device: TTY output to the screen with ANSI escape support,
//! line input from the keyboard.

use crate::screen;

struct Tty {
    /// Partial ANSI escape sequence.
    esc: [u8; 32],
    esc_len: usize,
    in_esc: bool,
    attr: u8,
    /// Cursor position saved by ESC[s.
    saved: (usize, usize),
    /// Output between ESC[s and ESC[u (status lines drawn elsewhere on the
    /// screen) is not echoed to the serial port.
    serial_hold: bool,
}

/// TTY state by session.
static TTY: crate::sync::SpinLock<alloc::collections::BTreeMap<u32, Tty>> =
    crate::sync::SpinLock::new(alloc::collections::BTreeMap::new());

const NEW_TTY: Tty = Tty { esc: [0; 32], esc_len: 0, in_esc: false, attr: 0x07, saved: (0, 0), serial_hold: false };

/// Write bytes as TTY output (VioWrtTTY semantics).
pub fn write(bytes: &[u8]) {
    let sid = crate::session::current();
    let mut ttys = TTY.lock();
    let t = ttys.entry(sid).or_insert(NEW_TTY);
    // The serial echo is a plain log of the text: escape sequences would
    // move the cursor of the terminal on the other end.
    let mut echo = alloc::vec::Vec::with_capacity(bytes.len());
    screen::with_session(sid, |s| {
        for &b in bytes {
            if t.in_esc {
                if t.esc_len < t.esc.len() {
                    let n = t.esc_len;
                    t.esc[n] = b;
                    t.esc_len += 1;
                }
                if b.is_ascii_alphabetic() {
                    let seq = t.esc;
                    let n = t.esc_len;
                    ansi(t, s, &seq[..n]);
                    match b {
                        b's' => t.serial_hold = true,
                        b'u' => t.serial_hold = false,
                        _ => {}
                    }
                    t.in_esc = false;
                    t.esc_len = 0;
                } else if t.esc_len == 1 && b != b'[' {
                    t.in_esc = false;
                    t.esc_len = 0;
                }
                continue;
            }
            match b {
                0x1b => {
                    t.in_esc = true;
                    t.esc_len = 0;
                }
                7 => {}
                _ => {
                    s.attr = t.attr;
                    s.write_char(b);
                    if !t.serial_hold {
                        echo.push(b);
                    }
                }
            }
        }
        let (r, c) = (s.cur_row, s.cur_col.min(s.cols - 1));
        s.set_cursor(Some((r, c)));
    });
    drop(ttys);
    for b in echo {
        crate::serial::write_byte(b);
    }
}

/// A session ended.
pub fn drop_session(sid: u32) {
    TTY.lock().remove(&sid);
}

fn params(seq: &[u8]) -> ([u16; 8], usize) {
    let mut p = [0u16; 8];
    let mut n = 0;
    let mut any = false;
    for &b in &seq[1..seq.len() - 1] {
        match b {
            b'0'..=b'9' => {
                p[n] = p[n] * 10 + (b - b'0') as u16;
                any = true;
            }
            b';' => {
                if n < 7 {
                    n += 1;
                }
            }
            _ => {}
        }
    }
    (p, if any || n > 0 { n + 1 } else { 0 })
}

fn ansi(t: &mut Tty, s: &mut screen::Screen, seq: &[u8]) {
    if seq.first() != Some(&b'[') {
        return;
    }
    let (p, n) = params(seq);
    let arg = |i: usize, d: u16| if i < n && p[i] != 0 { p[i] } else { d };
    match seq[seq.len() - 1] {
        b'H' | b'f' => {
            s.cur_row = (arg(0, 1) as usize - 1).min(s.rows - 1);
            s.cur_col = (arg(1, 1) as usize - 1).min(s.cols - 1);
        }
        b'A' => s.cur_row = s.cur_row.saturating_sub(arg(0, 1) as usize),
        b'B' => s.cur_row = (s.cur_row + arg(0, 1) as usize).min(s.rows - 1),
        b'C' => s.cur_col = (s.cur_col + arg(0, 1) as usize).min(s.cols - 1),
        b'D' => s.cur_col = s.cur_col.saturating_sub(arg(0, 1) as usize),
        b'J' => {
            let fill = 0x20 | (t.attr as u16) << 8;
            for r in 0..s.rows {
                for c in 0..s.cols {
                    s.put(r, c, fill);
                }
            }
            s.cur_row = 0;
            s.cur_col = 0;
        }
        b's' => t.saved = (s.cur_row, s.cur_col),
        b'u' => {
            s.cur_row = t.saved.0.min(s.rows - 1);
            s.cur_col = t.saved.1.min(s.cols - 1);
        }
        b'K' => {
            let fill = 0x20 | (t.attr as u16) << 8;
            for c in s.cur_col..s.cols {
                s.put(s.cur_row, c, fill);
            }
        }
        b'm' => {
            const MAP: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
            for i in 0..n.max(1) {
                let v = if n == 0 { 0 } else { p[i] };
                match v {
                    0 => t.attr = 0x07,
                    1 => t.attr |= 0x08,
                    5 => t.attr |= 0x80,
                    7 => t.attr = (t.attr & 0x88) | (t.attr & 0x07) << 4 | (t.attr & 0x70) >> 4,
                    8 => t.attr = (t.attr & 0xf0) | (t.attr >> 4 & 0x07),
                    30..=37 => t.attr = (t.attr & 0xf8) | MAP[(v - 30) as usize],
                    40..=47 => t.attr = (t.attr & 0x8f) | MAP[(v - 40) as usize] << 4,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
