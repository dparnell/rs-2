//! The Apple Event Manager (OPENDOC.DLL ordinals 1-71), as far as OS/2
//! OpenDoc's scripting uses it (OREXXSC: docs/OPENDOC.md).
//!
//! An AEDesc is 8 bytes: the descriptor type and a data handle. rs2's
//! handle points to a [`Data`] of its own: the bytes of a simple
//! descriptor, the items of a list ('list'), the keyword items of a
//! record ('reco', and the record types: object specifiers 'obj ',
//! ranges 'rang'...), and an Apple event's ('aevt') parameters and
//! attributes. Callers see the data only through the calls.
//!
//! Coercion (AECoerceDesc and the desired types of the Get calls): the
//! same type or typeWildCard ('****') copies; records change type into
//! each other; numbers ('shor', 'long', 'bool', 'true'/'fals') into each
//! other and to and from 'TEXT'; four-character codes ('enum', 'type',
//! 'keyw', 'prop') to and from 'TEXT' (the text ends in a NUL).
//!
//! Events: AEInstallEventHandler/AERemoveEventHandler keep handlers by
//! class and ID ('****' matches any); AESend gives an event addressed to
//! this process ('PID ' target, or none) to its handler, with a reply
//! event ('aevt'/'ansr') when the mode waits for one (kAEWaitReply).
//! Other processes' events are not delivered (procNotFound).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::rt::*;

/// A four-character code as OS/2 OpenDoc's callers have it: the
/// characters in memory order (IBM's OREXXSC compares 'TEXT' as
/// 0x54584554). typeLongInteger and typeShortInteger are the exceptions:
/// their values are 0x6C6F6E67 and 0x73686F72 (the characters reversed
/// in memory).
pub const fn code(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

const T_NULL: u32 = code(b"null");
const T_LIST: u32 = code(b"list");
const T_RECORD: u32 = code(b"reco");
const T_EVENT: u32 = code(b"aevt");
const T_WILD: u32 = code(b"****");
const T_TEXT: u32 = code(b"TEXT");
const T_LONG: u32 = 0x6c6f_6e67;
const T_SHORT: u32 = 0x7368_6f72;
const T_BOOL: u32 = code(b"bool");
const T_TRUE: u32 = code(b"true");
const T_FALSE: u32 = code(b"fals");
const T_PID: u32 = code(b"PID ");
const T_OBJ: u32 = code(b"obj ");
const T_RANGE: u32 = code(b"rang");
const T_ENUM: u32 = code(b"enum");
const T_TYPE: u32 = code(b"type");

pub const ERR_COERCE: i32 = -1700;
pub const ERR_NOT_FOUND: i32 = -1701;
pub const ERR_WRONG_TYPE: i32 = -1703;
pub const ERR_NOT_DESC: i32 = -1704;
pub const ERR_BAD_ITEM: i32 = -1705;
pub const ERR_NOT_EVENT: i32 = -1707;
pub const ERR_NOT_HANDLED: i32 = -1708;
pub const ERR_PROC_NOT_FOUND: i32 = -600;
pub const ERR_PARAM: i32 = -50;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Desc {
    pub ty: u32,
    pub h: usize,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Kind {
    #[default]
    Bytes,
    List,
    Record,
    Event,
}

#[derive(Default)]
pub struct Data {
    kind: Kind,
    bytes: Vec<u8>,
    /// List items (keyword 0), record items or event parameters.
    items: Vec<(u32, Desc)>,
    /// An event's attributes.
    attrs: Vec<(u32, Desc)>,
}

fn data(d: &Desc) -> Option<&'static mut Data> {
    if d.h == 0 { None } else { Some(unsafe { &mut *(d.h as *mut Data) }) }
}

fn make(ty: u32, d: Data) -> Desc {
    Desc { ty, h: Box::into_raw(Box::new(d)) as usize }
}

fn bytes(ty: u32, b: &[u8]) -> Desc {
    if ty == T_NULL { Desc { ty, h: 0 } } else { make(ty, Data { bytes: b.to_vec(), ..Data::default() }) }
}

fn dispose(d: &mut Desc) {
    if d.h != 0 {
        let b = unsafe { Box::from_raw(d.h as *mut Data) };
        let Data { items, attrs, .. } = *b;
        for (_, mut x) in items.into_iter().chain(attrs) {
            dispose(&mut x);
        }
    }
    *d = Desc { ty: T_NULL, h: 0 };
}

fn dup(d: &Desc) -> Desc {
    match data(d) {
        None => *d,
        Some(x) => make(d.ty, Data {
            kind: x.kind,
            bytes: x.bytes.clone(),
            items: x.items.iter().map(|(k, v)| (*k, dup(v))).collect(),
            attrs: x.attrs.iter().map(|(k, v)| (*k, dup(v))).collect(),
        }),
    }
}

fn kind(d: &Desc) -> Kind {
    data(d).map_or(Kind::Bytes, |x| x.kind)
}

fn is_code_type(t: u32) -> bool {
    matches!(&t.to_le_bytes(), b"enum" | b"type" | b"keyw" | b"prop")
}

fn num_of(d: &Desc) -> Option<i64> {
    let b = &data(d)?.bytes;
    match d.ty {
        T_LONG if b.len() >= 4 => Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64),
        T_SHORT if b.len() >= 2 => Some(i16::from_le_bytes([b[0], b[1]]) as i64),
        T_BOOL => Some(b.first().map_or(0, |&c| (c != 0) as i64)),
        T_TRUE => Some(1),
        T_FALSE => Some(0),
        T_TEXT => {
            let s: Vec<u8> = b.iter().copied().take_while(|&c| c != 0).collect();
            let s = core::str::from_utf8(&s).ok()?.trim();
            s.parse::<i64>().ok()
        }
        _ => None,
    }
}

/// The descriptor coerced to `to` (a new one), or an error.
fn coerce(d: &Desc, to: u32) -> Result<Desc, i32> {
    if to == d.ty || to == T_WILD {
        return Ok(dup(d));
    }
    let k = kind(d);
    if k == Kind::Record && (to == T_RECORD || to == T_OBJ || to == T_RANGE || is_record_type(to)) {
        let mut n = dup(d);
        n.ty = to;
        return Ok(n);
    }
    if k != Kind::Bytes {
        return Err(ERR_COERCE);
    }
    let b = data(d).map_or(&[][..], |x| &x.bytes[..]);
    match to {
        T_LONG | T_SHORT | T_BOOL => {
            let v = num_of(d).ok_or(ERR_COERCE)?;
            Ok(match to {
                T_LONG => bytes(T_LONG, &(v as i32).to_le_bytes()),
                T_SHORT => bytes(T_SHORT, &(v as i16).to_le_bytes()),
                _ => bytes(T_BOOL, &[(v != 0) as u8]),
            })
        }
        T_TEXT => {
            if let Some(v) = num_of(d).filter(|_| d.ty != T_TEXT) {
                return Ok(bytes(T_TEXT, alloc::format!("{v}").as_bytes()));
            }
            if is_code_type(d.ty) && b.len() >= 4 {
                let mut t = b[..4].to_vec();
                t.push(0);
                return Ok(bytes(T_TEXT, &t));
            }
            Err(ERR_COERCE)
        }
        _ if is_code_type(to) && (is_code_type(d.ty) || d.ty == T_TEXT) => {
            let c = if d.ty == T_TEXT {
                let mut c = [b' '; 4];
                for (i, &x) in b.iter().take_while(|&&x| x != 0).take(4).enumerate() {
                    c[i] = x;
                }
                u32::from_le_bytes(c)
            } else if b.len() >= 4 {
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            } else {
                return Err(ERR_COERCE);
            };
            Ok(bytes(to, &c.to_le_bytes()))
        }
        _ => Err(ERR_COERCE),
    }
}

fn is_record_type(t: u32) -> bool {
    matches!(&t.to_le_bytes(), b"obj " | b"rang" | b"cmpd" | b"logi" | b"insl" | b"reco")
}

/// Copy a descriptor's data out (for the ...Ptr calls).
fn copy_out(d: &Desc, ty_out: usize, ptr: usize, max: usize, size_out: usize) {
    let b = data(d).map_or(&[][..], |x| &x.bytes[..]);
    if ty_out != 0 {
        write::<u32>(ty_out, d.ty);
    }
    if size_out != 0 {
        write::<u32>(size_out, b.len() as u32);
    }
    if ptr != 0 {
        let n = b.len().min(max);
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), ptr as *mut u8, n) };
    }
}

fn get_desc(p: usize) -> Desc {
    if p == 0 { Desc::default() } else { read::<Desc>(p) }
}

fn put_desc(p: usize, d: Desc) {
    if p != 0 {
        write::<Desc>(p, d);
    }
}

fn find_key(v: &[(u32, Desc)], key: u32) -> Option<Desc> {
    v.iter().find(|(k, _)| *k == key).map(|(_, d)| *d)
}

fn set_key(v: &mut Vec<(u32, Desc)>, key: u32, d: Desc) {
    if let Some(i) = v.iter().position(|(k, _)| *k == key) {
        let mut old = core::mem::replace(&mut v[i].1, d);
        dispose(&mut old);
    } else {
        v.push((key, d));
    }
}

/// A keyword item of a record or event (its parameters).
fn param(ev: &Desc, key: u32) -> Option<Desc> {
    find_key(&data(ev)?.items, key)
}

fn attr(ev: &Desc, key: u32) -> Option<Desc> {
    let x = data(ev)?;
    if x.kind != Kind::Event {
        return None;
    }
    find_key(&x.attrs, key)
}

fn rc(r: Result<(), i32>) -> i32 {
    r.err().unwrap_or(0)
}

// ---- the calls ----

pub extern "C" fn AECoerceDesc(desc: usize, to: u32, result: usize) -> i32 {
    match coerce(&get_desc(desc), to) {
        Ok(d) => {
            put_desc(result, d);
            0
        }
        Err(e) => {
            put_desc(result, Desc { ty: T_NULL, h: 0 });
            e
        }
    }
}

pub extern "C" fn AECountItems(list: usize, count: usize) -> i32 {
    let d = get_desc(list);
    match data(&d) {
        Some(x) if x.kind != Kind::Bytes => {
            write::<i32>(count, x.items.len() as i32);
            0
        }
        _ => ERR_WRONG_TYPE,
    }
}

pub extern "C" fn AECreateDesc(ty: u32, ptr: usize, size: usize, result: usize) -> i32 {
    let b: &[u8] = if ptr == 0 || size == 0 { &[] } else { unsafe { core::slice::from_raw_parts(ptr as *const u8, size) } };
    put_desc(result, bytes(ty, b));
    0
}

pub extern "C" fn AECreateList(_fact: usize, _fact_size: usize, is_record: u32, result: usize) -> i32 {
    let rec = is_record & 0xff != 0;
    put_desc(result, make(if rec { T_RECORD } else { T_LIST }, Data { kind: if rec { Kind::Record } else { Kind::List }, ..Data::default() }));
    0
}

pub extern "C" fn AECreateAppleEvent(class: u32, id: u32, target: usize, return_id: i32, trans: i32, result: usize) -> i32 {
    let t = get_desc(target);
    let attrs = alloc::vec![
        (code(b"evcl"), bytes(T_TYPE, &class.to_le_bytes())),
        (code(b"evid"), bytes(T_TYPE, &id.to_le_bytes())),
        (code(b"addr"), if t.ty == T_NULL && t.h == 0 { Desc { ty: T_NULL, h: 0 } } else { dup(&t) }),
        (code(b"rtid"), bytes(T_SHORT, &(return_id as i16).to_le_bytes())),
        (code(b"tran"), bytes(T_LONG, &trans.to_le_bytes())),
    ];
    put_desc(result, make(T_EVENT, Data { kind: Kind::Event, attrs, ..Data::default() }));
    0
}

fn record(ty: u32, items: Vec<(u32, Desc)>) -> Desc {
    make(ty, Data { kind: Kind::Record, items, ..Data::default() })
}

/// CreateObjSpecifier(class, container, keyForm, keyData, dispose, result).
pub extern "C" fn CreateObjSpecifier(class: u32, container: usize, form: u32, key: usize, dispose_in: u32, result: usize) -> i32 {
    let mut c = get_desc(container);
    let mut k = get_desc(key);
    let items = alloc::vec![
        (code(b"want"), bytes(T_TYPE, &class.to_le_bytes())),
        (code(b"from"), dup(&c)),
        (code(b"form"), bytes(T_ENUM, &form.to_le_bytes())),
        (code(b"seld"), dup(&k)),
    ];
    if dispose_in & 0xff != 0 {
        dispose(&mut c);
        dispose(&mut k);
        put_desc(container, c);
        put_desc(key, k);
    }
    put_desc(result, record(T_OBJ, items));
    0
}

/// CreateOffsetDescriptor(offset, result): an absolute position.
pub extern "C" fn CreateOffsetDescriptor(offset: i32, result: usize) -> i32 {
    put_desc(result, bytes(T_LONG, &offset.to_le_bytes()));
    0
}

/// CreateRangeDescriptor(start, stop, dispose, result).
pub extern "C" fn CreateRangeDescriptor(start: usize, stop: usize, dispose_in: u32, result: usize) -> i32 {
    let (mut a, mut b) = (get_desc(start), get_desc(stop));
    let items = alloc::vec![(code(b"star"), dup(&a)), (code(b"stop"), dup(&b))];
    if dispose_in & 0xff != 0 {
        dispose(&mut a);
        dispose(&mut b);
        put_desc(start, a);
        put_desc(stop, b);
    }
    put_desc(result, record(T_RANGE, items));
    0
}

pub extern "C" fn AEDisposeDesc(desc: usize) -> i32 {
    if desc != 0 {
        let mut d = get_desc(desc);
        dispose(&mut d);
        put_desc(desc, d);
    }
    0
}

pub extern "C" fn AEDuplicateDesc(desc: usize, result: usize) -> i32 {
    put_desc(result, dup(&get_desc(desc)));
    0
}

pub extern "C" fn AEGetAttributePtr(event: usize, key: u32, ty: u32, ty_out: usize, ptr: usize, max: usize, size_out: usize) -> i32 {
    let e = get_desc(event);
    if kind(&e) != Kind::Event {
        return ERR_NOT_EVENT;
    }
    let Some(a) = attr(&e, key) else { return ERR_NOT_FOUND };
    match coerce(&a, ty) {
        Ok(mut c) => {
            copy_out(&c, ty_out, ptr, max, size_out);
            dispose(&mut c);
            0
        }
        Err(e) => e,
    }
}

/// Ordinal 24 (OS/2): a descriptor's type and data.
pub extern "C" fn AEGetDescData(desc: usize, ty_out: usize, ptr: usize, max: usize, size_out: usize) -> i32 {
    copy_out(&get_desc(desc), ty_out, ptr, max, size_out);
    0
}

/// Ordinal 66 (OS/2): the size of a descriptor's data.
pub extern "C" fn AEGetDescDataSize(desc: usize, size_out: usize) -> i32 {
    let d = get_desc(desc);
    write::<u32>(size_out, data(&d).map_or(0, |x| x.bytes.len() as u32));
    0
}

pub extern "C" fn AEGetNthDesc(list: usize, index: i32, ty: u32, key_out: usize, result: usize) -> i32 {
    let l = get_desc(list);
    let Some(x) = data(&l).filter(|x| x.kind != Kind::Bytes) else { return ERR_WRONG_TYPE };
    let Some(&(k, d)) = x.items.get((index as usize).wrapping_sub(1)) else { return ERR_BAD_ITEM };
    if key_out != 0 {
        write::<u32>(key_out, if k == 0 { T_WILD } else { k });
    }
    match coerce(&d, ty) {
        Ok(c) => {
            put_desc(result, c);
            0
        }
        Err(e) => e,
    }
}

pub extern "C" fn AEGetParamDesc(event: usize, key: u32, ty: u32, result: usize) -> i32 {
    let e = get_desc(event);
    let Some(p) = param(&e, key) else {
        put_desc(result, Desc { ty: T_NULL, h: 0 });
        return ERR_NOT_FOUND;
    };
    match coerce(&p, ty) {
        Ok(c) => {
            put_desc(result, c);
            0
        }
        Err(err) => err,
    }
}

pub extern "C" fn AEGetParamPtr(event: usize, key: u32, ty: u32, ty_out: usize, ptr: usize, max: usize, size_out: usize) -> i32 {
    let e = get_desc(event);
    let Some(p) = param(&e, key) else { return ERR_NOT_FOUND };
    match coerce(&p, ty) {
        Ok(mut c) => {
            copy_out(&c, ty_out, ptr, max, size_out);
            dispose(&mut c);
            0
        }
        Err(err) => err,
    }
}

pub extern "C" fn AEPutDesc(list: usize, index: i32, desc: usize) -> i32 {
    let l = get_desc(list);
    let Some(x) = data(&l).filter(|x| x.kind != Kind::Bytes) else { return ERR_WRONG_TYPE };
    let d = dup(&get_desc(desc));
    // index 0 (or past the end): add at the end
    let i = index as usize;
    if i == 0 || i > x.items.len() {
        x.items.push((0, d));
    } else {
        let mut old = core::mem::replace(&mut x.items[i - 1].1, d);
        dispose(&mut old);
    }
    0
}

pub extern "C" fn AEPutParamDesc(event: usize, key: u32, desc: usize) -> i32 {
    let e = get_desc(event);
    let Some(x) = data(&e).filter(|x| x.kind != Kind::Bytes) else { return ERR_WRONG_TYPE };
    set_key(&mut x.items, key, dup(&get_desc(desc)));
    0
}

pub extern "C" fn AESizeOfNthItem(list: usize, index: i32, ty_out: usize, size_out: usize) -> i32 {
    let l = get_desc(list);
    let Some(x) = data(&l).filter(|x| x.kind != Kind::Bytes) else { return ERR_WRONG_TYPE };
    let Some(&(_, d)) = x.items.get((index as usize).wrapping_sub(1)) else { return ERR_BAD_ITEM };
    copy_out(&d, ty_out, 0, 0, size_out);
    0
}

pub extern "C" fn AESizeOfParam(event: usize, key: u32, ty_out: usize, size_out: usize) -> i32 {
    let e = get_desc(event);
    let Some(p) = param(&e, key) else { return ERR_NOT_FOUND };
    copy_out(&p, ty_out, 0, 0, size_out);
    0
}

// ---- event handlers and AESend ----

struct Handler {
    class: u32,
    id: u32,
    proc_: usize,
    refcon: usize,
}

static mut HANDLERS: Vec<Handler> = Vec::new();

fn handlers() -> &'static mut Vec<Handler> {
    unsafe { &mut *(&raw mut HANDLERS) }
}

pub extern "C" fn AEInstallEventHandler(class: u32, id: u32, proc_: usize, refcon: usize, _sys: u32) -> i32 {
    let h = handlers();
    h.retain(|x| !(x.class == class && x.id == id));
    h.push(Handler { class, id, proc_, refcon });
    // other processes can send it events (once the session has PM)
    listen();
    0
}

/// Receive events from other processes, if there are handlers and the
/// session has an anchor block (SetHAB: its thread has a message queue).
pub fn listen() {
    let s = crate::services::session();
    if s != 0 && crate::services::sess(s).hab != 0 && !handlers().is_empty() {
        crate::ipc::listen(&own_name());
    }
}

pub extern "C" fn AERemoveEventHandler(class: u32, id: u32, proc_: usize, _sys: u32) -> i32 {
    let h = handlers();
    let n = h.len();
    h.retain(|x| !(x.class == class && x.id == id && (proc_ == 0 || x.proc_ == proc_)));
    if h.len() == n { -1717 } else { 0 }
}

fn type_attr(e: &Desc, key: u32) -> u32 {
    attr(e, key).and_then(|d| data(&d).map(|x| x.bytes.clone())).filter(|b| b.len() >= 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn own_pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        if pib.is_null() { 0 } else { (*pib).pid }
    }
}

/// The program's name without its directory (`ODSHELL.EXE`).
fn own_name() -> Vec<u8> {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    let mut buf = [0u8; 260];
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        if !pib.is_null() {
            os2::DosQueryModuleName((*pib).hmte, buf.len() as u32, buf.as_mut_ptr());
        }
    }
    let full: Vec<u8> = buf.iter().copied().take_while(|&c| c != 0).collect();
    let base = full.rsplit(|&c| c == b'\\' || c == b'/').next().unwrap_or(&[]);
    base.to_vec()
}

/// Whether an event goes to this process: no target, or this PID.
fn for_us(e: &Desc) -> bool {
    match attr(e, code(b"addr")) {
        None => true,
        Some(a) if a.ty == T_NULL => true,
        Some(a) if a.ty == T_PID => {
            let b = data(&a).map_or(Vec::new(), |x| x.bytes.clone());
            b.len() >= 4 && u32::from_le_bytes([b[0], b[1], b[2], b[3]]) == own_pid()
        }
        _ => false,
    }
}

pub extern "C" fn AESend(event: usize, reply: usize, mode: u32, _prio: i32, _timeout: i32, _idle: usize, _filter: usize) -> i32 {
    let e = get_desc(event);
    if kind(&e) != Kind::Event {
        return ERR_NOT_EVENT;
    }
    if !for_us(&e) {
        // another process's: through its receiver (ipc.rs)
        let pid = attr(&e, code(b"addr")).and_then(|a| data(&a).map(|x| x.bytes.clone())).filter(|b| b.len() >= 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let mut buf = Vec::new();
        encode(&e, &mut buf);
        return match crate::ipc::send(pid, mode, &buf) {
            Ok((rc, rbuf)) => {
                let mut p = 0;
                let r = decode(&rbuf, &mut p).unwrap_or(Desc { ty: T_NULL, h: 0 });
                put_desc(reply, if mode & 3 == 3 { r } else { Desc { ty: T_NULL, h: 0 } });
                rc
            }
            Err(e) => e,
        };
    }
    deliver(event, reply, mode)
}

/// Give an event of this process to its handler.
pub fn deliver(event: usize, reply: usize, mode: u32) -> i32 {
    let e = get_desc(event);
    let (class, id) = (type_attr(&e, code(b"evcl")), type_attr(&e, code(b"evid")));
    let h = handlers();
    let found = [(class, id), (class, T_WILD), (T_WILD, id), (T_WILD, T_WILD)]
        .iter()
        .find_map(|&(c, i)| h.iter().find(|x| x.class == c && x.id == i).map(|x| (x.proc_, x.refcon)));
    let wait = mode & 3 == 3;
    let mut r = Desc { ty: T_NULL, h: 0 };
    if wait {
        AECreateAppleEvent(code(b"aevt"), code(b"ansr"), 0, 0, 0, &mut r as *mut Desc as usize);
    }
    let Some((proc_, refcon)) = found else {
        put_desc(reply, r);
        return ERR_NOT_HANDLED;
    };
    // OSErr handler(const AppleEvent *event, AppleEvent *reply, long refcon)
    let f: extern "C" fn(usize, usize, usize) -> i32 = unsafe { core::mem::transmute(proc_) };
    let err = f(event, &mut r as *mut Desc as usize, refcon);
    if err != 0 && wait {
        let mut n = bytes(T_LONG, &err.to_le_bytes());
        AEPutParamDesc(&r as *const Desc as usize, code(b"errn"), &n as *const Desc as usize);
        dispose(&mut n);
    }
    put_desc(reply, r);
    0
}

/// Ordinal 42 (OS/2): the PID of a program by name (`ODSHELL` or
/// `ODSHELL.EXE`): this process, else one that receives Apple events.
pub extern "C" fn AEGetProcessID(name: usize, _timeout: i32, pid_out: usize) -> i32 {
    let want: Vec<u8> = cstr(name).iter().map(|c| c.to_ascii_uppercase()).collect();
    let own: Vec<u8> = own_name().iter().map(|c| c.to_ascii_uppercase()).collect();
    let stem = own.strip_suffix(b".EXE").unwrap_or(&own);
    if !want.is_empty() && (want == own || want == stem) {
        write::<u32>(pid_out, own_pid());
        return 0;
    }
    match crate::ipc::find_name(&want) {
        Some(pid) => {
            write::<u32>(pid_out, pid);
            0
        }
        None => ERR_PROC_NOT_FOUND,
    }
}

/// Ordinal 71 (OS/2): a program's name by PID.
pub extern "C" fn AEGetProcessName(pid: u32, buf: usize, _info: usize) -> i32 {
    if buf == 0 {
        return ERR_PROC_NOT_FOUND;
    }
    let mut n = if pid == own_pid() {
        own_name()
    } else {
        match crate::ipc::name_of(pid) {
            Some(n) => n,
            None => return ERR_PROC_NOT_FOUND,
        }
    };
    n.push(0);
    unsafe { core::ptr::copy_nonoverlapping(n.as_ptr(), buf as *mut u8, n.len()) };
    0
}

// ---- the Object Support Library: object accessors and AEResolve ----

struct Accessor {
    class: u32,
    container: u32,
    proc_: usize,
    refcon: usize,
}

static mut ACCESSORS: Vec<Accessor> = Vec::new();

fn accessors() -> &'static mut Vec<Accessor> {
    unsafe { &mut *(&raw mut ACCESSORS) }
}

pub const ERR_NO_SUCH_OBJECT: i32 = -1728;
pub const ERR_NOT_OBJ_SPEC: i32 = -1727;
pub const ERR_ACCESSOR_NOT_FOUND: i32 = -1723;

/// AEInstallObjectAccessor(desiredClass, containerType, accessor, refcon,
/// isSystem) (ordinal 39, rs2's assignment).
pub extern "C" fn AEInstallObjectAccessor(class: u32, container: u32, proc_: usize, refcon: usize, _sys: u32) -> i32 {
    let a = accessors();
    a.retain(|x| !(x.class == class && x.container == container));
    a.push(Accessor { class, container, proc_, refcon });
    0
}

/// AERemoveObjectAccessor(desiredClass, containerType, accessor, isSystem)
/// (56).
pub extern "C" fn AERemoveObjectAccessor(class: u32, container: u32, proc_: usize, _sys: u32) -> i32 {
    let a = accessors();
    let n = a.len();
    a.retain(|x| !(x.class == class && x.container == container && (proc_ == 0 || x.proc_ == proc_)));
    if a.len() == n { ERR_ACCESSOR_NOT_FOUND } else { 0 }
}

/// Resolve an object specifier to a token: its container first (none:
/// the application, a typeNull token), then the accessor for the class
/// wanted and the container token's type ('****' matches any), called as
/// `accessor(desiredClass, containerToken, containerClass, keyForm,
/// keyData, *token, refcon)`.
fn resolve(spec: &Desc) -> Result<Desc, i32> {
    if spec.ty == T_NULL {
        return Ok(Desc { ty: T_NULL, h: 0 });
    }
    if kind(spec) != Kind::Record {
        return Err(ERR_NOT_OBJ_SPEC);
    }
    let get = |k: &[u8; 4]| param(spec, code(k));
    let code_of = |d: Option<Desc>| -> u32 {
        d.and_then(|d| data(&d).map(|x| x.bytes.clone())).filter(|b| b.len() >= 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let want = code_of(get(b"want"));
    let form = code_of(get(b"form"));
    let seld = get(b"seld").unwrap_or(Desc { ty: T_NULL, h: 0 });
    let from = get(b"from").unwrap_or(Desc { ty: T_NULL, h: 0 });
    let container_class = if from.ty == T_NULL { T_NULL } else { param(&from, code(b"want")).map_or(T_NULL, |d| code_of(Some(d))) };
    let mut ctok = resolve(&from)?;
    let a = accessors();
    let found = [(want, ctok.ty), (want, T_WILD), (T_WILD, ctok.ty), (T_WILD, T_WILD)]
        .iter()
        .find_map(|&(c, t)| a.iter().find(|x| x.class == c && x.container == t).map(|x| (x.proc_, x.refcon)));
    let r = match found {
        None => Err(ERR_ACCESSOR_NOT_FOUND),
        Some((proc_, refcon)) => {
            let f: extern "C" fn(u32, usize, u32, u32, usize, usize, usize) -> i32 = unsafe { core::mem::transmute(proc_) };
            let mut tok = Desc { ty: T_NULL, h: 0 };
            match f(want, &ctok as *const Desc as usize, container_class, form, &seld as *const Desc as usize, &mut tok as *mut Desc as usize, refcon) {
                0 => Ok(tok),
                e => Err(e),
            }
        }
    };
    dispose(&mut ctok);
    r
}

/// AEResolve(objectSpecifier, callbackFlags, *token) (59).
pub extern "C" fn AEResolve(spec: usize, _flags: u32, result: usize) -> i32 {
    match resolve(&get_desc(spec)) {
        Ok(t) => {
            put_desc(result, t);
            0
        }
        Err(e) => {
            put_desc(result, Desc { ty: T_NULL, h: 0 });
            e
        }
    }
}

/// AEDisposeToken(token) (17).
pub extern "C" fn AEDisposeToken(token: usize) -> i32 {
    AEDisposeDesc(token)
}

/// AEPutParamPtr(event, keyword, type, data, size) (52).
pub extern "C" fn AEPutParamPtr(event: usize, key: u32, ty: u32, ptr: usize, size: usize) -> i32 {
    let mut d = Desc::default();
    AECreateDesc(ty, ptr, size, &mut d as *mut Desc as usize);
    let r = AEPutParamDesc(event, key, &d as *const Desc as usize);
    dispose(&mut d);
    r
}

/// AEPutPtr(list, index, type, data, size) (53).
pub extern "C" fn AEPutPtr(list: usize, index: i32, ty: u32, ptr: usize, size: usize) -> i32 {
    let mut d = Desc::default();
    AECreateDesc(ty, ptr, size, &mut d as *mut Desc as usize);
    let r = AEPutDesc(list, index, &d as *const Desc as usize);
    dispose(&mut d);
    r
}

/// Every call OREXXSC may make that rs2 does not have.
pub extern "C" fn AENotHandled() -> i32 {
    ERR_NOT_HANDLED
}

#[allow(dead_code)]
fn unused() -> i32 {
    rc(Ok(()))
}

// ---- descriptors as bytes (events between processes) ----

/// type, kind (0xFF: no data), data (length, bytes), items (count;
/// keyword and descriptor each), attributes (the same).
pub fn encode(d: &Desc, out: &mut Vec<u8>) {
    out.extend_from_slice(&d.ty.to_le_bytes());
    match data(d) {
        None => out.push(0xff),
        Some(x) => {
            out.push(x.kind as u8);
            out.extend_from_slice(&(x.bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&x.bytes);
            for v in [&x.items, &x.attrs] {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                for (k, d) in v.iter() {
                    out.extend_from_slice(&k.to_le_bytes());
                    encode(d, out);
                }
            }
        }
    }
}

fn u32_at(b: &[u8], p: &mut usize) -> Option<u32> {
    let v = b.get(*p..*p + 4)?;
    *p += 4;
    Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

pub fn decode(b: &[u8], p: &mut usize) -> Option<Desc> {
    let ty = u32_at(b, p)?;
    let k = *b.get(*p)?;
    *p += 1;
    if k == 0xff {
        return Some(Desc { ty, h: 0 });
    }
    let n = u32_at(b, p)? as usize;
    let bytes = b.get(*p..*p + n)?.to_vec();
    *p += n;
    let mut lists = [Vec::new(), Vec::new()];
    for l in lists.iter_mut() {
        for _ in 0..u32_at(b, p)? {
            let key = u32_at(b, p)?;
            l.push((key, decode(b, p)?));
        }
    }
    let [items, attrs] = lists;
    let kind = match k {
        1 => Kind::List,
        2 => Kind::Record,
        3 => Kind::Event,
        _ => Kind::Bytes,
    };
    Some(make(ty, Data { kind, bytes, items, attrs }))
}
