//! RS2DISP.DRV from a made-up VGA.DRV: names, entry points, the stubs'
//! callbacks and RETF sizes, the resources copied.

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

/// A minimal NE display driver: a resource table with one cursor
/// resource (type 8001h, id 8001h) of 32 bytes, alignment 16.
fn fake_vga() -> Vec<u8> {
    let mut f = vec![0u8; 0x40];
    f[0..2].copy_from_slice(b"MZ");
    f[0x3c] = 0x40;
    let mut ne = vec![0u8; 0x40];
    ne[0..2].copy_from_slice(b"NE");
    // Resource table at NE+40h: shift, one type with one entry, end.
    let mut rt = Vec::new();
    rt.extend_from_slice(&4u16.to_le_bytes());
    rt.extend_from_slice(&0x8001u16.to_le_bytes());
    rt.extend_from_slice(&1u16.to_le_bytes());
    rt.extend_from_slice(&[0; 4]);
    let data_at = 0x100u16;
    rt.extend_from_slice(&(data_at >> 4).to_le_bytes());
    rt.extend_from_slice(&2u16.to_le_bytes()); // 32 bytes
    rt.extend_from_slice(&0x0c30u16.to_le_bytes());
    rt.extend_from_slice(&0x8001u16.to_le_bytes());
    rt.extend_from_slice(&[0; 4]);
    rt.extend_from_slice(&0u16.to_le_bytes());
    rt.push(0);
    let rsrc = 0x40u16;
    let resident = rsrc + rt.len() as u16;
    ne[0x24..0x26].copy_from_slice(&rsrc.to_le_bytes());
    ne[0x26..0x28].copy_from_slice(&resident.to_le_bytes());
    f.extend_from_slice(&ne);
    f.extend_from_slice(&rt);
    f.extend_from_slice(b"\x07DISPLAY\x00\x00\x00");
    f.resize(data_at as usize, 0);
    f.extend((0..32u8).map(|i| i ^ 0x5a));
    f
}

#[test]
fn builds_a_display_driver() {
    let d = win16drv::build(&fake_vga()).expect("build");
    assert_eq!(&d[0..2], b"MZ");
    let ne = u16_at(&d, 0x3c) as usize;
    assert_eq!(&d[ne..ne + 2], b"NE");
    assert_eq!(u16_at(&d, ne + 0x0c), 0x8300, "a library without automatic data");
    assert_eq!(u16_at(&d, ne + 0x3e), 0x030a);
    // The module name, then every entry by name.
    let names = ne + u16_at(&d, ne + 0x26) as usize;
    assert_eq!(&d[names + 1..names + 8], b"DISPLAY");
    let text = String::from_utf8_lossy(&d);
    for &(_, n, _) in win16drv::ENTRIES {
        assert!(text.contains(n), "{n} is exported");
    }
    // The code segment: each entry's stub is FE 38 (callback) CA (RETF n).
    let seg = ne + u16_at(&d, ne + 0x22) as usize;
    let shift = u16_at(&d, ne + 0x32);
    let code = (u16_at(&d, seg) as usize) << shift;
    for (k, &(ord, _, args)) in win16drv::ENTRIES.iter().enumerate() {
        let s = code + 8 + k * 8;
        assert_eq!(&d[s..s + 2], &[0xfe, 0x38]);
        assert_eq!(u16_at(&d, s + 2), win16drv::CB_DISP + ord);
        assert_eq!(d[s + 4], 0xca);
        assert_eq!(u16_at(&d, s + 5), args);
    }
    // The cursor resource came along.
    let rt = ne + u16_at(&d, ne + 0x24) as usize;
    let rshift = u16_at(&d, rt);
    assert_eq!(u16_at(&d, rt + 2), 0x8001);
    let off = (u16_at(&d, rt + 10) as usize) << rshift;
    assert_eq!(d[off..off + 32].to_vec(), (0..32u8).map(|i| i ^ 0x5a).collect::<Vec<_>>());
}

#[test]
fn not_a_driver() {
    assert!(win16drv::build(b"MZ but nothing else").is_err());
}
