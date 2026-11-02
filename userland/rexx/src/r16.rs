//! The 16-bit entry points (`_pascal far`, 16:16 pointers), as IBM's
//! REXX.DLL converts them for the 32-bit ones: REXXSAA (RexxStart), RXVAR
//! (RexxVariablePool), RXHALTSET, RXTRACESET, RXTRACERESET and
//! RXBREAKCLEANUP. IBM's 16-bit CMD.EXE runs REXX procedures through
//! REXXSAA. 16-bit RXSTRINGs hold far pointers; a 16-bit RXSYSEXIT is a
//! far name and a SHORT code (6 bytes).

use alloc::vec::Vec;
use core::ptr::null_mut;

use thunk16::{Pascal, far, flat};

use crate::{RxString, RxSysExit, ShvBlock};

thunk16::entries16_pascal! {
    dispatch = dispatch16;
    RXTRACERESET = 6, 4;
    RXBREAKCLEANUP = 7, 0;
    RXVAR = 8, 4;
    REXXSAA = 9, 32;
    RXHALTSET = 10, 4;
    RXTRACESET = 11, 4;
}

fn rx16(p: *const u8) -> RxString {
    unsafe {
        let len = (p as *const u32).read_unaligned();
        let fp = (p.add(4) as *const u32).read_unaligned();
        RxString { len, ptr: flat(fp) }
    }
}

fn put_rx16(p: *mut u8, r: &RxString) {
    unsafe {
        (p as *mut u32).write_unaligned(r.len);
        (p.add(4) as *mut u32).write_unaligned(far(r.ptr));
    }
}

extern "C" fn dispatch16(idx: u32, args: *const u8) -> u32 {
    match idx {
        // REXXSAA(argc, argv, name, instore, env, calltype, exits, retc, result)
        9 => {
            let mut a = Pascal::new(args, 32);
            let argc = a.i();
            let argv = a.p();
            let name = a.p();
            let instore = a.p();
            let env = a.p();
            let calltype = a.i();
            let exits = a.p();
            let retc = a.p() as *mut i16;
            let result = a.p();
            let argv32: Vec<RxString> = (0..argc.max(0) as usize).map(|i| rx16(unsafe { argv.add(i * 8) })).collect();
            let mut ins = [RxString { len: 0, ptr: null_mut() }; 2];
            if !instore.is_null() {
                ins = [rx16(instore), rx16(unsafe { instore.add(8) })];
            }
            let mut ex: Vec<RxSysExit> = Vec::new();
            if !exits.is_null() {
                let mut i = 0;
                loop {
                    let e = unsafe { exits.add(i * 6) };
                    let code = unsafe { (e.add(4) as *const i16).read_unaligned() } as i32;
                    let n = flat(unsafe { (e as *const u32).read_unaligned() });
                    ex.push(RxSysExit { name: n, code });
                    if code == 0 {
                        break;
                    }
                    i += 1;
                }
            }
            let mut res = RxString { len: 0, ptr: null_mut() };
            let r = crate::rexx_start(
                argc,
                argv32.as_ptr(),
                name,
                if instore.is_null() { null_mut() } else { ins.as_mut_ptr() },
                env,
                calltype,
                if exits.is_null() { core::ptr::null() } else { ex.as_ptr() },
                retc,
                if result.is_null() { null_mut() } else { &mut res },
            );
            if !instore.is_null() {
                put_rx16(instore, &ins[0]);
                put_rx16(unsafe { instore.add(8) }, &ins[1]);
            }
            if !result.is_null() {
                put_rx16(result, &res);
            }
            r as u16 as u32
        }
        // RXVAR(PSHVBLOCK)
        8 => {
            let mut a = Pascal::new(args, 4);
            let mut p = a.p();
            let mut all = 0u32;
            while !p.is_null() {
                let mut b = ShvBlock {
                    next: null_mut(),
                    name: rx16(unsafe { p.add(4) }),
                    value: rx16(unsafe { p.add(12) }),
                    namelen: unsafe { (p.add(20) as *const u32).read_unaligned() },
                    valuelen: unsafe { (p.add(24) as *const u32).read_unaligned() },
                    code: unsafe { *p.add(28) },
                    ret: 0,
                };
                let r = crate::variable_pool(&mut b);
                if r == 144 {
                    return r;
                }
                all |= r;
                unsafe {
                    put_rx16(p.add(4), &b.name);
                    put_rx16(p.add(12), &b.value);
                    (p.add(20) as *mut u32).write_unaligned(b.namelen);
                    (p.add(24) as *mut u32).write_unaligned(b.valuelen);
                    *p.add(29) = b.ret;
                    p = flat((p as *const u32).read_unaligned());
                }
            }
            all
        }
        // RXHALTSET / RXTRACESET / RXTRACERESET(pid, tid)
        6 | 10 | 11 => {
            let mut a = Pascal::new(args, 4);
            let pid = a.w() as u32;
            let tid = a.w() as u32;
            match idx {
                10 => crate::set_halt(pid, tid),
                11 => crate::set_trace(pid, tid),
                _ => crate::reset_trace(pid, tid),
            }
        }
        // RXBREAKCLEANUP: nothing is left over after a break
        _ => 0,
    }
}
