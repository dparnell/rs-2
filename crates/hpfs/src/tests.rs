extern crate std;

use super::*;
use core::cell::RefCell;
use std::collections::BTreeSet;
use std::format;
use std::string::ToString;
use std::vec::Vec;

struct Mem(RefCell<Vec<u8>>);

impl BlockDevice for Mem {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        let o = lba as usize * 512;
        let d = self.0.borrow();
        if o + buf.len() > d.len() {
            return Err(Error::Io);
        }
        buf.copy_from_slice(&d[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> Result<()> {
        let o = lba as usize * 512;
        let mut d = self.0.borrow_mut();
        if o + buf.len() > d.len() {
            return Err(Error::Io);
        }
        d[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

const NOW: u32 = 1_760_000_000;

fn fresh(mb: usize) -> Volume<Mem> {
    let dev = Mem(RefCell::new(std::vec![0u8; mb << 20]));
    format(&dev, &FormatParams { label: "TEST", serial: 0x1234_5678, hidden_sectors: 0, code_page: 850, now: NOW })
        .unwrap();
    Volume::mount(dev).unwrap()
}

fn remount(v: Volume<Mem>) -> Volume<Mem> {
    Volume::mount(v.dev).unwrap()
}

/// Deterministic pseudo-random numbers.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

/// A consistency checker: walks every directory and file, verifies the
/// B-tree invariants and that every sector in use is marked so, exactly
/// once.
#[derive(Default)]
struct Check {
    seen: BTreeSet<u32>,
    /// Sectors charged to the main bitmap (beyond what format used).
    main: u32,
    /// DNODEs in use in the directory band.
    band: u32,
    files: u32,
    dirs: u32,
    /// Deepest leaf level seen (0 = the top DNODE is a leaf).
    depth: u32,
}

impl Check {
    fn claim(&mut self, v: &Volume<Mem>, s: u32, n: u32) {
        for x in s..s + n {
            assert!(self.seen.insert(x), "sector {x} used twice");
            assert!(!v.is_free(x), "sector {x} in use but free in bitmap");
        }
        self.main += n;
    }

    fn claim_dnode(&mut self, v: &Volume<Mem>, s: u32) {
        let b = &v.band;
        if s >= b.start && s < b.start + b.count * 4 {
            let k = ((s - b.start) / 4) as usize;
            assert_eq!(s % 4, b.start % 4);
            assert!(b.bits[k / 8] & (1 << (k % 8)) == 0, "dnode {s} free in band bitmap");
            assert!(self.seen.insert(s), "dnode {s} used twice");
            self.band += 1;
        } else {
            self.claim(v, s, 4);
        }
    }

    fn dnode(
        &mut self,
        v: &Volume<Mem>,
        sec: u32,
        up: u32,
        lo: Option<&[u8]>,
        hi: Option<&[u8]>,
        depth: u32,
        leaf_depth: &mut Option<u32>,
    ) {
        let d = v.read_dnode(sec).unwrap();
        assert_eq!(d.up, up, "dnode {sec} up");
        assert_eq!(d.is_root(), depth == 0, "dnode {sec} root flag");
        if depth > 0 {
            assert!(d.keys() >= 1, "non-top dnode {sec} without entries");
        }
        self.claim_dnode(v, sec);
        let mut prev: Option<&[u8]> = lo;
        for e in &d.entries {
            let name: Option<&[u8]> = if e.is_end() { hi } else { Some(&e.name) };
            if let Some(c) = e.down {
                self.dnode(v, c, sec, prev, name, depth + 1, leaf_depth);
            }
            if !e.is_end() {
                if let Some(p) = prev {
                    assert_eq!(v.case.cmp(p, &e.name), core::cmp::Ordering::Less, "order in dnode {sec}");
                }
                if let Some(h) = hi {
                    assert_eq!(v.case.cmp(&e.name, h), core::cmp::Ordering::Less, "upper bound in {sec}");
                }
                prev = Some(&e.name);
            }
        }
        if d.leaf() {
            self.depth = self.depth.max(depth);
            match leaf_depth {
                Some(l) => assert_eq!(*l, depth, "unbalanced directory tree"),
                None => *leaf_depth = Some(depth),
            }
        }
    }

    fn dir(&mut self, v: &mut Volume<Mem>, fno: u32, root: bool) {
        self.dirs += 1;
        if !root {
            self.claim(v, fno, 1);
        }
        let top = v.dir_root(fno).unwrap();
        let mut ld = None;
        self.dnode(v, top, fno, None, None, 0, &mut ld);
        let mut all = Vec::new();
        v.walk_dir(top, &mut all, 0).unwrap();
        assert_eq!(all.iter().filter(|d| d.flags & layout::de::SPECIAL != 0).count(), 1, "one .. entry");
        for d in all.iter().filter(|d| d.flags & layout::de::SPECIAL == 0) {
            let f = v.read_fnode(d.fnode).unwrap();
            assert_eq!(rd32(&f, layout::fnode::UP), fno, "fnode up of {:?}", d.name);
            assert_eq!(f[layout::fnode::NAME_LEN] as usize, d.name.len());
            if d.attr & attr::DIRECTORY != 0 {
                self.dir(v, d.fnode, false);
            } else {
                self.file(v, d.fnode, d.size);
            }
        }
    }

    fn file(&mut self, v: &mut Volume<Mem>, fno: u32, size: u32) {
        self.files += 1;
        self.claim(v, fno, 1);
        let f = v.read_fnode(fno).unwrap();
        assert_eq!(rd32(&f, layout::fnode::FILE_SIZE), size);
        let (ext, alsecs) = v.tree_of(&f, layout::fnode::BTREE).unwrap();
        for a in alsecs {
            self.claim(v, a, 1);
        }
        let mut next = 0;
        for x in &ext {
            assert_eq!(x.file, next, "extents not contiguous");
            next += x.len;
            self.claim(v, x.disk, x.len);
        }
        assert_eq!(next, size.div_ceil(512), "allocation does not match size");
    }
}

/// Check the whole volume against the free counts right after format.
fn check(v: &mut Volume<Mem>, free0: u32) -> Check {
    let mut c = Check::default();
    let root = v.root;
    c.dir(v, root, true);
    assert_eq!(v.band.count - v.free_dir_band_dnodes(), c.band, "band dnodes in use");
    assert_eq!(free0 - v.info().free_sectors, c.main, "sectors in use");
    // The cached free count matches the bitmaps.
    let counted = (0..v.total).filter(|&s| v.is_free(s)).count() as u32;
    assert_eq!(counted, v.info().free_sectors);
    c
}

fn pattern(n: usize, seed: u32) -> Vec<u8> {
    (0..n as u32).map(|i| (i.wrapping_mul(2654435761).wrapping_add(seed) >> 13) as u8).collect()
}

fn ls(v: &mut Volume<Mem>, path: &str) -> Vec<Entry> {
    let d = v.lookup(path).unwrap();
    v.read_dir(&d).unwrap()
}

fn read_all(v: &mut Volume<Mem>, path: &str) -> Vec<u8> {
    let e = v.lookup(path).unwrap();
    let mut b = std::vec![0u8; e.size as usize];
    assert_eq!(v.read(&e, 0, &mut b).unwrap(), e.size as usize);
    b
}

#[test]
fn format_and_mount() {
    let mut v = fresh(32);
    let i = v.info();
    assert_eq!(i.total_sectors, 65536);
    assert!(i.free_sectors > 60000);
    assert_eq!(v.serial, 0x1234_5678);
    assert_eq!(&v.label, b"TEST       ");
    assert_eq!(v.case.code_page, 850);
    assert_eq!(v.case.upper(0x82), 0x90);
    let root = v.lookup("").unwrap();
    assert!(root.is_dir());
    assert_eq!(root.modified, NOW);
    assert!(v.read_dir(&root).unwrap().is_empty());
    let free0 = v.info().free_sectors;
    check(&mut v, free0);
}

#[test]
fn names_and_nesting() {
    let mut v = fresh(16);
    let free0 = v.info().free_sectors;
    v.mkdir("Program Files", NOW).unwrap();
    v.mkdir("program files\\Long Directory Name.With.Dots", NOW).unwrap();
    let mut e = v.create_file("PROGRAM FILES\\long directory name.with.dots\\ReadMe First.Text", 0, NOW).unwrap();
    assert!(e.attr & attr::LONG_NAME != 0);
    v.write(&mut e, 0, b"hello, HPFS", NOW + 5).unwrap();
    let short = v.create_file("CONFIG.SYS", attr::HIDDEN, NOW).unwrap();
    assert_eq!(short.attr & attr::LONG_NAME, 0);
    assert_eq!(short.attr & (attr::HIDDEN | attr::ARCHIVE), attr::HIDDEN | attr::ARCHIVE);
    assert_eq!(v.create_file("config.sys", 0, NOW).unwrap_err(), Error::Exists);
    assert_eq!(v.create_file("bad|name", 0, NOW).unwrap_err(), Error::InvalidName);
    let long: std::string::String = "x".repeat(254);
    v.create_file(&long, 0, NOW).unwrap();
    assert_eq!(v.create_file(&"y".repeat(255), 0, NOW).unwrap_err(), Error::InvalidName);
    // Code page characters: é (0x82) matches É (0x90).
    v.create_file("caf\u{82}.txt", 0, NOW).unwrap();
    assert_eq!(v.lookup("CAF\u{90}.TXT").unwrap().name, "caf\u{82}.txt");

    let f = v.lookup("program files\\LONG DIRECTORY NAME.WITH.DOTS\\readme first.text").unwrap();
    assert_eq!(f.name, "ReadMe First.Text");
    assert_eq!(f.size, 11);
    assert_eq!(f.modified, NOW + 5);
    assert_eq!(read_all(&mut v, "Program Files\\Long Directory Name.With.Dots\\README FIRST.TEXT"), b"hello, HPFS");
    assert_eq!(v.lookup("Program Files\\nope\\x").unwrap_err(), Error::PathNotFound);
    assert_eq!(v.lookup("Program Files\\nope").unwrap_err(), Error::NotFound);
    assert_eq!(v.lookup("CONFIG.SYS\\x").unwrap_err(), Error::PathNotFound);
    assert_eq!(v.lookup("Program Files\\..\\config.sys").unwrap().name, "CONFIG.SYS");

    let names: Vec<_> = ls(&mut v, "").into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["caf\u{82}.txt", "CONFIG.SYS", "Program Files", long.as_str()]);

    let mut v = remount(v);
    assert_eq!(read_all(&mut v, "program files\\long directory name.with.dots\\readme first.text"), b"hello, HPFS");
    let c = check(&mut v, free0);
    assert_eq!((c.files, c.dirs), (4, 3));
    assert_eq!(v.rmdir("Program Files").unwrap_err(), Error::DirectoryNotEmpty);
    assert_eq!(v.delete("Program Files").unwrap_err(), Error::IsADirectory);
    assert_eq!(v.rmdir("CONFIG.SYS").unwrap_err(), Error::NotADirectory);
}

#[test]
fn large_and_fragmented_files() {
    let mut v = fresh(48);
    let free0 = v.info().free_sectors;
    // One big contiguous file.
    let big = pattern(3 << 20, 1);
    let mut e = v.create_file("big.bin", 0, NOW).unwrap();
    v.write(&mut e, 0, &big, NOW).unwrap();
    assert_eq!(read_all(&mut v, "BIG.BIN"), big);
    // Three files grown in turns, one sector at a time: each ends up with
    // hundreds of extents (ALSEC trees two levels deep).
    let names = ["frag a", "frag b", "frag c"];
    let mut ents: Vec<Entry> = names.iter().map(|n| v.create_file(n, 0, NOW).unwrap()).collect();
    let chunks = 700;
    let data: Vec<Vec<u8>> = (0..3).map(|i| pattern(chunks * 512, 10 + i)).collect();
    for k in 0..chunks {
        for i in 0..3 {
            v.write(&mut ents[i], (k * 512) as u64, &data[i][k * 512..(k + 1) * 512], NOW).unwrap();
        }
    }
    for i in 0..3 {
        let ext = v.extents(ents[i].fnode).unwrap();
        assert!(ext.len() > 480, "only {} extents", ext.len());
        let f = v.read_fnode(ents[i].fnode).unwrap();
        assert!(f[layout::fnode::BTREE] & layout::bt::INTERNAL != 0);
    }
    check(&mut v, free0);
    let mut v = remount(v);
    for i in 0..3 {
        assert_eq!(read_all(&mut v, names[i]), data[i]);
    }
    // Unaligned reads across extent boundaries.
    let e = v.lookup("frag b").unwrap();
    let mut b = std::vec![0u8; 3000];
    v.read(&e, 1001, &mut b).unwrap();
    assert_eq!(b, data[1][1001..4001]);
    // Truncate one into the middle, re-grow it with zeros.
    let mut e = v.lookup("frag a").unwrap();
    v.set_size(&mut e, 100_000, NOW).unwrap();
    check(&mut v, free0);
    v.set_size(&mut e, 150_000, NOW).unwrap();
    let back = read_all(&mut v, "frag a");
    assert_eq!(back[..100_000], data[0][..100_000]);
    assert!(back[100_000..].iter().all(|&b| b == 0));
    check(&mut v, free0);
    // Delete everything: all space comes back.
    for n in ["big.bin", "frag a", "frag b", "frag c"] {
        v.delete(n).unwrap();
    }
    check(&mut v, free0);
    assert_eq!(v.info().free_sectors, free0);
}

#[test]
fn writes_with_gaps_and_overwrites() {
    let mut v = fresh(8);
    let free0 = v.info().free_sectors;
    let mut e = v.create_file("sparse", 0, NOW).unwrap();
    v.write(&mut e, 10_000, b"tail", NOW).unwrap();
    assert_eq!(e.size, 10_004);
    let b = read_all(&mut v, "sparse");
    assert!(b[..10_000].iter().all(|&x| x == 0));
    assert_eq!(&b[10_000..], b"tail");
    v.write(&mut e, 511, b"XY", NOW).unwrap();
    let b = read_all(&mut v, "sparse");
    assert_eq!(&b[510..514], &[0, b'X', b'Y', 0]);
    assert_eq!(b.len(), 10_004);
    v.set_size(&mut e, 0, NOW).unwrap();
    assert_eq!(v.lookup("sparse").unwrap().size, 0);
    check(&mut v, free0);
    let mut e = v.lookup("sparse").unwrap();
    e.attr |= attr::READONLY;
    e.created = 1000;
    e.accessed = 2000;
    v.update_entry(&e).unwrap();
    let e = v.lookup("SPARSE").unwrap();
    assert_eq!((e.attr & attr::READONLY, e.created, e.accessed), (attr::READONLY, 1000, 2000));
    assert_eq!(v.delete("sparse").unwrap_err(), Error::AccessDenied);
}

#[test]
fn directory_btree_splits() {
    let mut v = fresh(32);
    let free0 = v.info().free_sectors;
    let band_free0 = v.free_dir_band_dnodes();
    v.mkdir("many", NOW).unwrap();
    let mut rng = Lcg(42);
    let mut names: Vec<std::string::String> = (0..600)
        .map(|i| {
            let pad = "abcdefghij".repeat(20);
            format!("Entry {:04} {}", (i * 7919) % 600, &pad[..(rng.next() as usize % 180) + 1])
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        let p = format!("many\\{n}");
        if i % 3 == 0 {
            v.mkdir(&p, NOW).unwrap();
        } else {
            let mut e = v.create_file(&p, 0, NOW).unwrap();
            v.write(&mut e, 0, n.as_bytes(), NOW).unwrap();
        }
        if i % 97 == 0 {
            check(&mut v, free0);
        }
    }
    let c = check(&mut v, free0);
    assert_eq!(c.files + c.dirs, 600 + 1 + 1);
    assert!(c.depth >= 2, "tree only {} levels deep", c.depth + 1);
    let many = v.lookup("many").unwrap().fnode;
    let top = v.dir_root(many).unwrap();
    assert!(!v.read_dnode(top).unwrap().leaf(), "directory did not split");
    let mut v = remount(v);
    let listed: Vec<_> = ls(&mut v, "MANY").into_iter().map(|e| e.name).collect();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| v.case.cmp(a.as_bytes(), b.as_bytes()));
    assert_eq!(listed, sorted);
    for n in names.iter().step_by(37) {
        let e = v.lookup(&format!("MANY\\{}", n.to_uppercase())).unwrap();
        assert_eq!(&e.name, n);
    }
    // Delete in random order, checking the tree as it shrinks.
    for i in (1..names.len()).rev() {
        let j = rng.next() as usize % (i + 1);
        names.swap(i, j);
    }
    for (i, n) in names.iter().enumerate() {
        let p = format!("many\\{n}");
        if v.lookup(&p).unwrap().is_dir() {
            v.rmdir(&p).unwrap();
        } else {
            v.delete(&p).unwrap();
        }
        if i % 53 == 0 {
            check(&mut v, free0);
        }
    }
    assert!(ls(&mut v, "many").is_empty());
    check(&mut v, free0);
    v.rmdir("many").unwrap();
    check(&mut v, free0);
    assert_eq!(v.info().free_sectors, free0);
    assert_eq!(v.free_dir_band_dnodes(), band_free0);
}

#[test]
fn root_directory_splits_and_collapses() {
    let mut v = fresh(16);
    let free0 = v.info().free_sectors;
    for i in 0..300 {
        v.create_file(&format!("{:03} {}", i, "n".repeat(200)), 0, NOW).unwrap();
    }
    check(&mut v, free0);
    for i in 0..300 {
        v.delete(&format!("{:03} {}", i, "n".repeat(200))).unwrap();
    }
    check(&mut v, free0);
    assert_eq!(v.info().free_sectors, free0);
    let top = v.dir_root(v.root).unwrap();
    assert!(v.read_dnode(top).unwrap().leaf());
}

#[test]
fn rename_and_move() {
    let mut v = fresh(16);
    let free0 = v.info().free_sectors;
    v.mkdir("a", NOW).unwrap();
    v.mkdir("a\\b", NOW).unwrap();
    v.mkdir("c", NOW).unwrap();
    let mut e = v.create_file("a\\b\\file.txt", 0, NOW).unwrap();
    v.write(&mut e, 0, b"data", NOW).unwrap();
    // Case-only rename.
    v.rename("a\\b\\file.txt", "a\\b\\File.TXT").unwrap();
    assert_eq!(v.lookup("a\\b\\file.txt").unwrap().name, "File.TXT");
    // Rename within the directory.
    v.rename("a\\b\\file.txt", "a\\b\\A much longer name.txt").unwrap();
    assert_eq!(v.lookup("a\\b\\file.txt").unwrap_err(), Error::NotFound);
    // Move to another directory.
    v.rename("a\\b\\a much longer name.txt", "c\\moved.txt").unwrap();
    assert_eq!(read_all(&mut v, "c\\MOVED.TXT"), b"data");
    // Moving a directory, and not into itself.
    v.rename("a\\b", "c\\b2").unwrap();
    assert_eq!(v.rename("c", "c\\b2\\c").unwrap_err(), Error::AccessDenied);
    v.create_file("c\\b2\\inner", 0, NOW).unwrap();
    assert!(v.lookup("c\\b2\\inner").is_ok());
    assert_eq!(v.rename("c\\moved.txt", "c\\b2\\inner").unwrap_err(), Error::Exists);
    // An entry renamed while "open" can still be updated.
    let mut stale = v.lookup("c\\moved.txt").unwrap();
    v.rename("c\\moved.txt", "a\\renamed.txt").unwrap();
    v.write(&mut stale, 4, b"++", NOW).unwrap();
    assert_eq!(read_all(&mut v, "a\\renamed.txt"), b"data++");
    let mut v = remount(v);
    let c = check(&mut v, free0);
    assert_eq!((c.files, c.dirs), (2, 4));
    assert_eq!(ls(&mut v, "a").len(), 1);
}

#[test]
fn disk_full_and_recovery() {
    let mut v = fresh(1);
    let free0 = v.info().free_sectors;
    let mut e = v.create_file("fill", 0, NOW).unwrap();
    let chunk = std::vec![0x5au8; 64 * 1024];
    let mut off = 0u64;
    loop {
        match v.write(&mut e, off, &chunk, NOW) {
            Ok(_) => off += chunk.len() as u64,
            Err(Error::DiskFull) => break,
            Err(x) => panic!("{x:?}"),
        }
    }
    assert!(off > 0);
    assert_eq!(v.lookup("fill").unwrap().size as u64, off);
    check(&mut v, free0);
    v.delete("fill").unwrap();
    assert_eq!(v.info().free_sectors, free0);
    check(&mut v, free0);
}

#[test]
fn extended_attributes() {
    let mut v = fresh(4);
    let e = v.create_file("withea", 0, NOW).unwrap();
    // Build an FEA list by hand: one in the F-node, one outside.
    let fea = |name: &[u8], value: &[u8]| {
        let mut b = std::vec![0u8, name.len() as u8];
        b.extend_from_slice(&(value.len() as u16).to_le_bytes());
        b.extend_from_slice(name);
        b.push(0);
        b.extend_from_slice(value);
        b
    };
    let inner = fea(b".TYPE", b"\xfd\xff\x04\x00Text");
    let outer = fea(b".LONGNAME", &[7u8; 700]);
    let sec = v.alloc_run(100, 2).unwrap().0;
    let mut buf = std::vec![0u8; 1024];
    buf[..outer.len()].copy_from_slice(&outer);
    v.write_secs(sec, &buf).unwrap();
    let mut f = v.read_fnode(e.fnode).unwrap();
    let o = layout::fnode::EA_AREA;
    f[o..o + inner.len()].copy_from_slice(&inner);
    wr16(&mut f, layout::fnode::EA_INT_SIZE, inner.len() as u16);
    wr32(&mut f, layout::fnode::EA_EXT_SIZE, outer.len() as u32);
    wr32(&mut f, layout::fnode::EA_SECTOR, sec);
    v.write_secs(e.fnode, &f).unwrap();
    let eas = v.read_eas(&e).unwrap();
    assert_eq!(eas.len(), 2);
    assert_eq!(eas[0].name, b".TYPE");
    assert_eq!(eas[1].value, std::vec![7u8; 700]);
    assert_eq!(fea2_size(&eas), 4 + 24 + (8 + 10 + 700));
    let free = v.info().free_sectors;
    v.delete("withea").unwrap();
    assert_eq!(v.info().free_sectors, free + 1 + 2);
}

#[test]
fn readonly_mount() {
    let mut v = fresh(4);
    v.set_readonly(true);
    assert_eq!(v.mkdir("x", NOW).unwrap_err(), Error::ReadOnly);
    assert_eq!(v.set_label("NEW").unwrap_err(), Error::ReadOnly);
    v.set_readonly(false);
    v.set_label("New Label").unwrap();
    let v = remount(v);
    assert_eq!(&v.label, b"NEW LABEL  ");
    assert_eq!(v.info().serial.to_string(), 0x1234_5678u32.to_string());
}

/// A disk built by `cargo xtask disk --hpfs` (and possibly written to by
/// the kernel in QEMU): check its consistency if present.
#[test]
fn xtask_disk_image() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/hpfs.img");
    let Ok(img) = std::fs::read(path) else { return };
    let part = img[2048 * 512..].to_vec();
    let mut v = Volume::mount(Mem(RefCell::new(part))).unwrap();
    let mut c = Check::default();
    let root = v.root;
    c.dir(&mut v, root, true);
    assert_eq!(v.band.count - v.free_dir_band_dnodes(), c.band);
    std::println!("{path}: {} files, {} directories", c.files, c.dirs);
}

#[test]
fn rejects_other_file_systems() {
    let dev = Mem(RefCell::new(std::vec![0u8; 1 << 20]));
    assert_eq!(Volume::mount(dev).err(), Some(Error::NotHpfs));
}
