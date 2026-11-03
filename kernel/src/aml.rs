//! Just enough of the ACPI namespace to route PCI INTx interrupts: the
//! `_PRT` (PCI routing table) objects of the PCI root bridges and bridges
//! in the DSDT and SSDTs, and the interrupts of the link devices they
//! name.
//!
//! This is not an AML interpreter. The definition blocks are walked as
//! a tree of Scope/Device/Name/Method objects; data objects (integers,
//! strings, buffers, packages) are decoded, methods are not run. A
//! `_PRT` that is a method (QEMU's q35: `If (PICF) { Return (PRTA) }
//! Return (PRTP)`) is looked into for the packages it returns, and the
//! one meant for APIC mode is chosen: the one whose entries give GSIs
//! directly or name link devices whose current resources (`_CRS`, else
//! the possible ones, `_PRS`) are interrupts of 16 and above. A link
//! device whose `_CRS` is computed (the PIIX's LNKA-D, which read the
//! chipset's PIRQ registers) falls back to the PCI interrupt line the
//! firmware programmed, which on those chipsets is the ISA interrupt the
//! PIRQ is wired to. Objects the walker does not understand end the scope
//! they are in; the rest of the namespace is still seen.

use alloc::string::String;
use alloc::vec::Vec;

/// A decoded data object.
#[derive(Clone, Debug)]
enum Obj {
    Int(u64),
    Str(String),
    Buffer(Vec<u8>),
    Package(Vec<Obj>),
    /// A name in a package (a reference).
    Name(String),
    /// A method: its body.
    Method(Vec<u8>),
    Device,
    Other,
}

struct Walker {
    objs: Vec<(String, Obj)>,
}

fn pkg_length(b: &[u8], p: &mut usize) -> Option<usize> {
    let lead = *b.get(*p)?;
    let n = (lead >> 6) as usize;
    let mut len = if n == 0 { (lead & 0x3f) as usize } else { (lead & 0x0f) as usize };
    for i in 0..n {
        len |= (*b.get(*p + 1 + i)? as usize) << (4 + 8 * i);
    }
    let start = *p;
    *p += 1 + n;
    Some(start + len)
}

fn name_seg(b: &[u8], p: &mut usize) -> Option<String> {
    let s = b.get(*p..*p + 4)?;
    if !(s[0].is_ascii_uppercase() || s[0] == b'_') {
        return None;
    }
    *p += 4;
    Some(s.iter().map(|&c| c as char).collect())
}

/// A NameString: (prefix "\\" or "^"s, segments).
fn name_string(b: &[u8], p: &mut usize) -> Option<(String, Vec<String>)> {
    let mut prefix = String::new();
    while let Some(&c) = b.get(*p) {
        if c == b'\\' || c == b'^' {
            prefix.push(c as char);
            *p += 1;
        } else {
            break;
        }
    }
    let mut segs = Vec::new();
    match *b.get(*p)? {
        0x00 => *p += 1,
        0x2e => {
            *p += 1;
            segs.push(name_seg(b, p)?);
            segs.push(name_seg(b, p)?);
        }
        0x2f => {
            let n = *b.get(*p + 1)? as usize;
            *p += 2;
            for _ in 0..n {
                segs.push(name_seg(b, p)?);
            }
        }
        _ => segs.push(name_seg(b, p)?),
    }
    Some((prefix, segs))
}

/// The absolute path of a name used in scope `scope`.
fn resolve(scope: &str, prefix: &str, segs: &[String]) -> String {
    let mut base: Vec<&str> = if prefix.starts_with('\\') { Vec::new() } else { scope.split('.').filter(|s| !s.is_empty()).collect() };
    for _ in prefix.chars().filter(|&c| c == '^') {
        base.pop();
    }
    let mut v: Vec<String> = base.iter().map(|s| String::from(*s)).collect();
    v.extend(segs.iter().cloned());
    v.join(".")
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_uppercase() || c == b'_' || c == b'\\' || c == b'^' || c == 0x2e || c == 0x2f
}

/// A data object (DataRefObject or a package element).
fn data(b: &[u8], p: &mut usize, scope: &str) -> Option<Obj> {
    let op = *b.get(*p)?;
    *p += 1;
    Some(match op {
        0x00 => Obj::Int(0),
        0x01 => Obj::Int(1),
        0xff => Obj::Int(u64::MAX),
        0x0a => {
            *p += 1;
            Obj::Int(*b.get(*p - 1)? as u64)
        }
        0x0b => {
            *p += 2;
            Obj::Int(u16::from_le_bytes([*b.get(*p - 2)?, *b.get(*p - 1)?]) as u64)
        }
        0x0c => {
            let v = b.get(*p..*p + 4)?;
            *p += 4;
            Obj::Int(u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as u64)
        }
        0x0e => {
            let v = b.get(*p..*p + 8)?;
            *p += 8;
            let mut a = [0u8; 8];
            a.copy_from_slice(v);
            Obj::Int(u64::from_le_bytes(a))
        }
        0x0d => {
            let mut s = String::new();
            while *b.get(*p)? != 0 {
                s.push(b[*p] as char);
                *p += 1;
            }
            *p += 1;
            Obj::Str(s)
        }
        0x11 => {
            let end = pkg_length(b, p)?;
            let size = match data(b, p, scope)? {
                Obj::Int(n) => n as usize,
                _ => return None,
            };
            let bytes = b.get(*p..end)?;
            *p = end;
            let mut v = bytes.to_vec();
            v.resize(size.max(v.len()), 0);
            Obj::Buffer(v)
        }
        0x12 | 0x13 => {
            let end = pkg_length(b, p)?;
            if op == 0x12 {
                *p += 1;
            } else {
                data(b, p, scope)?;
            }
            let mut v = Vec::new();
            while *p < end {
                let c = *b.get(*p)?;
                if is_name_start(c) {
                    let (pre, segs) = name_string(b, p)?;
                    v.push(Obj::Name(if pre.is_empty() && segs.len() == 1 { segs[0].clone() } else { resolve(scope, &pre, &segs) }));
                } else {
                    match data(b, p, scope) {
                        Some(o) => v.push(o),
                        None => {
                            *p = end;
                            v.push(Obj::Other);
                        }
                    }
                }
            }
            *p = end;
            Obj::Package(v)
        }
        0x5b if *b.get(*p)? == 0x30 => {
            *p += 1;
            Obj::Int(2)
        }
        _ => return None,
    })
}

impl Walker {
    /// Walk a TermList from `p` to `end` in `scope`.
    fn terms(&mut self, b: &[u8], mut p: usize, end: usize, scope: &str) {
        while p < end {
            if self.term(b, &mut p, end, scope).is_none() {
                return;
            }
        }
    }

    fn term(&mut self, b: &[u8], p: &mut usize, _end: usize, scope: &str) -> Option<()> {
        let op = *b.get(*p)?;
        *p += 1;
        match op {
            // Scope.
            0x10 => {
                let end = pkg_length(b, p)?;
                let (pre, segs) = name_string(b, p)?;
                let path = resolve(scope, &pre, &segs);
                self.terms(b, *p, end, &path);
                *p = end;
            }
            // Name.
            0x08 => {
                let (pre, segs) = name_string(b, p)?;
                let path = resolve(scope, &pre, &segs);
                let o = data(b, p, &path)?;
                self.objs.push((path, o));
            }
            // Method.
            0x14 => {
                let end = pkg_length(b, p)?;
                let (pre, segs) = name_string(b, p)?;
                let path = resolve(scope, &pre, &segs);
                *p += 1;
                self.objs.push((path, Obj::Method(b.get(*p..end)?.to_vec())));
                *p = end;
            }
            // Alias.
            0x06 => {
                name_string(b, p)?;
                name_string(b, p)?;
            }
            // External.
            0x15 => {
                name_string(b, p)?;
                *p += 2;
            }
            // If, Else, While: conditional definitions are not seen.
            0xa0 | 0xa1 | 0xa2 => {
                *p = pkg_length(b, p)?;
            }
            0x5b => {
                let ext = *b.get(*p)?;
                *p += 1;
                match ext {
                    // Device.
                    0x82 => {
                        let end = pkg_length(b, p)?;
                        let (pre, segs) = name_string(b, p)?;
                        let path = resolve(scope, &pre, &segs);
                        self.objs.push((path.clone(), Obj::Device));
                        self.terms(b, *p, end, &path);
                        *p = end;
                    }
                    // Processor, PowerResource, ThermalZone, Field,
                    // IndexField, BankField.
                    0x83 | 0x84 | 0x85 | 0x81 | 0x86 | 0x87 => {
                        *p = pkg_length(b, p)?;
                    }
                    // Mutex.
                    0x01 => {
                        name_string(b, p)?;
                        *p += 1;
                    }
                    // Event.
                    0x02 => {
                        name_string(b, p)?;
                    }
                    // OperationRegion: name, space, offset, length.
                    0x80 => {
                        name_string(b, p)?;
                        *p += 1;
                        data(b, p, scope)?;
                        data(b, p, scope)?;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn get(&self, path: &str) -> Option<&Obj> {
        self.objs.iter().find(|(p, _)| p == path).map(|(_, o)| o)
    }

    /// A single-segment name used in `scope`: searched there and in the
    /// scopes above (ACPI's rule), else anywhere by its last segment.
    fn lookup(&self, scope: &str, name: &str) -> Option<(String, &Obj)> {
        if name.contains('.') {
            return self.get(name).map(|o| (String::from(name), o));
        }
        let mut s: Vec<&str> = scope.split('.').filter(|x| !x.is_empty()).collect();
        loop {
            let mut p: Vec<&str> = s.clone();
            p.push(name);
            let path = p.join(".");
            if let Some(o) = self.get(&path) {
                return Some((path, o));
            }
            if s.pop().is_none() {
                break;
            }
        }
        self.objs.iter().find(|(p, _)| p.rsplit('.').next() == Some(name)).map(|(p, o)| (p.clone(), o))
    }
}

/// How a PCI interrupt pin reaches the IOAPIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gsi {
    pub gsi: u32,
    pub level: bool,
    pub active_low: bool,
}

/// One `_PRT` entry: device, pin (0 = INTA) and where it goes; None:
/// a link device with computed resources (use the interrupt line).
#[derive(Clone, Debug)]
struct Entry {
    dev: u8,
    pin: u8,
    to: Option<Gsi>,
}

/// The routing tables: (bus number, entries).
pub struct Routing {
    tables: Vec<(u8, Vec<Entry>)>,
}

/// The first interrupt of a resource template: IRQ or Extended Interrupt
/// descriptor.
fn interrupt_of(res: &[u8]) -> Option<Gsi> {
    let mut i = 0;
    while i < res.len() {
        let t = res[i];
        if t & 0x80 == 0 {
            let (kind, len) = ((t >> 3) & 0xf, (t & 7) as usize);
            if kind == 0xf {
                break;
            }
            if kind == 4 && len >= 2 {
                let mask = u16::from_le_bytes([*res.get(i + 1)?, *res.get(i + 2)?]);
                let flags = if len >= 3 { res[i + 3] } else { 1 };
                if mask != 0 {
                    // Bit 0: edge triggered; bit 3: active low.
                    return Some(Gsi { gsi: mask.trailing_zeros(), level: flags & 1 == 0, active_low: flags & 8 != 0 });
                }
            }
            i += 1 + len;
        } else {
            let len = u16::from_le_bytes([*res.get(i + 1)?, *res.get(i + 2)?]) as usize;
            if t & 0x7f == 0x09 && len >= 6 {
                let flags = res[i + 3];
                let n = res[i + 4];
                if n > 0 {
                    let g = u32::from_le_bytes([res[i + 5], res[i + 6], res[i + 7], res[i + 8]]);
                    // Bit 1: edge; bit 2: active low.
                    return Some(Gsi { gsi: g, level: flags & 2 == 0, active_low: flags & 4 != 0 });
                }
            }
            i += 3 + len;
        }
    }
    None
}

impl Walker {
    /// A link device's interrupt: `_CRS` if it is data, else `_PRS`'s
    /// first when it offers a single one; None when computed.
    fn link(&self, scope: &str, name: &str) -> Option<Gsi> {
        let (path, _) = self.lookup(scope, name)?;
        for res in ["_CRS", "_PRS"] {
            if let Some(Obj::Buffer(b)) = self.get(&alloc::format!("{path}.{res}")) {
                if let Some(g) = interrupt_of(b) {
                    // _PRS with several choices: the firmware's pick is not
                    // known.
                    if res == "_PRS" && count_interrupts(b) != 1 {
                        return None;
                    }
                    return Some(g);
                }
            }
        }
        None
    }

    /// The entries of a `_PRT` package.
    fn entries(&self, scope: &str, pkg: &[Obj]) -> Vec<Entry> {
        let mut v = Vec::new();
        for e in pkg {
            let Obj::Package(f) = e else { continue };
            let (Some(Obj::Int(adr)), Some(Obj::Int(pin))) = (f.first(), f.get(1)) else { continue };
            let to = match (f.get(2), f.get(3)) {
                (Some(Obj::Int(0)), Some(Obj::Int(gsi))) => Some(Gsi { gsi: *gsi as u32, level: true, active_low: true }),
                (Some(Obj::Name(n)), _) => self.link(scope, n),
                _ => None,
            };
            v.push(Entry { dev: (adr >> 16) as u8, pin: *pin as u8, to });
        }
        v
    }

    /// The packages a `_PRT` method returns (`Return (NAME)` or
    /// `Return (Package {...})`).
    fn returned(&self, scope: &str, body: &[u8]) -> Vec<Vec<Obj>> {
        let mut v = Vec::new();
        let mut i = 0;
        while i + 1 < body.len() {
            if body[i] == 0xa4 {
                let mut p = i + 1;
                if body[p] == 0x12 {
                    if let Some(Obj::Package(x)) = data(body, &mut p, scope) {
                        v.push(x);
                    }
                } else if is_name_start(body[p]) {
                    if let Some((pre, segs)) = name_string(body, &mut p) {
                        let n = if pre.is_empty() && segs.len() == 1 { segs[0].clone() } else { resolve(scope, &pre, &segs) };
                        if let Some((_, Obj::Package(x))) = self.lookup(scope, &n) {
                            v.push(x.clone());
                        }
                    }
                }
            }
            i += 1;
        }
        v
    }

    /// The `_PRT` of device `dev` for APIC mode.
    fn prt(&self, dev: &str) -> Option<Vec<Entry>> {
        let o = self.get(&alloc::format!("{dev}._PRT"))?;
        let candidates = match o {
            Obj::Package(p) => alloc::vec![p.clone()],
            Obj::Method(body) => self.returned(dev, body),
            _ => return None,
        };
        let tables: Vec<Vec<Entry>> = candidates.iter().map(|c| self.entries(dev, c)).filter(|e| !e.is_empty()).collect();
        // APIC mode: the table with GSIs beyond the ISA range, else the
        // one that resolves best.
        let score = |t: &Vec<Entry>| {
            let high = t.iter().filter(|e| e.to.is_some_and(|g| g.gsi >= 16)).count();
            let known = t.iter().filter(|e| e.to.is_some()).count();
            (high, known)
        };
        tables.into_iter().max_by_key(score)
    }
}

fn count_interrupts(res: &[u8]) -> u32 {
    let mut i = 0;
    let mut n = 0;
    while i < res.len() {
        let t = res[i];
        if t & 0x80 == 0 {
            let (kind, len) = ((t >> 3) & 0xf, (t & 7) as usize);
            if kind == 0xf {
                break;
            }
            if kind == 4 && len >= 2 && i + 2 < res.len() {
                n += u16::from_le_bytes([res[i + 1], res[i + 2]]).count_ones();
            }
            i += 1 + len;
        } else {
            if i + 2 >= res.len() {
                break;
            }
            let len = u16::from_le_bytes([res[i + 1], res[i + 2]]) as usize;
            if t & 0x7f == 0x09 && i + 4 < res.len() {
                n += res[i + 4] as u32;
            }
            i += 3 + len;
        }
    }
    n
}

const PNP0A03: u64 = 0x030a_d041;
const PNP0A08: u64 = 0x080a_d041;

impl Routing {
    /// Build the routing tables from AML definition blocks (DSDT first,
    /// then the SSDTs; each without its 36-byte header).
    pub fn from_aml(blocks: &[&[u8]]) -> Routing {
        let mut w = Walker { objs: Vec::new() };
        for b in blocks {
            w.terms(b, 0, b.len(), "");
        }
        let mut tables = Vec::new();
        let devices: Vec<String> = w.objs.iter().filter(|(_, o)| matches!(o, Obj::Device)).map(|(p, _)| p.clone()).collect();
        let int_of = |path: &str| match w.get(path) {
            Some(Obj::Int(v)) => Some(*v),
            _ => None,
        };
        let is_root = |d: &str| {
            [alloc::format!("{d}._HID"), alloc::format!("{d}._CID")].iter().any(|k| match w.get(k) {
                Some(Obj::Int(v)) => *v == PNP0A03 || *v == PNP0A08,
                Some(Obj::Str(s)) => s == "PNP0A03" || s == "PNP0A08",
                _ => false,
            })
        };
        for root in devices.iter().filter(|d| is_root(d)) {
            let bus = int_of(&alloc::format!("{root}._BBN")).unwrap_or(0) as u8;
            if let Some(e) = w.prt(root) {
                tables.push((bus, e));
            }
            // Bridges below the root with their own _PRT.
            let prefix = alloc::format!("{root}.");
            for br in devices.iter().filter(|d| d.starts_with(&prefix) && !d[prefix.len()..].contains('.')) {
                let Some(adr) = int_of(&alloc::format!("{br}._ADR")) else { continue };
                let Some(e) = w.prt(br) else { continue };
                let (dev, func) = ((adr >> 16) as u8, (adr & 0xff) as u8);
                let id = crate::drivers::pci::read32(bus, dev, func, 0);
                if id & 0xffff == 0xffff {
                    continue;
                }
                let secondary = (crate::drivers::pci::read32(bus, dev, func, 0x18) >> 8) as u8;
                if secondary != 0 {
                    tables.push((secondary, e));
                }
            }
        }
        Routing { tables }
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    pub fn entries(&self) -> usize {
        self.tables.iter().map(|t| t.1.len()).sum()
    }

    /// Where `pin` (1 = INTA, as the configuration space has it) of
    /// bus:dev reaches the interrupt controller: the bus's table, else
    /// through the bridges above (swizzling the pin by device number).
    /// `line` is the interrupt line the firmware programmed (used for link
    /// devices with computed resources). None if unknown.
    pub fn route(&self, bus: u8, dev: u8, pin: u8, line: u8) -> Option<Gsi> {
        if pin == 0 || pin > 4 {
            return None;
        }
        let (mut bus, mut dev, mut pin) = (bus, dev, pin - 1);
        for _ in 0..8 {
            if let Some((_, t)) = self.tables.iter().find(|(b, _)| *b == bus) {
                let e = t.iter().find(|e| e.dev == dev && e.pin == pin)?;
                return Some(e.to.unwrap_or(Gsi { gsi: line as u32, level: true, active_low: false })).filter(|g| g.gsi != 0xff);
            }
            // Up through the bridge whose secondary bus this is.
            let (pbus, pdev) = bridge_of(bus)?;
            pin = (pin + dev) % 4;
            bus = pbus;
            dev = pdev;
        }
        None
    }
}

/// The PCI-to-PCI bridge (bus, device) whose secondary bus is `bus`.
fn bridge_of(bus: u8) -> Option<(u8, u8)> {
    if bus == 0 {
        return None;
    }
    for b in 0..bus {
        for d in 0..32u8 {
            let id = crate::drivers::pci::read32(b, d, 0, 0);
            if id & 0xffff == 0xffff {
                continue;
            }
            let multi = crate::drivers::pci::read32(b, d, 0, 0x0c) >> 16 & 0x80 != 0;
            for f in 0..if multi { 8 } else { 1 } {
                let hdr = (crate::drivers::pci::read32(b, d, f, 0x0c) >> 16) & 0x7f;
                if hdr == 1 && (crate::drivers::pci::read32(b, d, f, 0x18) >> 8) as u8 == bus {
                    return Some((b, d));
                }
            }
        }
    }
    None
}
