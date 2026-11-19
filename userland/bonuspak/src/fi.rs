//! The Warp 4 Feature Installer's package descriptions (INSTDATA.INI) and
//! what this installer takes from them.
//!
//! A package file holds one block, `ID=(` ... `)`, with fields
//! (`Key=Value`) and nested blocks of the same form: `Variable` (names
//! used as `{Name}` in other values), `File`, `ObjectCreation`,
//! `ClassRegistration`, `AsciiFile` (CONFIG.SYS and other text files),
//! `Os2PrfIni` (OS2.INI, OS2SYS.INI), `UserExits` (programs run during
//! the install) and `Dependency`. In values `^;` is a semicolon and `^^`
//! a caret; variable names compare without regard to case.
//!
//! The CD's `OS2IMAGE\FI` holds the packages (`FEATURE_\OS2BONUS\BPxxx\
//! INSTDATA.INI`) and the files (`BONUSPAK\xxx`, named by each File's
//! MediaPath and MediaFileName). The extended attributes of the files with
//! an EAMediaIndex are the media directory's `0000000n.EAS` files (16-bit
//! FEALISTs), numbered in order through the packages that share the
//! directory (IBM Works' IBMWORKS, JetAdmin's HPJET...).

use alloc::vec::Vec;

pub struct Block {
    pub kind: Vec<u8>,
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
    pub children: Vec<Block>,
}

impl Block {
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_slice())
    }

    fn val(&self, key: &[u8]) -> &[u8] {
        self.get(key).unwrap_or(b"")
    }

    fn num(&self, key: &[u8]) -> u32 {
        dec_parse(self.val(key))
    }

    fn of(&self, kind: &[u8]) -> impl Iterator<Item = &Block> {
        self.children.iter().filter(move |b| b.kind.eq_ignore_ascii_case(kind))
    }
}

pub fn dec_parse(s: &[u8]) -> u32 {
    s.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |n, &c| n.wrapping_mul(10).wrapping_add((c - b'0') as u32))
}

/// The top-level blocks of a package file.
pub fn parse(text: &[u8]) -> Vec<Block> {
    let mut stack: Vec<Block> = Vec::new();
    let mut top = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        let l = line.trim_ascii();
        if l.is_empty() {
            continue;
        }
        if l == b")" {
            if let Some(b) = stack.pop() {
                match stack.last_mut() {
                    Some(p) => p.children.push(b),
                    None => top.push(b),
                }
            }
            continue;
        }
        let Some(eq) = l.iter().position(|&c| c == b'=') else { continue };
        let (k, v) = (&l[..eq], &l[eq + 1..]);
        if v == b"(" {
            stack.push(Block { kind: k.to_vec(), fields: Vec::new(), children: Vec::new() });
        } else if let Some(b) = stack.last_mut() {
            b.fields.push((k.to_vec(), v.to_vec()));
        }
    }
    top
}

/// Variables, innermost scope last.
#[derive(Clone, Default)]
pub struct Vars(pub Vec<(Vec<u8>, Vec<u8>)>);

impl Vars {
    pub fn set(&mut self, name: &[u8], value: &[u8]) {
        self.0.push((name.to_vec(), value.to_vec()));
    }

    /// A block's `Variable` children (each defined once: the first wins,
    /// so the system's own names stay as they were set).
    pub fn add_block(&mut self, b: &Block) {
        for v in b.of(b"Variable") {
            let name = v.val(b"Name");
            if !self.0.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                self.set(name, v.val(b"Value"));
            }
        }
    }

    fn lookup(&self, name: &[u8]) -> Option<&[u8]> {
        self.0.iter().rev().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice())
    }

    /// `v` with its `{Name}`s replaced (unknown names stay) and its `^`
    /// escapes undone.
    pub fn expand(&self, v: &[u8]) -> Vec<u8> {
        let mut cur = v.to_vec();
        for _ in 0..8 {
            let mut out = Vec::with_capacity(cur.len());
            let mut i = 0;
            let mut changed = false;
            while i < cur.len() {
                if cur[i] == b'^' && i + 1 < cur.len() {
                    out.extend_from_slice(&cur[i..i + 2]);
                    i += 2;
                    continue;
                }
                if cur[i] == b'{' {
                    if let Some(n) = cur[i + 1..].iter().position(|&c| c == b'}') {
                        if let Some(val) = self.lookup(&cur[i + 1..i + 1 + n]) {
                            out.extend_from_slice(val);
                            i += n + 2;
                            changed = true;
                            continue;
                        }
                    }
                }
                out.push(cur[i]);
                i += 1;
            }
            cur = out;
            if !changed {
                break;
            }
        }
        unescape(&cur)
    }

    /// A path value: also `C:os2\x` (one package has it) becomes `C:\os2\x`.
    pub fn path(&self, v: &[u8]) -> Vec<u8> {
        let mut p = self.expand(v);
        if p.len() > 2 && p[1] == b':' && p[2] != b'\\' {
            p.insert(2, b'\\');
        }
        p
    }
}

fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'^' && i + 1 < s.len() {
            out.push(s[i + 1]);
            i += 2;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// A file of a package.
pub struct File {
    /// The CD's directory below `OS2IMAGE\FI` (`BONUSPAK\HALITE`) and the
    /// file's name there.
    pub media_dir: Vec<u8>,
    pub media_name: Vec<u8>,
    /// Full target path (`C:\BonusPak\halite\halite.exe`).
    pub target: Vec<u8>,
    pub size: u32,
    pub date: u16,
    pub time: u16,
    pub attributes: u32,
    /// The number of its .EAS file in `media_dir`, if it has EAs.
    pub ea: Option<u32>,
}

pub struct Object {
    pub class: Vec<u8>,
    pub title: Vec<u8>,
    pub setup: Vec<u8>,
    pub location: Vec<u8>,
}

/// An edit of a text file (CONFIG.SYS, WIN.INI, a program's own file).
pub struct Edit {
    pub file: Vec<u8>,
    /// The line to change, its start (`LIBPATH=`, `SET HELP=`), or a
    /// pattern for the line (`RUN=.*\\LW731B.EXE`).
    pub edit_line: Vec<u8>,
    /// What to look for: in a list (with a delimiter), the entry; else
    /// the pattern of the line.
    pub search: Vec<u8>,
    pub new_line: Vec<u8>,
    pub delimiter: Option<u8>,
    pub add_after: Vec<u8>,
    pub add_before: Vec<u8>,
    /// What to do when a line is found: 1 replace it with new_line (an
    /// empty new_line removes it), 2 leave it.
    pub found: u32,
    /// When none is: 1 nothing, 2 add new_line at the end, 4 after
    /// add_after (or before add_before).
    pub not_found: u32,
}

pub struct Prf {
    pub file: Vec<u8>,
    pub app: Vec<u8>,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

/// A program the install runs: CallTime 9 at the end, earlier ones after
/// the files are copied.
pub struct Exit {
    pub program: Vec<u8>,
    pub args: Vec<u8>,
    pub at_end: bool,
}

pub struct Package {
    pub id: Vec<u8>,
    pub files: Vec<File>,
    pub objects: Vec<Object>,
    pub classes: Vec<(Vec<u8>, Vec<u8>)>,
    pub edits: Vec<Edit>,
    pub prfs: Vec<Prf>,
    pub exits: Vec<Exit>,
    /// Packages this one brings (Dependency: FeatureID when WhenFeatureID).
    pub needs: Vec<Vec<u8>>,
}

impl Package {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size as u64).sum()
    }
}

/// `6-14-16` (month, day, year from 1980) and `11:59:0` (hours, minutes,
/// two-second units) as FDATE and FTIME.
fn fdate(s: &[u8]) -> u16 {
    let p: Vec<u32> = s.split(|&c| c == b'-').map(dec_parse).collect();
    if p.len() != 3 || p[0] == 0 {
        return 0;
    }
    ((p[2].min(127) << 9) | (p[0].min(12) << 5) | p[1].min(31)) as u16
}

fn ftime(s: &[u8]) -> u16 {
    let p: Vec<u32> = s.split(|&c| c == b':').map(dec_parse).collect();
    if p.len() != 3 {
        return 0;
    }
    ((p[0].min(23) << 11) | (p[1].min(59) << 5) | p[2].min(29)) as u16
}

/// Read a package from its block. `ea_next` counts the .EAS files used so
/// far in each media directory (shared between packages read in order).
pub fn package(b: &Block, vars: &Vars, ea_next: &mut Vec<(Vec<u8>, u32)>) -> Package {
    let mut v = vars.clone();
    v.add_block(b);
    let mut p = Package {
        id: b.kind.clone(),
        files: Vec::new(),
        objects: Vec::new(),
        classes: Vec::new(),
        edits: Vec::new(),
        prfs: Vec::new(),
        exits: Vec::new(),
        needs: Vec::new(),
    };
    for f in b.of(b"File") {
        let media_dir = v.expand(f.val(b"MediaPath"));
        let ea = if f.num(b"EAMediaIndex") != 0 {
            let slot = match ea_next.iter().position(|(d, _)| d.eq_ignore_ascii_case(&media_dir)) {
                Some(i) => i,
                None => {
                    ea_next.push((media_dir.clone(), 0));
                    ea_next.len() - 1
                }
            };
            let n = ea_next[slot].1;
            ea_next[slot].1 += 1;
            Some(n)
        } else {
            None
        };
        let mut target = v.path(f.val(b"TargetPath"));
        if !target.ends_with(b"\\") {
            target.push(b'\\');
        }
        target.extend_from_slice(&v.expand(f.val(b"TargetFileName")));
        p.files.push(File {
            media_dir,
            media_name: v.expand(f.val(b"MediaFileName")),
            target,
            size: f.num(b"CBFile"),
            date: fdate(f.val(b"LastWriteDate")),
            time: ftime(f.val(b"LastWriteTime")),
            attributes: f.num(b"AttrFile") & 0x27,
            ea,
        });
    }
    for o in b.of(b"ObjectCreation") {
        // The title's parts are its lines.
        let mut title = v.expand(o.val(b"ObjectTitle[0]"));
        let second = v.expand(o.val(b"ObjectTitle[1]"));
        if !second.is_empty() {
            title.push(b'\n');
            title.extend_from_slice(&second);
        }
        p.objects.push(Object { class: v.expand(o.val(b"ClassName")), title, setup: v.expand(o.val(b"SetupString")), location: v.expand(o.val(b"Location")) });
    }
    for c in b.of(b"ClassRegistration") {
        p.classes.push((v.expand(c.val(b"ClassName")), v.path(c.val(b"DLLName"))));
    }
    for a in b.of(b"AsciiFile") {
        let delim = v.expand(a.val(b"Delimiter"));
        p.edits.push(Edit {
            file: v.path(a.val(b"FileName")),
            edit_line: v.expand(a.val(b"EditLine")),
            search: v.expand(a.val(b"SearchLine")),
            new_line: v.expand(a.val(b"NewLine")),
            delimiter: if a.num(b"DelimChk") != 0 { delim.first().copied() } else { None },
            add_after: v.expand(a.val(b"AddAfterLine")),
            add_before: v.expand(a.val(b"AddBeforeLine")),
            found: a.num(b"SearchFound"),
            not_found: a.num(b"SearchNotFound"),
        });
    }
    for r in b.of(b"Os2PrfIni") {
        p.prfs.push(Prf { file: v.path(r.val(b"FileName")), app: v.expand(r.val(b"Application")), key: v.expand(r.val(b"Key")), value: v.expand(r.val(b"Value")) });
    }
    for x in b.of(b"UserExits") {
        // ExitType 1: a program, its arguments in ExitData.
        if x.num(b"ExitType") == 1 {
            p.exits.push(Exit { program: v.path(x.val(b"FilePath")), args: v.expand(x.val(b"ExitData")), at_end: x.num(b"CallTime") >= 9 });
        }
    }
    for d in b.of(b"Dependency") {
        // Condition 3: when WhenFeatureID is installed, so is FeatureID.
        if d.num(b"Condition") == 3 && d.val(b"WhenFeatureID").eq_ignore_ascii_case(&p.id) {
            p.needs.push(d.val(b"FeatureID").to_vec());
        }
    }
    p
}

/// A 16-bit FEALIST (an .EAS file: cbList, then fEA, cbName, cbValue,
/// name, NUL, value) as a FEA2LIST for DosSetPathInfo.
pub fn fea2list(old: &[u8]) -> Option<Vec<u8>> {
    if old.len() < 4 {
        return None;
    }
    let end = (u32::from_le_bytes([old[0], old[1], old[2], old[3]]) as usize).min(old.len());
    let mut out = alloc::vec![0u8; 4];
    let mut last: Option<usize> = None;
    let mut i = 4;
    while i + 4 <= end {
        let (flags, cb_name) = (old[i], old[i + 1] as usize);
        let cb_value = u16::from_le_bytes([old[i + 2], old[i + 3]]) as usize;
        let name = old.get(i + 4..i + 4 + cb_name)?;
        let value = old.get(i + 5 + cb_name..i + 5 + cb_name + cb_value)?;
        i += 5 + cb_name + cb_value;
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let at = out.len();
        if let Some(l) = last {
            let off = (at - l) as u32;
            out[l..l + 4].copy_from_slice(&off.to_le_bytes());
        }
        last = Some(at);
        out.extend_from_slice(&[0, 0, 0, 0, flags, cb_name as u8]);
        out.extend_from_slice(&(cb_value as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.push(0);
        out.extend_from_slice(value);
    }
    last?;
    let n = out.len() as u32;
    out[0..4].copy_from_slice(&n.to_le_bytes());
    Some(out)
}
