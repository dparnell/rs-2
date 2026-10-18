//! Named pipe APIs (DosCreateNPipe and friends); the pipes themselves are
//! in crate::npipe.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::file::{add, get};
use super::{err, Ctx};
use crate::fs::Handle;
use crate::npipe::{self, End};

fn end(c: &Ctx, h: u32) -> Result<Arc<End>, u32> {
    match get(c, h) {
        Some(Handle::NPipe(e)) => Ok(e),
        Some(_) => Err(npipe::ERROR_BAD_PIPE),
        None => Err(err::INVALID_HANDLE),
    }
}

fn name_arg(c: &Ctx, p: u32) -> Result<alloc::string::String, u32> {
    let s = c.read_str(p, 260).ok_or(err::INVALID_ADDRESS)?;
    npipe::pipe_name(&s).ok_or(err::PATH_NOT_FOUND)
}

fn status(r: Result<u32, u32>) -> u32 {
    r.unwrap_or_else(|e| e)
}

/// DosCreateNPipe(pszName, phpipe, openmode, pipemode, cbOut, cbIn, msec)
pub fn dos_create_npipe(c: &mut Ctx) -> u32 {
    status((|| {
        let name = name_arg(c, c.arg(0))?;
        let open_mode = c.arg(2);
        let e = npipe::create(name, open_mode, c.arg(3), c.arg(4), c.arg(5), c.arg(6))?;
        let h = add(c, Handle::NPipe(e))?;
        if open_mode & npipe::NP_NOINHERIT != 0 {
            c.process.noinherit.lock().insert(h);
        }
        c.write_u32(c.arg(1), h);
        Ok(0)
    })())
}

pub fn dos_connect_npipe(c: &mut Ctx) -> u32 {
    status(end(c, c.arg(0)).map(|e| e.connect()))
}

pub fn dos_disconnect_npipe(c: &mut Ctx) -> u32 {
    status(end(c, c.arg(0)).map(|e| e.disconnect()))
}

/// DosPeekNPipe(hpipe, pBuf, cbBuf, pcbActual, pAvail, pState)
pub fn dos_peek_npipe(c: &mut Ctx) -> u32 {
    status((|| {
        let e = end(c, c.arg(0))?;
        let (data, in_pipe, in_msg, state) = e.peek(c.arg(2) as usize);
        if !data.is_empty() && !c.write(c.arg(1), &data) {
            return Err(err::INVALID_ADDRESS);
        }
        c.write_u32(c.arg(3), data.len() as u32);
        // AVAILDATA: cbpipe, cbmessage (USHORTs).
        c.write_u16(c.arg(4), in_pipe.min(0xffff) as u16);
        c.write_u16(c.arg(4) + 2, in_msg.min(0xffff) as u16);
        c.write_u32(c.arg(5), state);
        Ok(0)
    })())
}

pub fn dos_query_nph_state(c: &mut Ctx) -> u32 {
    status(end(c, c.arg(0)).map(|e| {
        c.write_u32(c.arg(1), e.state_bits());
        0
    }))
}

pub fn dos_set_nph_state(c: &mut Ctx) -> u32 {
    status(end(c, c.arg(0)).and_then(|e| e.set_state(c.arg(1)).map(|_| 0)))
}

/// DosQueryNPipeInfo(hpipe, level, pBuf, cbBuf): level 1 is PIPEINFO.
pub fn dos_query_npipe_info(c: &mut Ctx) -> u32 {
    status((|| {
        let e = end(c, c.arg(0))?;
        if c.arg(1) != 1 {
            return Err(err::INVALID_LEVEL);
        }
        let i = &e.inst;
        let mut v = Vec::new();
        v.extend_from_slice(&(i.out_size.min(0xffff) as u16).to_le_bytes());
        v.extend_from_slice(&(i.in_size.min(0xffff) as u16).to_le_bytes());
        v.push(i.max_instances() as u8);
        v.push(i.current_instances().min(0xff) as u8);
        v.push(i.name.len() as u8);
        v.extend_from_slice(i.name.as_bytes());
        v.push(0);
        let cb = c.arg(3) as usize;
        let n = v.len().min(cb);
        if !c.write(c.arg(2), &v[..n]) {
            return Err(err::INVALID_ADDRESS);
        }
        Ok(if n < v.len() { err::BUFFER_OVERFLOW } else { 0 })
    })())
}

/// DosSetNPipeSem(hpipe, hsem, key)
pub fn dos_set_npipe_sem(c: &mut Ctx) -> u32 {
    status((|| {
        let e = end(c, c.arg(0))?;
        if !super::sem::is_event_handle(c, c.arg(1)) {
            return Err(err::INVALID_HANDLE);
        }
        e.set_sem(c.arg(1), c.arg(2) as u16);
        Ok(0)
    })())
}

/// DosQueryNPipeSemState(hsem, pPipeSemState, cbBuf): PIPESEMSTATE records
/// (status, flag, key, avail) for the calling process's pipes.
pub fn dos_query_npipe_sem_state(c: &mut Ctx) -> u32 {
    let ends: Vec<Arc<End>> = c
        .process
        .handles
        .lock()
        .iter()
        .filter_map(|h| match h {
            Some(Handle::NPipe(e)) => Some(e.clone()),
            _ => None,
        })
        .collect();
    let recs = npipe::sem_records(&ends, c.arg(0));
    let mut v = Vec::new();
    for (s, f, k, a) in recs {
        v.push(s);
        v.push(f);
        v.extend_from_slice(&k.to_le_bytes());
        v.extend_from_slice(&a.to_le_bytes());
    }
    let cb = (c.arg(2) as usize) / 6 * 6;
    if cb < 6 {
        return err::BUFFER_OVERFLOW;
    }
    let n = v.len().min(cb);
    if !c.write(c.arg(1), &v[..n]) {
        return err::INVALID_ADDRESS;
    }
    if n < v.len() { err::BUFFER_OVERFLOW } else { 0 }
}

/// Write a message and read the reply.
fn transact(c: &Ctx, e: &End, out: u32, cb_out: u32, inb: u32, cb_in: u32) -> Result<(u32, u32), u32> {
    let data = c.read_vec(out, cb_out).ok_or(err::INVALID_ADDRESS)?;
    e.write(&data)?;
    let (reply, st) = e.read(cb_in as usize)?;
    if !reply.is_empty() && !c.write(inb, &reply) {
        return Err(err::INVALID_ADDRESS);
    }
    Ok((reply.len() as u32, st))
}

/// DosTransactNPipe(hpipe, pOutBuf, cbOut, pInBuf, cbIn, pcbRead)
pub fn dos_transact_npipe(c: &mut Ctx) -> u32 {
    status((|| {
        let e = end(c, c.arg(0))?;
        if e.state_bits() & npipe::NP_READMODE_MESSAGE == 0 {
            return Err(npipe::ERROR_BAD_PIPE);
        }
        let (n, st) = transact(c, &e, c.arg(1), c.arg(2), c.arg(3), c.arg(4))?;
        c.write_u32(c.arg(5), n);
        Ok(st)
    })())
}

pub fn dos_wait_npipe(c: &mut Ctx) -> u32 {
    status(name_arg(c, c.arg(0)).map(|n| npipe::wait(&n, c.arg(1))))
}

/// DosCallNPipe(pszName, pInBuf, cbIn, pOutBuf, cbOut, pcbActual, msec):
/// open, transact, close.
pub fn dos_call_npipe(c: &mut Ctx) -> u32 {
    status((|| {
        let name = name_arg(c, c.arg(0))?;
        let e = match npipe::open(&name) {
            Err(npipe::ERROR_PIPE_BUSY) => match npipe::wait(&name, c.arg(6)) {
                0 => npipe::open(&name)?,
                r => return Err(r),
            },
            r => r?,
        };
        if e.state_bits() & npipe::NP_TYPE_MESSAGE == 0 {
            return Err(npipe::ERROR_BAD_PIPE);
        }
        e.set_state(npipe::NP_READMODE_MESSAGE)?;
        let (n, st) = transact(c, &e, c.arg(1), c.arg(2), c.arg(3), c.arg(4))?;
        c.write_u32(c.arg(5), n);
        Ok(st)
    })())
}
