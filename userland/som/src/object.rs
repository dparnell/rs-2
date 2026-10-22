//! SOMObject's methods, among them the generic initializer, destructor,
//! copy and assignment walkers every class has unless it defines its
//! own: they call the ancestors' ones as the control structure's mask
//! says and then the class's legacy somInit (or first its somUninit).

use crate::id::{self, SomId};
use crate::rt::*;
use crate::sys::*;

type Init = extern "C" fn(usize, *mut Ctrl);
type Destruct = extern "C" fn(usize, u32, *mut Ctrl);
type CopyInit = extern "C" fn(usize, *mut Ctrl, usize);
type Plain = extern "C" fn(usize);

unsafe fn f<T: Copy>(p: usize) -> T {
    unsafe { core::mem::transmute_copy(&p) }
}

/// The class and control a walker works with: the object's class and a
/// fresh copy of its control when called without one.
unsafe fn start(obj: usize, ctrl: *mut Ctrl, local: *mut Ctrl, which: u32) -> (*mut Class, *mut Ctrl) {
    unsafe {
        if ctrl.is_null() {
            let c = class_of_obj(obj);
            if !(*c).ctrl_built {
                build_ctrls(c);
            }
            let pm = &*(*c).pmtab;
            *local = match which {
                0 => pm.init_ctrl,
                1 => pm.destruct_ctrl,
                _ => pm.assign_ctrl,
            };
            (c, local)
        } else {
            let cls = *((*ctrl).info as *const usize);
            (rec(cls), ctrl)
        }
    }
}

pub extern "C" fn somDefaultInit(obj: usize, ctrl: *mut Ctrl) {
    unsafe {
        let mut local = Ctrl::empty();
        let (c, ctrl) = start(obj, ctrl, &mut local, 0);
        if c.is_null() {
            return;
        }
        let entry = &*((*ctrl).info as *const InitInfo);
        let legacy = entry.legacy_init;
        let mask = (*ctrl).mask;
        let np = (*c).parents.len();
        (*ctrl).info += (*ctrl).info_size as usize;
        (*ctrl).mask = mask.add(np);
        for i in 0..np {
            if *mask.add(i) & 1 != 0 {
                let next = &*((*ctrl).info as *const InitInfo);
                f::<Init>(next.default_init)(obj, ctrl);
            }
        }
        if legacy != 0 {
            f::<Plain>(legacy)(obj);
        }
    }
}

pub extern "C" fn somDestruct(obj: usize, do_free: u32, ctrl: *mut Ctrl) {
    unsafe {
        let mut local = Ctrl::empty();
        let (c, ctrl) = start(obj, ctrl, &mut local, 1);
        if c.is_null() {
            return;
        }
        let entry = &*((*ctrl).info as *const DestructInfo);
        if entry.legacy_uninit != 0 {
            f::<Plain>(entry.legacy_uninit)(obj);
        }
        let mask = (*ctrl).mask;
        let np = (*c).parents.len();
        (*ctrl).info += (*ctrl).info_size as usize;
        (*ctrl).mask = mask.add(np);
        for i in 0..np {
            if *mask.add(i) & 1 != 0 {
                let next = &*((*ctrl).info as *const DestructInfo);
                f::<Destruct>(next.default_destruct)(obj, 0, ctrl);
            }
        }
        if do_free & 0xff != 0 {
            crate::class::deallocate((*c).obj, obj);
        }
    }
}

/// Copy initializers and assignment: the ancestors' ones, then the
/// class's own data copied.
unsafe fn copy_walk(obj: usize, ctrl: *mut Ctrl, from: usize, which: u32, field: usize) {
    unsafe {
        let mut local = Ctrl::empty();
        let (c, ctrl) = start(obj, ctrl, &mut local, which);
        if c.is_null() {
            return;
        }
        let entry = (*ctrl).info as *const usize;
        let mask = (*ctrl).mask;
        let np = (*c).parents.len();
        (*ctrl).info += (*ctrl).info_size as usize;
        (*ctrl).mask = mask.add(np);
        for i in 0..np {
            if *mask.add(i) & 1 != 0 {
                let next = (*ctrl).info as *const usize;
                let p = *next.add(field);
                if p != 0 {
                    f::<CopyInit>(p)(obj, ctrl, from);
                } else {
                    (*ctrl).info += (*ctrl).info_size as usize;
                }
            }
        }
        // The class's data: at the offset its info gives (the object and
        // the source share their leftmost ancestors' layout).
        let off = if which == 2 { *(entry.add(6)) } else { *(entry.add(5)) } as usize;
        let n = (*c).data_size;
        if from != 0 && n != 0 {
            let src_class = class_of_obj(from);
            let src_off = if src_class.is_null() { None } else { (*src_class).data_offset_of(c) };
            if let Some(so) = src_off {
                core::ptr::copy((from + so) as *const u8, (obj + off) as *mut u8, n);
            }
        }
    }
}

pub extern "C" fn somDefaultCopyInit(obj: usize, ctrl: *mut Ctrl, from: usize) {
    unsafe { copy_walk(obj, ctrl, from, 0, 2) }
}
pub extern "C" fn somDefaultConstCopyInit(obj: usize, ctrl: *mut Ctrl, from: usize) {
    unsafe { copy_walk(obj, ctrl, from, 0, 3) }
}
pub extern "C" fn somDefaultVCopyInit(obj: usize, ctrl: *mut Ctrl, from: usize) {
    unsafe { copy_walk(obj, ctrl, from, 0, 2) }
}
pub extern "C" fn somDefaultConstVCopyInit(obj: usize, ctrl: *mut Ctrl, from: usize) {
    unsafe { copy_walk(obj, ctrl, from, 0, 3) }
}
pub extern "C" fn somDefaultAssign(obj: usize, ctrl: *mut Ctrl, from: usize) -> usize {
    unsafe { copy_walk(obj, ctrl, from, 2, 1) };
    obj
}
pub extern "C" fn somDefaultConstAssign(obj: usize, ctrl: *mut Ctrl, from: usize) -> usize {
    unsafe { copy_walk(obj, ctrl, from, 2, 2) };
    obj
}
pub extern "C" fn somDefaultVAssign(obj: usize, ctrl: *mut Ctrl, from: usize) -> usize {
    unsafe { copy_walk(obj, ctrl, from, 2, 1) };
    obj
}
pub extern "C" fn somDefaultConstVAssign(obj: usize, ctrl: *mut Ctrl, from: usize) -> usize {
    unsafe { copy_walk(obj, ctrl, from, 2, 2) };
    obj
}

pub extern "C" fn somInit(_obj: usize) {}
pub extern "C" fn somUninit(_obj: usize) {}

pub extern "C" fn somFree(obj: usize) {
    // somDestruct(obj, TRUE, NULL) through the object's own destructor.
    crate::call_om_destruct(obj, 1);
}

pub extern "C" fn somGetClass(obj: usize) -> usize {
    unsafe { (**(obj as *const *const Mtab)).class_object }
}

pub extern "C" fn somGetClassName(obj: usize) -> *const u8 {
    unsafe { (**(obj as *const *const Mtab)).class_name }
}

pub extern "C" fn somGetSize(obj: usize) -> i32 {
    unsafe { (**(obj as *const *const Mtab)).instance_size }
}

pub extern "C" fn somIsA(obj: usize, cls: usize) -> u32 {
    unsafe {
        let c = class_of_obj(obj);
        let r = rec(cls);
        (!c.is_null() && !r.is_null() && (*c).descends_from(r)) as u32
    }
}

pub extern "C" fn somIsInstanceOf(obj: usize, cls: usize) -> u32 {
    unsafe { (class_of_obj(obj) == rec(cls) && cls != 0) as u32 }
}

pub extern "C" fn somRespondsTo(obj: usize, mid: SomId) -> u32 {
    unsafe {
        let c = class_of_obj(obj);
        (!c.is_null() && lookup(&*c, id::name_bytes(mid)).is_some()) as u32
    }
}

/// Call a method with arguments from a va_list (which starts with the
/// object), storing its result in `*ret`.
pub unsafe fn apply(proc_: usize, m: Option<&MethodDef>, ret: *mut u8, ap: *const u32) {
    let n = match m {
        Some(m) if m.va_size != 0 => m.va_size.div_ceil(4),
        _ => 16,
    };
    let call_type = m.map_or(0, |m| m.call_type) & !1;
    if call_type == 8 {
        // A structure: its address goes first.
        let mut args = alloc::vec::Vec::with_capacity(n as usize + 1);
        args.push(ret as u32);
        args.extend_from_slice(unsafe { core::slice::from_raw_parts(ap, n as usize) });
        call_args(proc_, args.as_ptr(), args.len() as u32);
        return;
    }
    match call_type {
        2 | 4 | 6 => {
            let v = call_args_f64(proc_, ap, n);
            if !ret.is_null() {
                unsafe {
                    if call_type == 2 {
                        *(ret as *mut f32) = v as f32;
                    } else {
                        *(ret as *mut f64) = v;
                    }
                }
            }
        }
        _ => {
            let v = call_args(proc_, ap, n);
            if !ret.is_null() {
                unsafe {
                    match call_type {
                        10 => *ret = v as u8,
                        12 => *(ret as *mut u16) = v as u16,
                        _ => *(ret as *mut u32) = v as u32,
                    }
                }
            }
        }
    }
}

unsafe fn dispatch_in(c: *mut Class, ret: *mut u8, mid: SomId, ap: *const u32) -> u32 {
    if c.is_null() {
        return 0;
    }
    let Some((p, m)) = lookup(unsafe { &*c }, id::name_bytes(mid)) else { return 0 };
    if p == 0 {
        return 0;
    }
    unsafe { apply(p, m, ret, ap) };
    1
}

pub extern "C" fn somDispatch(obj: usize, ret: *mut u8, mid: SomId, ap: *const u32) -> u32 {
    unsafe { dispatch_in(class_of_obj(obj), ret, mid, ap) }
}

pub extern "C" fn somClassDispatch(_obj: usize, cls: usize, ret: *mut u8, mid: SomId, ap: *const u32) -> u32 {
    unsafe { dispatch_in(rec(cls), ret, mid, ap) }
}

pub extern "C" fn somDispatchV(obj: usize, mid: SomId, _desc: SomId, ap: *const u32) {
    let mut r = [0u32; 4];
    somDispatch(obj, r.as_mut_ptr() as *mut u8, mid, ap);
}

pub extern "C" fn somDispatchL(obj: usize, mid: SomId, _desc: SomId, ap: *const u32) -> u32 {
    let mut r = [0u32; 4];
    somDispatch(obj, r.as_mut_ptr() as *mut u8, mid, ap);
    r[0]
}

pub extern "C" fn somDispatchA(obj: usize, mid: SomId, desc: SomId, ap: *const u32) -> u32 {
    somDispatchL(obj, mid, desc, ap)
}

pub extern "C" fn somDispatchD(obj: usize, mid: SomId, _desc: SomId, ap: *const u32) -> f64 {
    let mut r = [0f64; 2];
    unsafe {
        let c = class_of_obj(obj);
        if let Some((p, m)) = lookup(&*c, id::name_bytes(mid)) {
            let n = m.map_or(16, |m| if m.va_size != 0 { m.va_size.div_ceil(4) } else { 16 });
            r[0] = call_args_f64(p, ap, n);
        }
    }
    r[0]
}

pub extern "C" fn somCastObj(_obj: usize, _cls: usize) -> u32 {
    0
}

pub extern "C" fn somResetObj(_obj: usize) -> u32 {
    0
}

pub extern "C" fn somPrintSelf(obj: usize) -> usize {
    crate::printf(
        b"{An instance of class %s at address %08X}\n\0",
        &[somGetClassName(obj) as u32, obj as u32],
    );
    obj
}

pub extern "C" fn somDumpSelf(obj: usize, level: i32) {
    crate::prefix_level(level);
    crate::printf(b"{An instance of class %s at address %08X\n\0", &[somGetClassName(obj) as u32, obj as u32]);
    crate::call_om(obj, om::DUMP_SELF_INT, &[obj as u32, (level + 1) as u32]);
    crate::prefix_level(level);
    crate::printf(b"}\n\0", &[]);
}

pub extern "C" fn somDumpSelfInt(_obj: usize, _level: i32) {}

pub extern "C" fn somPrivate(_obj: usize) -> usize {
    0
}
