//! The crash screen: what a kernel panic or kernel trap leaves on the
//! display when there is no serial port to read (real hardware such as
//! the Surface Pro 4).
//!
//! The left side says in large text what happened (the lines the panic
//! or trap handler printed: message, registers, backtrace) and shows the
//! end of the log; the right side is a QR code with a crash report:
//! a header (version, uptime, command line) and as much of the log as
//! fits, compressed (`crates/crashqr`: `RS2C1:` + base45(zlib(text))).
//! Scan it with a phone and run `tools/scripts/crashqr.py` on the text.
//!
//! Nothing here allocates or takes a lock that another CPU may hold:
//! the buffers are static, the log comes from the ring in `klog.rs`.

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use bootinfo::{Framebuffer, PHYS_MAP_BASE};
use crashqr::deflate::Deflate;
use crashqr::qr::{Ecc, QrCode};

/// Log bytes considered for the QR code (it holds about 2.8 KB of
/// compressed data, which is 8-20 KB of log).
const LOG: usize = 48 * 1024;
const HEADER: usize = 2048;
/// The largest base45 text a version 40-L code holds after the prefix.
const QR_TEXT: usize = 4296;
const QR_BYTES: usize = (QR_TEXT - 6) / 3 * 2;

struct Work {
    z: Deflate,
    qr: QrCode,
    log: [u8; LOG],
    details: [u8; 8192],
    report: [u8; HEADER + LOG],
    zbuf: [u8; QR_BYTES],
    text: [u8; QR_TEXT],
}

struct Cell(core::cell::UnsafeCell<Work>);
// SAFETY: used by the one CPU that won `ACTIVE`.
unsafe impl Sync for Cell {}

static WORK: Cell = Cell(core::cell::UnsafeCell::new(Work {
    z: Deflate::new(),
    qr: QrCode::new(),
    log: [0; LOG],
    details: [0; 8192],
    report: [0; HEADER + LOG],
    zbuf: [0; QR_BYTES],
    text: [0; QR_TEXT],
}));

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Start a crash report: the log position its details start at, or None
/// if a crash is already being reported (a fault in the crash screen
/// itself: the caller just halts).
pub fn begin() -> Option<usize> {
    (!ACTIVE.swap(true, Ordering::AcqRel)).then(crate::klog::position)
}

/// A `fmt::Write` into a byte slice (the rest is cut off).
struct Buf<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl Write for Buf<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &c in s.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        Ok(())
    }
}

/// Show the crash screen for `headline` ("kernel panic", "kernel trap
/// 0xe: page fault"); the details are what was logged since `mark`.
pub fn show(mark: usize, headline: &str) {
    let w = unsafe { &mut *WORK.0.get() };
    let dn = crate::klog::since(mark, &mut w.details);
    let ln = crate::klog::tail(&mut w.log);
    let ms = crate::rtc::mono_ms();

    // The report: header, then the end of the log (which ends with the
    // details).
    let mut h = Buf { b: &mut w.report[..HEADER], n: 0 };
    let _ = write!(h, "rs2 crash report\nversion {}\n{headline}\nuptime {}.{:03} s, {} cpu(s)\ncommand line: {}\n",
        env!("CARGO_PKG_VERSION"), ms / 1000, ms % 1000, crate::smp::online_count(), crate::boot_info().cmdline());
    if let Some(fb) = crate::screen::framebuffer_for_crash() {
        let _ = write!(h, "framebuffer {}x{}\n", fb.width, fb.height);
    }
    let _ = write!(h, "--- log ({} bytes since boot) ---\n", crate::klog::position());
    let hn = h.n;

    // As much of the log as fits the QR code, from a line start.
    let fit = |w: &mut Work, t: usize| -> Option<usize> {
        let mut s = ln - t;
        if s > 0 {
            while s < ln && w.log[s - 1] != b'\n' {
                s += 1;
            }
        }
        let t = ln - s;
        w.report[hn..hn + t].copy_from_slice(&w.log[s..ln]);
        crashqr::pack(&mut w.z, &w.report[..hn + t], &mut w.zbuf, &mut w.text)
    };
    let (mut lo, mut hi) = (0, ln);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fit(w, mid).is_some() { lo = mid } else { hi = mid - 1 }
    }
    let text_len = fit(w, lo);
    let qr_ok = text_len.and_then(|n| w.qr.encode(&w.text[..n], Ecc::L, 1, 40)).is_some();
    crate::log::print_unlocked(format_args!(
        "crash: QR code with {} of {} log bytes ({} characters)\n",
        lo,
        ln,
        text_len.unwrap_or(0)
    ));

    if let Some(fb) = crate::screen::framebuffer_for_crash() {
        // The small log on the screen ends where the details begin.
        draw(&fb, w, headline, &w.details[..dn], &w.log[..ln.saturating_sub(dn)], qr_ok, lo, ms);
    }
}

const BG: (u8, u8, u8) = (0x10, 0x18, 0x40);
const FG: (u8, u8, u8) = (0xf0, 0xf0, 0xf0);
const HEAD: (u8, u8, u8) = (0xff, 0xd8, 0x40);
const DIM: (u8, u8, u8) = (0xa0, 0xa8, 0xc0);

struct Screen<'a> {
    fb: &'a Framebuffer,
    px: *mut u32,
}

impl Screen<'_> {
    fn color(&self, (r, g, b): (u8, u8, u8)) -> u32 {
        if self.fb.format == 0 {
            r as u32 | (g as u32) << 8 | (b as u32) << 16
        } else {
            b as u32 | (g as u32) << 8 | (r as u32) << 16
        }
    }

    fn fill(&self, x: usize, y: usize, w: usize, h: usize, c: (u8, u8, u8)) {
        let c = self.color(c);
        let (fw, fh) = (self.fb.width as usize, self.fb.height as usize);
        for yy in y..(y + h).min(fh) {
            for xx in x..(x + w).min(fw) {
                unsafe { self.px.add(yy * self.fb.stride as usize + xx).write_volatile(c) };
            }
        }
    }

    /// One character cell of 8x16 times `s`.
    fn glyph(&self, x: usize, y: usize, ch: u8, s: usize, c: (u8, u8, u8)) {
        let g = crate::screen::glyph16(ch as char);
        for (row, bits) in g.iter().enumerate().take(16) {
            for col in 0..8 {
                if bits & (0x80 >> col) != 0 {
                    self.fill(x + col * s, y + row * s, s, s, c);
                }
            }
        }
    }

    /// A line cut at `cols` characters.
    fn text(&self, x: usize, y: usize, t: &[u8], s: usize, cols: usize, c: (u8, u8, u8)) {
        for (i, &ch) in t.iter().take(cols).enumerate() {
            if ch >= 0x20 {
                self.glyph(x + i * 8 * s, y, ch, s, c);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(fb: &Framebuffer, w: &Work, headline: &str, details: &[u8], log: &[u8], qr_ok: bool, qr_log: usize, ms: u64) {
    let scr = Screen { fb, px: (PHYS_MAP_BASE + fb.base) as *mut u32 };
    let (fw, fh) = (fb.width as usize, fb.height as usize);
    if fw < 320 || fh < 200 {
        return;
    }
    scr.fill(0, 0, fw, fh, BG);
    let s = (fh / 540).min(fw / 960).max(1);
    let m = 12 * s;
    let line = 16 * s;

    // The QR code: right side, as large as fits.
    let mut text_right = fw - m;
    if qr_ok {
        let q = &w.qr;
        let side = (fh - 2 * m - line).min(fw * 45 / 100);
        let module = (side / (q.size() + 8)).max(1);
        let total = module * (q.size() + 8);
        let (x0, y0) = (fw - m - total, m);
        scr.fill(x0, y0, total, total, (0xff, 0xff, 0xff));
        for y in 0..q.size() {
            for x in 0..q.size() {
                if q.dark(x, y) {
                    scr.fill(x0 + (x + 4) * module, y0 + (y + 4) * module, module, module, (0, 0, 0));
                }
            }
        }
        let mut cap = [0u8; 96];
        let mut b = Buf { b: &mut cap, n: 0 };
        let _ = write!(b, "QR version {}: {} KB of log", q.version(), qr_log.div_ceil(1024));
        let cols = total / (8 * s);
        let n = b.n;
        scr.text(x0, y0 + total + s * 4, &cap[..n], s, cols, DIM);
        text_right = x0 - m;
    }

    // The text: headline, summary, details, how to read the code, the log.
    let x = m;
    let cols = (text_right - x) / (8 * s);
    let mut y = m;
    let big = 2 * s;
    scr.text(x, y, b"rs2 has stopped", big, (text_right - x) / (8 * big), HEAD);
    y += 16 * big + s * 4;
    let mut sum = [0u8; 160];
    let mut b = Buf { b: &mut sum, n: 0 };
    let _ = write!(b, "{headline} - version {} - up {}.{:01} s", env!("CARGO_PKG_VERSION"), ms / 1000, ms % 1000 / 100);
    let n = b.n;
    scr.text(x, y, &sum[..n], s, cols, HEAD);
    y += line + line / 2;

    // The details, wrapped, in at most 60% of the screen: in the small font
    // when they do not fit in the large one.
    let limit = fh * 6 / 10;
    let wrapped = |cols: usize| -> usize {
        details.split(|&c| c == b'\n').filter(|l| !l.starts_with(b"crash: QR")).map(|l| l.len().div_ceil(cols).max(1)).sum()
    };
    let small = s.div_ceil(2).max(1);
    let ds = if y + wrapped(cols) * line <= limit { s } else { small };
    let (dcols, dline) = ((text_right - x) / (8 * ds), 16 * ds);
    for l in details.split(|&c| c == b'\n') {
        if l.starts_with(b"crash: QR") {
            continue;
        }
        let mut l = l;
        loop {
            if y + dline > limit {
                break;
            }
            let k = l.len().min(dcols);
            scr.text(x, y, &l[..k], ds, dcols, FG);
            y += dline;
            l = &l[k..];
            if l.is_empty() {
                break;
            }
        }
    }
    y += line / 2;
    let help: [&[u8]; 3] = if qr_ok {
        [
            b"Scan the QR code with a phone, then on the development machine:",
            b"  python3 -I tools/scripts/crashqr.py 'RS2C1:...'   (or a photo)",
            b"With 'bootlog' on the command line the log is in \\RS2\\BOOT.LOG.",
        ]
    } else {
        [b"(no QR code: the report did not fit)", b"", b"With 'bootlog' on the command line the log is in \\RS2\\BOOT.LOG."]
    };
    for h in help {
        scr.text(x, y, h, s, cols, DIM);
        y += line;
    }
    y += line / 2;

    // The end of the log in the small font, as much as is left.
    let sl = 16 * small;
    let scols = (text_right - x) / (8 * small);
    if y + sl >= fh {
        return;
    }
    let rows = (fh - m - y) / sl;
    let mut lines = [&[][..]; 128];
    let mut n = 0;
    for l in log.rsplit(|&c| c == b'\n') {
        if n == rows.min(lines.len()) {
            break;
        }
        if n == 0 && l.is_empty() {
            continue;
        }
        lines[n] = l;
        n += 1;
    }
    for l in lines[..n].iter().rev() {
        scr.text(x, y, l, small, scols, DIM);
        y += sl;
    }
}

/// `crashtest=panic|trap[,SECONDS]` on the command line: crash on purpose
/// (default after 5 s), to check this screen on a new machine.
pub fn start_test() {
    let Some(arg) = crate::boot_info().cmdline().split_whitespace().find_map(|w| w.strip_prefix("crashtest=")) else { return };
    let (kind, secs) = match arg.split_once(',') {
        Some((k, s)) => (k, s.parse().unwrap_or(5)),
        None => (arg, 5),
    };
    let trap = kind.eq_ignore_ascii_case("trap");
    crate::task::kthread::spawn("crashtest", move || {
        crate::task::sched::sleep_ms(secs * 1000);
        crate::println!("crashtest: crashing on purpose ({})", if trap { "page fault" } else { "panic" });
        if trap {
            unsafe { core::ptr::read_volatile(0x18usize as *const u64) };
        }
        panic!("crashtest: the crash screen test asked for on the command line");
    });
}
