extern crate std;

use super::*;
use alloc::vec;
use alloc::vec::Vec;

/// A small format 0 song: tempo 1 s per quarter, two notes.
fn song_bytes() -> Vec<u8> {
    let track: Vec<u8> = vec![
        0x00, 0xff, 0x51, 0x03, 0x0f, 0x42, 0x40, // tempo 1,000,000 us
        0x00, 0xc0, 0x00, // program 0
        0x00, 0x90, 60, 100, // note on
        0x60, 0x80, 60, 0, // after 96 ticks (one quarter) note off
        0x00, 0x90, 64, 100, // running status test below
        0x30, 64, 0, // running status note-on velocity 0 = off, 48 ticks
        0x00, 0xff, 0x2f, 0x00,
    ];
    let mut v = b"MThd".to_vec();
    v.extend_from_slice(&6u32.to_be_bytes());
    v.extend_from_slice(&[0, 0, 0, 1, 0, 96]);
    v.extend_from_slice(b"MTrk");
    v.extend_from_slice(&(track.len() as u32).to_be_bytes());
    v.extend_from_slice(&track);
    v
}

#[test]
fn parses_and_times() {
    let s = parse(&song_bytes()).unwrap();
    assert_eq!(s.division, 96);
    assert_eq!(s.events.len(), 5);
    assert_eq!(s.events[3].tick, 96);
    assert_eq!(s.events[3].us, 1_000_000);
    assert_eq!(s.events[4].msg.data2, 0);
    assert_eq!(s.length_ticks, 144);
    assert_eq!(s.length_us, 1_500_000);
    assert_eq!(s.us_to_tick(500_000), 48);
    assert_eq!(s.event_at(1_000_000), 2);
    assert_eq!(parse(b"not midi at all").err(), Some(Error::NotMidi));
}

#[test]
fn renders_sound_then_silence() {
    let s = parse(&song_bytes()).unwrap();
    let mut p = Player::new(s, 22050);
    let mut buf = vec![0i16; 22050 * 2 / 2];
    p.render(&mut buf);
    let peak = buf.iter().map(|v| v.unsigned_abs()).max().unwrap();
    assert!(peak > 1000, "peak {peak}");
    // Long after the last note-off everything has died away.
    for _ in 0..8 {
        p.render(&mut buf);
    }
    assert!(p.finished());
    assert!(buf.iter().all(|&v| v == 0));
}

#[test]
fn seek_lands_on_time() {
    let s = parse(&song_bytes()).unwrap();
    let mut p = Player::new(s, 44100);
    p.seek(1_200_000);
    assert_eq!(p.position_us(), 1_200_000);
    let mut buf = vec![0i16; 4410 * 2];
    p.render(&mut buf);
    assert_eq!(p.position_us(), 1_300_000);
}

#[test]
fn freqs() {
    assert!((synth::note_freq(69, 0.0) - 440.0).abs() < 0.1);
    assert!((synth::note_freq(81, 0.0) - 880.0).abs() < 0.2);
    assert!((synth::note_freq(69, 2.0) - 493.88).abs() < 0.3);
}

/// OS/2's MMPM/2 songs (work/ is not in git: skipped without it).
#[test]
fn mmpm_songs() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/unpacked/disk_21/MMTEMP");
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut n = 0;
    for e in rd.flatten() {
        let path = e.path();
        if !path.to_string_lossy().to_ascii_uppercase().ends_with(".MID") {
            continue;
        }
        let d = std::fs::read(&path).unwrap();
        let s = parse(&d).unwrap_or_else(|e| panic!("{path:?}: {e:?}"));
        assert!(s.events.len() > 50, "{path:?}");
        assert!(s.length_us > 10_000_000 && s.length_us < 600_000_000, "{path:?} {}", s.length_us);
        // Render 3 s from the middle: sound, no overflow trouble.
        let mut p = Player::new(s, 22050);
        let mid = p.length_us() / 2;
        p.seek(mid);
        let mut buf = vec![0i16; 22050 * 2 * 3];
        p.render(&mut buf);
        let peak = buf.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak > 500, "{path:?} silent");
        n += 1;
    }
    assert!(n >= 20);
}
