//! A tiny AML builder: enough to write test definition blocks in Rust
//! (the unit tests, and the battery SSDT given to QEMU with
//! `-acpitable`; see `examples/battery_ssdt.rs`). Each function returns
//! the encoded bytes of one term.

use alloc::vec;
use alloc::vec::Vec;

/// Prefix `body` with its PkgLength.
pub fn pkglen(body: Vec<u8>) -> Vec<u8> {
    let n = body.len();
    let mut v = if n + 1 < 64 {
        vec![(n + 1) as u8]
    } else if n + 2 < 1 << 12 {
        let l = n + 2;
        vec![0x40 | (l & 0xf) as u8, (l >> 4) as u8]
    } else {
        let l = n + 3;
        vec![0x80 | (l & 0xf) as u8, (l >> 4) as u8, (l >> 12) as u8]
    };
    v.extend(body);
    v
}

fn seg(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().take(4).collect();
    while v.len() < 4 {
        v.push(b'_');
    }
    v
}

/// A NameString: `\_SB.BAT0`, `^PCI0.LPCB`, `BIFP`.
pub fn name(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    let mut rest = s;
    while let Some(r) = rest.strip_prefix('\\').or_else(|| rest.strip_prefix('^')) {
        v.push(rest.as_bytes()[0]);
        rest = r;
    }
    let segs: Vec<&str> = rest.split('.').filter(|x| !x.is_empty()).collect();
    match segs.len() {
        0 => v.push(0),
        1 => {}
        2 => v.push(0x2e),
        n => v.extend([0x2f, n as u8]),
    }
    for s in segs {
        v.extend(seg(s));
    }
    v
}

pub fn int(x: u64) -> Vec<u8> {
    match x {
        0 => vec![0],
        1 => vec![1],
        2..=0xff => vec![0x0a, x as u8],
        0x100..=0xffff => {
            let mut v = vec![0x0b];
            v.extend((x as u16).to_le_bytes());
            v
        }
        0x1_0000..=0xffff_ffff => {
            let mut v = vec![0x0c];
            v.extend((x as u32).to_le_bytes());
            v
        }
        _ => {
            let mut v = vec![0x0e];
            v.extend(x.to_le_bytes());
            v
        }
    }
}

pub fn string(s: &str) -> Vec<u8> {
    let mut v = vec![0x0d];
    v.extend(s.bytes());
    v.push(0);
    v
}

/// `EisaId ("PNP0C0A")`.
pub fn eisa(id: &str) -> Vec<u8> {
    let b = id.as_bytes();
    let c = |i: usize| (b[i] - 0x40) as u32 & 0x1f;
    let h = |i: usize| (b[i] as char).to_digit(16).unwrap_or(0);
    let v = (c(0) << 10 | c(1) << 5 | c(2)) as u16;
    let mut out = [0u8; 4];
    out[0] = (v >> 8) as u8;
    out[1] = v as u8;
    out[2] = (h(3) << 4 | h(4)) as u8;
    out[3] = (h(5) << 4 | h(6)) as u8;
    int(u32::from_le_bytes(out) as u64)
}

pub fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

pub fn def_name(n: &str, value: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x08], name(n), value])
}

pub fn method(n: &str, args: u8, body: &[Vec<u8>]) -> Vec<u8> {
    let mut b = name(n);
    b.push(args & 7);
    b.extend(body.concat());
    cat(&[vec![0x14], pkglen(b)])
}

pub fn scope(n: &str, body: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0x10], pkglen(cat(&[name(n), body.concat()]))])
}

pub fn device(n: &str, body: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0x5b, 0x82], pkglen(cat(&[name(n), body.concat()]))])
}

pub fn package(items: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![items.len() as u8];
    b.extend(items.concat());
    cat(&[vec![0x12], pkglen(b)])
}

pub fn buffer(bytes: &[u8]) -> Vec<u8> {
    cat(&[vec![0x11], pkglen(cat(&[int(bytes.len() as u64), bytes.to_vec()]))])
}

pub fn ret(x: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0xa4], x])
}

pub fn store(src: Vec<u8>, dst: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x70], src, dst])
}

pub fn local(n: u8) -> Vec<u8> {
    vec![0x60 + n]
}

pub fn arg(n: u8) -> Vec<u8> {
    vec![0x68 + n]
}

/// No target.
pub fn none() -> Vec<u8> {
    vec![0]
}

/// A two-operand operator with a target (Add 0x72, Subtract 0x74,
/// Multiply 0x77, ShiftLeft 0x79, ShiftRight 0x7A, And 0x7B, Or 0x7D,
/// Xor 0x7F, Mod 0x85).
pub fn op2(op: u8, a: Vec<u8>, b: Vec<u8>, target: Vec<u8>) -> Vec<u8> {
    cat(&[vec![op], a, b, target])
}

/// A comparison without a target (LEqual 0x93, LGreater 0x94, LLess
/// 0x95, LAnd 0x90, LOr 0x91).
pub fn cmp(op: u8, a: Vec<u8>, b: Vec<u8>) -> Vec<u8> {
    cat(&[vec![op], a, b])
}

pub fn lnot(a: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x92], a])
}

pub fn divide(a: Vec<u8>, b: Vec<u8>, rem: Vec<u8>, quot: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x78], a, b, rem, quot])
}

pub fn increment(t: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x75], t])
}

pub fn index(src: Vec<u8>, i: Vec<u8>, target: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x88], src, i, target])
}

pub fn deref(x: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x83], x])
}

pub fn sizeof(x: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x87], x])
}

pub fn if_(cond: Vec<u8>, body: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0xa0], pkglen(cat(&[cond, body.concat()]))])
}

pub fn else_(body: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0xa1], pkglen(body.concat())])
}

pub fn while_(cond: Vec<u8>, body: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0xa2], pkglen(cat(&[cond, body.concat()]))])
}

pub fn brk() -> Vec<u8> {
    vec![0xa5]
}

pub fn notify(obj: &str, v: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x86], name(obj), v])
}

/// A call of method `n`.
pub fn call(n: &str, args: &[Vec<u8>]) -> Vec<u8> {
    cat(&[name(n), args.concat()])
}

pub fn mutex(n: &str) -> Vec<u8> {
    cat(&[vec![0x5b, 0x01], name(n), vec![0]])
}

pub fn acquire(n: &str) -> Vec<u8> {
    cat(&[vec![0x5b, 0x23], name(n), vec![0xff, 0xff]])
}

pub fn release(n: &str) -> Vec<u8> {
    cat(&[vec![0x5b, 0x27], name(n)])
}

pub fn op_region(n: &str, space: u8, offset: Vec<u8>, len: Vec<u8>) -> Vec<u8> {
    cat(&[vec![0x5b, 0x80], name(n), vec![space], offset, len])
}

/// One entry of a field list: a named unit of `bits`, or reserved bits
/// (name "").
pub fn unit(n: &str, bits: usize) -> Vec<u8> {
    let mut v = if n.is_empty() { vec![0] } else { seg(n) };
    if bits < 64 {
        v.push(bits as u8);
    } else if bits < 1 << 12 {
        v.extend([0x40 | (bits & 0xf) as u8, (bits >> 4) as u8]);
    } else {
        v.extend([0x80 | (bits & 0xf) as u8, (bits >> 4) as u8, (bits >> 12) as u8]);
    }
    v
}

/// `Field (region, flags) { units }`; flags: access type (bits 0-3) and
/// update rule (bits 5-6).
pub fn field(region: &str, flags: u8, units: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0x5b, 0x81], pkglen(cat(&[name(region), vec![flags], units.concat()]))])
}

pub fn index_field(index: &str, data: &str, flags: u8, units: &[Vec<u8>]) -> Vec<u8> {
    cat(&[vec![0x5b, 0x86], pkglen(cat(&[name(index), name(data), vec![flags], units.concat()]))])
}

pub fn create_dword_field(src: Vec<u8>, byte: Vec<u8>, n: &str) -> Vec<u8> {
    cat(&[vec![0x8a], src, byte, name(n)])
}

/// A whole table: the 36-byte header (checksummed) and the AML.
pub fn table(sig: &[u8; 4], revision: u8, oem_table: &str, aml: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend(sig);
    t.extend(((36 + aml.len()) as u32).to_le_bytes());
    t.push(revision);
    t.push(0);
    t.extend(b"RS2   ");
    let mut id: Vec<u8> = oem_table.bytes().take(8).collect();
    id.resize(8, b' ');
    t.extend(id);
    t.extend(1u32.to_le_bytes());
    t.extend(b"RS2 ");
    t.extend(1u32.to_le_bytes());
    t.extend(aml);
    let sum = t.iter().fold(0u8, |a, &c| a.wrapping_add(c));
    t[9] = 0u8.wrapping_sub(sum);
    t
}

/// The SSDT rs2 tests battery support with under QEMU (which has no
/// battery): `\_SB.BAT0` (PNP0C0A) with fixed `_BIF`/`_BIX` and a `_BST`
/// computed from them, charging at 72%; `\_SB.ADP1` (ACPI0003) on line.
/// `discharging`: on battery at 35% instead, 1 h 24 min left.
pub fn battery_ssdt(discharging: bool) -> Vec<u8> {
    let (state, percent, rate, psr) = if discharging { (1, 35, 1200, 0) } else { (2, 72, 1500, 1) };
    let bat = device(
        "BAT0",
        &[
            def_name("_HID", eisa("PNP0C0A")),
            def_name("_UID", int(1)),
            def_name("PCNT", int(percent)),
            mutex("BMTX"),
            method("_STA", 0, &[ret(int(0x1f))]),
            // mAh, design 5000, last full 4800, rechargeable, 11.1 V,
            // warning 480, low 240.
            def_name(
                "BIFP",
                package(&[
                    int(1),
                    int(5000),
                    int(4800),
                    int(1),
                    int(11100),
                    int(480),
                    int(240),
                    int(10),
                    int(10),
                    string("RS2 Test Battery"),
                    string("0001"),
                    string("LION"),
                    string("rs2"),
                ]),
            ),
            method("_BIF", 0, &[ret(name("BIFP"))]),
            def_name("BSTP", package(&[int(0), int(0), int(0), int(0)])),
            method(
                "_BST",
                0,
                &[
                    acquire("BMTX"),
                    store(int(state), index(name("BSTP"), int(0), none())),
                    store(int(rate), index(name("BSTP"), int(1), none())),
                    // Remaining = last full * PCNT / 100, counted up in a
                    // loop to exercise While.
                    store(int(0), local(1)),
                    store(int(0), local(2)),
                    while_(cmp(0x95, local(2), name("PCNT")), &[op2(0x72, local(1), deref(index(name("BIFP"), int(2), none())), local(1)), increment(local(2))]),
                    divide(local(1), int(100), none(), local(0)),
                    store(local(0), index(name("BSTP"), int(2), none())),
                    if_(cmp(0x93, name("PCNT"), int(0)), &[store(int(0), index(name("BSTP"), int(3), none()))]),
                    else_(&[store(int(11400), index(name("BSTP"), int(3), none()))]),
                    release("BMTX"),
                    ret(name("BSTP")),
                ],
            ),
        ],
    );
    let adp = device(
        "ADP1",
        &[def_name("_HID", string("ACPI0003")), method("_STA", 0, &[ret(int(0x0f))]), method("_PSR", 0, &[ret(int(psr))])],
    );
    table(b"SSDT", 2, "RS2BATT", &scope("\\_SB", &[bat, adp]))
}
