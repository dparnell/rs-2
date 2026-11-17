//! EXEPACK round trips through the loader's own expanders.
use omflink::pack;
use os2_exe::lx::{expand_exepack2, expand_iterated};

fn samples() -> Vec<Vec<u8>> {
    let mut v = vec![vec![0u8; 4096], (0..4096).map(|i| (i * 7 % 251) as u8).collect()];
    let mut text = Vec::new();
    while text.len() < 4096 {
        text.extend_from_slice(b"The quick brown fox jumps over the lazy dog. ");
        text.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3]);
    }
    text.truncate(4096);
    v.push(text);
    // Pseudo-random bytes with some structure (like code).
    let mut x = 12345u32;
    let mut r = Vec::new();
    for i in 0..4000 {
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        r.push(if i % 13 < 4 { 0x90 } else { (x >> 16) as u8 });
    }
    v.push(r);
    v.push(vec![1, 2, 3]);
    v.push(Vec::new());
    v
}

#[test]
fn exepack2_round_trip() {
    for s in samples() {
        let p = pack::exepack2(&s);
        let mut out = vec![0u8; 4096];
        let n = expand_exepack2(&p, &mut out).unwrap();
        assert_eq!(n, s.len());
        assert_eq!(&out[..s.len()], &s[..]);
    }
    // Zeros compress to a few bytes.
    assert!(pack::exepack2(&vec![0u8; 4096]).len() < 64);
}

#[test]
fn iterated_round_trip() {
    for s in samples() {
        let p = pack::iterate(&s);
        let mut out = vec![0u8; 4096];
        expand_iterated(&p, &mut out).unwrap();
        assert_eq!(&out[..s.len()], &s[..]);
    }
}
