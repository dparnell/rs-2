//! What every OpenDoc class here is built from: method trampolines, class
//! data, per-object Rust state behind the instance data, OpenDoc's
//! exceptions and byte arrays, and calls through method tokens.

use alloc::boxed::Box;
use alloc::vec::Vec;

use somsup::Environment;

/// A method implementation as it goes in a method table.
pub type M = extern "C" fn();

/// A stack-aligning trampoline (OS/2 callers keep 4-byte alignment) that
/// copies `$copy` argument words to an aligned frame and calls `$target`.
#[macro_export]
macro_rules! tramp {
    ($tramp:ident => $target:path, $copy:expr) => {
        #[unsafe(naked)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    };
}

#[macro_export]
macro_rules! one {
    ($x:ident) => {
        1
    };
}

/// Methods: `fn Name(this: usize, ev: usize, ...) -> usize { ... }`, each
/// with a trampoline `Name::t` for the method table.
#[macro_export]
macro_rules! methods {
    ($(fn $name:ident($($a:ident: $t:ty),* $(,)?) -> usize $body:block)*) => {$(
        #[allow(non_snake_case, unused_variables, clippy::all)]
        pub extern "C" fn $name($($a: $t),*) -> usize $body
        #[allow(non_snake_case)]
        pub mod $name {
            $crate::tramp!(t => super::$name, (0 $(+ $crate::one!($a))*));
        }
    )*};
}

/// Class data: `<Class>ClassData` (the class object and the method
/// tokens) and `<Class>CClassData` (parent method tables, instance data
/// token, ...), exported.
#[macro_export]
macro_rules! class_data {
    ($($cd:ident, $ccd:ident, $n:expr;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; $n + 4] = [0; $n + 4];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

#[macro_export]
macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}

/// A class as it is built: release order and overrides.
pub struct Def {
    pub name: &'static str,
    pub parents: &'static [&'static str],
    pub meta: Option<&'static str>,
    /// Bytes of instance data (0 or 4: a pointer to the object's state).
    pub data: u32,
    pub methods: &'static [(&'static str, M)],
    pub overrides: &'static [(&'static str, M)],
    pub cd: *mut usize,
    pub ccd: *mut usize,
}

/// What a method nobody implemented returns: 0 (FALSE, NULL, kODNULL).
pub use somsup::ret0;

// ---- Object state ----

/// The Rust state of an object, behind its 4 bytes of instance data
/// (made on first use).
pub fn state<T: Default>(ccd: *const usize, obj: usize) -> &'static mut T {
    let slot: &mut usize = somsup::data(ccd, obj);
    if *slot == 0 {
        *slot = Box::into_raw(Box::new(T::default())) as usize;
    }
    unsafe { &mut *(*slot as *mut T) }
}

pub fn drop_state<T>(ccd: *const usize, obj: usize) {
    let slot: &mut usize = somsup::data(ccd, obj);
    if *slot != 0 {
        drop(unsafe { Box::from_raw(*slot as *mut T) });
        *slot = 0;
    }
}

/// Call method `i` (release order) of the class whose data is `cd`.
pub fn call(cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call(cd, i, args)
}

pub fn call_parent(ccd: *const usize, cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call_parent(ccd, cd, i, args)
}

pub fn som_free(obj: usize) {
    somsup::free_object(obj)
}

/// Whether `ev` carries an exception.
pub fn failed(ev: usize) -> bool {
    ev != 0 && unsafe { (*(ev as *const Environment)).major } != 0
}

// ---- Errors (OpenDoc's ODException in the environment) ----

/// ODException's parameters: the error code and a message.
#[repr(C)]
pub struct ODException {
    pub error: i32,
    pub message: [u8; 256],
}

pub mod err {
    pub const UNDEFINED: i32 = -29600;
    pub const SUBCLASS_RESPONSIBILITY: i32 = -29601;
    pub const OUT_OF_MEMORY: i32 = -29602;
    pub const ILLEGAL_NULL_INPUT: i32 = -29604;
    pub const ILLEGAL_PROPERTY: i32 = -29611;
    pub const VALUE_NOT_FOUND: i32 = -29612;
    pub const PROPERTY_NOT_FOUND: i32 = -29613;
    pub const UNFOCUSED_STORAGE_UNIT: i32 = -29614;
    pub const VALUE_INDEX_OUT_OF_RANGE: i32 = -29615;
    pub const INVALID_STORAGE_UNIT_REF: i32 = -29616;
    pub const INVALID_ID: i32 = -29617;
    pub const CANNOT_CREATE_PART: i32 = -29630;
    pub const CANNOT_CREATE_FRAME: i32 = -29631;
    pub const CONTAINER_DOES_NOT_EXIST: i32 = -29640;
    pub const CANNOT_OPEN_CONTAINER: i32 = -29641;
    pub const INVALID_TOKEN: i32 = -29650;
    pub const NOT_IMPLEMENTED: i32 = -29690;
}

pub fn od_error(ev: usize, code: i32) {
    if ev == 0 {
        return;
    }
    let p = somsup::calloc(core::mem::size_of::<ODException>()) as *mut ODException;
    if !p.is_null() {
        unsafe { (*p).error = code };
    }
    somsup::set_exception(ev as *mut Environment, somsup::USER_EXCEPTION, b"ODException\0", p as *mut u8);
}

// ---- Byte arrays and strings ----

/// ODByteArray (and the other IDL sequences of octets).
#[repr(C)]
pub struct ByteArray {
    pub maximum: u32,
    pub length: u32,
    pub buffer: *mut u8,
}

pub unsafe fn ba_bytes<'a>(ba: usize) -> &'a [u8] {
    if ba == 0 {
        return &[];
    }
    let b = unsafe { &*(ba as *const ByteArray) };
    if b.buffer.is_null() {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(b.buffer, b.length as usize) }
}

/// Fill a byte array the caller owns with a SOMMalloc'd copy of `data`.
pub fn ba_fill(ba: usize, data: &[u8]) {
    if ba == 0 {
        return;
    }
    let p = somsup::malloc(data.len().max(1));
    if !p.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len()) };
    }
    unsafe {
        let b = &mut *(ba as *mut ByteArray);
        b.maximum = data.len() as u32;
        b.length = data.len() as u32;
        b.buffer = p;
    }
}

pub fn cstr<'a>(p: usize) -> &'a [u8] {
    unsafe { somsup::cstr(p as *const u8) }
}

/// A SOMMalloc'd C string (what OpenDoc methods returning a string give
/// their caller to free).
pub fn dup(s: &[u8]) -> usize {
    somsup::dup(s) as usize
}

pub fn z(s: &[u8]) -> Vec<u8> {
    somsup::z(s)
}

/// 16.16 fixed point (ODCoordinate, ODFixed).
pub const ONE: i32 = 0x10000;

pub fn fmul(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}

pub fn fdiv(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { (((a as i64) << 16) / b as i64) as i32 }
}

/// ODPoint and ODRect (OS/2's OpenDoc: y grows upwards, top > bottom).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn normalized(self) -> Rect {
        Rect {
            left: self.left.min(self.right),
            right: self.left.max(self.right),
            top: self.top.max(self.bottom),
            bottom: self.top.min(self.bottom),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.top <= self.bottom
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.bottom && p.y < self.top
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect { left: self.left.max(o.left), right: self.right.min(o.right), top: self.top.min(o.top), bottom: self.bottom.max(o.bottom) };
        if r.is_empty() { Rect::default() } else { r }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect { left: self.left.min(o.left), right: self.right.max(o.right), top: self.top.max(o.top), bottom: self.bottom.min(o.bottom) }
    }
}

pub fn read<T: Copy>(p: usize) -> T {
    unsafe { *(p as *const T) }
}

pub fn write<T>(p: usize, v: T) {
    if p != 0 {
        unsafe { *(p as *mut T) = v };
    }
}

/// Bytes of a value as stored (little-endian u32).
pub fn u32_bytes(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

pub fn bytes_u32(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    let n = b.len().min(4);
    a[..n].copy_from_slice(&b[..n]);
    u32::from_le_bytes(a)
}
