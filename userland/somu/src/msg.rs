//! somutMsgManager: messages from an OS/2 message file (IBM's keeps the
//! file's name and reads a message with DosGetMessage; SOMEM and SOMUC
//! use it for their messages).

use somsup::{call, data, methods};

use crate::*;

#[repr(C)]
struct Data {
    file: *mut u8,
    open: u8,
    _pad: [u8; 7],
}

fn me(obj: usize) -> &'static mut Data {
    data::<Data>(p!(somutMsgManagerCClassData), obj)
}

extern "C" fn init(obj: usize) {
    let d = me(obj);
    d.file = somsup::malloc(0x100);
    d.open = 0;
}

extern "C" fn uninit(obj: usize) {
    let d = me(obj);
    if !d.file.is_null() {
        somsup::free(d.file);
        d.file = core::ptr::null_mut();
    }
}

/// somutMsgCatOpen(rc, file): remember the message file (at most 255
/// characters; *rc -1 when longer).
extern "C" fn cat_open(obj: usize, rc: *mut i32, name: *const u8) {
    let d = me(obj);
    let n = unsafe { somsup::cstr(name) };
    if n.len() > 0xff || d.file.is_null() {
        unsafe { *rc = -1 };
        return;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(n.as_ptr(), d.file, n.len());
        *d.file.add(n.len()) = 0;
        *rc = 0;
    }
    d.open = 1;
}

unsafe extern "C" {
    #[link_name = "SOM.80"]
    fn somfixMsgTemplate(a: *mut u8, b: usize) -> *mut u8;
}

/// somutMsgGetMessage(rc, set, number, buffer): message `number` of the
/// file into a 256-byte buffer without its line end.
extern "C" fn get_message(obj: usize, rc: *mut i32, _set: u32, number: u32, buf: *mut u8) {
    let d = me(obj);
    if d.open == 0 {
        unsafe {
            *buf = 0;
            *rc = -1;
        }
        return;
    }
    let mut len = 0u32;
    let r = unsafe { os2::DosGetMessage(core::ptr::null(), 0, buf, 0x100, number, d.file, &mut len) };
    unsafe { *rc = r as i32 };
    if r == 0 && len > 1 {
        unsafe {
            *buf.add(len as usize - 2) = 0;
            somfixMsgTemplate(buf, 0);
            *rc = 0;
        }
    }
}

/// M_somutMsgManager::somutCreateMsgManager(file): a manager with the
/// file open.
extern "C" fn create(cls: usize, file: *const u8) -> usize {
    let o = somsup::new_object(cls);
    if o == 0 {
        return 0;
    }
    let mut rc = 0i32;
    call(p!(somutMsgManagerClassData), 1, &[o, &mut rc as *mut i32 as usize, file as usize]);
    if rc != 0 {
        somsup::printf(b"SOM Message: catopen for %s failed\n\0", &[file as u32]);
    }
    o
}

methods! {
    __m_init => init, 1;
    __m_uninit => uninit, 1;
    __m_cat_open => cat_open, 3;
    __m_get_message => get_message, 5;
    __m_create => create, 2;
}
