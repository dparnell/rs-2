//! The console device: TTY output to the screen (VioWrtTTY, DosWrite to
//! CON) with the ANSI escape sequences of OS/2's ANSI support.
//!
//! | sequence | effect |
//! |----------|--------|
//! | `ESC[r;cH`, `ESC[r;cf` | cursor position |
//! | `ESC[nA` `B` `C` `D` | cursor up, down, forward, back |
//! | `ESC[6n` | device status report: `ESC[r;cR` is typed into the keyboard buffer |
//! | `ESC[s`, `ESC[u` | save, restore the cursor position |
//! | `ESC[2J` | clear the screen, cursor home |
//! | `ESC[K` | erase to the end of the line |
//! | `ESC[n;...m` | attributes: 0 normal, 1 bold, 2 faint, 4 underline, 5 blink, 7 reverse, 8 concealed, 22/24/25/27/28 resets, 30-37 foreground, 39 default, 40-47 background, 49 default, 90-97/100-107 bright |
//! | `ESC[=nh`, `ESC[=nl` | screen mode 0-3 (40/80 columns), 7 line wrap on/off (also `ESC[?7h/l`) |
//!
//! ANSI processing is per session and can be turned off (VioSetAnsi),
//! after which ESC is an ordinary character. Ctrl+S suspends the output of
//! a session (see `conkbd`).

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
    /// ANSI sequences are interpreted (VioSetAnsi).
    ansi: bool,
    /// Lines wrap at the right edge (ESC[=7h).
    wrap: bool,
    concealed: bool,
}

/// TTY state by session.
static TTY: crate::sync::SpinLock<alloc::collections::BTreeMap<u32, Tty>> =
    crate::sync::SpinLock::new(alloc::collections::BTreeMap::new());

const NEW_TTY: Tty = Tty {
    esc: [0; 32],
    esc_len: 0,
    in_esc: false,
    attr: 0x07,
    saved: (0, 0),
    serial_hold: false,
    ansi: true,
    wrap: true,
    concealed: false,
};

/// Write bytes as TTY output (VioWrtTTY semantics).
pub fn write(bytes: &[u8]) {
    let sid = crate::session::io_session();
    crate::conkbd::wait_output(sid);
    let mut ttys = TTY.lock();
    let t = ttys.entry(sid).or_insert(NEW_TTY);
    // The serial echo is a plain log of the text: escape sequences would
    // move the cursor of the terminal on the other end.
    let mut echo = alloc::vec::Vec::with_capacity(bytes.len());
    let mut reply: Option<alloc::string::String> = None;
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
                    if let Some(r) = ansi(t, s, &seq[..n]) {
                        reply = Some(r);
                    }
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
                0x1b if t.ansi => {
                    t.in_esc = true;
                    t.esc_len = 0;
                }
                7 => {}
                b'\r' | b'\n' | 8 | b'\t' => {
                    s.attr = t.attr;
                    s.write_char(b);
                    if !t.serial_hold {
                        echo.push(b);
                    }
                }
                _ => {
                    let a = if t.concealed { (t.attr & 0xf0) | (t.attr >> 4 & 0x07) } else { t.attr };
                    s.attr = t.attr;
                    s.write_cell(b as u16 | (a as u16) << 8, t.wrap);
                    if !t.serial_hold {
                        echo.push(b);
                    }
                }
            }
        }
        s.scroll_if_needed();
        let (r, c) = (s.cur_row, s.cur_col.min(s.cols - 1));
        s.set_cursor(Some((r, c)));
    });
    drop(ttys);
    for b in echo {
        crate::serial::write_byte(b);
    }
    if let Some(r) = reply {
        crate::conkbd::inject(sid, r.as_bytes());
    }
}

/// A session ended.
pub fn drop_session(sid: u32) {
    TTY.lock().remove(&sid);
}

/// VioGetAnsi / VioSetAnsi for the calling thread's session.
pub fn ansi_enabled() -> bool {
    let sid = crate::session::io_session();
    TTY.lock().get(&sid).is_none_or(|t| t.ansi)
}

pub fn set_ansi(on: bool) {
    let sid = crate::session::io_session();
    let mut g = TTY.lock();
    let t = g.entry(sid).or_insert(NEW_TTY);
    t.ansi = on;
    if !on {
        t.in_esc = false;
        t.esc_len = 0;
    }
}

fn params(seq: &[u8]) -> ([u16; 8], usize) {
    let mut p = [0u16; 8];
    let mut n = 0;
    let mut any = false;
    for &b in &seq[1..seq.len() - 1] {
        match b {
            b'0'..=b'9' => {
                p[n] = p[n].saturating_mul(10).saturating_add((b - b'0') as u16);
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

/// Interpret one sequence (after ESC). Returns a reply to type into the
/// keyboard buffer.
fn ansi(t: &mut Tty, s: &mut screen::Screen, seq: &[u8]) -> Option<alloc::string::String> {
    if seq.first() != Some(&b'[') {
        return None;
    }
    let (p, n) = params(seq);
    let arg = |i: usize, d: u16| if i < n && p[i] != 0 { p[i] } else { d };
    let private = matches!(seq.get(1), Some(b'=') | Some(b'?'));
    match seq[seq.len() - 1] {
        b'H' | b'f' => {
            s.cur_row = (arg(0, 1) as usize - 1).min(s.rows - 1);
            s.cur_col = (arg(1, 1) as usize - 1).min(s.cols - 1);
        }
        b'A' => s.cur_row = s.cur_row.saturating_sub(arg(0, 1) as usize),
        b'B' => s.cur_row = (s.cur_row + arg(0, 1) as usize).min(s.rows - 1),
        b'C' => s.cur_col = (s.cur_col + arg(0, 1) as usize).min(s.cols - 1),
        b'D' => s.cur_col = s.cur_col.min(s.cols - 1).saturating_sub(arg(0, 1) as usize),
        b'n' if n > 0 && p[0] == 6 => {
            return Some(alloc::format!("\x1b[{:02};{:02}R\r", s.cur_row + 1, s.cur_col.min(s.cols - 1) + 1));
        }
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
            for c in s.cur_col.min(s.cols)..s.cols {
                s.put(s.cur_row, c, fill);
            }
        }
        b'h' | b'l' if private => {
            let on = seq[seq.len() - 1] == b'h';
            match arg(0, 0) {
                7 => t.wrap = on,
                0 | 1 if on => s.set_mode(40, 25),
                2 | 3 if on => s.set_mode(80, 25),
                _ => {}
            }
        }
        b'm' => {
            const MAP: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
            for i in 0..n.max(1) {
                let v = if n == 0 { 0 } else { p[i] };
                match v {
                    0 => {
                        t.attr = 0x07;
                        t.concealed = false;
                    }
                    1 => t.attr |= 0x08,
                    2 | 22 => t.attr &= !0x08,
                    // Underline exists on monochrome displays only: there
                    // it is foreground 1.
                    4 => t.attr = (t.attr & 0xf8) | 0x01,
                    24 => t.attr = (t.attr & 0xf8) | 0x07,
                    5 | 6 => t.attr |= 0x80,
                    25 => t.attr &= !0x80,
                    7 => t.attr = (t.attr & 0x88) | (t.attr & 0x07) << 4 | (t.attr & 0x70) >> 4,
                    27 => t.attr = (t.attr & 0x88) | (t.attr & 0x07) << 4 | (t.attr & 0x70) >> 4,
                    8 => t.concealed = true,
                    28 => t.concealed = false,
                    30..=37 => t.attr = (t.attr & 0xf8) | MAP[(v - 30) as usize],
                    39 => t.attr = (t.attr & 0xf8) | 0x07,
                    40..=47 => t.attr = (t.attr & 0x8f) | MAP[(v - 40) as usize] << 4,
                    49 => t.attr &= 0x8f,
                    90..=97 => t.attr = (t.attr & 0xf0) | 0x08 | MAP[(v - 90) as usize],
                    100..=107 => t.attr = (t.attr & 0x0f) | 0x80 | MAP[(v - 100) as usize] << 4,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    None
}
