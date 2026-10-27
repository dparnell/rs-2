//! The part of OLE 2 that WIN16 has: the task allocator (CoGetMalloc's
//! IMalloc, on global memory), which programs and Borland's BOLE16D ask
//! for when they start; the rest of COMPOBJ, OLE2 and STORAGE are stubs
//! (stubs.rs). A 16-bit COM object is a far pointer to a far pointer to
//! its table of `FAR CDECL` methods, `this` the first argument.

use thunk16::Args;

use crate::Global;

const S_OK: u32 = 0;
const E_NOINTERFACE: u32 = 0x8000_4002;

/// IID_IUnknown {00000000-0000-0000-C000-000000000046} and IID_IMalloc
/// {00000002-0000-0000-C000-000000000046}: their first dword, the rest
/// shared.
const IID_TAIL: [u8; 12] = [0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

thunk16::entries16! {
    dispatch = dispatch;
    m_queryinterface = 9001;
    m_addref = 9002;
    m_release = 9003;
    m_alloc = 9004;
    m_realloc = 9005;
    m_free = 9006;
    m_getsize = 9007;
    m_didalloc = 9008;
    m_heapminimize = 9009;
}

/// The allocator object (16:16).
static MALLOC: Global<u32> = Global::new(0);

fn malloc_object() -> u32 {
    let m = MALLOC.get();
    if *m != 0 {
        return *m;
    }
    unsafe extern "C" {
        fn m_queryinterface();
        fn m_addref();
        fn m_release();
        fn m_alloc();
        fn m_realloc();
        fn m_free();
        fn m_getsize();
        fn m_didalloc();
        fn m_heapminimize();
    }
    let methods: [unsafe extern "C" fn(); 9] = [m_queryinterface, m_addref, m_release, m_alloc, m_realloc, m_free, m_getsize, m_didalloc, m_heapminimize];
    // The object (its table pointer), then the table.
    let h = crate::kernel::global_alloc(0x40, 4 + 4 * 9);
    let p = thunk16::flat((h as u32) << 16);
    if p.is_null() {
        return 0;
    }
    let far = (h as u32) << 16;
    unsafe {
        (p as *mut u32).write_unaligned(far + 4);
        for (i, f) in methods.iter().enumerate() {
            (p.add(4 + 4 * i) as *mut u32).write_unaligned(thunk16::far(*f as *const u8));
        }
    }
    *m = far;
    far
}

/// A block of the allocator: a global block, its address.
fn alloc(n: u32) -> u32 {
    let h = crate::kernel::global_alloc(0, n.max(1));
    if h == 0 { 0 } else { ((h | 7) as u32) << 16 }
}

extern "C" fn dispatch(idx: u32, args: *const u8) -> u32 {
    let a = Args(args);
    match idx {
        // QueryInterface(this, riid, ppv)
        9001 => {
            let (this, riid, ppv) = (a.l(0), a.p(4), a.p(8));
            let iid = if riid.is_null() { [0xffu8; 16] } else { unsafe { core::ptr::read_unaligned(riid as *const [u8; 16]) } };
            let ok = iid[4..] == IID_TAIL && (iid[..4] == [0; 4] || iid[..4] == [2, 0, 0, 0]);
            if !ppv.is_null() {
                unsafe { (ppv as *mut u32).write_unaligned(if ok { this } else { 0 }) };
            }
            if ok { S_OK } else { E_NOINTERFACE }
        }
        // AddRef, Release: the allocator lives as long as the task.
        9002 | 9003 => 1,
        // Alloc(this, cb)
        9004 => alloc(a.l(4)),
        // Realloc(this, pv, cb)
        9005 => {
            let (pv, cb) = (a.l(4), a.l(8));
            if pv == 0 {
                return alloc(cb);
            }
            let h = crate::kernel::global_realloc((pv >> 16) as u16, cb.max(1), 0x02);
            if h == 0 { 0 } else { ((h | 7) as u32) << 16 }
        }
        // Free(this, pv)
        9006 => {
            let pv = a.l(4);
            if pv != 0 {
                crate::kernel::global_free((pv >> 16) as u16);
            }
            0
        }
        // GetSize(this, pv)
        9007 => crate::kernel::global_size((a.l(4) >> 16) as u16),
        // DidAlloc(this, pv): -1, "don't know".
        9008 => 0xffff,
        _ => 0,
    }
}

/// COMPOBJ's and OLE2's calls WIN16 has.
pub fn call(ord: u32, a: &mut thunk16::Pascal) -> Option<u32> {
    Some(match ord {
        // CoGetMalloc(memctx, ppMalloc): MEMCTX_TASK.
        7004 => {
            let (_ctx, pp) = (a.l(), a.p());
            let m = malloc_object();
            if !pp.is_null() {
                unsafe { (pp as *mut u32).write_unaligned(m) };
            }
            if m == 0 { 0x8007_000e } else { S_OK }
        }
        // CoInitialize(pMalloc), OleInitialize(pMalloc): S_OK.
        7002 | 7102 => {
            a.l();
            S_OK
        }
        // CoUninitialize(), OleUninitialize()
        7003 | 7103 => 0,
        // IsEqualGUID(a, b)
        7018 => {
            let (x, y) = (a.p(), a.p());
            if x.is_null() || y.is_null() {
                return Some(0);
            }
            (unsafe { core::slice::from_raw_parts(x, 16) == core::slice::from_raw_parts(y, 16) }) as u32
        }
        _ => return None,
    })
}
