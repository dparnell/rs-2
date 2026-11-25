//! An ELF64 file without its debug sections.

fn num(d: &[u8], o: usize, n: usize) -> Option<u64> {
    Some(d.get(o..o + n)?.iter().rev().fold(0, |v, &b| v << 8 | b as u64))
}

fn put(d: &mut [u8], o: usize, n: usize, v: u64) {
    d[o..o + n].copy_from_slice(&v.to_le_bytes()[..n]);
}

/// Without `.debug*` (what objcopy --strip-debug does, but for the symbols
/// of files). Segments stay where they are in the file.
pub fn strip_debug(d: &[u8]) -> Option<Vec<u8>> {
    const SH: usize = 64;
    const SHT_SYMTAB: u64 = 2;
    const SHT_NOBITS: u64 = 8;
    if d.get(..5)? != b"\x7fELF\x02" {
        return None;
    }
    let (phoff, shoff) = (num(d, 0x20, 8)? as usize, num(d, 0x28, 8)? as usize);
    let (phnum, shnum, shstrndx) = (num(d, 0x38, 2)? as usize, num(d, 0x3c, 2)? as usize, num(d, 0x3e, 2)? as usize);
    let sections: Vec<&[u8]> = (0..shnum).map(|i| d.get(shoff + i * SH..shoff + (i + 1) * SH)).collect::<Option<_>>()?;
    let names = num(sections.get(shstrndx)?, 0x18, 8)? as usize;
    let keep = |s: &&[u8]| -> Option<bool> { Some(!d.get(names + num(s, 0, 4)? as usize..)?.starts_with(b".debug")) };
    // Old section numbers to new ones (0 for what goes).
    let (mut map, mut kept) = (vec![0u64; shnum], Vec::new());
    for (i, s) in sections.iter().enumerate() {
        if keep(s)? {
            map[i] = kept.len() as u64;
            kept.push(*s);
        }
    }
    // The headers and the segments, then what is in no segment.
    let mut end = phoff + phnum * 56;
    for i in 0..phnum {
        let p = phoff + i * 56;
        end = end.max((num(d, p + 8, 8)? + num(d, p + 0x20, 8)?) as usize);
    }
    let mut out = d.get(..end)?.to_vec();
    let mut headers = Vec::new();
    for s in kept {
        let mut h = s.to_vec();
        let (kind, off, size) = (num(s, 4, 4)?, num(s, 0x18, 8)? as usize, num(s, 0x20, 8)? as usize);
        if kind != SHT_NOBITS && size != 0 && off + size > end {
            out.resize(out.len().next_multiple_of(16), 0);
            put(&mut h, 0x18, 8, out.len() as u64);
            let mut data = d.get(off..off + size)?.to_vec();
            if kind == SHT_SYMTAB {
                for y in (0..size).step_by(24) {
                    let x = num(&data, y + 6, 2)? as usize;
                    if x < shnum {
                        put(&mut data, y + 6, 2, map[x]);
                    }
                }
            }
            out.extend(data);
        }
        let link = num(s, 0x28, 4)? as usize;
        put(&mut h, 0x28, 4, *map.get(link)?);
        headers.push(h);
    }
    out.resize(out.len().next_multiple_of(16), 0);
    let at = out.len();
    for h in &headers {
        out.extend(h);
    }
    put(&mut out, 0x28, 8, at as u64);
    put(&mut out, 0x3c, 2, headers.len() as u64);
    put(&mut out, 0x3e, 2, map[shstrndx]);
    Some(out)
}
