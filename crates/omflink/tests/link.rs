//! End-to-end links of object modules built here, checked with os2-exe's
//! NE and LX readers.
use omflink::{Prog, run};
use os2_exe::{Format, detect, lx, ne};
use std::collections::HashMap;

#[derive(Default)]
struct Mem {
    files: HashMap<String, Vec<u8>>,
    out: String,
}

impl omflink::Host for Mem {
    fn read(&mut self, p: &str) -> Option<Vec<u8>> {
        self.files.get(&p.to_ascii_uppercase()).cloned()
    }
    fn write(&mut self, p: &str, d: &[u8]) -> bool {
        self.files.insert(p.to_ascii_uppercase(), d.to_vec());
        true
    }
    fn delete(&mut self, p: &str) {
        self.files.remove(&p.to_ascii_uppercase());
    }
    fn env(&mut self, _: &str) -> Option<String> {
        None
    }
    fn out(&mut self, s: &str) {
        self.out.push_str(s);
    }
    fn prompt(&mut self) -> Option<String> {
        None
    }
}

/// An OMF record with its checksum.
fn rec(ty: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![ty];
    v.extend_from_slice(&((body.len() + 1) as u16).to_le_bytes());
    v.extend_from_slice(body);
    let sum: u8 = v.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    v.push(0u8.wrapping_sub(sum));
    v
}

fn name(s: &str) -> Vec<u8> {
    let mut v = vec![s.len() as u8];
    v.extend_from_slice(s.as_bytes());
    v
}

fn names(list: &[&str]) -> Vec<u8> {
    list.iter().flat_map(|s| name(s)).collect()
}

/// A 16-bit program: CODE segment with a far call to an import, a far
/// pointer to its own code, a selector of DGROUP; DATA in DGROUP.
fn obj16() -> Vec<u8> {
    let mut o = rec(0x80, &name("t16.asm"));
    // LNAMES: 1 "", 2 T_TEXT, 3 CODE, 4 _DATA, 5 DATA, 6 DGROUP
    o.extend(rec(0x96, &names(&["", "T_TEXT", "CODE", "_DATA", "DATA", "DGROUP"])));
    // SEGDEF word public, length 16 / 4.
    o.extend(rec(0x98, &[0x48, 16, 0, 2, 3, 1]));
    o.extend(rec(0x98, &[0x48, 4, 0, 4, 5, 1]));
    o.extend(rec(0x9a, &[6, 0xff, 2]));
    let mut ext = name("DOSEXIT");
    ext.push(0);
    o.extend(rec(0x8c, &ext));
    // Code: call far DOSEXIT (9A ofs sel) at 0; mov ax, seg DGROUP (B8 sel)
    // at 5; far pointer to the code's own offset 0 at 8 (dd).
    let mut d = vec![1, 0, 0];
    d.extend_from_slice(&[0x9a, 0, 0, 0, 0, 0xb8, 0, 0, 0, 0, 0, 0, 0xcb, 0x90, 0x90, 0x90]);
    o.extend(rec(0xa0, &d));
    // FIXUPP: ptr16:16 at 1 to EI(1) frame target; base at 6 to GI(1);
    // ptr16:16 at 8 to SI(1) frame target.
    let fix = [
        0xcc, 0x01, 0x56, 0x01, // M=1 loc 3 (ptr) off 1; frame target, target EI, P=1
        0xc8, 0x06, 0x55, 0x01, // loc 2 (base) off 6; frame target, target GI(1)
        0xcc, 0x08, 0x54, 0x01, // loc 3 off 8; frame target, target SI(1)
    ];
    o.extend(rec(0x9c, &fix));
    o.extend(rec(0xa0, &[2, 0, 0, 1, 2, 3, 4]));
    // MODEND with start: frame target (5), target SI(1), displacement 0.
    o.extend(rec(0x8a, &[0xc1, 0x50, 1, 0, 0]));
    o
}

/// An import library module: COMENT A0/01 DOSEXIT = DOSCALLS.5.
fn implib() -> Vec<u8> {
    let mut m = rec(0x80, &name("DOSEXIT"));
    let mut c = vec![0x00, 0xa0, 0x01, 0x01];
    c.extend(name("DOSEXIT"));
    c.extend(name("DOSCALLS"));
    c.extend_from_slice(&5u16.to_le_bytes());
    m.extend(rec(0x88, &c));
    m.extend(rec(0x8a, &[0]));
    // Library: header page 16, the module at 16, end record.
    let page = 16usize;
    let mut lib = vec![0xf0];
    lib.extend_from_slice(&((page - 3) as u16).to_le_bytes());
    lib.resize(page, 0);
    lib.extend(m);
    lib.resize(lib.len().next_multiple_of(page), 0);
    lib.extend(rec(0xf1, &[]));
    lib
}

#[test]
fn ne_program() {
    let mut h = Mem::default();
    h.files.insert("T16.OBJ".into(), obj16());
    h.files.insert("IMP.LIB".into(), implib());
    h.files.insert("T16.DEF".into(), b"NAME T16 WINDOWCOMPAT\r\nSTACKSIZE 4096\r\n".to_vec());
    let rc = run(&mut h, Prog::Link, "/NOLOGO t16,t16.exe,t16.map,imp,t16;");
    assert_eq!(rc, 0, "{}", h.out);
    let exe = h.files["T16.EXE"].clone();
    let Ok(Format::Ne { header }) = detect(&exe) else { panic!("not NE") };
    let m = ne::Module::parse(&exe, header).unwrap();
    assert_eq!(m.header.flags & 0x0300, 0x0200);
    assert_eq!(m.header.stack, 4096);
    assert_eq!((m.header.cs, m.header.ip), (1, 0));
    assert_eq!(m.import_modules().unwrap(), vec![&b"DOSCALLS"[..]]);
    let segs = m.segments().unwrap();
    assert_eq!(segs.len(), 2);
    let relocs = m.relocs(&segs[0]).unwrap();
    // The import, and one chain of selectors for each segment.
    assert!(relocs.iter().any(|r| r.kind == ne::SourceKind::Pointer1616
        && r.target == ne::Target::ImportOrdinal { module: 1, ordinal: 5 }));
    assert!(relocs.iter().any(|r| r.kind == ne::SourceKind::Selector && r.target == ne::Target::Internal { segment: 2, offset: 0 }));
    assert!(relocs.iter().any(|r| r.kind == ne::SourceKind::Selector && r.target == ne::Target::Internal { segment: 1, offset: 0 }));
    let code = m.segment_data(&segs[0]).unwrap();
    // The far pointer's offset part is resolved at link time.
    assert_eq!(&code[8..10], &[0, 0]);
    // The checksum is the sum of the file's dwords.
    let mut f = exe.clone();
    f[header + 8..header + 12].fill(0);
    f.resize(f.len().next_multiple_of(4), 0);
    let sum = f.chunks(4).fold(0u32, |a, c| a.wrapping_add(u32::from_le_bytes(c.try_into().unwrap())));
    assert_eq!(sum, u32::from_le_bytes(exe[header + 8..header + 12].try_into().unwrap()));
    assert!(h.files.contains_key("T16.MAP"));
}

/// A 32-bit flat program: a call to an import, a flat pointer to data.
fn obj32() -> Vec<u8> {
    let mut o = rec(0x80, &name("t32.asm"));
    o.extend(rec(0x96, &names(&["", "_TEXT", "CODE", "_DATA", "DATA", "FLAT", "DGROUP"])));
    // SEGDEF386 dword public use32.
    o.extend(rec(0x99, &[0xa9, 16, 0, 0, 0, 2, 3, 1]));
    o.extend(rec(0x99, &[0xa9, 8, 0, 0, 0, 4, 5, 1]));
    o.extend(rec(0x9a, &[6]));
    o.extend(rec(0x9a, &[7, 0xff, 2]));
    let mut ext = name("DosExit");
    ext.push(0);
    o.extend(rec(0x8c, &ext));
    // mov eax, offset data (B8 imm32) at 0; call DosExit (E8 rel32) at 5.
    let mut d = vec![1, 0, 0, 0, 0];
    d.extend_from_slice(&[0xb8, 4, 0, 0, 0, 0xe8, 0, 0, 0, 0, 0xc3, 0x90, 0x90, 0x90, 0x90, 0x90]);
    o.extend(rec(0xa1, &d));
    let fix = [
        0xe4, 0x01, 0x14, 0x01, 0x02, // M=1 loc 9 (off32) at 1, frame GI(1)=FLAT, target SI(2) +P
        0xa4, 0x06, 0x16, 0x01, 0x01, // M=0 (self) loc 9 at 6, frame GI(1), target EI(1)
    ];
    o.extend(rec(0x9d, &fix));
    o.extend(rec(0xa1, &[2, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]));
    o.extend(rec(0x8b, &[0xc1, 0x50, 1, 0, 0, 0, 0]));
    o
}

fn implib32() -> Vec<u8> {
    let mut m = rec(0x80, &name("DosExit"));
    let mut c = vec![0x00, 0xa0, 0x01, 0x01];
    c.extend(name("DosExit"));
    c.extend(name("DOSCALLS"));
    c.extend_from_slice(&234u16.to_le_bytes());
    m.extend(rec(0x88, &c));
    m.extend(rec(0x8a, &[0]));
    let page = 16usize;
    let mut lib = vec![0xf0];
    lib.extend_from_slice(&((page - 3) as u16).to_le_bytes());
    lib.resize(page, 0);
    lib.extend(m);
    lib.resize(lib.len().next_multiple_of(page), 0);
    lib.extend(rec(0xf1, &[]));
    lib
}

#[test]
fn lx_program() {
    let mut h = Mem::default();
    h.files.insert("T32.OBJ".into(), obj32());
    h.files.insert("IMP.LIB".into(), implib32());
    h.files.insert("T32.DEF".into(), b"NAME T32 WINDOWCOMPAT\nSTACKSIZE 8192\n".to_vec());
    let rc = run(&mut h, Prog::Link386, "/NOLOGO /E:2 t32,t32.exe,,imp,t32;");
    assert_eq!(rc, 0, "{}", h.out);
    let exe = h.files["T32.EXE"].clone();
    let Ok(Format::Lx { header }) = detect(&exe) else { panic!("not LX") };
    let m = lx::Module::parse(&exe, header).unwrap();
    assert_eq!(m.header.module_flags & 0x0300, 0x0200);
    // EIP in the code object, ESP at the end of the data object + stack.
    assert_eq!(m.header.eip_object, 1);
    assert_eq!(m.header.esp_object, 2);
    assert_eq!(m.header.esp, 8 + 8192);
    let mut page = vec![0u8; 4096];
    m.load_page(1, &mut page).unwrap();
    // The flat offset holds the data object's address (base 0x20000) + 4.
    assert_eq!(u32::from_le_bytes(page[1..5].try_into().unwrap()), 0x20004);
    let fx = m.fixups(1).unwrap();
    assert_eq!(fx.len(), 2, "{fx:?}");
}
