//! Queues (QUECALLS): named, owned by the creating process, written by
//! any process that opened them. Elements carry a request word and a
//! pointer to data in shared memory.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

const QUE_FIFO: u32 = 0;
const QUE_LIFO: u32 = 1;
const QUE_PRIORITY: u32 = 2;

#[derive(Clone, Copy)]
struct Element {
    pid: u32,
    request: u32,
    len: u32,
    addr: u32,
    priority: u32,
    code: u32,
}

struct Queue {
    name: String,
    owner: u32,
    order: u32,
    elements: Vec<Element>,
    next_code: u32,
    /// Processes with the queue open.
    opens: Vec<u32>,
    /// Event semaphores to post when an element arrives.
    notify: Vec<u32>,
}

static QUEUES: SpinLock<BTreeMap<u32, Queue>> = SpinLock::new(BTreeMap::new());
static NEXT: SpinLock<u32> = SpinLock::new(1);
static ARRIVALS: WaitQueue = WaitQueue::new();

fn name_arg(c: &Ctx, p: u32) -> Result<String, u32> {
    let n = c.read_str(p, 260).ok_or(err::INVALID_ADDRESS)?.to_ascii_uppercase();
    if !n.starts_with("\\QUEUES\\") {
        return Err(err::QUE_INVALID_NAME);
    }
    Ok(n)
}

fn create(c: &Ctx, order: u32, name: String) -> Result<u32, u32> {
    let mut qs = QUEUES.lock();
    if qs.values().any(|q| q.name == name) {
        return Err(err::QUE_DUPLICATE);
    }
    let h = {
        let mut n = NEXT.lock();
        let h = *n;
        *n += 1;
        h
    };
    qs.insert(h, Queue { name, owner: c.process.pid, order: order & 3, elements: Vec::new(), next_code: 1, opens: alloc::vec![c.process.pid], notify: Vec::new() });
    Ok(h)
}

fn open(c: &Ctx, name: &str) -> Result<(u32, u32), u32> {
    let mut qs = QUEUES.lock();
    let Some((&h, q)) = qs.iter_mut().find(|(_, q)| q.name == name) else { return Err(err::QUE_NAME_NOT_EXIST) };
    q.opens.push(c.process.pid);
    Ok((h, q.owner))
}

fn write(c: &Ctx, h: u32, request: u32, len: u32, addr: u32, priority: u32) -> u32 {
    let notify = {
        let mut qs = QUEUES.lock();
        let Some(q) = qs.get_mut(&h) else { return err::QUE_INVALID_HANDLE };
        if !q.opens.contains(&c.process.pid) {
            return err::QUE_PROC_NO_ACCESS;
        }
        if priority > 15 {
            return err::QUE_INVALID_PRIORITY;
        }
        let code = q.next_code;
        q.next_code = q.next_code.wrapping_add(1).max(1);
        let e = Element { pid: c.process.pid, request, len, addr, priority, code };
        match q.order {
            QUE_LIFO => q.elements.insert(0, e),
            QUE_PRIORITY => {
                // Higher priority first; FIFO within a priority.
                let at = q.elements.iter().position(|x| x.priority < priority).unwrap_or(q.elements.len());
                q.elements.insert(at, e);
            }
            _ => q.elements.push(e),
        }
        q.notify.clone()
    };
    for s in notify {
        super::sem::post_event_handle(s);
    }
    ARRIVALS.wake_all();
    0
}

/// Read or peek. `code` 0 = first element, else the element with that
/// code. Returns the element.
fn take(c: &Ctx, h: u32, code: u32, wait: bool, remove: bool, hsem: u32) -> Result<Element, u32> {
    let pid = c.process.pid;
    let find = || -> Result<Option<Element>, u32> {
        let mut qs = QUEUES.lock();
        let Some(q) = qs.get_mut(&h) else { return Err(err::QUE_INVALID_HANDLE) };
        if q.owner != pid {
            return Err(err::QUE_PROC_NOT_OWNED);
        }
        let idx = if code == 0 {
            if q.elements.is_empty() { None } else { Some(0) }
        } else {
            match q.elements.iter().position(|e| e.code == code) {
                Some(i) => Some(i),
                None => return Err(err::QUE_ELEMENT_NOT_EXIST),
            }
        };
        Ok(idx.map(|i| if remove { q.elements.remove(i) } else { q.elements[i] }))
    };
    if let Some(e) = find()? {
        return Ok(e);
    }
    if !wait {
        if hsem != 0 {
            if let Some(q) = QUEUES.lock().get_mut(&h) {
                if !q.notify.contains(&hsem) {
                    q.notify.push(hsem);
                }
            }
        }
        return Err(err::QUE_EMPTY);
    }
    let mut got = None;
    let mut fail = None;
    ARRIVALS.wait_until(None, || match find() {
        Ok(Some(e)) => {
            got = Some(e);
            true
        }
        Ok(None) => false,
        Err(e) => {
            fail = Some(e);
            true
        }
    });
    if let Some(e) = fail {
        return Err(e);
    }
    got.ok_or(err::INTERRUPT)
}

// ---- 32-bit ----

pub fn dos_create_queue(c: &mut Ctx) -> u32 {
    let (phq, order, pname) = if c.is16 { (c.ptr(0), c.arg(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let name = match name_arg(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let _ = (QUE_FIFO, QUE_LIFO);
    match create(c, order, name) {
        Ok(h) => {
            if c.is16 { c.write_u16(phq, h as u16) } else { c.write_u32(phq, h) };
            0
        }
        Err(e) => e,
    }
}

pub fn dos_open_queue(c: &mut Ctx) -> u32 {
    let (ppid, phq, pname) = if c.is16 { (c.ptr(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let name = match name_arg(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match open(c, &name) {
        Ok((h, owner)) => {
            if c.is16 {
                c.write_u16(ppid, owner as u16);
                c.write_u16(phq, h as u16);
            } else {
                c.write_u32(ppid, owner);
                c.write_u32(phq, h);
            }
            0
        }
        Err(e) => e,
    }
}

pub fn dos_write_queue(c: &mut Ctx) -> u32 {
    // 32: (hq, ulRequest, cbData, pbData, ulPriority)
    // 16: (hq, usRequest, cbBuf, pBuf far, fPriority)
    let (h, req, len, addr, prio) = if c.is16 {
        (c.arg(0), c.arg(1), c.arg(2), c.ptr(3), c.arg(4) & 0xff)
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4))
    };
    write(c, h, req, len, addr, prio)
}

fn read_or_peek(c: &mut Ctx, peek: bool) -> u32 {
    // (hq, pRequest, pcbData, ppbuf, element(in) | pElement(in/out), fWait, pbPriority, hsem)
    let (h, preq, pcb, ppbuf, elem, wait_flag, pprio, hsem) = if c.is16 {
        (c.arg(0), c.ptr(1), c.ptr(2), c.ptr(3), if peek { c.ptr(4) } else { c.arg(4) }, c.arg(5) & 0xff, c.ptr(6), c.arg(7))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6), c.arg(7))
    };
    let code = if peek {
        if c.is16 { c.read_u16(elem).unwrap_or(0) as u32 } else { c.read_u32(elem).unwrap_or(0) }
    } else {
        elem
    };
    // For peek the element code names the element after which to look.
    let (look, remove) = if peek { (0, false) } else { (code, true) };
    let r = take(c, h, look, wait_flag == 0, remove, hsem);
    let e = match r {
        Ok(e) => e,
        Err(e) => return e,
    };
    if c.is16 {
        // QUEUERESULT: pidProcess, usEventCode (request)
        c.write_u16(preq, e.pid as u16);
        c.write_u16(preq + 2, e.request as u16);
        c.write_u16(pcb, e.len as u16);
        let far = (crate::vm::tile_selector(e.addr) as u32) << 16 | (e.addr & 0xffff);
        c.write_u32(ppbuf, far);
        if peek {
            c.write_u16(elem, e.code as u16);
        }
    } else {
        c.write_u32(preq, e.pid);
        c.write_u32(preq + 4, e.request);
        c.write_u32(pcb, e.len);
        c.write_u32(ppbuf, e.addr);
        if peek {
            c.write_u32(elem, e.code);
        }
    }
    c.write_u8(pprio, e.priority as u8);
    0
}

pub fn dos_read_queue(c: &mut Ctx) -> u32 {
    read_or_peek(c, false)
}

pub fn dos_peek_queue(c: &mut Ctx) -> u32 {
    read_or_peek(c, true)
}

pub fn dos_purge_queue(c: &mut Ctx) -> u32 {
    let mut qs = QUEUES.lock();
    let Some(q) = qs.get_mut(&c.arg(0)) else { return err::QUE_INVALID_HANDLE };
    if q.owner != c.process.pid {
        return err::QUE_PROC_NOT_OWNED;
    }
    q.elements.clear();
    0
}

pub fn dos_query_queue(c: &mut Ctx) -> u32 {
    let n = {
        let qs = QUEUES.lock();
        let Some(q) = qs.get(&c.arg(0)) else { return err::QUE_INVALID_HANDLE };
        q.elements.len() as u32
    };
    if c.is16 { c.write_u16(c.ptr(1), n as u16) } else { c.write_u32(c.arg(1), n) };
    0
}

pub fn dos_close_queue(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let pid = c.process.pid;
    let mut qs = QUEUES.lock();
    let Some(q) = qs.get_mut(&h) else { return err::QUE_INVALID_HANDLE };
    if let Some(i) = q.opens.iter().position(|&p| p == pid) {
        q.opens.remove(i);
    }
    // Closing by the owner deletes the queue.
    if q.owner == pid || q.opens.is_empty() {
        qs.remove(&h);
        drop(qs);
        ARRIVALS.wake_all();
    }
    0
}

pub fn process_ended(pid: u32) {
    let mut qs = QUEUES.lock();
    qs.retain(|_, q| {
        q.opens.retain(|&p| p != pid);
        q.owner != pid && !q.opens.is_empty()
    });
}
