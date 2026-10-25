//! Win-OS/2 pieces of the DOS machine that need no Windows: RS2DISP's
//! raster operations and palette, VWIN's shield messages.

use dos::disp::{cref_of, nearest, rop2, rop3, PALETTE};
use dos::vwin::{Request, Vwin};

#[test]
fn rop3_is_the_truth_table() {
    for rop in 0..=255u8 {
        for (p, s, d) in [(0xf0u8, 0xccu8, 0xaau8), (0x3c, 0x5a, 0x0f)] {
            let mut want = 0u8;
            for bit in 0..8 {
                let k = (p >> bit & 1) << 2 | (s >> bit & 1) << 1 | (d >> bit & 1);
                want |= (rop >> k & 1) << bit;
            }
            assert_eq!(rop3(rop, p, s, d), want, "ROP {rop:#04x}");
        }
    }
}

#[test]
fn rop2_codes() {
    let (p, d) = (0b1100u8, 0b1010u8);
    assert_eq!(rop2(13, p, d), p); // R2_COPYPEN
    assert_eq!(rop2(7, p, d), p ^ d); // R2_XORPEN
    assert_eq!(rop2(6, p, d), !d); // R2_NOT
    assert_eq!(rop2(9, p, d), p & d); // R2_MASKPEN
    assert_eq!(rop2(15, p, d), p | d); // R2_MERGEPEN
    assert_eq!(rop2(1, p, d), 0); // R2_BLACK
    assert_eq!(rop2(16, p, d), 0xff); // R2_WHITE
    assert_eq!(rop2(3, p, d), !p & d); // R2_MASKNOTPEN
}

#[test]
fn palette_inverts_by_index() {
    // Black and white at the ends, the cube and the greys in inverse
    // pairs: inverting an index inverts the colour.
    assert_eq!(PALETTE[0], 0);
    assert_eq!(PALETTE[255], 0xffffff);
    for i in 10..118 {
        assert_eq!(PALETTE[i] ^ PALETTE[255 - i], 0xffffff, "entry {i}");
    }
    // Every entry is its own nearest colour.
    for i in 0..256 {
        let c = cref_of(i as u8);
        assert_eq!(PALETTE[nearest(c) as usize], PALETTE[i], "entry {i}");
    }
}

fn msg(code: u16, hwnd: u16, rect: (i16, i16, i16, i16), flags: u32) -> Vec<u8> {
    let mut m = vec![0u8; 0x2c];
    let (top, left, h, w) = rect;
    for (o, v) in [(8, top), (0x0a, left), (0x0c, h), (0x0e, w)] {
        m[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    m[0x18..0x1c].copy_from_slice(&flags.to_le_bytes());
    m[0x20..0x22].copy_from_slice(&hwnd.to_le_bytes());
    m[0x24..0x26].copy_from_slice(&code.to_le_bytes());
    m
}

#[test]
fn shield_messages_make_the_window_list() {
    let mut v = Vwin::default();
    // Created (a visible overlapped window with a title at +2Ch).
    let mut c = msg(1, 0x1318, (10, 20, 300, 200), 0x10cf_0000);
    c[0x12] = 0x2c;
    c.extend_from_slice(b"Clock\0");
    v.learn(&c);
    v.learn(&msg(1, 0x1400, (0, 0, 100, 100), 0x10cf_0000));
    assert_eq!(v.windows.len(), 2);
    let w = &v.windows[0];
    assert_eq!((w.hwnd, w.x, w.y, w.w, w.h, w.visible), (0x1318, 20, 10, 200, 300, true));
    assert_eq!(w.title, b"Clock");
    // Moved, then to the top, then activated.
    v.learn(&msg(4, 0x1318, (50, 60, 300, 200), 0x20));
    assert_eq!((v.windows[0].x, v.windows[0].y), (60, 50));
    v.learn(&msg(4, 0x1318, (0, 0, 0, 0), 0x01));
    assert_eq!(v.windows.last().unwrap().hwnd, 0x1318);
    v.learn(&msg(3, 0x1318, (1, 0, 0, 0), 0));
    assert_eq!(v.active, 0x1318);
    // Hidden, minimised, destroyed.
    v.learn(&msg(4, 0x1400, (0, 0, 0, 0), 0x10));
    assert!(!v.windows[0].visible);
    v.learn(&msg(4, 0x1318, (0, 0, 32, 32), 0x20 | 0x1000));
    assert!(v.windows.iter().find(|w| w.hwnd == 0x1318).unwrap().minimized);
    v.learn(&msg(2, 0x1318, (0, 0, 0, 0), 0));
    assert_eq!(v.windows.len(), 1);
    assert_eq!(v.active, 0);
}

#[test]
fn requests_become_messages() {
    let mut v = Vwin::default();
    v.id = 7;
    v.request(Request::Place { hwnd: 0x1318, x: 5, y: 6, w: 300, h: 200 });
    let m = v.next_message().unwrap();
    let u16_at = |o: usize| u16::from_le_bytes([m[o], m[o + 1]]);
    assert_eq!((u16_at(0x24), u16_at(0x20), u16_at(4)), (4, 0x1318, 7));
    assert_eq!((u16_at(8), u16_at(0x0a), u16_at(0x0c), u16_at(0x0e), u16_at(0x18)), (6, 5, 200, 300, 0x20));
    let mut cmd = [0u8; 128];
    cmd[..11].copy_from_slice(b"NOTEPAD.EXE");
    v.request(Request::Run(cmd));
    let m = v.next_message().unwrap();
    assert_eq!(u16::from_le_bytes([m[0x24], m[0x25]]), 0x16);
    let at = u16::from_le_bytes([m[8], m[9]]) as usize;
    assert_eq!(&m[at..at + 12], b"NOTEPAD.EXE\0");
    // Every message is numbered.
    assert!(u32::from_le_bytes([m[0], m[1], m[2], m[3]]) > 0);
    assert!(v.call_due);
}
