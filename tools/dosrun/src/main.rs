//! Run a DOS program on rs2's DOS machine with the host's files:
//!
//!     dosrun [options] PROGRAM [ARGS...]
//!
//! Drive C: is the current directory (or `--c DIR`); `--drive D=DIR`
//! adds drives. `--keys TEXT` types keys when the program waits for
//! input (`\n` Enter, `{esc}`, `{f1}`..`{f10}`, `{up}` `{down}` `{left}`
//! `{right}`, `{wait}` half a second (`{wait N}` N seconds), `{alt-x}`,
//! `{ctrl-c}`; the mouse at a pixel of the screen: `{mouse X Y}` moves,
//! `{click X Y}`, `{rclick X Y}`, `{dclick X Y}`, `{press X Y}` and
//! `{release X Y}` with the left button; `{shot FILE}` saves the screen
//! as a PNG then). At the end
//! the text screen is printed (or written to `--screen FILE`); `--png
//! FILE` saves the screen of a graphics mode. `--timeout SECS` ends a
//! program that runs too long, `--trace` logs every BIOS/DOS call,
//! `--display WxH` sets the size of RS2DISP.DRV's screen (800x600).

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dos::video::{Kind, Vga};
use dos::{Config, Create, DateTime, DirEntry, Event, Host, Machine, OpenMode};

struct Run {
    drives: Vec<Option<PathBuf>>,
    files: Vec<Option<File>>,
    start: Instant,
    keys: VecDeque<KeyItem>,
    next_key_at: f64,
    idle_since_key: bool,
    screen: Vec<String>,
    last_gfx: Option<(usize, usize, Vec<u8>, [u32; 256])>,
    trace: bool,
    cp437: Vec<char>,
    /// Sound Blaster output collected for --wav (rate, samples).
    pcm: (u32, Vec<u8>),
    /// --display: RS2DISP.DRV's screen size.
    display: (u32, u32),
}

enum KeyItem {
    Scan(Vec<(u8, u8)>),
    Wait(f64),
    /// The mouse at a pixel, with these buttons.
    Mouse(i32, i32, u8),
    Shot(String),
}

fn now_secs(s: &Instant) -> f64 {
    s.elapsed().as_secs_f64()
}

impl Run {
    /// The host path for a DOS path, matching names case-insensitively.
    fn path(&self, dos: &str) -> Result<PathBuf, u16> {
        let b = dos.as_bytes();
        let d = (b[0] - b'A') as usize;
        let Some(Some(root)) = self.drives.get(d) else { return Err(15) };
        let mut p = root.clone();
        let comps: Vec<&str> = dos[3..].split('\\').filter(|c| !c.is_empty()).collect();
        for (i, c) in comps.iter().enumerate() {
            let exact = p.join(c);
            if exact.exists() {
                p = exact;
                continue;
            }
            let found = fs::read_dir(&p).ok().and_then(|rd| rd.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(c)));
            match found {
                Some(e) => p = e.path(),
                None => {
                    p = exact;
                    if i + 1 < comps.len() {
                        return Err(3);
                    }
                }
            }
        }
        Ok(p)
    }

    fn handle(&mut self, h: u32) -> Result<&mut File, u16> {
        self.files.get_mut(h as usize).and_then(|f| f.as_mut()).ok_or(6)
    }

    fn add_file(&mut self, f: File) -> u32 {
        if let Some(i) = self.files.iter().position(|f| f.is_none()) {
            self.files[i] = Some(f);
            return i as u32;
        }
        self.files.push(Some(f));
        (self.files.len() - 1) as u32
    }
}

fn io_err(e: std::io::Error) -> u16 {
    match e.kind() {
        std::io::ErrorKind::NotFound => 2,
        std::io::ErrorKind::PermissionDenied => 5,
        std::io::ErrorKind::AlreadyExists => 80,
        _ => 5,
    }
}

fn dos_time(t: SystemTime) -> (u16, u16) {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let dt = civil(secs);
    dos::pack_dt(&dt)
}

/// UTC date and time for seconds since 1970.
fn civil(secs: u64) -> DateTime {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    DateTime {
        year: y as u16,
        month: m as u8,
        day: d as u8,
        hour: (rem / 3600) as u8,
        minute: (rem / 60 % 60) as u8,
        second: (rem % 60) as u8,
        hundredths: 0,
        weekday: ((days + 4).rem_euclid(7)) as u8,
    }
}

impl Host for Run {
    fn open(&mut self, path: &str, mode: OpenMode) -> Result<u32, u16> {
        if self.trace {
            eprintln!("open {path}");
        }
        let p = self.path(path)?;
        if p.is_dir() {
            return Err(5);
        }
        let f = match mode.access() {
            0 => File::open(&p),
            1 => OpenOptions::new().write(true).open(&p),
            _ => OpenOptions::new().read(true).write(true).open(&p).or_else(|_| File::open(&p)),
        }
        .map_err(io_err)?;
        Ok(self.add_file(f))
    }
    fn create(&mut self, path: &str, _attr: u8, how: Create) -> Result<u32, u16> {
        let p = self.path(path)?;
        let mut o = OpenOptions::new();
        o.read(true).write(true);
        match how {
            Create::Truncate => o.create(true).truncate(true),
            Create::New => o.create_new(true),
        };
        let f = o.open(&p).map_err(io_err)?;
        Ok(self.add_file(f))
    }
    fn close(&mut self, h: u32) {
        if let Some(f) = self.files.get_mut(h as usize) {
            *f = None;
        }
    }
    fn read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u16> {
        let f = self.handle(h)?;
        let mut n = 0;
        while n < buf.len() {
            match f.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(io_err(e)),
            }
        }
        Ok(n)
    }
    fn write(&mut self, h: u32, buf: &[u8]) -> Result<usize, u16> {
        self.handle(h)?.write_all(buf).map_err(io_err)?;
        Ok(buf.len())
    }
    fn seek(&mut self, h: u32, off: i64, whence: u8) -> Result<u64, u16> {
        let f = self.handle(h)?;
        let to = match whence {
            0 => SeekFrom::Start(off.max(0) as u64),
            1 => SeekFrom::Current(off),
            _ => SeekFrom::End(off),
        };
        f.seek(to).map_err(|_| 25)
    }
    fn set_size(&mut self, h: u32, size: u64) -> Result<(), u16> {
        self.handle(h)?.set_len(size).map_err(io_err)
    }
    fn file_time(&mut self, h: u32) -> Result<(u16, u16), u16> {
        let m = self.handle(h)?.metadata().map_err(io_err)?;
        Ok(dos_time(m.modified().unwrap_or(UNIX_EPOCH)))
    }
    fn set_file_time(&mut self, _h: u32, _date: u16, _time: u16) -> Result<(), u16> {
        Ok(())
    }
    fn delete(&mut self, path: &str) -> Result<(), u16> {
        let p = self.path(path)?;
        if p.is_dir() {
            return Err(5);
        }
        fs::remove_file(p).map_err(io_err)
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), u16> {
        let a = self.path(from)?;
        let b = self.path(to)?;
        if b.exists() {
            return Err(5);
        }
        fs::rename(a, b).map_err(io_err)
    }
    fn mkdir(&mut self, path: &str) -> Result<(), u16> {
        fs::create_dir(self.path(path)?).map_err(io_err)
    }
    fn rmdir(&mut self, path: &str) -> Result<(), u16> {
        fs::remove_dir(self.path(path)?).map_err(|_| 5)
    }
    fn get_attr(&mut self, path: &str) -> Result<u8, u16> {
        let p = self.path(path)?;
        if path.len() == 3 {
            return Ok(0x10);
        }
        let m = fs::metadata(&p).map_err(|_| 2u16)?;
        Ok(if m.is_dir() { 0x10 } else if m.permissions().readonly() { 0x21 } else { 0x20 })
    }
    fn set_attr(&mut self, path: &str, _attr: u8) -> Result<(), u16> {
        self.path(path)?.exists().then_some(()).ok_or(2)
    }
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, u16> {
        let p = self.path(path)?;
        let rd = fs::read_dir(&p).map_err(|_| 3u16)?;
        let mut v = Vec::new();
        if path.len() > 3 {
            for n in [".", ".."] {
                v.push(DirEntry { name: n.into(), attr: 0x10, size: 0, date: 0, time: 0 });
            }
        }
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(m) = e.metadata() else { continue };
            let (date, time) = dos_time(m.modified().unwrap_or(UNIX_EPOCH));
            v.push(DirEntry { name, attr: if m.is_dir() { 0x10 } else { 0x20 }, size: m.len().min(u32::MAX as u64) as u32, date, time });
        }
        Ok(v)
    }
    fn drive_exists(&mut self, d: u8) -> bool {
        self.drives.get(d as usize).is_some_and(|x| x.is_some())
    }
    fn disk_space(&mut self, d: u8) -> Result<(u16, u16, u32, u32), u16> {
        if !self.drive_exists(d) {
            return Err(15);
        }
        Ok((8, 512, 30000, 60000))
    }
    fn now(&mut self) -> DateTime {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut dt = civil(secs);
        dt.hundredths = (SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0) / 10) as u8;
        dt
    }
    fn micros(&mut self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
    fn poll(&mut self, out: &mut Vec<Event>) {
        let t = now_secs(&self.start);
        if t < self.next_key_at || !self.idle_since_key && t < self.next_key_at + 0.2 {
            return;
        }
        match self.keys.pop_front() {
            Some(KeyItem::Scan(codes)) => {
                for (scan, ch) in codes {
                    out.push(Event::Key { scan, ch });
                }
                self.next_key_at = t + 0.03;
                self.idle_since_key = false;
            }
            Some(KeyItem::Wait(s)) => self.next_key_at = t + s,
            Some(KeyItem::Mouse(x, y, buttons)) => {
                let (w, h) = self.last_gfx.as_ref().map_or((640, 480), |g| (g.0 as i64, g.1 as i64));
                let ax = ((x as i64 * 65536 + 32768) / w).clamp(0, 65535) as u32;
                let ay = ((y as i64 * 65536 + 32768) / h).clamp(0, 65535) as u32;
                out.push(Event::MouseAbs { x: ax, y: ay, buttons });
                self.next_key_at = t + 0.05;
                self.idle_since_key = false;
            }
            Some(KeyItem::Shot(p)) => {
                if let Some((w, h, px, pal)) = &self.last_gfx {
                    write_png(Path::new(&p), *w, *h, px, pal);
                }
            }
            None => {}
        }
    }
    fn wait(&mut self, us: u64) {
        self.idle_since_key = true;
        std::thread::sleep(std::time::Duration::from_micros(us.min(5000)));
    }
    fn present(&mut self, v: &Vga, ram: &[u8]) {
        match v.kind() {
            Kind::Text { cols, rows, base } => {
                let start = (v.text_start() * 2) as usize;
                self.screen.clear();
                for r in 0..rows as usize {
                    let mut line = String::new();
                    for c in 0..cols as usize {
                        let a = base as usize + start + (r * cols as usize + c) * 2;
                        let ch = ram.get(a).copied().unwrap_or(b' ');
                        line.push(self.cp437[ch as usize]);
                    }
                    self.screen.push(line.trim_end().to_string());
                }
                self.last_gfx = None;
            }
            Kind::Graphics { .. } => {
                let mut px = Vec::new();
                let (w, h) = v.render(ram, &mut px);
                self.last_gfx = Some((w, h, px, v.palette()));
            }
        }
    }
    fn display_size(&mut self) -> (u32, u32) {
        self.display
    }
    fn present_display(&mut self, d: &dos::disp::Display, _damage: &[dos::disp::Rect]) {
        // The picture with the pointer drawn in.
        let mut px = d.pix.clone();
        if let Some(p) = &d.pointer {
            let (x0, y0) = (d.pointer_pos.0 - p.hot.0, d.pointer_pos.1 - p.hot.1);
            for r in 0..p.h {
                for c in 0..p.w {
                    let (x, y) = (x0 + c as i32, y0 + r as i32);
                    if x < 0 || y < 0 || x >= d.w as i32 || y >= d.h as i32 {
                        continue;
                    }
                    let bit = |m: &[u8]| m[r * p.width_bytes + c / 8] >> (7 - c % 8) & 1 != 0;
                    let o = y as usize * d.w + x as usize;
                    let v = if bit(&p.and) { px[o] } else { 0 };
                    px[o] = if bit(&p.xor) { v ^ 0xff } else { v };
                }
            }
        }
        self.last_gfx = Some((d.w, d.h, px, d.pal));
    }
    fn pcm(&mut self, rate: u32, data: &[u8]) {
        if self.pcm.0 == 0 {
            self.pcm.0 = rate;
        }
        self.pcm.1.extend_from_slice(data);
    }
    fn log(&mut self, msg: &str) {
        if self.trace || !msg.starts_with("cb ") {
            eprintln!("{msg}");
        }
    }
    fn font(&mut self, height: u8) -> Option<Vec<u8>> {
        let data: &[u8] = match height {
            8 => include_bytes!("../../../kernel/assets/unscii-8.bin"),
            14 => include_bytes!("../../../kernel/assets/unscii-14.bin"),
            _ => include_bytes!("../../../kernel/assets/unscii-16.bin"),
        };
        let h = height as usize;
        let rec = 4 + h;
        let mut out = vec![0u8; 256 * h];
        for b in 0..256 {
            let c = self.cp437[b] as u32;
            let n = data.len() / rec;
            if let Some(i) = (0..n).find(|&i| u32::from_le_bytes(data[i * rec..i * rec + 4].try_into().unwrap()) == c) {
                out[b * h..b * h + h].copy_from_slice(&data[i * rec + 4..i * rec + 4 + h]);
            }
        }
        Some(out)
    }
}

/// Code page 437 as Unicode, from the kernel's NLS tables.
fn cp437() -> Vec<char> {
    let src = include_str!("../../../kernel/src/nls_data.rs");
    let at = src.find("(437, [").unwrap();
    let body = &src[at..];
    let end = body.find("]),").unwrap();
    let mut v: Vec<char> = body[7..end]
        .split(',')
        .filter_map(|t| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok())
        .map(|u| char::from_u32(u).unwrap_or('?'))
        .collect();
    v.truncate(256);
    v[0] = ' ';
    v
}

/// US layout: the scan code of an ASCII character and whether Shift is
/// needed.
fn scan_of(c: char) -> Option<(u8, bool)> {
    let rows: [(&str, &str, u8); 4] = [("1234567890-=", "!@#$%^&*()_+", 0x02), ("qwertyuiop[]", "QWERTYUIOP{}", 0x10), ("asdfghjkl;'`", "ASDFGHJKL:\"~", 0x1e), ("zxcvbnm,./", "ZXCVBNM<>?", 0x2c)];
    for (n, s, base) in rows {
        if let Some(i) = n.find(c) {
            return Some((base + i as u8, false));
        }
        if let Some(i) = s.find(c) {
            return Some((base + i as u8, true));
        }
    }
    match c {
        '\\' => Some((0x2b, false)),
        '|' => Some((0x2b, true)),
        ' ' => Some((0x39, false)),
        '\n' => Some((0x1c, false)),
        '\t' => Some((0x0f, false)),
        '\x08' => Some((0x0e, false)),
        _ => None,
    }
}

fn parse_keys(s: &str) -> VecDeque<KeyItem> {
    let mut q = VecDeque::new();
    let press = |sc: u8, ch: u8| vec![(sc, ch), (sc | 0x80, 0)];
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'n') {
            chars.next();
            q.push_back(KeyItem::Scan(press(0x1c, b'\r')));
            continue;
        }
        if c == '{' {
            let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            let ext = |sc: u8| vec![(0xe0, 0), (sc, 0), (0xe0, 0), (sc | 0x80, 0)];
            let words: Vec<&str> = name.split_whitespace().collect();
            let xy = |i: usize| words.get(i).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            match words.first().copied() {
                Some("mouse") => {
                    q.push_back(KeyItem::Mouse(xy(1), xy(2), 0));
                    continue;
                }
                Some(c @ ("click" | "rclick" | "dclick" | "press" | "release")) => {
                    let b = if c == "rclick" { 2 } else { 1 };
                    let (x, y) = (xy(1), xy(2));
                    q.push_back(KeyItem::Mouse(x, y, 0));
                    for _ in 0..if c == "dclick" { 2 } else { 1 } {
                        if c != "release" {
                            q.push_back(KeyItem::Mouse(x, y, b));
                        }
                        if c != "press" {
                            q.push_back(KeyItem::Mouse(x, y, 0));
                        }
                    }
                    continue;
                }
                Some("shot") => {
                    q.push_back(KeyItem::Shot(words.get(1).unwrap_or(&"shot.png").to_string()));
                    continue;
                }
                Some("wait") if words.len() > 1 => {
                    q.push_back(KeyItem::Wait(words[1].parse().unwrap_or(0.5)));
                    continue;
                }
                _ => {}
            }
            let item = match name.as_str() {
                "wait" => KeyItem::Wait(0.5),
                "enter" => KeyItem::Scan(press(0x1c, b'\r')),
                "esc" => KeyItem::Scan(press(0x01, 0x1b)),
                "up" => KeyItem::Scan(ext(0x48)),
                "down" => KeyItem::Scan(ext(0x50)),
                "left" => KeyItem::Scan(ext(0x4b)),
                "right" => KeyItem::Scan(ext(0x4d)),
                "ctrl-c" => KeyItem::Scan(vec![(0x1d, 0), (0x2e, 0), (0xae, 0), (0x9d, 0)]),
                n if n.starts_with('f') => {
                    let k: u8 = n[1..].parse().unwrap_or(1);
                    let sc = if k <= 10 { 0x3a + k } else { 0x57 + k - 11 };
                    KeyItem::Scan(press(sc, 0))
                }
                n if n.starts_with("alt-") => {
                    let (sc, _) = scan_of(n.chars().nth(4).unwrap_or('x')).unwrap_or((0x2d, false));
                    KeyItem::Scan(vec![(0x38, 0), (sc, 0), (sc | 0x80, 0), (0xb8, 0)])
                }
                _ => continue,
            };
            q.push_back(item);
            continue;
        }
        if let Some((sc, shift)) = scan_of(c) {
            let ch = if c == '\n' { b'\r' } else { c as u8 };
            let mut v = Vec::new();
            if shift {
                v.push((0x2a, 0));
            }
            v.extend(press(sc, ch));
            if shift {
                v.push((0xaa, 0));
            }
            q.push_back(KeyItem::Scan(v));
        }
    }
    q
}

/// Write an RGB PNG (stored deflate blocks).
fn write_png(path: &Path, w: usize, h: usize, px: &[u8], pal: &[u32; 256]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = pal[px[y * w + x] as usize];
            raw.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |t: &[u8], d: &[u8]| {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc(&td).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    fs::write(path, out).unwrap();
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut drives: Vec<Option<PathBuf>> = vec![None; 26];
    drives[2] = Some(std::env::current_dir().unwrap());
    let mut keys = String::new();
    let mut timeout = 30.0;
    let mut screen_out = None;
    let mut png_out = None;
    let mut cfg = Config::default();
    let mut cwd = String::new();
    let mut watch = u32::MAX;
    let mut stats = false;
    let mut wav_out: Option<String> = None;
    let mut display = (800, 600);
    while let Some(a) = args.first().cloned() {
        if !a.starts_with("--") {
            break;
        }
        args.remove(0);
        let mut val = || args.remove(0);
        match a.as_str() {
            "--c" => drives[2] = Some(PathBuf::from(val())),
            "--drive" => {
                let v = val();
                let (d, p) = v.split_once('=').expect("--drive D=DIR");
                drives[(d.as_bytes()[0].to_ascii_uppercase() - b'A') as usize] = Some(PathBuf::from(p));
            }
            "--keys" => keys = val(),
            "--timeout" => timeout = val().parse().unwrap(),
            "--screen" => screen_out = Some(val()),
            "--png" => png_out = Some(val()),
            "--trace" => cfg.trace = true,
            "--cwd" => cwd = val(),
            "--nofpu" => cfg.fpu = false,
            "--noems" => cfg.ems_kb = 0,
            "--stats" => stats = true,
            "--watch" => watch = u32::from_str_radix(val().trim_start_matches("0x"), 16).unwrap(),
            "--wav" => wav_out = Some(val()),
            "--display" => {
                let v = val();
                let (w, h) = v.split_once('x').expect("--display WxH");
                display = (w.parse().unwrap(), h.parse().unwrap());
            }
            _ => panic!("unknown option {a}"),
        }
    }
    let Some(prog) = args.first().cloned() else {
        eprintln!("usage: dosrun [--c DIR] [--drive D=DIR] [--keys TEXT] [--timeout S] [--screen F] [--png F] [--trace] PROGRAM [ARGS]");
        std::process::exit(2);
    };
    let tail = args[1..].join(" ");
    // Drive Z: holds rs2's COMMAND.COM.
    let z = std::env::temp_dir().join(format!("dosrun-z-{}", std::process::id()));
    fs::create_dir_all(&z).unwrap();
    fs::write(z.join("COMMAND.COM"), dos::shell::command_com()).unwrap();
    drives[25] = Some(z.clone());
    cfg.env = vec!["COMSPEC=Z:\\COMMAND.COM".into(), "PATH=Z:\\;C:\\".into(), "PROMPT=$P$G".into()];
    let trace = cfg.trace;
    let mut host = Run {
        drives,
        files: Vec::new(),
        start: Instant::now(),
        keys: parse_keys(&keys),
        next_key_at: 0.3,
        idle_since_key: false,
        screen: Vec::new(),
        last_gfx: None,
        trace,
        cp437: cp437(),
        pcm: (0, Vec::new()),
        display,
    };
    let mut m = Machine::new(&mut host, cfg);
    m.dos.cwd[2] = cwd.to_ascii_uppercase();
    m.hw.watch = watch;
    let path = if prog.contains(':') {
        prog.clone()
    } else if prog.eq_ignore_ascii_case("COMMAND.COM") && !host.drives[2].as_ref().unwrap().join(&prog).exists() {
        String::from("Z:\\COMMAND.COM")
    } else {
        format!("C:{}", prog)
    };
    if let Err(e) = m.start(&mut host, &path, &tail) {
        eprintln!("dosrun: cannot start {prog}: DOS error {e}");
        std::process::exit(2);
    }
    let t0 = Instant::now();
    let code = loop {
        if let Some(c) = m.exited {
            break c as i32;
        }
        m.step(&mut host);
        if t0.elapsed().as_secs_f64() > timeout {
            let (cs, ip) = m.where_now();
            eprintln!("dosrun: timeout at {cs:04x}:{ip:04x}");
            break 124;
        }
    };
    m.present(&mut host);
    if stats {
        let secs = t0.elapsed().as_secs_f64();
        eprintln!("dosrun: {} instructions in {:.2} s: {:.1} MIPS", m.cpu.icount, secs, m.cpu.icount as f64 / secs / 1e6);
    }
    let _ = fs::remove_dir_all(&z);
    if let Some(w) = &wav_out {
        // 8-bit mono WAV.
        let (rate, data) = (host.pcm.0.max(1), &host.pcm.1);
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&8u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(data);
        fs::write(w, f).unwrap();
        eprintln!("dosrun: {} samples at {} Hz to {w}", data.len(), rate);
    }
    if let (Some(p), Some((w, h, px, pal))) = (&png_out, &host.last_gfx) {
        write_png(Path::new(p), *w, *h, px, pal);
    }
    let mut text = host.screen.join("\n");
    while text.ends_with('\n') {
        text.pop();
    }
    match screen_out {
        Some(f) => fs::write(f, text + "\n").unwrap(),
        None => println!("{}", text.trim_end()),
    }
    std::process::exit(code);
}
