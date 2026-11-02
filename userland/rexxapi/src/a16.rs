//! The 16-bit entry points (`_pascal far`, 16:16 pointers): the OS/2 1.x
//! REXX API (RXSUBCOMREGISTER... RXMACROREORDER) on the 32-bit one, and
//! calls of 16-bit subcommand handlers and functions registered through
//! it. IBM's 16-bit CMD.EXE registers its "CMD" environment with
//! RXSUBCOMREGISTER and an SCBLOCK:
//!
//! ```text
//! +0 next  +4 name  +8 DLL name  +12 procedure  +16 user (8 bytes)
//! +24 address (16:16)  +28 module handle  +30 drop authority  +32 PID
//! +34 session
//! ```

use alloc::vec::Vec;
use core::ptr::null_mut;

use thunk16::{Pascal, PascalCall, far, flat};

use crate::{KIND_EXIT, KIND_FUNC, KIND_SUBCOM, RxString};

thunk16::entries16_pascal! {
    dispatch = dispatch16;
    RXMACROLOAD = 30, 10;
    RXMACROSAVE = 31, 10;
    RXSUBCOMLOAD = 32, 8;
    RXMACRODROP = 33, 4;
    RXEXITQUERY = 34, 16;
    RXMACROERASE = 35, 0;
    RXSUBCOMDROP = 36, 8;
    RXMACROQUERY = 37, 8;
    RXMACROCHANGE = 38, 10;
    RXSUBCOMQUERY = 39, 16;
    RXFUNCTIONCALL = 40, 22;
    RXEXITREGISTER = 41, 4;
    RXMACROREORDER = 42, 6;
    RXSUBCOMEXECUTE = 43, 24;
    RXSUBCOMREGISTER = 44, 4;
    RXFUNCTIONQUERY = 45, 4;
    RXFUNCTIONREGISTER = 46, 14;
    RXEXITDROP = 47, 8;
    RXFUNCTIONDEREGISTER = 48, 4;
}

fn rx16(p: *const u8) -> RxString {
    unsafe { RxString { len: (p as *const u32).read_unaligned(), ptr: flat((p.add(4) as *const u32).read_unaligned()) } }
}

fn put_rx16(p: *mut u8, r: &RxString) {
    unsafe {
        (p as *mut u32).write_unaligned(r.len);
        (p.add(4) as *mut u32).write_unaligned(far(r.ptr));
    }
}

/// A 16-bit RXSTRING (far pointer) in memory a 16-bit procedure can use.
#[repr(C, align(16))]
struct Rx16([u8; 8]);

/// Call a 16-bit subcommand handler: USHORT handler(PRXSTRING command,
/// PUSHORT flags, PRXSTRING result).
pub fn call_subcom16(addr: u32, cmd: *const RxString, flags: *mut u16, ret: *mut RxString) -> u32 {
    let mut c16 = Rx16([0; 8]);
    let mut r16 = Rx16([0; 8]);
    let mut fl: u16 = 0;
    put_rx16(c16.0.as_mut_ptr(), unsafe { &*cmd });
    put_rx16(r16.0.as_mut_ptr(), unsafe { &*ret });
    let r = PascalCall::new().l(far(c16.0.as_ptr())).l(far(&fl as *const u16 as *const u8)).l(far(r16.0.as_ptr())).call(addr);
    unsafe {
        *ret = rx16(r16.0.as_ptr());
        if !flags.is_null() {
            *flags = fl;
        }
    }
    let _ = &mut c16;
    r & 0xffff
}

/// Call a 16-bit external function: USHORT f(PSZ name, USHORT argc,
/// PRXSTRING argv, PSZ queue, PRXSTRING result).
pub fn call_function16(addr: u32, name: *const u8, argc: u32, argv: *const RxString, queue: *const u8, ret: *mut RxString) -> u32 {
    let mut a16: Vec<u8> = alloc::vec![0; argc as usize * 8];
    for i in 0..argc as usize {
        put_rx16(unsafe { a16.as_mut_ptr().add(i * 8) }, unsafe { &*argv.add(i) });
    }
    let mut r16 = Rx16([0; 8]);
    put_rx16(r16.0.as_mut_ptr(), unsafe { &*ret });
    let r = PascalCall::new()
        .l(far(name))
        .w(argc as u16)
        .l(far(a16.as_ptr()))
        .l(far(queue))
        .l(far(r16.0.as_ptr()))
        .call(addr);
    unsafe { *ret = rx16(r16.0.as_ptr()) };
    r & 0xffff
}

fn scb_register(kind: u32, scb: *const u8) -> u32 {
    if scb.is_null() {
        return 1003;
    }
    unsafe {
        let name = flat((scb.add(4) as *const u32).read_unaligned());
        let dll = flat((scb.add(8) as *const u32).read_unaligned());
        let proc_ = flat((scb.add(12) as *const u32).read_unaligned());
        let user = scb.add(16);
        let addr = (scb.add(24) as *const u32).read_unaligned();
        let drop = (scb.add(30) as *const u16).read_unaligned() as u32;
        if dll.is_null() || *dll == 0 {
            crate::register_exe(kind, name, addr as usize, user, true)
        } else {
            crate::register_dll(kind, name, dll, proc_, user, drop)
        }
    }
}

extern "C" fn dispatch16(idx: u32, args: *const u8) -> u32 {
    let size = match idx {
        30 | 31 | 38 => 10,
        32 | 36 | 37 | 47 => 8,
        34 | 39 => 16,
        40 => 22,
        42 => 6,
        43 => 24,
        46 => 14,
        35 => 0,
        _ => 4,
    };
    let mut a = Pascal::new(args, size);
    match idx {
        44 => scb_register(KIND_SUBCOM, a.p()),
        41 => scb_register(KIND_EXIT, a.p()),
        36 | 47 => {
            let (n, d) = (a.p(), a.p());
            crate::deregister(if idx == 36 { KIND_SUBCOM } else { KIND_EXIT }, n, d)
        }
        39 | 34 => {
            let (n, d, flag, user) = (a.p(), a.p(), a.p() as *mut u16, a.p());
            crate::query(if idx == 39 { KIND_SUBCOM } else { KIND_EXIT }, n, d, flag, user)
        }
        32 => {
            let (n, d) = (a.p(), a.p());
            crate::load_subcom(n, d)
        }
        // RXSUBCOMEXECUTE(env, dll, command, flags, rc, result)
        43 => {
            let (n, d, cmd, flags, rc, ret) = (a.p(), a.p(), a.p(), a.p() as *mut u16, a.p() as *mut u16, a.p());
            let c = rx16(cmd);
            let mut r = rx16(ret);
            let x = crate::call_subcom(n, d, &c, flags, rc, &mut r);
            put_rx16(ret, &r);
            x
        }
        // RXFUNCTIONREGISTER(name, dll, entry, type): 1 DLL, 2 address
        46 => {
            let (n, d, e) = (a.p(), a.p(), a.l());
            let ty = a.w();
            if ty == 2 {
                crate::register_exe(KIND_FUNC, n, e as usize, core::ptr::null(), true)
            } else {
                crate::register_function_dll(n, d, flat(e))
            }
        }
        48 => crate::deregister_function(a.p()),
        45 => crate::query_function(a.p()),
        // RXFUNCTIONCALL(name, argc, argv, rc, result, queue)
        40 => {
            let (n, argc, argv, rc, ret, q) = (a.p(), a.w() as u32, a.p(), a.p() as *mut u16, a.p(), a.p());
            let args: Vec<RxString> = (0..argc as usize).map(|i| rx16(unsafe { argv.add(i * 8) })).collect();
            let mut r = rx16(ret);
            let x = crate::call_function(n, argc, args.as_ptr(), rc, &mut r, q);
            put_rx16(ret, &r);
            x
        }
        38 => {
            let (n, f, pos) = (a.p(), a.p(), a.w() as u32);
            crate::add_macro(n, f, pos)
        }
        33 => crate::drop_macro(a.p()),
        35 => crate::clear_macro_space(),
        30 | 31 => {
            let (cnt, list, file) = (a.w() as u32, a.p(), a.p());
            let names: Vec<*const u8> = (0..cnt as usize).map(|i| flat(unsafe { (list.add(i * 4) as *const u32).read_unaligned() }) as *const u8).collect();
            if idx == 30 {
                crate::load_macro_space(cnt, names.as_ptr(), file)
            } else {
                crate::save_macro_space(cnt, names.as_ptr(), file)
            }
        }
        37 => {
            let (n, pos) = (a.p(), a.p() as *mut u16);
            crate::query_macro(n, pos)
        }
        42 => {
            let (n, pos) = (a.p(), a.w() as u32);
            crate::reorder_macro(n, pos)
        }
        _ => {
            let _ = null_mut::<u8>();
            1
        }
    }
}
