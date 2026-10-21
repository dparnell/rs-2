//! Tests on OS/2's own files (under work/, not in git): skipped when they
//! are missing.

use std::path::{Path, PathBuf};

use mmfmt::{flc, wav};

fn work(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work").join(rel)
}

fn flc_file(rel: &str, frames: u16) {
    let path = work(rel);
    let Ok(data) = std::fs::read(&path) else {
        eprintln!("skipped: {} missing", path.display());
        return;
    };
    let mut d = flc::Decoder::new(data).unwrap();
    assert_eq!((d.width(), d.height(), d.frame_count()), (320, 200, frames));
    let mut first = Vec::new();
    let mut busy = 0;
    for i in 0..frames {
        let f = d.next_frame().unwrap_or_else(|e| panic!("{rel} frame {i}: {e:?}"));
        assert_eq!((f.index, f.looped), (i, false));
        assert!(f.delay_ms > 0);
        if i == 0 {
            assert!(f.palette_changed);
            first = d.frame().to_vec();
        }
        // Pictures (not blank frames) with more than one colour.
        busy += d.frame().iter().any(|&p| p != d.frame()[0]) as u32;
        assert!(d.palette().iter().any(|c| *c != d.palette()[0]));
    }
    assert!(busy > frames as u32 / 2, "{rel}: {busy} pictures");
    // The ring frame brings back the first frame.
    let f = d.next_frame().unwrap();
    assert_eq!((f.index, f.looped), (0, true));
    assert!(d.frame() == &first[..], "{rel}: ring frame does not give frame 0");
    assert_eq!(d.next_frame().unwrap().index, 1);
}

#[test]
fn klondike_flc() {
    flc_file("unpacked/disk_12/OS2/APPS/KLONDIKE.FLC", 186);
}

#[test]
fn chess_flc() {
    flc_file("unpacked/disk_12/OS2/APPS/OS2CHESS.FLC", 182);
}

#[test]
fn mahjongg_flc() {
    flc_file("unpacked/disk_16/OS2/APPS/MAHJONGG.FLC", 160);
}

#[test]
fn system_wavs() {
    let Ok(dir) = std::fs::read_dir(work("unpacked/disk_21/MMTEMP")) else {
        eprintln!("skipped: MMTEMP missing");
        return;
    };
    let mut n = 0;
    for e in dir.flatten() {
        let path = e.path();
        if path.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("wav")) {
            continue;
        }
        let file = std::fs::read(&path).unwrap();
        let w = wav::parse(&file).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        assert!(w.format.is_plain_pcm(), "{}: {:?}", path.display(), w.format);
        assert_eq!(w.data_offset, 44);
        assert_eq!(w.data(&file).len(), w.data_len);
        let pcm = wav::decode_pcm16(&w.format, w.data(&file)).unwrap();
        assert_eq!(pcm.len(), w.data_len / (w.format.bits as usize / 8));
        assert!(w.format.duration_ms(w.data_len as u32) > 50);
        n += 1;
    }
    assert!(n >= 30, "only {n} WAVs");
}
