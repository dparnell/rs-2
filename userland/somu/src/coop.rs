//! The cooperative metaclasses, SOMMBeforeAfter and its dispatcher,
//! SOMMTraced and SOMMSingleInstance (see lib.rs for how the before and
//! after methods get called).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use somsup::{SomId, call, call_parent, cm, data, methods, om, sc, so};

use crate::*;

// ---- The cooperative framework (IBM's internals; kept as entry points) ----

extern "C" fn add_req_overrides(_cls: usize, _ev: usize, seq: usize) -> usize {
    seq
}
extern "C" fn add_coop_override(_cls: usize, _ev: usize, _mid: usize, _m: usize) -> usize {
    0
}
extern "C" fn add_coop(_cls: usize, _mid: SomId, _m: usize) -> usize {
    0
}
extern "C" fn request(_cls: usize, _mid: SomId, _m: usize, _slot: usize) -> usize {
    0
}
extern "C" fn satisfy(_cls: usize) -> u32 {
    1
}
/// IBM's sister class is a copy of the class keeping its own method
/// table; here the class itself keeps the implementations (the stubs
/// call them directly).
extern "C" fn sister(cls: usize) -> usize {
    cls
}

/// SOMMCooperativeRedispatched::somClassReady: the class is ready, then
/// its methods get the before/after stubs.
extern "C" fn redispatched_ready(cls: usize) {
    call_parent(p!(SOMMCooperativeRedispatchedCClassData), sc(), cm::SOM_CLASS_READY, &[cls]);
    wrap_class(cls);
}

methods! {
    __m_add_req_overrides => add_req_overrides, 3;
    __m_add_coop_override => add_coop_override, 4;
    __m_add_coop => add_coop, 3;
    __m_request => request, 4;
    __m_satisfy => satisfy, 1;
    __m_sister => sister, 1;
    __m_redispatched_ready => redispatched_ready, 1;
}

// ---- Method stubs ----

struct Wrap {
    mid: SomId,
    orig: usize,
    /// Where the object is among the arguments (1 for methods returning
    /// structures, whose address comes first).
    obj: usize,
}

static mut STUBS: BTreeMap<usize, *const Wrap> = BTreeMap::new();
static mut EXEC_AT: usize = 0;
static mut EXEC_END: usize = 0;

fn exec_alloc(n: usize) -> *mut u8 {
    unsafe {
        if EXEC_AT + n > EXEC_END {
            let mut p: *mut u8 = core::ptr::null_mut();
            if os2::DosAllocMem(&mut p, 0x10000, 1 | 2 | 4 | 0x10) != 0 {
                return core::ptr::null_mut();
            }
            EXEC_AT = p as usize;
            EXEC_END = EXEC_AT + 0x10000;
        }
        let p = EXEC_AT as *mut u8;
        EXEC_AT += n;
        p
    }
}

/// `push wrap; jmp ba_entry`.
fn make_stub(w: Wrap) -> usize {
    let w = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(w));
    let p = exec_alloc(16);
    if p.is_null() {
        return 0;
    }
    unsafe {
        *p = 0x68;
        core::ptr::write_unaligned(p.add(1) as *mut u32, w as u32);
        *p.add(5) = 0xe9;
        let rel = (ba_entry as usize).wrapping_sub(p as usize + 10);
        core::ptr::write_unaligned(p.add(6) as *mut u32, rel as u32);
        STUBS.insert(p as usize, w);
    }
    p as usize
}

/// Where a method's token takes the object from (rs2's and IBM's tokens
/// start with `mov ecx,[esp+4]`, or `[esp+8]` for methods returning
/// structures).
fn object_index(token: usize) -> usize {
    if token == 0 {
        return 0;
    }
    let b = unsafe { core::slice::from_raw_parts(token as *const u8, 4) };
    if b[..3] == [0x8b, 0x4c, 0x24] && b[3] == 8 { 1 } else { 0 }
}

/// Methods that keep their implementations: the dispatch methods (IBM's
/// somOverrideMtab leaves them), and somInit/somUninit (rs2's SOM keeps
/// overrides of these as the class's legacy initializers).
const KEEP: &[&[u8]] = &[
    b"somDispatch",
    b"somDispatchV",
    b"somDispatchL",
    b"somDispatchA",
    b"somDispatchD",
    b"somClassDispatch",
    b"somInit",
    b"somUninit",
];

#[repr(C)]
struct MethodData {
    id: SomId,
    kind: u32,
    descriptor: SomId,
    token: usize,
    method: usize,
    shared: usize,
}

fn wrap_class(cls: usize) {
    let n = call(sc(), cm::SOM_GET_NUM_METHODS, &[cls]) as i32;
    for i in 0..n {
        let mut md = MethodData { id: core::ptr::null_mut(), kind: 0, descriptor: core::ptr::null_mut(), token: 0, method: 0, shared: 0 };
        if call(sc(), cm::SOM_GET_NTH_METHOD_DATA, &[cls, i as usize, &mut md as *mut _ as usize]) & 0xff == 0 {
            continue;
        }
        if md.kind != 0 || md.method == 0 || md.id.is_null() || unsafe { STUBS.contains_key(&md.method) } {
            continue;
        }
        let name = unsafe { somsup::cstr(somsup::somStringFromId(md.id)) };
        if KEEP.contains(&name) {
            continue;
        }
        let stub = make_stub(Wrap { mid: md.id, orig: md.method, obj: object_index(md.token) });
        if stub != 0 {
            call(sc(), cm::SOM_OVERRIDE_S_METHOD, &[cls, md.id as usize, stub]);
        }
    }
}

// ---- Calling the before and after methods ----

struct Frame {
    ret: usize,
    cls: usize,
    obj: usize,
    mid: SomId,
    ap: usize,
    afters: Vec<usize>,
}

static mut FRAMES: [Vec<Frame>; 512] = [const { Vec::new() }; 512];

fn tid() -> usize {
    let t: usize;
    unsafe { core::arch::asm!("mov {0}, fs:[0x0c]", "mov {0}, [{0}]", out(reg) t) };
    t
}

fn frames() -> &'static mut Vec<Frame> {
    unsafe { &mut FRAMES[tid() & 511] }
}

/// The class object of an object, from its method table (calling
/// somGetClass would come back here).
unsafe fn class_of_raw(obj: usize) -> usize {
    if obj < 0x10000 {
        return 0;
    }
    unsafe {
        let mt = *(obj as *const usize);
        if mt < 0x10000 { 0 } else { *(mt as *const usize) }
    }
}

/// The sommBeforeMethod (or sommAfterMethod) implementations of a
/// before/after metaclass, as IBM's sommSetBAMethodSeq collects them: a
/// class's own, else its parents' (left to right for before methods,
/// right to left for after methods), each class once.
fn ba_sequence(meta: usize, before: bool) -> Vec<usize> {
    let tok = unsafe { if before { SOMMBeforeAfterClassData[1] } else { SOMMBeforeAfterClassData[2] } };
    let ba = unsafe { SOMMBeforeAfterClassData[0] };
    let mut out = Vec::new();
    let mut seen = Vec::new();
    fn rec(c: usize, tok: usize, ba: usize, before: bool, seen: &mut Vec<usize>, out: &mut Vec<usize>) {
        if seen.contains(&c) {
            return;
        }
        seen.push(c);
        let mine = unsafe { somsup::somClassResolve(c, tok) };
        let parents = parents_of(c);
        let supporting: Vec<usize> = parents.into_iter().filter(|&p| descends(p, ba)).collect();
        if mine != 0 && supporting.iter().all(|&p| unsafe { somsup::somClassResolve(p, tok) } != mine) {
            out.push(mine);
            return;
        }
        let order: Vec<usize> = if before { supporting } else { supporting.into_iter().rev().collect() };
        for p in order {
            rec(p, tok, ba, before, seen, out);
        }
    }
    if ba != 0 && descends(meta, ba) {
        rec(meta, tok, ba, before, &mut seen, &mut out);
    }
    out
}

pub fn descends(cls: usize, of: usize) -> bool {
    cls != 0 && of != 0 && call(sc(), cm::SOM_DESCENDED_FROM, &[cls, of]) & 0xff != 0
}

#[repr(C)]
struct Seq {
    max: u32,
    len: u32,
    buf: *const usize,
}

pub fn parents_of(cls: usize) -> Vec<usize> {
    let mut s = Seq { max: 0, len: 0, buf: core::ptr::null() };
    let f: extern "C" fn(*mut Seq, usize) -> *mut Seq = unsafe { core::mem::transmute(*sc().add(1 + cm::SOM_GET_PARENTS)) };
    f(&mut s, cls);
    let v = (0..s.len as usize).map(|i| unsafe { *s.buf.add(i) }).collect();
    if !s.buf.is_null() {
        somsup::free(s.buf as *mut u8);
    }
    v
}

static mut LISTS: BTreeMap<usize, (Vec<usize>, Vec<usize>)> = BTreeMap::new();

fn lists(meta: usize) -> &'static (Vec<usize>, Vec<usize>) {
    unsafe {
        if !LISTS.contains_key(&meta) {
            let b = ba_sequence(meta, true);
            let a = ba_sequence(meta, false);
            LISTS.insert(meta, (b, a));
        }
        LISTS.get(&meta).unwrap()
    }
}

/// The stub's first half: the before methods; what to run next.
/// `*mode`: 0 jump to the method, 1 jump with the after methods to come,
/// 2 return 0 (a before method said no).
extern "C" fn ba_enter(w: *const Wrap, args: *const usize, ret: usize, mode: *mut u32) -> usize {
    let w = unsafe { &*w };
    let ap = unsafe { args.add(w.obj) } as usize;
    let obj = unsafe { *(ap as *const usize) };
    let cls = unsafe { class_of_raw(obj) };
    let meta = unsafe { class_of_raw(cls) };
    if meta == 0 {
        unsafe { *mode = 0 };
        return w.orig;
    }
    let (befores, afters) = lists(meta);
    if befores.is_empty() && afters.is_empty() {
        unsafe { *mode = 0 };
        return w.orig;
    }
    let mut ran = 0;
    for &b in befores {
        if somsup::call_fn(b, &[cls, 0, obj, w.mid as usize, ap]) & 0xff == 0 {
            break;
        }
        ran += 1;
    }
    let n = afters.len();
    let mine: Vec<usize> = afters[n.saturating_sub(ran)..].to_vec();
    if ran < befores.len() {
        let zero = [0u32; 4];
        for a in mine {
            somsup::call_fn(a, &[cls, 0, obj, w.mid as usize, zero.as_ptr() as usize, ap]);
        }
        unsafe { *mode = 2 };
        return 0;
    }
    frames().push(Frame { ret, cls, obj, mid: w.mid, ap, afters: mine });
    unsafe { *mode = 1 };
    w.orig
}

#[repr(C)]
struct Result {
    eax: u32,
    edx: u32,
    st0: f64,
    has_st0: u32,
}

/// The stub's second half, when the method returns: the after methods;
/// the caller's return address.
extern "C" fn ba_leave(r: *mut Result) -> usize {
    let Some(f) = frames().pop() else {
        // Lost track (a method left through an exception handler):
        // nowhere sensible to go.
        unsafe { os2::DosExit(1, 99) };
        return 0;
    };
    let value = unsafe { if (*r).has_st0 != 0 { &raw mut (*r).st0 as usize } else { r as usize } };
    for a in f.afters {
        somsup::call_fn(a, &[f.cls, 0, f.obj, f.mid as usize, value, f.ap]);
    }
    f.ret
}

/// Entered from a stub: `[esp]` the stub's Wrap, `[esp+4]` the caller's
/// return address, the arguments above.
#[unsafe(naked)]
extern "C" fn ba_entry() {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "sub esp, 4",
        "and esp, -16",
        "sub esp, 16",
        "mov eax, [ebp + 4]", "mov [esp], eax",
        "lea eax, [ebp + 12]", "mov [esp + 4], eax",
        "mov eax, [ebp + 8]", "mov [esp + 8], eax",
        "lea eax, [ebp - 4]", "mov [esp + 12], eax",
        "call {enter}",
        "mov ecx, [ebp - 4]",
        "mov esp, ebp",
        "pop ebp",
        "add esp, 4",
        "cmp ecx, 1",
        "jb 2f",
        "ja 3f",
        "mov dword ptr [esp], offset {leave_entry}",
        "2:",
        "jmp eax",
        "3:",
        "xor eax, eax",
        "xor edx, edx",
        "ret",
        enter = sym ba_enter,
        leave_entry = sym ba_return,
    );
}

/// Where a wrapped method returns to: the result is kept (EAX, EDX and a
/// floating point result in ST0) while the after methods run.
#[unsafe(naked)]
extern "C" fn ba_return() {
    core::arch::naked_asm!(
        "push 0",
        "push ebp",
        "mov ebp, esp",
        "sub esp, 32",
        "mov [ebp - 32], eax",
        "mov [ebp - 28], edx",
        "mov dword ptr [ebp - 16], 0",
        "fxam",
        "fnstsw ax",
        "and ax, 0x4500",
        "cmp ax, 0x4100",
        "je 2f",
        "fstp qword ptr [ebp - 24]",
        "mov dword ptr [ebp - 16], 1",
        "2:",
        "lea eax, [ebp - 32]",
        "and esp, -16",
        "sub esp, 16",
        "mov [esp], eax",
        "call {leave}",
        "mov [ebp + 4], eax",
        "cmp dword ptr [ebp - 16], 0",
        "je 3f",
        "fld qword ptr [ebp - 24]",
        "3:",
        "mov eax, [ebp - 32]",
        "mov edx, [ebp - 28]",
        "mov esp, ebp",
        "pop ebp",
        "ret",
        leave = sym ba_leave,
    );
}

// ---- SOMMBeforeAfter and its dispatcher ----

extern "C" fn before(_cls: usize, _ev: usize, _obj: usize, _mid: SomId, _ap: usize) -> u32 {
    1
}
extern "C" fn after(_cls: usize, _ev: usize, _obj: usize, _mid: SomId, _ret: usize, _ap: usize) {}

/// sommBAMethodCount(before): how many before (after) methods the
/// metaclass's instances get.
extern "C" fn ba_method_count(meta: usize, _ev: usize, before: u32) -> i32 {
    ba_sequence(meta, before & 0xff != 0).len() as i32
}

/// sommSetBAMethodSeq(before, seq): the before (after) methods into a
/// sequence `{ max, length, buffer }`.
extern "C" fn set_ba_method_seq(meta: usize, _ev: usize, before: u32, seq: *mut [usize; 3]) {
    if seq.is_null() {
        return;
    }
    for m in ba_sequence(meta, before & 0xff != 0) {
        unsafe {
            let s = &mut *seq;
            if s[1] < s[0] && s[2] != 0 {
                *(s[2] as *mut usize).add(s[1]) = m;
                s[1] += 1;
            }
        }
    }
}

methods! {
    __m_before => before, 5;
    __m_after => after, 6;
    __m_ba_method_count => ba_method_count, 3;
    __m_set_ba_method_seq => set_ba_method_seq, 4;
}

/// somva_SOMMBeforeAfter_sommBeforeMethod(cls, ev, obj, mid, ...): the
/// method with the address of the variable arguments as its va_list.
#[unsafe(naked)]
pub extern "C" fn __v_before() {
    core::arch::naked_asm!(
        "lea eax, [esp + 20]",
        "push eax",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "mov eax, dword ptr [{cd} + 4]",
        "call eax",
        "add esp, 20",
        "ret",
        cd = sym crate::SOMMBeforeAfterClassData,
    );
}

/// somva_SOMMBeforeAfter_sommAfterMethod(cls, ev, obj, mid, ret, ...).
#[unsafe(naked)]
pub extern "C" fn __v_after() {
    core::arch::naked_asm!(
        "lea eax, [esp + 24]",
        "push eax",
        "push dword ptr [esp + 24]",
        "push dword ptr [esp + 24]",
        "push dword ptr [esp + 24]",
        "push dword ptr [esp + 24]",
        "push dword ptr [esp + 24]",
        "mov eax, dword ptr [{cd} + 8]",
        "call eax",
        "add esp, 24",
        "ret",
        cd = sym crate::SOMMBeforeAfterClassData,
    );
}

// ---- SOMMTraced ----

static mut DEPTH: i32 = 0;

fn trace_on(cls: usize) -> bool {
    *data::<u8>(p!(SOMMTracedCClassData), cls) != 0
}

extern "C" fn set_trace(cls: usize, _ev: usize, on: u32) {
    *data::<u8>(p!(SOMMTracedCClassData), cls) = on as u8;
}
extern "C" fn get_trace(cls: usize, _ev: usize) -> u32 {
    trace_on(cls) as u32
}

/// somInitMIClass: tracing is on when SOMM_TRACED is set to nothing (or
/// `""`) or names the class.
extern "C" fn traced_init_mi(
    cls: usize,
    inherit: usize,
    name: *const u8,
    parents: usize,
    size: usize,
    align: usize,
    max: usize,
    major: usize,
    minor: usize,
) {
    let mut v: *const u8 = core::ptr::null();
    let on = if unsafe { os2::DosScanEnv(b"SOMM_TRACED\0".as_ptr(), &mut v) } != 0 || v.is_null() {
        false
    } else {
        let s = unsafe { somsup::cstr(v) };
        let n = unsafe { somsup::cstr(name) };
        let s = s.trim_ascii_end();
        s.is_empty() || s == b"\"\"" || (!n.is_empty() && s.windows(n.len()).any(|w| w == n))
    };
    set_trace(cls, 0, on as u32);
    unsafe { DEPTH = 0 };
    call_parent(p!(SOMMTracedCClassData), sc(), cm::SOM_INIT_MI_CLASS, &[cls, inherit, name as usize, parents, size, align, max, major, minor]);
}

fn indent() {
    for _ in 0..unsafe { DEPTH }.max(0) {
        somsup::printf(b" \0", &[]);
    }
}

extern "C" fn traced_before(cls: usize, _ev: usize, obj: usize, mid: SomId, ap: usize) -> u32 {
    if !trace_on(cls) {
        return 1;
    }
    indent();
    unsafe { DEPTH += 1 };
    let name = unsafe { somsup::somStringFromId(mid) };
    somsup::printf(b"%s( %s at 0x%08lx\0", &[name as u32, somsup::class_name(cls) as u32, obj as u32]);
    if !crate::desc::trace_args(obj, mid, ap) {
        somsup::printf(b", ...\0", &[]);
    }
    somsup::printf(b" ) called\n\0", &[]);
    1
}

extern "C" fn traced_after(cls: usize, _ev: usize, obj: usize, mid: SomId, ret: usize, _ap: usize) {
    if !trace_on(cls) {
        return;
    }
    unsafe { DEPTH -= 1 };
    indent();
    let name = unsafe { somsup::somStringFromId(mid) };
    somsup::printf(b"%s returns \0", &[name as u32]);
    if unsafe { somsup::somIsObj(obj) } != 0 && !crate::desc::trace_result(obj, mid, ret) {
        somsup::printf(b" ...\0", &[]);
    }
    somsup::printf(b"\n\0", &[]);
}

methods! {
    __m_set_trace => set_trace, 3;
    __m_get_trace => get_trace, 2;
    __m_traced_init_mi => traced_init_mi, 9;
    __m_traced_before => traced_before, 5;
    __m_traced_after => traced_after, 6;
}

// ---- SOMMSingleInstance ----

/// HiddenClassMeta's data in a class of SOMMSingleInstance.
#[repr(C)]
struct Single {
    instance: usize,
    initialized: u32,
    count: u32,
    proxy: usize,
    free: usize,
    rest: [usize; 6],
}

fn single(cls: usize) -> &'static mut Single {
    data::<Single>(p!(HiddenClassMetaCClassData), cls)
}

/// The class (or ancestor) whose instances are counted for `obj`.
fn single_class(obj: usize) -> usize {
    let hidden = unsafe { HiddenClassMetaClassData[0] };
    let mut c = unsafe { class_of_raw(obj) };
    while c != 0 {
        if somsup::is_a(c, hidden) && single(c).instance == obj {
            return c;
        }
        let ps = parents_of(c);
        c = ps.first().copied().unwrap_or(0);
    }
    0
}

/// somNew: the class's one instance (made the first time).
extern "C" fn single_new(cls: usize) -> usize {
    unsafe { somsup::somStartCriticalSection() };
    let s = single(cls);
    s.count += 1;
    if s.instance == 0 {
        s.instance = call_parent(p!(HiddenClassMetaCClassData), sc(), cm::SOM_NEW, &[cls]);
        if s.instance == 0 {
            s.count = 0;
        }
    }
    let r = s.instance;
    unsafe { somsup::somEndCriticalSection() };
    r
}

extern "C" fn single_new_no_init(cls: usize) -> usize {
    unsafe { somsup::somStartCriticalSection() };
    let s = single(cls);
    s.count += 1;
    if s.instance == 0 {
        s.instance = call_parent(p!(HiddenClassMetaCClassData), sc(), cm::SOM_NEW_NO_INIT, &[cls]);
        if s.instance == 0 {
            s.count = 0;
        }
    }
    let r = s.instance;
    unsafe { somsup::somEndCriticalSection() };
    r
}

/// The instance's somFree: only the last reference frees it.
extern "C" fn single_free(obj: usize) {
    let cls = single_class(obj);
    if cls == 0 {
        return;
    }
    unsafe { somsup::somStartCriticalSection() };
    let s = single(cls);
    let last = s.count <= 1;
    let f = s.free;
    if last {
        s.count = 0;
        s.instance = 0;
    } else {
        s.count -= 1;
    }
    unsafe { somsup::somEndCriticalSection() };
    if last && f != 0 {
        somsup::call_fn(f, &[obj]);
    }
}

/// somClassReady: the class's instances get the counting somFree.
extern "C" fn single_ready(cls: usize) {
    call_parent(p!(HiddenClassMetaCClassData), sc(), cm::SOM_CLASS_READY, &[cls]);
    let mut md = MethodData { id: core::ptr::null_mut(), kind: 0, descriptor: core::ptr::null_mut(), token: 0, method: 0, shared: 0 };
    let mid = somsup::id(b"somFree");
    if call(sc(), cm::SOM_GET_METHOD_DATA, &[cls, mid as usize, &mut md as *mut _ as usize]) & 0xff != 0
        && md.method != __m_single_free as usize
    {
        single(cls).free = md.method;
        call(sc(), cm::SOM_OVERRIDE_S_METHOD, &[cls, mid as usize, __m_single_free as usize]);
    }
}

/// sommGetSingleInstance: somNew (which counts a reference).
extern "C" fn get_single(cls: usize, _ev: usize) -> usize {
    call(sc(), cm::SOM_NEW, &[cls])
}

/// sommFreeSingleInstance: free the instance (a reference).
extern "C" fn free_single(cls: usize, _ev: usize) {
    let i = single(cls).instance;
    if i != 0 {
        call(so(), om::SOM_FREE, &[i]);
    }
}

methods! {
    __m_single_new => single_new, 1;
    __m_single_new_no_init => single_new_no_init, 1;
    __m_single_free => single_free, 1;
    __m_single_ready => single_ready, 1;
    __m_get_single => get_single, 2;
    __m_free_single => free_single, 2;
}
