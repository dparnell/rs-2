//! Windows ANSI (code page 1252) text to the IPF file's code page 437.

/// The code page 437 byte for 0x80-0xFF of code page 1252 (0: none of
/// its own, see [`spell`]).
const HIGH: [u8; 128] = [
    // 0x80: euro, -, sbquo, fnof, bdquo, hellip, dagger, Dagger, circ, permil, Scaron, lsaquo, OElig, -, Zcaron, -
    0, 0, b',', 0x9f, b'"', 0, 0, 0, b'^', 0, b'S', b'<', 0, 0, b'Z', 0,
    // 0x90: -, lsquo, rsquo, ldquo, rdquo, bull, ndash, mdash, tilde, trade, scaron, rsaquo, oelig, -, zcaron, Yuml
    0, b'\'', b'\'', b'"', b'"', 0x07, b'-', b'-', b'~', 0, b's', b'>', 0, 0, b'z', b'Y',
    // 0xA0: nbsp ! cent pound curren yen brvbar sect uml copy ordf laquo not shy reg macr
    0xff, 0xad, 0x9b, 0x9c, 0, 0x9d, b'|', 0x15, b'"', 0, 0xa6, 0xae, 0xaa, b'-', 0, b'-',
    // 0xB0: deg plusmn sup2 sup3 acute micro para middot cedil sup1 ordm raquo frac14 frac12 frac34 iquest
    0xf8, 0xf1, 0xfd, b'3', b'\'', 0xe6, 0x14, 0xfa, b',', b'1', 0xa7, 0xaf, 0xac, 0xab, 0, 0xa8,
    // 0xC0: A grave acute circ tilde uml ring AElig Ccedil E grave acute circ uml I grave acute circ uml
    b'A', b'A', b'A', b'A', 0x8e, 0x8f, 0x92, 0x80, b'E', 0x90, b'E', b'E', b'I', b'I', b'I', b'I',
    // 0xD0: ETH Ntilde O grave acute circ tilde uml times Oslash U grave acute circ uml Y acute THORN szlig
    b'D', 0xa5, b'O', b'O', b'O', b'O', 0x99, b'x', b'O', b'U', b'U', b'U', 0x9a, b'Y', 0, 0xe1,
    // 0xE0: a grave acute circ tilde uml ring aelig ccedil e grave acute circ uml i grave acute circ uml
    0x85, 0xa0, 0x83, b'a', 0x84, 0x86, 0x91, 0x87, 0x8a, 0x82, 0x88, 0x89, 0x8d, 0xa1, 0x8c, 0x8b,
    // 0xF0: eth ntilde o grave acute circ tilde uml divide oslash u grave acute circ uml y acute thorn yuml
    b'd', 0xa4, 0x95, 0xa2, 0x93, b'o', 0x94, 0xf6, b'o', 0x97, 0xa3, 0x96, 0x81, b'y', 0, 0x98,
];

/// Characters that become several: (1252 code, spelling).
fn spell(c: u8) -> Option<&'static [u8]> {
    Some(match c {
        0x80 => b"EUR",
        0x85 => b"...",
        0x86 => b"+",
        0x87 => b"++",
        0x89 => b"o/oo",
        0x8c => b"OE",
        0x99 => b"(TM)",
        0x9c => b"oe",
        0xa4 => b"*",
        0xa9 => b"(C)",
        0xae => b"(R)",
        0xbe => b"3/4",
        0xde => b"Th",
        0xfe => b"th",
        _ => return None,
    })
}

/// Append `c` (code page 1252) to `out` as code page 437.
pub fn push(out: &mut alloc::vec::Vec<u8>, c: u8) {
    if c < 0x80 {
        out.push(c);
    } else if let Some(s) = spell(c) {
        out.extend_from_slice(s);
    } else {
        match HIGH[(c - 0x80) as usize] {
            0 => out.push(b'?'),
            b => out.push(b),
        }
    }
}

/// A character of the Symbol font as code page 437 (Greek letters,
/// arrows, bullets), '?' when there is none.
pub fn symbol(c: u8) -> u8 {
    match c {
        b'a' => 0xe0,
        b'b' => 0xe1,
        b'G' => 0xe2,
        b'p' => 0xe3,
        b'S' => 0xe4,
        b's' => 0xe5,
        b'm' => 0xe6,
        b't' => 0xe7,
        b'F' => 0xe8,
        b'Q' => 0xe9,
        b'W' => 0xea,
        b'd' => 0xeb,
        b'f' => 0xed,
        b'e' => 0xee,
        0xa3 => 0xf3,
        0xb3 => 0xf2,
        0xb1 => 0xf1,
        0xb8 => 0xf6,
        0xb0 => 0xf8,
        0xd6 => 0xfb,
        0xa5 => 0xec,
        0xb7 | 0xb2 => 0x07,
        0xac => 0x1b,
        0xad => 0x18,
        0xae => 0x1a,
        0xaf => 0x19,
        0xab => 0x1d,
        0xb4 => b'x',
        0xb9 => 0xf0,
        0x20..=0x40 => c,
        b'[' | b']' | b'_' | b'{' | b'}' | b'|' => c,
        _ => b'?',
    }
}

/// A character of the Wingdings font: its bullets and arrows.
pub fn wingdings(c: u8) -> u8 {
    match c {
        0x6c | 0x6e | 0x77 | 0x9f | 0xa1 | 0xa7 | 0x75 => 0x07,
        0xd8 | 0xe0 | 0xe8 | 0xf0 => 0x1a,
        0xd7 | 0xdf | 0xe7 | 0xef => 0x1b,
        0xd9 | 0xe1 | 0xe9 | 0xf1 => 0x18,
        0xda | 0xe2 | 0xea | 0xf2 => 0x19,
        0xfc => 0xfb,
        0x20 => b' ',
        _ => b'?',
    }
}
