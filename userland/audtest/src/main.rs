//! Self-checks of the RS2API audio calls (ordinals 300-308): the kernel
//! mixer and the sound driver. Each check prints PASS or FAIL; the last
//! line is "audtest done: N failure(s)" and the exit code N. Without
//! sound hardware only the ERROR_NOT_SUPPORTED answers are checked.
//!
//! It plays, in this order, with short silences between them (what
//! tools/scripts/audcheck.py expects to find in the recording of
//! `cargo xtask run --sound-wav FILE`):
//!
//! | Tone | Stream format | Length |
//! |------|---------------|--------|
//! | 440 Hz | 48000 Hz, 16-bit stereo | 1.0 s |
//! | 880 Hz | 22050 Hz, 8-bit mono | 1.0 s |
//! | 660 Hz | 11025 Hz, 16-bit mono | 0.5 s |
//! | 1000 Hz | 44100 Hz, 16-bit stereo | 0.5 s |
//! | 330 Hz | 8000 Hz, 8-bit stereo | 0.5 s |
//! | 300 Hz + 1500 Hz | 44100 Hz 16-bit stereo and 16000 Hz 16-bit mono at once | 1.0 s |
//! | 500 Hz | 48000 Hz, 16-bit mono, stopped after 0.3 s | 2.0 s written |
//! | 750 Hz | 32000 Hz, 16-bit mono, paused 0.4 s halfway | 0.6 s |
//! | 600 Hz | 24000 Hz, 16-bit mono, stopped after a 0.1 s drain timeout | 1.0 s written |
//!
//! `AUDTEST RECORD [HZ]` checks the recording calls (309-312) instead,
//! with the host feeding the input a sine of HZ (default 440): `cargo
//! xtask run --headless --timeout 60 --init "RS2\AUDTEST.EXE RECORD"
//! --sound-in tone:440` (with `--sound ac97` or `es1370` for the other
//! cards). Each format records a second, which must take about a second (QEMU hands over the first part at once)
//! and hold the tone; then the level, pause, stop, volume, source and
//! error answers.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "RS2API.300"]
    fn Rs2AudioQuery(devices: *mut u32) -> u32;
    #[link_name = "RS2API.301"]
    fn Rs2AudioOpen(rate: u32, channels: u32, bits: u32, h: *mut u32) -> u32;
    #[link_name = "RS2API.302"]
    fn Rs2AudioWrite(h: u32, data: *const u8, cb: u32) -> u32;
    #[link_name = "RS2API.303"]
    fn Rs2AudioQueryPos(h: u32, bytes: *mut u32) -> u32;
    #[link_name = "RS2API.304"]
    fn Rs2AudioDrain(h: u32, ms: u32) -> u32;
    #[link_name = "RS2API.305"]
    fn Rs2AudioStop(h: u32) -> u32;
    #[link_name = "RS2API.306"]
    fn Rs2AudioClose(h: u32) -> u32;
    #[link_name = "RS2API.307"]
    fn Rs2AudioSetVolume(h: u32, volume: u32) -> u32;
    #[link_name = "RS2API.308"]
    fn Rs2AudioPause(h: u32, pause: u32) -> u32;
    #[link_name = "RS2API.309"]
    fn Rs2AudioOpenInput(rate: u32, channels: u32, bits: u32, h: *mut u32) -> u32;
    #[link_name = "RS2API.310"]
    fn Rs2AudioRead(h: u32, buf: *mut u8, cb: u32, ms: u32, read: *mut u32) -> u32;
    #[link_name = "RS2API.311"]
    fn Rs2AudioQueryLevel(h: u32, level: *mut u32) -> u32;
    #[link_name = "RS2API.312"]
    fn Rs2AudioSetInput(source: u32, taken: *mut u32) -> u32;
}

const BUSY: u32 = 170;

const NOT_SUPPORTED: u32 = 50;
const INVALID_HANDLE: u32 = 6;
const INVALID_PARAMETER: u32 = 87;
const TIMEOUT: u32 = 640;
const QSV_MS_COUNT: u32 = 14;

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn ms() -> u32 {
    let mut v = 0u32;
    unsafe { DosQuerySysInfo(QSV_MS_COUNT, QSV_MS_COUNT, &mut v, 4) };
    v
}

/// sin(2 pi p) for p in [0, 1).
fn sin_turns(p: f64) -> f64 {
    const PI: f64 = core::f64::consts::PI;
    // sin(2 pi p) = -sin(x) with x = 2 pi p - pi in [-pi, pi).
    let x = 2.0 * PI * p - PI;
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for n in 1..12 {
        term *= -x2 / ((2 * n) as f64 * (2 * n + 1) as f64);
        sum += term;
    }
    -sum
}

/// `secs` of a `freq` Hz sine at a third of full scale, as PCM.
fn tone(freq: f64, rate: u32, channels: u32, bits: u32, secs: f64) -> Vec<u8> {
    let n = (rate as f64 * secs) as usize;
    let mut out = Vec::with_capacity(n * (channels * bits / 8) as usize);
    let mut p = 0.0f64;
    let dp = freq / rate as f64;
    for _ in 0..n {
        let v = sin_turns(p) / 3.0;
        p += dp;
        if p >= 1.0 {
            p -= 1.0;
        }
        for _ in 0..channels {
            if bits == 8 {
                out.push((128.0 + v * 127.0) as i32 as u8);
            } else {
                out.extend_from_slice(&((v * 32767.0) as i32 as i16).to_le_bytes());
            }
        }
    }
    out
}

fn open(rate: u32, channels: u32, bits: u32) -> Option<u32> {
    let mut h = 0;
    let rc = unsafe { Rs2AudioOpen(rate, channels, bits, &mut h) };
    if rc != 0 {
        println!("  Rs2AudioOpen({rate}, {channels}, {bits}) = {rc}");
        return None;
    }
    Some(h)
}

fn write(h: u32, data: &[u8]) -> u32 {
    unsafe { Rs2AudioWrite(h, data.as_ptr(), data.len() as u32) }
}

fn pos(h: u32) -> u32 {
    let mut p = 0;
    unsafe { Rs2AudioQueryPos(h, &mut p) };
    p
}

/// Play one tone to the end: write, drain, check the position.
fn play(name: &str, freq: f64, rate: u32, channels: u32, bits: u32, secs: f64) {
    let Some(h) = open(rate, channels, bits) else {
        check(name, false);
        return;
    };
    let data = tone(freq, rate, channels, bits, secs);
    let t0 = ms();
    let w = write(h, &data);
    let d = unsafe { Rs2AudioDrain(h, 5000) };
    let took = ms() - t0;
    let p = pos(h);
    let want = (secs * 1000.0) as u32;
    // Write blocks only beyond half a second queued, so the whole tone
    // takes about its own length, plus the mixer's 0.1 s lead, less what
    // an emulator's sound backend takes in at once when the stream starts
    // (QEMU's CoreAudio output: 0.3 s).
    let timed = took * 2 >= want && took <= want + 400;
    if !timed || p != data.len() as u32 {
        println!("  write {w}, drain {d}, {took} ms for {want} ms, position {p} of {}", data.len());
    }
    check(name, w == 0 && d == 0 && p == data.len() as u32 && timed);
    check("Rs2AudioClose", unsafe { Rs2AudioClose(h) } == 0);
    unsafe { DosSleep(250) };
}

/// Thread arguments: stream handle and the tone to write.
static BG_HANDLE: AtomicU32 = AtomicU32::new(0);
static BG_RC: AtomicU32 = AtomicU32::new(u32::MAX);
static BG_DONE_MS: AtomicU32 = AtomicU32::new(0);
static mut BG_DATA: Vec<u8> = Vec::new();

unsafe extern "C" fn writer(_: u32) {
    let h = BG_HANDLE.load(Ordering::Acquire);
    let data = unsafe { &*core::ptr::addr_of!(BG_DATA) };
    let rc = write(h, data);
    BG_DONE_MS.store(ms(), Ordering::Release);
    BG_RC.store(rc, Ordering::Release);
}

/// Start a thread writing `data` to `h`; its id.
fn spawn_writer(h: u32, data: Vec<u8>) -> u32 {
    unsafe { *core::ptr::addr_of_mut!(BG_DATA) = data };
    BG_HANDLE.store(h, Ordering::Release);
    BG_RC.store(u32::MAX, Ordering::Release);
    let mut tid = 0;
    unsafe { DosCreateThread(&mut tid, writer, 0, 0, 65536) };
    tid
}

fn join(mut tid: u32) {
    unsafe { DosWaitThread(&mut tid, DCWW_WAIT) };
}

fn errors() {
    let mut h = 0;
    check("Rs2AudioOpen bad rate", unsafe { Rs2AudioOpen(100, 2, 16, &mut h) } == INVALID_PARAMETER);
    check("Rs2AudioOpen bad channels", unsafe { Rs2AudioOpen(44100, 3, 16, &mut h) } == INVALID_PARAMETER);
    check("Rs2AudioOpen bad bits", unsafe { Rs2AudioOpen(44100, 2, 24, &mut h) } == INVALID_PARAMETER);
    check("Rs2AudioWrite bad handle", write(12345, &[0; 4]) == INVALID_HANDLE);
    check("Rs2AudioDrain bad handle", unsafe { Rs2AudioDrain(12345, 0) } == INVALID_HANDLE);
    check("Rs2AudioClose bad handle", unsafe { Rs2AudioClose(12345) } == INVALID_HANDLE);
    check("Rs2AudioSetVolume range", unsafe { Rs2AudioSetVolume(0, 101) } == INVALID_PARAMETER);
    check("Rs2AudioSetVolume master", unsafe { Rs2AudioSetVolume(0, 100) } == 0);
    if let Some(h) = open(22050, 1, 8) {
        check("Rs2AudioSetVolume stream", unsafe { Rs2AudioSetVolume(h, 50) } == 0);
        check("Rs2AudioDrain empty", unsafe { Rs2AudioDrain(h, 1000) } == 0);
        check("Rs2AudioQueryPos empty", pos(h) == 0);
        check("Rs2AudioClose", unsafe { Rs2AudioClose(h) } == 0);
        check("closed handle", unsafe { Rs2AudioPause(h, 1) } == INVALID_HANDLE);
    }
}

/// Two streams at once, the second written by another thread.
fn two_streams() {
    let (Some(a), Some(b)) = (open(44100, 2, 16), open(16000, 1, 16)) else {
        check("two streams", false);
        return;
    };
    let da = tone(300.0, 44100, 2, 16, 1.0);
    let db = tone(1500.0, 16000, 1, 16, 1.0);
    let (la, lb) = (da.len() as u32, db.len() as u32);
    let tid = spawn_writer(b, db);
    let wa = write(a, &da);
    let ra = unsafe { Rs2AudioDrain(a, 5000) };
    join(tid);
    let rb = unsafe { Rs2AudioDrain(b, 5000) };
    check("two streams", wa == 0 && ra == 0 && rb == 0 && BG_RC.load(Ordering::Acquire) == 0);
    check("two streams positions", pos(a) == la && pos(b) == lb);
    unsafe {
        Rs2AudioClose(a);
        Rs2AudioClose(b);
        DosSleep(250);
    }
}

/// Stop while another thread is blocked writing.
fn stop() {
    let Some(h) = open(48000, 1, 16) else {
        check("Rs2AudioStop", false);
        return;
    };
    let data = tone(500.0, 48000, 1, 16, 2.0);
    let len = data.len() as u32;
    let tid = spawn_writer(h, data);
    unsafe { DosSleep(300) };
    let t0 = ms();
    let rc = unsafe { Rs2AudioStop(h) };
    join(tid);
    let wrote = BG_RC.load(Ordering::Acquire);
    let unblocked = BG_DONE_MS.load(Ordering::Acquire).wrapping_sub(t0);
    let d = unsafe { Rs2AudioDrain(h, 1000) };
    let p = pos(h);
    println!("  stopped at {p} of {len} bytes; writer returned {wrote} {unblocked} ms after the stop");
    check("Rs2AudioStop", rc == 0 && wrote == 0 && unblocked < 200);
    check("Rs2AudioStop drain", d == 0 && p > 0 && p < len / 2);
    unsafe { Rs2AudioClose(h) };
    unsafe { DosSleep(250) };
}

fn pause() {
    let Some(h) = open(32000, 1, 16) else {
        check("Rs2AudioPause", false);
        return;
    };
    let data = tone(750.0, 32000, 1, 16, 0.6);
    let len = data.len() as u32;
    let w = write(h, &data);
    unsafe { DosSleep(250) };
    let rc = unsafe { Rs2AudioPause(h, 1) };
    // What was mixed ahead still plays out.
    unsafe { DosSleep(150) };
    let p1 = pos(h);
    unsafe { DosSleep(250) };
    let p2 = pos(h);
    let rc2 = unsafe { Rs2AudioPause(h, 0) };
    let d = unsafe { Rs2AudioDrain(h, 3000) };
    let p3 = pos(h);
    println!("  paused at {p1}, {p2}; at the end {p3} of {len}");
    check("Rs2AudioPause", w == 0 && rc == 0 && rc2 == 0 && p1 == p2 && p1 > 0 && p1 < len);
    check("Rs2AudioPause resume", d == 0 && p3 == len);
    unsafe { Rs2AudioClose(h) };
    unsafe { DosSleep(250) };
}

fn drain_timeout() {
    let Some(h) = open(24000, 1, 16) else {
        check("Rs2AudioDrain timeout", false);
        return;
    };
    let data = tone(600.0, 24000, 1, 16, 1.0);
    let t0 = ms();
    let w = write(h, &data);
    let d = unsafe { Rs2AudioDrain(h, 100) };
    let took = ms() - t0;
    unsafe { Rs2AudioStop(h) };
    check("Rs2AudioDrain timeout", w == 0 && d == TIMEOUT && took < 1000);
    // Closing with data queued.
    let w2 = write(h, &data[..4800]);
    check("Rs2AudioClose busy", w2 == 0 && unsafe { Rs2AudioClose(h) } == 0);
    unsafe { DosSleep(250) };
}

/// Read `want` bytes in pieces; the bytes and the milliseconds it took.
fn record(h: u32, want: usize) -> (Vec<u8>, u32) {
    let mut out = alloc::vec![0u8; want];
    let mut got = 0;
    let t0 = ms();
    while got < want {
        let mut n = 0;
        let piece = (want - got).min(4096);
        let rc = unsafe { Rs2AudioRead(h, out[got..].as_mut_ptr(), piece as u32, 2000, &mut n) };
        if rc != 0 || n == 0 {
            println!("  Rs2AudioRead = {rc}, {n} bytes, after {got} of {want}");
            break;
        }
        got += n as usize;
    }
    out.truncate(got);
    (out, ms().wrapping_sub(t0))
}

/// Channel 0's samples as 16-bit values.
fn samples(data: &[u8], channels: u32, bits: u32) -> Vec<i32> {
    let fb = (channels * bits / 8) as usize;
    data.chunks_exact(fb).map(|f| if bits == 8 { (f[0] as i32 - 128) << 8 } else { i16::from_le_bytes([f[0], f[1]]) as i32 }).collect()
}

/// Frequency (rising zero crossings) and peak of samples.
fn analyse(s: &[i32], rate: u32) -> (f64, i32) {
    let crossings = s.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
    let peak = s.iter().map(|v| v.abs()).max().unwrap_or(0);
    (crossings as f64 * rate as f64 / s.len().max(1) as f64, peak)
}

fn open_input(rate: u32, channels: u32, bits: u32) -> Option<u32> {
    let mut h = 0;
    let rc = unsafe { Rs2AudioOpenInput(rate, channels, bits, &mut h) };
    if rc != 0 {
        println!("  Rs2AudioOpenInput({rate}, {channels}, {bits}) = {rc}");
        return None;
    }
    Some(h)
}

fn record_format(hz: f64, rate: u32, channels: u32, bits: u32) {
    let name = alloc::format!("record {rate}/{bits}/{}", if channels == 2 { "stereo" } else { "mono" });
    let Some(h) = open_input(rate, channels, bits) else {
        check(&name, false);
        return;
    };
    let fb = (channels * bits / 8) as usize;
    let want = rate as usize * fb;
    let (data, took) = record(h, want);
    let mut level = 0;
    let lrc = unsafe { Rs2AudioQueryLevel(h, &mut level) };
    let mut pos = 0;
    unsafe { Rs2AudioQueryPos(h, &mut pos) };
    let s = samples(&data, channels, bits);
    // The second half: the input may start with a little silence.
    let (f, peak) = analyse(&s[s.len() / 2..], rate);
    let stereo_ok = channels == 1 || data.chunks_exact(fb).skip(data.len() / fb / 2).all(|c| c[..fb / 2] == c[fb / 2..]);
    println!("  {} bytes in {took} ms, {f:.1} Hz, peak {peak}, level {level:#x}, position {pos}", data.len());
    check(&name, data.len() == want && (600..=1600).contains(&took) && (f - hz).abs() < hz * 0.03 && peak > 4000 && stereo_ok);
    check(&alloc::format!("{name} level and position"), lrc == 0 && level & 0xffff > 2000 && pos as usize >= want);
    check("Rs2AudioClose input", unsafe { Rs2AudioClose(h) } == 0);
}

static RD_RC: AtomicU32 = AtomicU32::new(u32::MAX);
static RD_N: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn reader(h: u32) {
    let mut buf = alloc::vec![0u8; 65536];
    let mut n = 0;
    let rc = unsafe { Rs2AudioRead(h, buf.as_mut_ptr(), buf.len() as u32, u32::MAX, &mut n) };
    RD_N.store(n, Ordering::Release);
    RD_RC.store(rc, Ordering::Release);
}

fn record_tests(hz: f64) {
    let mut h = 0;
    check("Rs2AudioOpenInput bad rate", unsafe { Rs2AudioOpenInput(100, 1, 16, &mut h) } == INVALID_PARAMETER);
    check("Rs2AudioOpenInput bad bits", unsafe { Rs2AudioOpenInput(22050, 1, 12, &mut h) } == INVALID_PARAMETER);
    let mut n = 0;
    check("Rs2AudioRead bad handle", unsafe { Rs2AudioRead(12345, core::ptr::null_mut(), 0, 0, &mut n) } == INVALID_HANDLE);
    check("Rs2AudioQueryLevel bad handle", unsafe { Rs2AudioQueryLevel(12345, &mut n) } == INVALID_HANDLE);
    check("Rs2AudioSetInput bad source", unsafe { Rs2AudioSetInput(9, &mut n) } == INVALID_PARAMETER);
    let mut taken = 0;
    let rc = unsafe { Rs2AudioSetInput(2, &mut taken) };
    println!("  microphone: source {taken}");
    check("Rs2AudioSetInput microphone", rc == 0 && (taken == 1 || taken == 2));
    let rc = unsafe { Rs2AudioSetInput(1, &mut taken) };
    check("Rs2AudioSetInput line in", rc == 0 && taken == 1);
    for (rate, ch, bits) in [(48000, 2, 16), (22050, 1, 16), (11025, 1, 8), (44100, 2, 16), (8000, 2, 8)] {
        record_format(hz, rate, ch, bits);
    }
    let Some(h) = open_input(22050, 1, 16) else {
        check("record checks", false);
        return;
    };
    check("Rs2AudioOpenInput busy", unsafe { Rs2AudioOpenInput(22050, 1, 16, &mut n) } == BUSY);
    // Nothing waits with a timeout of 0.
    let mut buf = alloc::vec![0u8; 22050 * 2];
    let t0 = ms();
    let rc = unsafe { Rs2AudioRead(h, buf.as_mut_ptr(), buf.len() as u32, 0, &mut n) };
    check("Rs2AudioRead no wait", rc == 0 && ms().wrapping_sub(t0) < 50 && (n as usize) < buf.len());
    // Pause: what comes meanwhile is dropped.
    unsafe { DosSleep(200) };
    let (mut p1, mut p2) = (0, 0);
    unsafe {
        Rs2AudioPause(h, 1);
        Rs2AudioQueryPos(h, &mut p1);
        DosSleep(300);
        Rs2AudioQueryPos(h, &mut p2);
        Rs2AudioPause(h, 0);
    }
    println!("  paused at {p1}, {p2} after 0.3 s");
    check("Rs2AudioPause input", p2 - p1 < 2000);
    // Volume 0: silence.
    unsafe {
        Rs2AudioStop(h);
        Rs2AudioSetVolume(h, 0);
        DosSleep(100);
        Rs2AudioStop(h);
    }
    let (data, _) = record(h, 11025 * 2);
    let (_, peak) = analyse(&samples(&data, 1, 16), 22050);
    check("Rs2AudioSetVolume input 0", data.len() == 22050 && peak == 0);
    unsafe { Rs2AudioSetVolume(h, 50) };
    unsafe { Rs2AudioStop(h) };
    let (data, _) = record(h, 22050);
    let (_, peak) = analyse(&samples(&data, 1, 16)[2000..], 22050);
    println!("  volume 50: peak {peak}");
    check("Rs2AudioSetVolume input 50", (4000..7000).contains(&peak));
    // Stop wakes a reader waiting for more than will come for a while.
    RD_RC.store(u32::MAX, Ordering::Release);
    let mut tid = 0;
    unsafe { DosCreateThread(&mut tid, reader, h, 0, 65536) };
    unsafe { DosSleep(300) };
    let t0 = ms();
    check("Rs2AudioStop input", unsafe { Rs2AudioStop(h) } == 0);
    join(tid);
    let took = ms().wrapping_sub(t0);
    let (rc, got) = (RD_RC.load(Ordering::Acquire), RD_N.load(Ordering::Acquire));
    println!("  reader returned {rc} with {got} bytes {took} ms after the stop");
    check("Rs2AudioStop wakes the reader", rc == 0 && took < 200 && got < 65536);
    check("Rs2AudioClose input", unsafe { Rs2AudioClose(h) } == 0);
    check("closed input", unsafe { Rs2AudioQueryLevel(h, &mut n) } == INVALID_HANDLE);
}

fn main(s: &Startup) -> u32 {
    let args = core::str::from_utf8(s.args()).unwrap_or("").trim();
    let mut words = args.split_whitespace();
    if words.next().is_some_and(|w| w.eq_ignore_ascii_case("record")) {
        let hz = words.next().and_then(|w| w.parse::<f64>().ok()).unwrap_or(440.0);
        let mut taken = 0;
        if unsafe { Rs2AudioSetInput(0, &mut taken) } == NOT_SUPPORTED {
            println!("no sound input");
            let mut h = 0;
            check("Rs2AudioOpenInput without hardware", unsafe { Rs2AudioOpenInput(22050, 1, 16, &mut h) } == NOT_SUPPORTED);
        } else {
            record_tests(hz);
        }
        let f = FAILS.load(Ordering::Relaxed);
        println!("audtest done: {f} failure(s)");
        return f;
    }
    let mut n = 0u32;
    let rc = unsafe { Rs2AudioQuery(&mut n) };
    if rc == NOT_SUPPORTED {
        println!("no sound hardware");
        let mut h = 0;
        check("Rs2AudioOpen without hardware", unsafe { Rs2AudioOpen(44100, 2, 16, &mut h) } == NOT_SUPPORTED);
    } else {
        check("Rs2AudioQuery", rc == 0 && n == 1);
        errors();
        play("440 Hz 48000/16/stereo", 440.0, 48000, 2, 16, 1.0);
        play("880 Hz 22050/8/mono", 880.0, 22050, 1, 8, 1.0);
        play("660 Hz 11025/16/mono", 660.0, 11025, 1, 16, 0.5);
        play("1000 Hz 44100/16/stereo", 1000.0, 44100, 2, 16, 0.5);
        play("330 Hz 8000/8/stereo", 330.0, 8000, 2, 8, 0.5);
        two_streams();
        stop();
        pause();
        drain_timeout();
    }
    let f = FAILS.load(Ordering::Relaxed);
    println!("audtest done: {f} failure(s)");
    f
}
