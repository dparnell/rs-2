//! SOMBuf: a gap buffer of fixed-size items, as IBM's: the items are in
//! `[bBuf, bGap)` and `[eGap, eBuf)`; an item goes in at the start of the
//! gap's far side after the gap moved to its position. sombfGet, ...
//! return the address of an item in the buffer.

use somsup::{call, data, methods};

use crate::*;

#[repr(C)]
struct Buf {
    bbuf: usize,
    ebuf: usize,
    bgap: usize,
    egap: usize,
    size: usize,
}

fn me(obj: usize) -> &'static mut Buf {
    data::<Buf>(p!(SOMBufCClassData), obj)
}

impl Buf {
    /// The address of item `i`.
    fn at(&self, i: i32) -> usize {
        let p = self.bbuf.wrapping_add((i as usize).wrapping_mul(self.size));
        if p < self.bgap { p } else { p + (self.egap - self.bgap) }
    }
    fn count(&self) -> i32 {
        if self.size == 0 {
            return 0;
        }
        ((self.bgap + self.ebuf - self.bbuf - self.egap) / self.size) as i32
    }
    /// Move the gap so that it starts where `to` (an address of an item)
    /// is: items after the gap move before it, or items before it move
    /// after it.
    fn move_gap(&mut self, to: usize) {
        unsafe {
            if self.egap <= to {
                let n = to - self.egap;
                core::ptr::copy(self.egap as *const u8, self.bgap as *mut u8, n);
                self.bgap += n;
                self.egap = to;
            } else {
                let n = self.bgap - to;
                core::ptr::copy(to as *const u8, (self.egap - n) as *mut u8, n);
                self.egap -= n;
                self.bgap = to;
            }
        }
    }
    /// More room (twice the item size at least); the gap at the end.
    fn grow(&mut self) {
        let used = self.ebuf - self.bbuf;
        let more = used.max(self.size * 2).max(4);
        if self.egap != self.ebuf {
            self.move_gap(self.ebuf);
        }
        let p = if self.bbuf == 0 { somsup::malloc(more) as usize } else { somsup::realloc(self.bbuf as *mut u8, used + more) as usize };
        if p == 0 {
            return;
        }
        let content = self.bgap - self.bbuf;
        self.bbuf = p;
        self.ebuf = p + used + more;
        self.bgap = p + content;
        self.egap = self.ebuf;
    }
    fn put(&mut self, at: usize, item: *const u8) {
        unsafe {
            if item.is_null() {
                core::ptr::write_bytes(at as *mut u8, 0, self.size);
            } else {
                core::ptr::copy_nonoverlapping(item, at as *mut u8, self.size);
            }
        }
    }
}

extern "C" fn init(obj: usize) {
    *me(obj) = Buf { bbuf: 0, ebuf: 0, bgap: 0, egap: 0, size: 4 };
}

extern "C" fn uninit(obj: usize) {
    let b = me(obj);
    if b.bbuf != 0 {
        somsup::free(b.bbuf as *mut u8);
    }
    b.bbuf = 0;
}

/// sombfInit(count, itemSize).
extern "C" fn init_buf(obj: usize, _count: i32, size: i32) {
    let b = me(obj);
    if b.bbuf != 0 {
        somsup::free(b.bbuf as *mut u8);
    }
    *b = Buf { bbuf: 0, ebuf: 0, bgap: 0, egap: 0, size: size.max(1) as usize };
    b.grow();
}

extern "C" fn count(obj: usize) -> i32 {
    me(obj).count()
}

extern "C" fn item_size(obj: usize) -> i32 {
    me(obj).size as i32
}

extern "C" fn get(obj: usize, i: i32) -> usize {
    me(obj).at(i)
}

extern "C" fn get_last(obj: usize) -> usize {
    let b = me(obj);
    if b.egap != b.ebuf {
        return b.ebuf - b.size;
    }
    if b.bbuf == b.bgap { 0 } else { b.bgap - b.size }
}

extern "C" fn overwrite(obj: usize, i: i32, item: *const u8) {
    let b = me(obj);
    let at = b.at(i);
    b.put(at, item);
}

/// sombfInsert(i, item): the new item's address.
extern "C" fn insert(obj: usize, i: i32, item: *const u8) -> usize {
    let b = me(obj);
    if b.egap == b.bgap {
        b.grow();
    }
    let at = b.at(i);
    if b.egap != at {
        b.move_gap(at);
    }
    b.egap -= b.size;
    let at = b.egap;
    b.put(at, item);
    at
}

/// sombfAppend(item).
extern "C" fn append(obj: usize, item: *const u8) -> usize {
    let b = me(obj);
    if b.egap == b.bgap {
        b.grow();
    }
    if b.egap != b.ebuf {
        b.move_gap(b.ebuf);
    }
    b.egap -= b.size;
    let at = b.egap;
    b.put(at, item);
    at
}

/// sombfDelete(i): where the item was.
extern "C" fn delete(obj: usize, i: i32) -> usize {
    let b = me(obj);
    let at = b.at(i);
    if b.egap != at {
        b.move_gap(at);
    }
    b.egap += b.size;
    at
}

extern "C" fn delete_last(obj: usize) -> usize {
    let b = me(obj);
    let last = b.ebuf - b.size;
    if b.egap != last {
        b.move_gap(last);
    }
    b.egap += b.size;
    last
}

extern "C" fn copy(obj: usize) -> usize {
    let cls = somsup::class_of(obj);
    let n = somsup::new_object(cls);
    let b = me(obj);
    let cnt = b.count();
    call(p!(SOMBufClassData), 2, &[n, cnt as usize, b.size]);
    for i in 0..cnt {
        let at = me(obj).at(i);
        call(p!(SOMBufClassData), 10, &[n, at]);
    }
    n
}

extern "C" fn compare(obj: usize, other: usize) -> u32 {
    let b = me(obj);
    let cnt = b.count();
    if call(p!(SOMBufClassData), 3, &[other]) as i32 != cnt || call(p!(SOMBufClassData), 4, &[other]) != b.size {
        return 0;
    }
    for i in 0..cnt {
        let x = b.at(i);
        let y = call(p!(SOMBufClassData), 5, &[other, i as usize]);
        let eq = unsafe { core::slice::from_raw_parts(x as *const u8, b.size) == core::slice::from_raw_parts(y as *const u8, b.size) };
        if !eq {
            return 0;
        }
    }
    1
}

fn dump_items(b: &Buf) {
    let show = |from: usize, to: usize| {
        let mut p = from;
        while p < to {
            somsup::printf(b"%02X\0", &[unsafe { *(p as *const u8) } as u32]);
            for k in 1..b.size {
                somsup::printf(b" %02X\0", &[unsafe { *((p + k) as *const u8) } as u32]);
            }
            somsup::printf(b"\n\0", &[]);
            p += b.size;
        }
    };
    somsup::printf(b"[%d]\n\0", &[(b.ebuf - b.bbuf) as u32]);
    show(b.bbuf, b.bgap);
    somsup::printf(b"<gap %d>\n\0", &[(b.egap - b.bgap) as u32]);
    show(b.egap, b.ebuf);
}

extern "C" fn print_self(obj: usize) -> usize {
    dump_items(me(obj));
    somsup::printf(b"\n\0", &[]);
    obj
}

extern "C" fn dump_self_int(obj: usize, level: i32) {
    let b = me(obj);
    let lines: [(&[u8], usize, usize); 4] = [
        (b"sombfBBuf = %08X (%d)\n\0", b.bbuf, 0),
        (b"sombfEBuf = %08X (%d)\n\0", b.ebuf, b.ebuf.wrapping_sub(b.bbuf)),
        (b"sombfBGap = %08X (%d)\n\0", b.bgap, b.bgap.wrapping_sub(b.bbuf)),
        (b"sombfEGap = %08X (%d)\n\0", b.egap, b.egap.wrapping_sub(b.bbuf)),
    ];
    for (f, a, n) in lines {
        unsafe { somsup::somPrefixLevel(level) };
        somsup::printf(f, &[a as u32, n as u32]);
    }
    unsafe { somsup::somPrefixLevel(level) };
    somsup::printf(b"itemSize = %d\n\0", &[b.size as u32]);
    unsafe { somsup::somPrefixLevel(level) };
    somsup::printf(b"body:\n\0", &[]);
    dump_items(b);
}

methods! {
    __m_init => init, 1;
    __m_uninit => uninit, 1;
    __m_init_buf => init_buf, 3;
    __m_count => count, 1;
    __m_item_size => item_size, 1;
    __m_get => get, 2;
    __m_get_last => get_last, 1;
    __m_overwrite => overwrite, 3;
    __m_insert => insert, 3;
    __m_append => append, 2;
    __m_delete => delete, 2;
    __m_delete_last => delete_last, 1;
    __m_copy => copy, 1;
    __m_compare => compare, 2;
    __m_print_self => print_self, 1;
    __m_dump_self_int => dump_self_int, 2;
}
