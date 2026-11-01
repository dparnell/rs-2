//! The print queues as registry keys, as in IBM's REGISTRY: a key with
//! OS2_INI_ACCESS_METHOD "spooler_key" gets one subkey per print queue
//! (PMSPL, SplEnumQueue level 3) whenever it is opened, named by the
//! queue's printer driver device (its DRIVDATA's szDeviceName) and itself
//! marked "spooler_value"; the queue the applications print to by default
//! (PRQ3_TYPE_APPDEFAULT) is the key's "Default" value. A "spooler_value"
//! key shows the queue and its printer (SplQueryDevice level 3) as 23
//! values: numbers as REG_DWORD, texts as REG_SZ (IBM's names and
//! fields; Attributes, Default DevMode, Location and Description are
//! empty).

use alloc::vec::Vec;

use crate::{ERROR_FILE_NOT_FOUND, ERROR_GEN_FAILURE, ERROR_INVALID_NAME, ERROR_NO_MORE_ITEMS, Key, REG_SZ, apps, create, encode, file, map, put, value_key, z};

unsafe extern "C" {
    #[link_name = "PMSPL.399"]
    fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, returned: *mut u32, total: *mut u32, needed: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.381"]
    fn SplQueryDevice(comp: *const u8, dev: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32;
}

const REG_DWORD: u32 = 4;
const PRQ3_TYPE_APPDEFAULT: u16 = 4;
/// PRQINFO3's size.
const PRQ3: usize = 44;

#[derive(Clone, Copy)]
enum Field {
    /// A USHORT of PRQINFO3 at this offset.
    QueueNum(usize),
    /// A string pointer of PRQINFO3.
    QueueText(usize),
    /// A USHORT of PRDINFO3.
    DevNum(usize),
    /// A string pointer of PRDINFO3.
    DevText(usize),
    /// The DRIVDATA's device name and version.
    DriverName,
    DriverVersion,
    Empty,
}

/// IBM's value names, in its order.
const VALUES: [(&str, Field); 23] = [
    ("Priority", Field::QueueNum(4)),
    ("StartTime", Field::QueueNum(6)),
    ("UntilTime", Field::QueueNum(8)),
    ("Datatype", Field::QueueNum(10)),
    ("Attributes", Field::Empty),
    ("Name", Field::DriverName),
    ("Default DevMode", Field::Empty),
    ("Share Name", Field::DevText(20)),
    ("Print Processor", Field::QueueText(16)),
    ("Printer Driver", Field::QueueText(36)),
    ("Location", Field::Empty),
    ("Description", Field::Empty),
    ("Parameters", Field::QueueText(20)),
    ("Separator File", Field::QueueText(12)),
    ("Version", Field::DriverVersion),
    ("Printer Name", Field::DevText(0)),
    ("User Name", Field::DevText(4)),
    ("Port", Field::DevText(8)),
    ("Job ID", Field::DevNum(12)),
    ("Status", Field::DevNum(14)),
    ("Drivers", Field::DevText(24)),
    ("Time", Field::DevNum(28)),
    ("Time Out", Field::DevNum(30)),
];

fn rd32(b: &[u8], off: usize) -> u32 {
    b.get(off..off + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd16(b: &[u8], off: usize) -> u16 {
    b.get(off..off + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

/// The string at an address the spooler returned.
fn text_at(p: u32) -> Vec<u8> {
    crate::cstr(p as *const u8)
}

/// The queues' PRQINFO3 records (their pointers point into the buffer).
fn queues() -> Vec<u8> {
    let (mut ret, mut tot, mut need) = (0, 0, 0);
    let rc = unsafe { SplEnumQueue(core::ptr::null(), 3, core::ptr::null_mut(), 0, &mut ret, &mut tot, &mut need, 0) };
    if (rc != 234 && rc != 2123) || need == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; need as usize];
    if unsafe { SplEnumQueue(core::ptr::null(), 3, buf.as_mut_ptr(), need, &mut ret, &mut tot, &mut need, 0) } != 0 {
        return Vec::new();
    }
    buf.truncate(ret as usize * PRQ3);
    buf
}

/// A queue's printer driver device name ("" without driver data).
fn device_name(q: &[u8]) -> Vec<u8> {
    match rd32(q, 40) {
        0 => Vec::new(),
        p => text_at(p + 8),
    }
}

/// Value `name` of the queue whose printer driver device is `printer`.
pub fn value(printer: &[u8], name: &[u8]) -> Result<(u32, Vec<u8>), u32> {
    if name.is_empty() {
        return Err(ERROR_INVALID_NAME);
    }
    let all = queues();
    let q = all.chunks(PRQ3).find(|q| map::same(&device_name(q), printer)).ok_or(ERROR_FILE_NOT_FOUND)?;
    let field = VALUES.iter().find(|v| map::same(v.0.as_bytes(), name)).ok_or(ERROR_FILE_NOT_FOUND)?.1;
    // The printer (PRDINFO3) of the queue.
    let printers = z(&text_at(rd32(q, 32)));
    let mut need = 0;
    unsafe { SplQueryDevice(core::ptr::null(), printers.as_ptr(), 3, core::ptr::null_mut(), 0, &mut need) };
    let mut dev = alloc::vec![0u8; need.max(32) as usize];
    if unsafe { SplQueryDevice(core::ptr::null(), printers.as_ptr(), 3, dev.as_mut_ptr(), dev.len() as u32, &mut need) } != 0 {
        return Err(ERROR_GEN_FAILURE);
    }
    let num = |n: u32| Ok((REG_DWORD, n.to_le_bytes().to_vec()));
    let txt = |mut t: Vec<u8>| {
        t.push(0);
        Ok((REG_SZ, t))
    };
    match field {
        Field::QueueNum(o) => num(rd16(q, o) as u32),
        Field::QueueText(o) => txt(text_at(rd32(q, o))),
        Field::DevNum(o) => num(rd16(&dev, o) as u32),
        Field::DevText(o) => txt(text_at(rd32(&dev, o))),
        Field::DriverName => txt(device_name(q)),
        Field::DriverVersion => num(match rd32(q, 40) {
            0 => 0,
            p => unsafe { (p as *const u32).add(1).read_unaligned() },
        }),
        Field::Empty => txt(Vec::new()),
    }
}

/// The i-th value of a spooler_value key.
pub fn ith(printer: &[u8], i: u32) -> Result<(Vec<u8>, u32, Vec<u8>), u32> {
    let (n, _) = VALUES.get(i as usize).ok_or(ERROR_NO_MORE_ITEMS)?;
    match value(printer, n.as_bytes()) {
        Ok((t, d)) => Ok((n.as_bytes().to_vec(), t, d)),
        Err(ERROR_FILE_NOT_FOUND) => Err(ERROR_NO_MORE_ITEMS),
        Err(e) => Err(e),
    }
}

/// Make a spooler_key key's subkeys the queues again.
pub fn refresh(k: &Key) {
    let hini = file(k.hive);
    let mut prefix = k.path.clone();
    prefix.push(b'\\');
    for a in apps(k.hive) {
        if a.starts_with(&prefix) {
            put(hini, &a, None, None);
        }
    }
    let all = queues();
    for q in all.chunks(PRQ3) {
        let name = device_name(q);
        if name.is_empty() || name.contains(&b'\\') || !create(k.hive, &k.path, &name) {
            continue;
        }
        let sub = crate::join(&k.path, &name);
        let method = z(b"spooler_value");
        put(hini, &sub, Some(&value_key(map::ACCESS_METHOD)), Some(&encode(map::ACCESS_METHOD, REG_SZ, &method)));
        if rd16(q, 10) & PRQ3_TYPE_APPDEFAULT != 0 {
            put(hini, &k.path, Some(&value_key(b"Default")), Some(&encode(b"Default", REG_SZ, &z(&name))));
        }
    }
}
