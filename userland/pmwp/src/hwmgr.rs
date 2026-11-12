//! The Hardware Manager (WPHwManager, `<WP_HWMGR>` in System Setup) and
//! its device objects (WPDevice and its classes), as IBM's PNP.DLL has
//! them (studied with Ghidra):
//!
//! - WPHwManager is a folder. Populating it asks the resource manager
//!   (the kernel's `RESMGR$`, kernel/src/resmgr.rs, the IOCtls IBM's
//!   RMINFO.DLL makes) for the physical view and the drivers, and makes
//!   a transient device object for each adapter and device it does not
//!   have yet (by the node's handle, its UNIQUEID; the node above is its
//!   PARENTID), of the class IBM's DetermineNameAndClass picks: by the
//!   adapter's base type and subtype (keyboard, mouse, serial, parallel,
//!   display, audio, memory, timer, system peripheral, hard disk or
//!   diskette controller), by its name (`CPU_`, `ISA_`, `PCI_`, `MCA_`
//!   name a processor or a bus), by the device's type and name (`HD_`,
//!   `DSKT_`, `MOUSE_`, `KBD_`, CD-ROM, audio); planar chipset devices
//!   are left out. The title is the node's description.
//! - Its details view (the default view, as IBM's) shows each device's
//!   type, interrupts, DMA channels, I/O ports and memory; its tree view
//!   shows the devices under the adapters and buses they are on.
//! - A device opens on its settings: the Device page (the driver: its
//!   name, description, vendor, version and date) and the IRQ, DMA, I/O
//!   and Memory pages, with what the resource manager says it uses.
//!
//! The device classes' private methods (wpDevicePrivate1-52) are IBM's
//! wpGetUniqueID, wpGetParentID, wpGetNodeType ... in its release order;
//! the ones that answer questions are implemented.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_DETAILS, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::{cstr, z};
use crate::wps::{self, Impl};
use pmwin::hwicons as hi;

// ---- The resource manager (RESMGR$, IBM's RMINFO structures) ----

const RMTYPE_ADAPTER: u32 = 0;
const RMTYPE_DEVICE: u32 = 1;

const RS_EXCLUSIVE: u16 = 1;
const RS_SHARED: u16 = 4;

#[derive(Clone, Copy)]
pub enum Res {
    Io { base: u16, len: u16, flags: u16, lines: u16 },
    Irq { level: u16, pin: u16, flags: u16 },
    Mem { base: u32, size: u32, flags: u16 },
    Dma { channel: u16, flags: u16 },
}

/// A node of the physical view.
struct Node {
    handle: u32,
    depth: u32,
    /// RMTYPE_ADAPTER or RMTYPE_DEVICE.
    ty: u32,
    /// An adapter's base type and subtype; a device's type in `base`.
    base: u16,
    sub: u16,
    desc: Vec<u8>,
    driver: u32,
    res: Vec<Res>,
}

/// A driver node.
#[derive(Clone, Default)]
struct Driver {
    handle: u32,
    name: Vec<u8>,
    desc: Vec<u8>,
    vendor: Vec<u8>,
    version: (u8, u8),
    date: (u16, u8, u8),
}

fn rm_open() -> Option<u32> {
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYNONE | OPEN_ACCESS_READWRITE
    (unsafe { os2::DosOpen(b"RESMGR$\0".as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, core::ptr::null_mut()) } == 0).then_some(h)
}

/// RM_ENUM_NODES: (handle, depth) of a view's nodes.
fn enum_nodes(h: u32, cmd: u16) -> Vec<(u32, u32)> {
    let mut p = cmd.to_le_bytes();
    let mut n = 16usize;
    loop {
        let mut d = alloc::vec![0u8; 4 + n * 8];
        let (mut pl, mut dl) = (2u32, d.len() as u32);
        let rc = unsafe { os2::DosDevIOCtl(h, 0x80, 2, p.as_mut_ptr(), 2, &mut pl, d.as_mut_ptr(), d.len() as u32, &mut dl) };
        let count = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
        if rc != 0 && count > n && n < 4096 {
            n = count;
            continue;
        }
        let rd = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        return (0..count.min(n)).map(|i| (rd(4 + i * 8), rd(8 + i * 8))).collect();
    }
}

/// RM_GET_NODEINFO: the node's data (RMNodeSize, RMNODE, ...) in a
/// buffer whose address the pointers in it are relative to.
fn node_info(h: u32, handle: u32) -> Option<Vec<u8>> {
    let mut size = 0x400u32;
    for _ in 0..3 {
        let mut d = alloc::vec![0u8; size as usize];
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&handle.to_le_bytes());
        p[4..8].copy_from_slice(&(d.as_ptr() as u32).to_le_bytes());
        let (mut pl, mut dl) = (8u32, size);
        let rc = unsafe { os2::DosDevIOCtl(h, 0x80, 1, p.as_mut_ptr(), 8, &mut pl, d.as_mut_ptr(), size, &mut dl) };
        let need = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        if rc == 0 {
            return Some(d);
        }
        if need <= size {
            return None;
        }
        size = need;
    }
    None
}

/// Reading a node's data: words at offsets, strings at the (linear)
/// addresses it holds.
struct Info<'a> {
    b: &'a [u8],
    base: u32,
}

impl Info<'_> {
    fn u8(&self, o: usize) -> u8 {
        self.b.get(o).copied().unwrap_or(0)
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.u8(o), self.u8(o + 1)])
    }
    fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.u8(o), self.u8(o + 1), self.u8(o + 2), self.u8(o + 3)])
    }
    /// The offset in the buffer of a linear address.
    fn at(&self, p: u32) -> Option<usize> {
        p.checked_sub(self.base).map(|o| o as usize).filter(|&o| o < self.b.len())
    }
    fn str(&self, p: u32) -> Vec<u8> {
        let Some(o) = self.at(p) else { return Vec::new() };
        let s = &self.b[o..];
        let t = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
        // IBM's Hardware Manager trims the names' trailing blanks.
        let n = t.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
        t[..n].to_vec()
    }
}

/// The resource list of a node.
fn resources(i: &Info, p: u32) -> Vec<Res> {
    let Some(o) = i.at(p) else { return Vec::new() };
    let n = i.u32(o).min(256) as usize;
    let mut v = Vec::new();
    for k in 0..n {
        let e = o + 4 + k * 20;
        let w = |x: usize| i.u16(e + 4 + 2 * x);
        v.push(match i.u32(e) {
            1 => Res::Io { base: w(0), len: w(1), flags: w(2), lines: w(3) },
            2 => Res::Irq { level: w(0), pin: w(1), flags: w(2) },
            3 => Res::Mem { base: i.u32(e + 4), size: i.u32(e + 8), flags: w(4) },
            4 => Res::Dma { channel: w(0), flags: w(1) },
            _ => continue,
        });
    }
    v
}

/// The physical view and the drivers, as the resource manager has them
/// now.
fn snapshot() -> (Vec<Node>, Vec<Driver>) {
    let Some(h) = rm_open() else { return (Vec::new(), Vec::new()) };
    let mut nodes = Vec::new();
    for (handle, depth) in enum_nodes(h, 0) {
        let Some(b) = node_info(h, handle) else { continue };
        // RM_GETNODE_DATA: RMNodeSize, then RMNODE: VersionInfo, NodeType,
        // DriverHandle, the node's structure, its resource list.
        let i = Info { b: &b, base: b.as_ptr() as u32 };
        let ty = i.u32(8);
        let Some(s) = i.at(i.u32(16)) else { continue };
        let (base, sub) = match ty {
            RMTYPE_ADAPTER => (i.u16(s + 6), i.u16(s + 8)),
            RMTYPE_DEVICE => (i.u16(s + 6), 0),
            _ => continue,
        };
        nodes.push(Node { handle, depth, ty, base, sub, desc: i.str(i.u32(s)), driver: i.u32(12), res: resources(&i, i.u32(20)) });
    }
    let mut drivers = Vec::new();
    for (handle, depth) in enum_nodes(h, 2) {
        if depth != 0 {
            continue;
        }
        let Some(b) = node_info(h, handle) else { continue };
        let i = Info { b: &b, base: b.as_ptr() as u32 };
        let Some(s) = i.at(i.u32(16)) else { continue };
        drivers.push(Driver {
            handle,
            name: i.str(i.u32(s)),
            desc: i.str(i.u32(s + 4)),
            vendor: i.str(i.u32(s + 8)),
            version: (i.u8(s + 0x0c), i.u8(s + 0x0d)),
            date: (i.u16(s + 0x0e), i.u8(s + 0x10), i.u8(s + 0x11)),
        });
    }
    unsafe { os2::DosClose(h) };
    (nodes, drivers)
}

/// The class of a node's object, as IBM's DetermineNameAndClass picks
/// it; None for nodes that get none (planar chipset devices).
fn class_for(n: &Node) -> Option<&'static str> {
    let starts = |p: &[u8]| n.desc.len() >= p.len() && n.desc[..p.len()].eq_ignore_ascii_case(p);
    Some(match n.ty {
        RMTYPE_ADAPTER => match (n.base, n.sub) {
            (1, 1 | 2) => "WPDevHarddrive",
            (1, 3) => "WPDevDiskette",
            (2 | 6 | 0x0a..=0x0d, _) => "WPDevice",
            (3, _) => "WPDevDisplay",
            (4, 2) => "WPDevAudio",
            (5, _) => "WPDevMemory",
            (7, 1) => "WPDevSerial",
            (7, 2) => "WPDevParallel",
            (8, 1 | 2) => "WPDevPeriph",
            (8, 3 | 4) => "WPDevTimer",
            (9, 1) => "WPDevKeyboard",
            (9, 3) => "WPDevMouse",
            (1 | 4 | 7 | 8 | 9, _) => "WPDevice",
            _ if starts(b"CPU_") => "WPDevCPU",
            _ if starts(b"ISA_") || starts(b"PCI_") || starts(b"MCA_") => "WPDevBus",
            _ => "WPDevice",
        },
        _ => match n.base {
            0 if starts(b"HD_") => "WPDevHarddrive",
            0 if starts(b"DSKT_") => "WPDevDiskette",
            1 => "WPDevTape",
            5 => "WPDevCDRom",
            0x0e => return None,
            0x0f if starts(b"MOUSE_") => "WPDevMouse",
            0x0f if starts(b"KBD_") => "WPDevKeyboard",
            0x10 => "WPDevAudio",
            _ => "WPDevice",
        },
    })
}

// ---- Device objects ----

/// What a device object knows (WPDevice's instance data points at it).
pub struct DevInfo {
    unique: u32,
    parent: u32,
    node_type: u32,
    base: u16,
    sub: u16,
    driver: Driver,
    res: Vec<Res>,
    /// The details view's texts (with NULs): type, IRQ, DMA, I/O, memory.
    texts: [Vec<u8>; 5],
    /// What the getter methods hand out (with NULs): driver file, name,
    /// vendor, version, date.
    strings: [Vec<u8>; 5],
}

#[repr(C)]
struct DevData {
    info: *mut DevInfo,
}

fn dd<'a>(o: usize) -> &'a mut DevData {
    som::data(wps::ccd("WPDevice"), o)
}

fn info<'a>(o: usize) -> Option<&'a mut DevInfo> {
    if o == 0 || !wps::is(o, "WPDevice") {
        return None;
    }
    let p = dd(o).info;
    (!p.is_null()).then(|| unsafe { &mut *p })
}

fn info_or_new<'a>(o: usize) -> &'a mut DevInfo {
    if dd(o).info.is_null() {
        dd(o).info = Box::into_raw(Box::new(DevInfo {
            unique: 0,
            parent: 0,
            node_type: 0,
            base: 0,
            sub: 0,
            driver: Driver::default(),
            res: Vec::new(),
            texts: Default::default(),
            strings: Default::default(),
        }));
    }
    unsafe { &mut *dd(o).info }
}

/// The device objects made: (unique ID, object).
static mut DEVICES: Vec<(u32, usize)> = Vec::new();

#[allow(static_mut_refs)]
fn devices() -> &'static mut Vec<(u32, usize)> {
    unsafe { &mut DEVICES }
}

fn hex4(v: u32) -> Vec<u8> {
    alloc::format!("{v:04X}").into_bytes()
}

fn hex8(v: u32) -> Vec<u8> {
    alloc::format!("{v:08X}").into_bytes()
}

fn excl(flags: u16) -> &'static [u8] {
    if flags & RS_SHARED != 0 {
        b"shared"
    } else if flags & RS_EXCLUSIVE != 0 {
        b"exclusive"
    } else {
        b"not exclusive"
    }
}

/// A resource as a line of its settings page.
fn res_line(r: &Res) -> Vec<u8> {
    let mut s = Vec::new();
    match *r {
        Res::Irq { level, pin, flags } => {
            s.extend_from_slice(alloc::format!("IRQ {level}").as_bytes());
            if (1..=8).contains(&pin) {
                s.extend_from_slice(b", PCI interrupt pin ");
                s.push(b'A' + pin as u8 - 1);
            }
            s.extend_from_slice(b" (");
            s.extend_from_slice(excl(flags));
            s.push(b')');
        }
        Res::Dma { channel, flags } => {
            s.extend_from_slice(alloc::format!("Channel {channel} (").as_bytes());
            s.extend_from_slice(excl(flags));
            s.push(b')');
        }
        Res::Io { base, len, flags, lines } => {
            s.extend_from_slice(b"0x");
            s.extend_from_slice(&hex4(base as u32));
            s.extend_from_slice(b" - 0x");
            s.extend_from_slice(&hex4((base as u32 + len.max(1) as u32 - 1) & 0xffff));
            s.extend_from_slice(alloc::format!(" ({lines} address lines, ").as_bytes());
            s.extend_from_slice(excl(flags));
            s.push(b')');
        }
        Res::Mem { base, size, flags } => {
            s.extend_from_slice(b"0x");
            s.extend_from_slice(&hex8(base));
            s.extend_from_slice(b" - 0x");
            s.extend_from_slice(&hex8(base.wrapping_add(size.max(1) - 1)));
            s.extend_from_slice(b" (");
            s.extend_from_slice(excl(flags));
            s.push(b')');
        }
    }
    s
}

/// The details view's texts of the resources of one kind.
fn res_column(res: &[Res], kind: u8) -> Vec<u8> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for r in res {
        let t = match (*r, kind) {
            (Res::Irq { level, .. }, 0) => alloc::format!("{level}").into_bytes(),
            (Res::Dma { channel, .. }, 1) => alloc::format!("{channel}").into_bytes(),
            (Res::Io { base, len, .. }, 2) => [hex4(base as u32), b"-".to_vec(), hex4((base as u32 + len.max(1) as u32 - 1) & 0xffff)].concat(),
            (Res::Mem { base, size, .. }, 3) => [hex8(base), b"-".to_vec(), hex8(base.wrapping_add(size.max(1) - 1))].concat(),
            _ => continue,
        };
        if !parts.contains(&t) {
            parts.push(t);
        }
    }
    let mut s = parts.join(&b", "[..]);
    s.push(0);
    s
}

fn class_title_of(o: usize) -> Vec<u8> {
    let cls = som::class_of(o);
    cstr(wps::call("M_WPObject", m::M_WPObject::wpclsQueryTitle, &[cls]) as *const u8).to_vec()
}

/// Fill a device object from its node and driver.
fn set_from(o: usize, n: &Node, parent: u32, drivers: &[Driver]) {
    let i = info_or_new(o);
    i.unique = n.handle;
    i.parent = parent;
    i.node_type = n.ty;
    i.base = n.base;
    i.sub = n.sub;
    i.res = n.res.clone();
    i.driver = drivers.iter().find(|d| d.handle == n.driver && n.driver != 0).cloned().unwrap_or_default();
    let d = &i.driver;
    let version = if d.handle != 0 { alloc::format!("{}.{}", d.version.0, d.version.1).into_bytes() } else { Vec::new() };
    let date = if d.date.0 != 0 { alloc::format!("{}/{}/{}", d.date.1, d.date.2, d.date.0).into_bytes() } else { Vec::new() };
    i.strings = [z(&d.name), z(&d.desc), z(&d.vendor), z(&version), z(&date)];
    i.texts = [z(&class_title_of(o)), res_column(&i.res, 0), res_column(&i.res, 1), res_column(&i.res, 2), res_column(&i.res, 3)];
}

/// Make the objects of the devices the folder does not show yet.
fn populate_devices(folder: usize) {
    let (nodes, drivers) = snapshot();
    // Each node's parent: the nearest one before it a level up (IBM's
    // GetParentHandle).
    for (k, n) in nodes.iter().enumerate() {
        let parent = nodes[..k].iter().rev().find(|p| p.depth + 1 == n.depth).map_or(0, |p| p.handle);
        let Some(class) = class_for(n) else { continue };
        if let Some(&(_, o)) = devices().iter().find(|(u, o)| *u == n.handle && obj::d(*o).folder == folder) {
            // Known: brought up to date.
            set_from(o, n, parent, &drivers);
            continue;
        }
        let cls = wps::class(class);
        if cls == 0 {
            continue;
        }
        let title = z(if n.desc.is_empty() { b"Device" } else { &n.desc });
        let style = obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NOLINK | obj::OBJSTYLE_NOCOPY | obj::OBJSTYLE_NODELETE | obj::OBJSTYLE_NORENAME;
        let o = wps::call("M_WPObject", m::M_WPObject::wpclsMakeAwake, &[cls, title.as_ptr() as usize, style as usize, 0, 0, folder, 0]);
        if o == 0 {
            continue;
        }
        set_from(o, n, parent, &drivers);
        obj::d(o).folder = folder;
        devices().push((n.handle, o));
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, o]);
    }
}

// ---- What the folder views ask (view.rs) ----

pub fn is_manager(folder: usize) -> bool {
    folder != 0 && wps::is(folder, "WPHwManager")
}

/// The details view's columns of the Hardware Manager.
pub const COLUMNS: [&[u8]; 7] = [b"Icon\0", b"Title\0", b"Device type\0", b"IRQ\0", b"DMA\0", b"I/O ports\0", b"Memory\0"];

/// A device's details: the five text columns' strings (pointers).
pub fn detail_strings(o: usize) -> Option<[u32; 5]> {
    let i = info(o)?;
    Some([0, 1, 2, 3, 4].map(|k| i.texts[k].as_ptr() as u32))
}

/// The tree view of the Hardware Manager: its devices, each with the
/// object it is on (0: at the top), parents before children.
pub fn tree(folder: usize, content: &[usize]) -> Option<Vec<(usize, usize)>> {
    if !is_manager(folder) {
        return None;
    }
    let of = |u: u32| content.iter().copied().find(|&o| info(o).is_some_and(|i| i.unique == u && u != 0));
    let mut out: Vec<(usize, usize)> = Vec::new();
    // Parents first: repeat until every device is placed.
    let mut left: Vec<usize> = content.to_vec();
    for _ in 0..64 {
        let before = left.len();
        left.retain(|&o| {
            let p = info(o).map_or(0, |i| i.parent);
            let po = if p == 0 { None } else { of(p) };
            match po {
                Some(po) if !out.iter().any(|&(x, _)| x == po) => true,
                po => {
                    out.push((o, po.unwrap_or(0)));
                    false
                }
            }
        });
        if left.is_empty() || left.len() == before {
            break;
        }
    }
    out.extend(left.into_iter().map(|o| (o, 0)));
    Some(out)
}

// ---- WPHwManager ----

extern "C" fn mgr_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    let ok = som::call_parent(wps::ccd("WPHwManager"), wps::cd("WPFolder"), m::WPFolder::wpPopulate, &[obj, r as usize, p as usize, folders_only as usize]);
    if ok != 0 {
        populate_devices(obj);
    }
    1
}

extern "C" fn mgr_cls_title(_cls: usize) -> *const u8 {
    b"Hardware Manager\0".as_ptr()
}

extern "C" fn mgr_cls_style(_cls: usize) -> u32 {
    obj::OBJSTYLE_NOCOPY | obj::OBJSTYLE_NODELETE | 0x10
}

extern "C" fn mgr_cls_view(_cls: usize) -> u32 {
    OPEN_DETAILS
}

static mut ICONS: [u32; hi::LAST as usize + 1] = [0; hi::LAST as usize + 1];

fn icon(id: u32) -> u32 {
    let c = unsafe { &mut *(&raw mut ICONS) };
    if c[id as usize] == 0 {
        c[id as usize] = hi::icon(id).map_or(0, |(big, mini)| make_icon(&big, &mini));
    }
    c[id as usize]
}

extern "C" fn mgr_cls_icon(_cls: usize) -> u32 {
    icon(hi::MANAGER)
}

// ---- WPDevice and its classes ----

/// The class titles (rs2's own texts) and icons.
fn device_class(name: &[u8]) -> (&'static [u8], u32) {
    match name {
        b"WPDevAudio" => (b"Audio device\0", hi::AUDIO),
        b"WPDevBus" => (b"Bus\0", hi::BUS),
        b"WPDevCDRom" => (b"CD-ROM drive\0", hi::CD_ROM),
        b"WPDevCPU" => (b"Processor\0", hi::CPU),
        b"WPDevDisplay" => (b"Display adapter\0", hi::DISPLAY),
        b"WPDevDiskette" => (b"Diskette drive\0", hi::DISKETTE),
        b"WPDevHarddrive" => (b"Hard disk\0", hi::HARD_DISK),
        b"WPDevKeyboard" => (b"Keyboard device\0", hi::KEYBOARD),
        b"WPDevMemory" => (b"Memory\0", hi::MEMORY),
        b"WPDevMouse" => (b"Pointing device\0", hi::MOUSE),
        b"WPDevParallel" => (b"Parallel port\0", hi::PARALLEL),
        b"WPDevPeriph" => (b"System peripheral\0", hi::PERIPHERAL),
        b"WPDevSerial" => (b"Serial port\0", hi::SERIAL),
        b"WPDevTape" => (b"Tape drive\0", hi::TAPE),
        b"WPDevTimer" => (b"Timer\0", hi::TIMER),
        _ => (b"Device\0", hi::DEVICE),
    }
}

/// The device class a class is or derives from.
fn device_class_of(cls: usize) -> (&'static [u8], u32) {
    let mut c = cls;
    for _ in 0..32 {
        if c == 0 {
            break;
        }
        let n = som::class_name(c);
        if n.starts_with(b"WPDev") {
            return device_class(n);
        }
        c = som::sc_call(som::sc::SOM_GET_PARENT, &[c]);
    }
    device_class(b"")
}

extern "C" fn dev_cls_title(cls: usize) -> *const u8 {
    device_class_of(cls).0.as_ptr()
}

extern "C" fn dev_cls_icon(cls: usize) -> u32 {
    icon(device_class_of(cls).1)
}

extern "C" fn dev_cls_style(_cls: usize) -> u32 {
    obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NOLINK | obj::OBJSTYLE_NOCOPY | obj::OBJSTYLE_NODELETE | 0x10
}

extern "C" fn dev_cls_view(_cls: usize) -> u32 {
    OPEN_SETTINGS
}

extern "C" fn dev_init(obj: usize) -> u32 {
    som::call_parent(wps::ccd("WPDevice"), wps::cd("WPTransient"), m::WPObject::wpInitData, &[obj]);
    dd(obj).info = core::ptr::null_mut();
    0
}

extern "C" fn dev_uninit(obj: usize) -> u32 {
    let p = dd(obj).info;
    if !p.is_null() {
        dd(obj).info = core::ptr::null_mut();
        drop(unsafe { Box::from_raw(p) });
    }
    devices().retain(|&(_, o)| o != obj);
    som::call_parent(wps::ccd("WPDevice"), wps::cd("WPTransient"), m::WPObject::wpUnInitData, &[obj]);
    0
}

/// UNIQUEID= and PARENTID= (the resource manager's handles), as IBM's.
extern "C" fn dev_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPDevice"), wps::cd("WPTransient"), m::WPObject::wpSetup, &[obj, setup as usize]);
    if setup.is_null() {
        return 1;
    }
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        let n = crate::util::parse_dec(&v).unwrap_or(0);
        if k == b"UNIQUEID" {
            info_or_new(obj).unique = n;
        } else if k == b"PARENTID" {
            info_or_new(obj).parent = n;
        }
    }
    1
}

// The private methods IBM's release order has (wpDevicePrivateN is its
// index N-1): the getters.
extern "C" fn get_unique(o: usize) -> u32 {
    info(o).map_or(0, |i| i.unique)
}
extern "C" fn get_parent(o: usize) -> u32 {
    info(o).map_or(0, |i| i.parent)
}
extern "C" fn get_node_type(o: usize) -> u32 {
    info(o).map_or(0, |i| i.node_type)
}
extern "C" fn get_base_type(o: usize) -> u32 {
    info(o).map_or(0, |i| i.base as u32)
}
extern "C" fn get_sub_type(o: usize) -> u32 {
    info(o).map_or(0, |i| i.sub as u32)
}
fn string(o: usize, k: usize) -> *const u8 {
    info(o).map_or(b"\0".as_ptr(), |i| i.strings[k].as_ptr())
}
extern "C" fn get_dd_file(o: usize) -> *const u8 {
    string(o, 0)
}
extern "C" fn get_dd_name(o: usize) -> *const u8 {
    string(o, 1)
}
extern "C" fn get_dd_vendor(o: usize) -> *const u8 {
    string(o, 2)
}
extern "C" fn get_dd_version(o: usize) -> *const u8 {
    string(o, 3)
}
extern "C" fn get_dd_date(o: usize) -> *const u8 {
    string(o, 4)
}
extern "C" fn set_unique(o: usize, v: u32) -> u32 {
    info_or_new(o).unique = v;
    1
}
extern "C" fn set_parent(o: usize, v: u32) -> u32 {
    info_or_new(o).parent = v;
    1
}

// ---- The settings pages ----

const ID_LIST: u16 = 700;
const ID_TYPE: u16 = 701;
const ID_FILE: u16 = 702;
const ID_NAME: u16 = 703;
const ID_VENDOR: u16 = 704;
const ID_VERSION: u16 = 705;
const ID_DATE: u16 = 706;

fn value(id: u16, (x, y, cx): (i16, i16, i16)) -> Item {
    item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (x, y, cx, 8), id)
}

extern "C" fn device_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = b as usize;
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            set_item_text(h, ID_TYPE as u32, &class_title_of(o));
            let none = |s: &[u8]| if s.is_empty() { b"(none)".to_vec() } else { s.to_vec() };
            if let Some(i) = info(o) {
                let d = &i.driver;
                set_item_text(h, ID_FILE as u32, &none(&d.name));
                set_item_text(h, ID_NAME as u32, &none(&d.desc));
                set_item_text(h, ID_VENDOR as u32, &none(&d.vendor));
                set_item_text(h, ID_VERSION as u32, &none(cstr(i.strings[3].as_ptr())));
                set_item_text(h, ID_DATE as u32, &none(cstr(i.strings[4].as_ptr())));
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn add_page(o: usize, nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, items: Vec<Item>, tab: &'static [u8]) -> u32 {
    let tpl = template(book::page_frame(), items);
    let page = book::make_page(nb, proc_, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, tab)
}

extern "C" fn wpAddDeviceDevice1Page(o: usize, nb: u32) -> u32 {
    add_page(o, nb, device_page_proc, alloc::vec![
        book::label(b"Device type:", (6, 104, 70)),
        value(ID_TYPE, (80, 104, 150)),
        book::label(b"Device driver:", (6, 84, 70)),
        value(ID_FILE, (80, 84, 150)),
        book::label(b"Description:", (6, 72, 70)),
        value(ID_NAME, (80, 72, 150)),
        book::label(b"Vendor:", (6, 60, 70)),
        value(ID_VENDOR, (80, 60, 150)),
        book::label(b"Version:", (6, 48, 70)),
        value(ID_VERSION, (80, 48, 150)),
        book::label(b"Date:", (6, 36, 70)),
        value(ID_DATE, (80, 36, 150)),
    ], b"~Device\0")
}

/// IBM's second device page is empty; so is rs2's (no page).
extern "C" fn wpAddDeviceDevice2Page(_o: usize, _nb: u32) -> u32 {
    1
}

/// A resource page: the resources of one kind (by the page's id) in a
/// list.
fn res_page_proc(h: u32, msg: u32, a: u32, b: u32, kind: u8) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = b as usize;
            let mut any = false;
            if let Some(i) = info(o) {
                for r in &i.res {
                    let k = match r {
                        Res::Irq { .. } => 0,
                        Res::Dma { .. } => 1,
                        Res::Io { .. } => 2,
                        Res::Mem { .. } => 3,
                    };
                    if k == kind {
                        let t = z(&res_line(r));
                        unsafe { WinSendDlgItemMsg(h, ID_LIST as u32, LM_INSERTITEM, 0xffff, t.as_ptr() as u32) };
                        any = true;
                    }
                }
            }
            if !any {
                unsafe { WinSendDlgItemMsg(h, ID_LIST as u32, LM_INSERTITEM, 0xffff, b"(none)\0".as_ptr() as u32) };
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn irq_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    res_page_proc(h, msg, a, b, 0)
}
extern "C" fn dma_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    res_page_proc(h, msg, a, b, 1)
}
extern "C" fn io_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    res_page_proc(h, msg, a, b, 2)
}
extern "C" fn mem_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    res_page_proc(h, msg, a, b, 3)
}

fn res_page(o: usize, nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, what: &'static [u8], tab: &'static [u8]) -> u32 {
    add_page(o, nb, proc_, alloc::vec![
        book::label(what, (6, 104, 224)),
        // LS_NOADJUSTPOS | LS_HORZSCROLL
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (6, 6, 224, 94), ID_LIST),
    ], tab)
}

extern "C" fn wpAddDeviceIRQ1Page(o: usize, nb: u32) -> u32 {
    res_page(o, nb, irq_proc, b"Interrupt levels (IRQ) the device uses:", b"~IRQ\0")
}

extern "C" fn wpAddDeviceDMA1Page(o: usize, nb: u32) -> u32 {
    res_page(o, nb, dma_proc, b"DMA channels the device uses:", b"D~MA\0")
}

extern "C" fn wpAddDeviceIO1Page(o: usize, nb: u32) -> u32 {
    res_page(o, nb, io_proc, b"Input/output port ranges the device uses:", b"I/~O\0")
}

extern "C" fn wpAddDeviceMemory1Page(o: usize, nb: u32) -> u32 {
    res_page(o, nb, mem_proc, b"Memory ranges the device uses:", b"~Memory\0")
}

/// The pages as IBM adds them (each in front): Device, IRQ, DMA, I/O,
/// Memory, then the object's own.
extern "C" fn dev_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPDevice"), wps::cd("WPTransient"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    for i in [m::WPDevice::wpAddDeviceMemory1Page, m::WPDevice::wpAddDeviceIO1Page, m::WPDevice::wpAddDeviceDMA1Page, m::WPDevice::wpAddDeviceIRQ1Page, m::WPDevice::wpAddDeviceDevice1Page] {
        if wps::call("WPDevice", i, &[o, nb as usize]) == 0 {
            return 0;
        }
    }
    1
}

methods! {
    __m_mgr_populate => mgr_populate, 4;
    __m_mgr_cls_title => mgr_cls_title, 1;
    __m_mgr_cls_style => mgr_cls_style, 1;
    __m_mgr_cls_view => mgr_cls_view, 1;
    __m_mgr_cls_icon => mgr_cls_icon, 1;
    __m_dev_cls_title => dev_cls_title, 1;
    __m_dev_cls_icon => dev_cls_icon, 1;
    __m_dev_cls_style => dev_cls_style, 1;
    __m_dev_cls_view => dev_cls_view, 1;
    __m_dev_init => dev_init, 1;
    __m_dev_uninit => dev_uninit, 1;
    __m_dev_setup => dev_setup, 2;
    __m_dev_add_pages => dev_add_pages, 2;
    __m_get_unique => get_unique, 1;
    __m_get_parent => get_parent, 1;
    __m_get_node_type => get_node_type, 1;
    __m_get_base_type => get_base_type, 1;
    __m_get_sub_type => get_sub_type, 1;
    __m_get_dd_file => get_dd_file, 1;
    __m_get_dd_name => get_dd_name, 1;
    __m_get_dd_vendor => get_dd_vendor, 1;
    __m_get_dd_version => get_dd_version, 1;
    __m_get_dd_date => get_dd_date, 1;
    __m_set_unique => set_unique, 2;
    __m_set_parent => set_parent, 2;
    __m_wpAddDeviceDevice1Page => wpAddDeviceDevice1Page, 2;
    __m_wpAddDeviceDevice2Page => wpAddDeviceDevice2Page, 2;
    __m_wpAddDeviceIRQ1Page => wpAddDeviceIRQ1Page, 2;
    __m_wpAddDeviceDMA1Page => wpAddDeviceDMA1Page, 2;
    __m_wpAddDeviceIO1Page => wpAddDeviceIO1Page, 2;
    __m_wpAddDeviceMemory1Page => wpAddDeviceMemory1Page, 2;
}

pub fn impl_manager() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![("wpPopulate", __m_mgr_populate as usize)],
    }
}

pub fn impl_m_manager() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpclsQueryTitle", __m_mgr_cls_title as usize),
            ("wpclsQueryStyle", __m_mgr_cls_style as usize),
            ("wpclsQueryDefaultView", __m_mgr_cls_view as usize),
            ("wpclsQueryIcon", __m_mgr_cls_icon as usize),
        ],
    }
}

pub fn impl_device() -> Impl {
    Impl {
        data: core::mem::size_of::<DevData>() as u32,
        methods: alloc::vec![
            ("wpDevicePrivate1", __m_get_unique as usize),
            ("wpDevicePrivate2", __m_get_parent as usize),
            ("wpDevicePrivate3", __m_get_node_type as usize),
            ("wpDevicePrivate4", __m_get_base_type as usize),
            ("wpDevicePrivate5", __m_get_sub_type as usize),
            ("wpDevicePrivate22", __m_get_dd_file as usize),
            ("wpDevicePrivate23", __m_get_dd_name as usize),
            ("wpDevicePrivate24", __m_get_dd_vendor as usize),
            ("wpDevicePrivate25", __m_get_dd_version as usize),
            ("wpDevicePrivate26", __m_get_dd_date as usize),
            ("wpDevicePrivate28", __m_set_unique as usize),
            ("wpDevicePrivate29", __m_set_parent as usize),
            ("wpAddDeviceDevice1Page", __m_wpAddDeviceDevice1Page as usize),
            ("wpAddDeviceDevice2Page", __m_wpAddDeviceDevice2Page as usize),
            ("wpAddDeviceDMA1Page", __m_wpAddDeviceDMA1Page as usize),
            ("wpAddDeviceIRQ1Page", __m_wpAddDeviceIRQ1Page as usize),
            ("wpAddDeviceMemory1Page", __m_wpAddDeviceMemory1Page as usize),
            ("wpAddDeviceIO1Page", __m_wpAddDeviceIO1Page as usize),
        ],
        overrides: alloc::vec![
            ("wpInitData", __m_dev_init as usize),
            ("wpUnInitData", __m_dev_uninit as usize),
            ("wpSetup", __m_dev_setup as usize),
            ("wpAddSettingsPages", __m_dev_add_pages as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}

pub fn impl_m_device() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpclsQueryTitle", __m_dev_cls_title as usize),
            ("wpclsQueryIcon", __m_dev_cls_icon as usize),
            ("wpclsQueryStyle", __m_dev_cls_style as usize),
            ("wpclsQueryDefaultView", __m_dev_cls_view as usize),
        ],
    }
}
