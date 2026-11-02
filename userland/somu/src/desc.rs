//! SOMDesc, SOMArgList and SOMCommand: a method's description from the
//! interface repository (SOMIR: its OperationDef, or the AttributeDef of
//! a `_get_`/`_set_` accessor, and the interface's `callstyle`), an
//! argument list taken apart with it (IBM's: the environment first for
//! callstyle idl, floating point arguments 8 bytes, everything else 4),
//! and SOMMTraced's printing of arguments and results with them.
//! SOMCommand, the text form of argument lists and the externalization
//! functions are in cmd.rs.

use alloc::vec::Vec;

use somsup::{SomId, call, data, methods};

use crate::*;

/// A method that fails (-1), whatever its arguments.
#[unsafe(naked)]
pub extern "C" fn fail() {
    core::arch::naked_asm!("mov eax, -1", "ret");
}

// ---- The interface repository (SOMIR's class data) ----

unsafe extern "C" {
    #[link_name = "SOMIR.51"]
    static RepositoryClassData: [usize; 7];
    #[link_name = "SOMIR.39"]
    fn RepositoryNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMIR.35"]
    pub static ContainedClassData: [usize; 36];
    #[link_name = "SOMIR.54"]
    static AttributeDefClassData: [usize; 6];
    #[link_name = "SOMTC.26"]
    pub fn tcKind(t: usize, ev: usize) -> u32;
    #[link_name = "SOMTC.29"]
    fn tcSize(t: usize, ev: usize) -> i32;
    #[link_name = "SOMTC.7"]
    fn tcParameter(ret: *mut [usize; 2], t: usize, ev: usize, i: i32) -> *mut [usize; 2];
    #[link_name = "SOMTC.39"]
    static TC__void: u32;
    #[link_name = "SOM.82"]
    fn somGetClassFromMToken(token: usize) -> usize;
}

pub fn cdp(a: &'static [usize]) -> *const usize {
    a.as_ptr()
}

/// The class manager's repository (made the first time, as IBM's SOM
/// does when somInterfaceRepository is asked for).
fn repository() -> usize {
    let mgr = unsafe { somsup::SOMClassMgrObject };
    let r = call(somsup::scm(), somsup::mm::SOM_GET_INTERFACE_REPOSITORY, &[mgr]);
    if r != 0 {
        return r;
    }
    let cls = unsafe { RepositoryNewClass(0, 0) };
    let r = somsup::new_object(cls);
    if r != 0 {
        call(somsup::scm(), somsup::mm::SOM_SET_INTERFACE_REPOSITORY, &[mgr, r]);
    }
    r
}

pub fn lookup(id: &[u8]) -> usize {
    let r = repository();
    if r == 0 {
        return 0;
    }
    let z = somsup::z(id);
    call(cdp(unsafe { &RepositoryClassData }), 0, &[r, 0, z.as_ptr() as usize])
}

pub fn modifier(id: &[u8], m: &[u8]) -> *mut u8 {
    let r = repository();
    if r == 0 {
        return core::ptr::null_mut();
    }
    let (z, mz) = (somsup::z(id), somsup::z(m));
    call(cdp(unsafe { &RepositoryClassData }), 1, &[r, 0, z.as_ptr() as usize, mz.as_ptr() as usize]) as *mut u8
}

/// What SOMDesc keeps (IBM's 28 bytes).
#[repr(C)]
pub struct Ds {
    pub kind: u32,
    pub def: usize,
    /// The interface's id (SOMMalloc'd).
    pub spare: usize,
    /// The OperationDescription (from describe).
    pub op: *const usize,
    /// 1 oidl, 2 idl, 3 unknown.
    pub callstyle: u32,
    pub is_attr: u8,
    pub is_get: u8,
    pub pad: [u8; 2],
    pub attr_type: usize,
}

pub fn ds(o: usize) -> &'static mut Ds {
    data::<Ds>(p!(SOMDescCClassData), o)
}

/// Describe a method by its descriptor ("Class::method").
fn set_desc(d: &mut Ds, descriptor: &[u8]) -> i32 {
    somsup::free(d.spare as *mut u8);
    *d = Ds { kind: 0, def: 0, spare: 0, op: core::ptr::null(), callstyle: 3, is_attr: 0, is_get: 0, pad: [0; 2], attr_type: 0 };
    let desc = descriptor.strip_prefix(b"::").unwrap_or(descriptor);
    let Some(split) = desc.windows(2).rposition(|w| w == b"::") else { return -1 };
    let (iface, method) = (&desc[..split], &desc[split + 2..]);
    let mut id = b"::".to_vec();
    id.extend_from_slice(desc);
    let def = lookup(&id);
    if def != 0 && unsafe { somsup::cstr(somsup::class_name(somsup::class_of(def))) } == b"OperationDef" {
        let mut dsc = [0usize; 3];
        call(cdp(unsafe { &ContainedClassData }), 1, &[&mut dsc as *mut _ as usize, def, 0]);
        d.def = def;
        d.op = dsc[2] as *const usize;
    } else {
        let (attr, get) = match (method.strip_prefix(b"_get_"), method.strip_prefix(b"_set_")) {
            (Some(a), _) => (a, true),
            (_, Some(a)) => (a, false),
            _ => return -1,
        };
        let mut aid = b"::".to_vec();
        aid.extend_from_slice(iface);
        aid.extend_from_slice(b"::");
        aid.extend_from_slice(attr);
        let a = lookup(&aid);
        if a == 0 {
            return -1;
        }
        d.def = a;
        d.is_attr = 1;
        d.is_get = get as u8;
        d.attr_type = call(cdp(unsafe { &AttributeDefClassData }), 0, &[a, 0]);
    }
    let mut iid = b"::".to_vec();
    iid.extend_from_slice(iface);
    d.spare = somsup::dup(&iid) as usize;
    let cs = modifier(&iid, b"callstyle");
    d.callstyle = if cs.is_null() {
        2
    } else {
        let v = unsafe { somsup::cstr(cs) };
        let r = if v == b"idl" { 2 } else if v == b"oidl" { 1 } else { 3 };
        somsup::free(cs);
        r
    };
    0
}

fn raw_class(obj: usize) -> usize {
    if obj < 0x10000 {
        return 0;
    }
    let mt = unsafe { *(obj as *const usize) };
    if mt < 0x10000 { 0 } else { unsafe { *(mt as *const usize) } }
}

/// A method's descriptor for an object ("Class::method").
fn descriptor(obj: usize, name: *const u8) -> Option<Vec<u8>> {
    // The class from the method table (somGetClass on a traced object
    // would be traced too).
    let cls = raw_class(obj);
    if cls == 0 {
        return None;
    }
    let id = somsup::id(unsafe { somsup::cstr(name) });
    let d = call(somsup::sc(), somsup::cm::SOM_GET_METHOD_DESCRIPTOR, &[cls, id as usize]) as SomId;
    if !d.is_null() {
        return Some(unsafe { somsup::cstr(somsup::somStringFromId(d)) }.to_vec());
    }
    // No descriptor: "IntroducingClass::method", the class from the
    // method's token.
    let mut md = [0usize; 6];
    if call(somsup::sc(), somsup::cm::SOM_GET_METHOD_DATA, &[cls, id as usize, md.as_mut_ptr() as usize]) & 0xff == 0 || md[3] == 0 {
        return None;
    }
    let intro = unsafe { somGetClassFromMToken(md[3]) };
    if intro == 0 {
        return None;
    }
    let mut v = unsafe { somsup::cstr(somsup::class_name(intro)) }.to_vec();
    v.extend_from_slice(b"::");
    v.extend_from_slice(unsafe { somsup::cstr(name) });
    Some(v)
}

pub fn num_args(d: &Ds) -> u32 {
    if d.is_attr != 0 {
        return (d.is_get == 0) as u32;
    }
    if d.op.is_null() { 0 } else { unsafe { *d.op.add(9) as u32 } }
}

/// The parameter description of argument `i`: name, id, defined_in,
/// type, mode.
pub fn arg_info(d: &Ds, i: u32) -> Option<[usize; 5]> {
    if d.is_attr != 0 {
        return if d.is_get == 0 && i == 0 { Some([0, 0, 0, d.attr_type, 1]) } else { None };
    }
    if d.op.is_null() || i >= num_args(d) {
        return None;
    }
    let p = unsafe { (*d.op.add(10) as *const usize).add(5 * i as usize) };
    Some(unsafe { [*p, *p.add(1), *p.add(2), *p.add(3), *p.add(4)] })
}

fn ret_info(d: &Ds) -> (usize, u32) {
    if d.is_attr != 0 {
        return (if d.is_get != 0 { d.attr_type } else { unsafe { &TC__void as *const u32 as usize } }, 1);
    }
    if d.op.is_null() {
        return (0, 1);
    }
    unsafe { (*d.op.add(3), *d.op.add(4) as u32) }
}

// ---- SOMDesc's methods ----

pub fn set_som_desc_for(o: usize, obj: usize, name: *const u8) -> i32 {
    set_som_desc(o, obj, name)
}
extern "C" fn set_som_desc(o: usize, obj: usize, name: *const u8) -> i32 {
    match descriptor(obj, name) {
        Some(d) => set_desc(ds(o), &d),
        None => -1,
    }
}
extern "C" fn set_som_desc2(o: usize, descriptor: *const u8) -> i32 {
    set_desc(ds(o), unsafe { somsup::cstr(descriptor) })
}
extern "C" fn get_num_args(o: usize, n: *mut u32) -> i32 {
    let d = ds(o);
    if d.def == 0 {
        return -1;
    }
    unsafe { *n = num_args(d) };
    0
}
extern "C" fn get_ret_info(o: usize, tc: *mut usize, mode: *mut u32) -> i32 {
    let d = ds(o);
    if d.def == 0 {
        return -1;
    }
    let (t, m) = ret_info(d);
    unsafe {
        *tc = t;
        *mode = m;
    }
    0
}
extern "C" fn get_ret_size(o: usize, size: *mut i32) -> i32 {
    let d = ds(o);
    if d.def == 0 {
        return -1;
    }
    unsafe { *size = tcSize(ret_info(d).0, 0) };
    0
}
extern "C" fn get_arg_info(o: usize, i: u32, name: *mut usize, id: *mut usize, defined_in: *mut usize, tc: *mut usize, mode: *mut usize) -> i32 {
    let Some(a) = arg_info(ds(o), i & 0xffff) else { return -1 };
    unsafe {
        *name = a[0];
        *id = a[1];
        *defined_in = a[2];
        *tc = a[3];
        *mode = a[4];
    }
    0
}
/// somutGetArgSize(i, &size, &kind, &mode).
extern "C" fn get_arg_size(o: usize, i: u32, size: *mut i32, kind: *mut u32, mode: *mut u32) -> i32 {
    let Some(a) = arg_info(ds(o), i & 0xffff) else { return -1 };
    unsafe {
        *size = tcSize(a[3], 0);
        *kind = tcKind(a[3], 0);
        *mode = a[4] as u32;
    }
    0
}
extern "C" fn get_call_style(o: usize, style: *mut u32) -> i32 {
    let d = ds(o);
    unsafe { *style = d.callstyle };
    if d.callstyle == 3 { -1 } else { 0 }
}
/// somutGetContextIds(&seq): the operation's contexts.
extern "C" fn get_context_ids(o: usize, seq: *mut [usize; 3]) -> i32 {
    let d = ds(o);
    if d.def == 0 {
        return -1;
    }
    unsafe {
        *seq = if d.op.is_null() { [0; 3] } else { [*d.op.add(5), *d.op.add(6), *d.op.add(7)] };
    }
    0
}
extern "C" fn is_oneway_ack(o: usize) -> u32 {
    let (_, m) = ret_info(ds(o));
    (m == 2) as u32
}
extern "C" fn desc_nothing(_o: usize) {}

methods! {
    __m_set_som_desc => set_som_desc, 3;
    __m_set_som_desc2 => set_som_desc2, 2;
    __m_get_num_args => get_num_args, 2;
    __m_get_ret_size => get_ret_size, 2;
    __m_get_ret_info => get_ret_info, 3;
    __m_get_arg_size => get_arg_size, 5;
    __m_get_arg_info => get_arg_info, 7;
    __m_get_context_ids => get_context_ids, 2;
    __m_get_call_style => get_call_style, 2;
    __m_is_oneway_ack => is_oneway_ack, 1;
    __m_desc_init => desc_nothing, 1;
    __m_desc_uninit => desc_nothing, 1;
}

pub fn desc_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
    ("somutSetSomDesc", __m_set_som_desc as usize),
    ("somutSetSomDesc2", __m_set_som_desc2 as usize),
    ("somutGetNumArgs", __m_get_num_args as usize),
    ("somutGetRetSize", __m_get_ret_size as usize),
    ("somutGetRetInfo", __m_get_ret_info as usize),
    ("somutGetArgSize", __m_get_arg_size as usize),
    ("somutGetArgInfo", __m_get_arg_info as usize),
    ("somutGetContextIds", __m_get_context_ids as usize),
    ("somutGetCallStyle", __m_get_call_style as usize),
    ("somutIsOnewayAck", __m_is_oneway_ack as usize),
    ("somutFlushCache", crate::cmd::__m_flush as usize),
    ("somutFlushEntry", crate::cmd::__m_flush as usize),
    ("somutGetOperationModifier", crate::cmd::__m_get_op_modifier as usize),
    ("_get_somutReturnResultOwner", crate::cmd::__m_result_owner as usize),
    ("somutGetArgOwner", crate::cmd::__m_arg_owner as usize),
    ("somutGetArgIndex", crate::cmd::__m_arg_index as usize),
    ]
}

// ---- SOMArgList ----

/// What SOMArgList keeps: its SOMDesc and the arguments, copied.
#[repr(C)]
pub struct Al {
    pub desc: usize,
    pub list: *mut Vec<u8>,
    pub offsets: *mut Vec<u32>,
    pub rest: [usize; 6],
}

pub fn al(o: usize) -> &'static mut Al {
    let a = data::<Al>(p!(SOMArgListCClassData), o);
    if a.list.is_null() {
        a.list = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Vec::new()));
        a.offsets = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Vec::new()));
    }
    a
}

pub fn add(o: usize, bytes: &[u8]) -> i32 {
    let a = al(o);
    let (l, off) = unsafe { (&mut *a.list, &mut *a.offsets) };
    off.push(l.len() as u32);
    l.extend_from_slice(bytes);
    while l.len() % 4 != 0 {
        l.push(0);
    }
    0
}

pub fn arg_at(o: usize, i: u32) -> *const u8 {
    let a = al(o);
    let (l, off) = unsafe { (&*a.list, &*a.offsets) };
    match off.get((i & 0xffff) as usize) {
        Some(&k) => unsafe { l.as_ptr().add(k as usize) },
        None => [0u8; 8].as_ptr(),
    }
}

/// Take a call's arguments apart with a SOMDesc (`ap` after the object).
pub fn set_arg_list(o: usize, desc: usize, ap: *const u32) -> i32 {
    let a = al(o);
    unsafe {
        (*a.list).clear();
        (*a.offsets).clear();
    }
    a.desc = desc;
    let d = ds(desc);
    if d.def == 0 || d.callstyle == 3 {
        return -1;
    }
    let mut p = ap;
    unsafe {
        if d.callstyle == 2 {
            add(o, &(*p).to_le_bytes());
            p = p.add(1);
            if !d.op.is_null() && *d.op.add(6) != 0 {
                add(o, &(*p).to_le_bytes());
                p = p.add(1);
            }
        }
        for i in 0..num_args(d) {
            let Some(info) = arg_info(d, i) else { break };
            let k = if info[4] == 1 { tcKind(info[3], 0) } else { 0 };
            match k {
                7 | 8 => {
                    let mut b = [0u8; 8];
                    core::ptr::copy_nonoverlapping(p as *const u8, b.as_mut_ptr(), 8);
                    add(o, &b);
                    p = p.add(2);
                }
                _ => {
                    add(o, &(*p).to_le_bytes());
                    p = p.add(1);
                }
            }
        }
    }
    0
}

extern "C" fn set_arg_desc(o: usize, desc: usize) -> i32 {
    al(o).desc = desc;
    0
}
extern "C" fn get_arg_desc(o: usize) -> usize {
    al(o).desc
}
extern "C" fn add_word(o: usize, v: u32) -> i32 {
    add(o, &v.to_le_bytes())
}
extern "C" fn add_f64(o: usize, lo: u32, hi: u32) -> i32 {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&lo.to_le_bytes());
    b[4..].copy_from_slice(&hi.to_le_bytes());
    add(o, &b)
}
extern "C" fn add_f32(o: usize, v: u32) -> i32 {
    add_f64_value(o, f32::from_bits(v) as f64)
}
fn add_f64_value(o: usize, v: f64) -> i32 {
    add(o, &v.to_le_bytes())
}
extern "C" fn get_i1(o: usize, i: u32) -> i32 {
    unsafe { *(arg_at(o, i) as *const i8) as i32 }
}
extern "C" fn get_i2(o: usize, i: u32) -> i32 {
    unsafe { core::ptr::read_unaligned(arg_at(o, i) as *const i16) as i32 }
}
extern "C" fn get_u2(o: usize, i: u32) -> u32 {
    unsafe { core::ptr::read_unaligned(arg_at(o, i) as *const u16) as u32 }
}
extern "C" fn get_word(o: usize, i: u32) -> u32 {
    unsafe { core::ptr::read_unaligned(arg_at(o, i) as *const u32) }
}
extern "C" fn get_f64(o: usize, i: u32) -> f64 {
    unsafe { core::ptr::read_unaligned(arg_at(o, i) as *const f64) }
}
extern "C" fn get_bool(o: usize, i: u32) -> u32 {
    unsafe { *arg_at(o, i) as u32 }
}
extern "C" fn get_num_args_list(o: usize) -> u32 {
    let d = al(o).desc;
    if d == 0 { 0 } else { num_args(ds(d)) }
}
extern "C" fn set_arg_list_m(o: usize, desc: usize, ap: *const u32) -> i32 {
    set_arg_list(o, desc, ap)
}
/// somutGetArgList: the copied arguments (a va_list).
extern "C" fn get_arg_list(o: usize) -> *const u8 {
    unsafe { (*al(o).list).as_ptr() }
}
extern "C" fn args_nothing(_o: usize) {}

methods! {
    __m_set_arg_desc => set_arg_desc, 2;
    __m_get_arg_desc => get_arg_desc, 1;
    __m_add_word => add_word, 2;
    __m_add_f32 => add_f32, 2;
    __m_add_f64 => add_f64, 3;
    __m_get_i1 => get_i1, 2;
    __m_get_i2 => get_i2, 2;
    __m_get_u2 => get_u2, 2;
    __m_get_word => get_word, 2;
    __m_get_f64 => get_f64, 2;
    __m_get_bool => get_bool, 2;
    __m_get_num_args_list => get_num_args_list, 1;
    __m_set_arg_list => set_arg_list_m, 3;
    __m_get_arg_list => get_arg_list, 1;
    __m_args_init => args_nothing, 1;
    __m_args_uninit => args_nothing, 1;
}

pub fn arglist_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
    ("somutSetArgDesc", __m_set_arg_desc as usize),
    ("somutGetArgDesc", __m_get_arg_desc as usize),
    ("somutAddInteger1Arg", __m_add_word as usize),
    ("somutAddInteger2Arg", __m_add_word as usize),
    ("somutAddUInteger2Arg", __m_add_word as usize),
    ("somutAddInteger4Arg", __m_add_word as usize),
    ("somutAddUInteger4Arg", __m_add_word as usize),
    ("somutAddFloat4Arg", __m_add_f32 as usize),
    ("somutAddFloat8Arg", __m_add_f64 as usize),
    ("somutAddStringArg", __m_add_word as usize),
    ("somutAddFStringArg", __m_add_word as usize),
    ("somutAddSomTokenArg", __m_add_word as usize),
    ("somutAddBooleanArg", __m_add_word as usize),
    ("somutGetInteger1Arg", __m_get_i1 as usize),
    ("somutGetInteger2Arg", __m_get_i2 as usize),
    ("somutGetUInteger2Arg", __m_get_u2 as usize),
    ("somutGetInteger4Arg", __m_get_word as usize),
    ("somutGetUInteger4Arg", __m_get_word as usize),
    ("somutGetFloat4Arg", __m_get_f64 as usize),
    ("somutGetFloat8Arg", __m_get_f64 as usize),
    ("somutGetStringArg", __m_get_word as usize),
    ("somutGetFStringArg", __m_get_word as usize),
    ("somutGetSomTokenArg", __m_get_word as usize),
    ("somutGetBooleanArg", __m_get_bool as usize),
    ("somutGetNumArgsList", __m_get_num_args_list as usize),
    ("somutGetArgList", __m_get_arg_list as usize),
    ("somutSetArgList", __m_set_arg_list as usize),
    ("somutGetArgs", crate::cmd::__m_get_args as usize),
    ("somutSetArgs", crate::cmd::__m_set_args as usize),
    ("somutAddSomEnvArg", __m_add_word as usize),
    ]
}

pub fn command_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
    ("somutSetCommand", crate::cmd::__m_set_command as usize),
    ("somutSetClassCommand", crate::cmd::__m_set_class_command as usize),
    ("somutGetExtRep", crate::cmd::__m_get_ext_rep as usize),
    ("somutSetExtRep", crate::cmd::__m_set_ext_rep as usize),
    ("somutDoCommand", crate::cmd::__m_do_command as usize),
    ]
}

extern "C" fn nothing(_o: usize) {}

methods! {
    __m_cmd_init => nothing, 1;
    __m_cmd_uninit => nothing, 1;
}


// ---- SOMMTraced's printing (IBM's formats) ----

fn out(fmt: &[u8], args: &[u32]) {
    somsup::printf(fmt, args);
}

/// A value of a type (`p` its address), as IBM's tracer prints it.
fn print_value(p: *const u8, t: usize) {
    let k = unsafe { tcKind(t, 0) };
    let w = unsafe { core::ptr::read_unaligned(p as *const u32) };
    let param = |i: i32| -> usize {
        let mut a = [0usize; 2];
        unsafe { tcParameter(&mut a, t, 0, i) };
        if a[1] == 0 { 0 } else { unsafe { *(a[1] as *const usize) } }
    };
    match k {
        1 | 2 => {}
        3 => out(b"%d\0", &[unsafe { core::ptr::read_unaligned(p as *const i16) } as i32 as u32]),
        4 => out(b"%ld\0", &[w]),
        5 => out(b"%u\0", &[unsafe { core::ptr::read_unaligned(p as *const u16) } as u32]),
        6 => out(b"%lu\0", &[w]),
        7 | 8 => {
            let v = unsafe { core::ptr::read_unaligned(p as *const f64) }.to_bits();
            out(b"%g\0", &[v as u32, (v >> 32) as u32]);
        }
        9 => out(b"%s\0", &[if unsafe { *p } != 0 { b"TRUE\0".as_ptr() } else { b"FALSE\0".as_ptr() } as u32]),
        10 => out(b"'%c'\0", &[unsafe { *p } as u32]),
        11 => out(b"%u\0", &[unsafe { *p } as u32]),
        12 => out(b"(any)0x%08lx\0", &[w]),
        13 => out(b"(TypeCode)0x%08lx\0", &[w]),
        14 => out(b"(Principal)0x%08lx\0", &[w]),
        15 => {
            let name = if w != 0 && unsafe { somsup::somIsObj(w as usize) } != 0 {
                somsup::class_name(raw_class(w as usize)) as usize
            } else {
                b"?\0".as_ptr() as usize
            };
            out(b"%s at 0x%08lx\0", &[name as u32, w]);
        }
        16 => out(b"(%s)0x%08lx\0", &[param(0) as u32, p as u32]),
        17 => out(b"(%s){_d=%ld,_u=0x%08lx}\0", &[param(0) as u32, w, p as u32]),
        18 => {
            let name = if w >= 1 { param(w as i32) } else { 0 };
            out(b"(%s)%s\0", &[param(0) as u32, if name == 0 { b"?\0".as_ptr() as u32 } else { name as u32 }]);
            out(b"=0x%lx\0", &[w]);
        }
        19 => out(b"\"%s\"\0", &[w]),
        20 => out(b"(%s[%ld])0x%08lx\0", &[b"sequence\0".as_ptr() as u32, unsafe { *(p as *const u32).add(1) }, p as u32]),
        21 => out(b"(array)0x%08lx\0", &[w]),
        _ => out(b"0x%08lx\0", &[w]),
    }
}

/// The arguments of a call (`ap` at the object): ", value" each; FALSE
/// when the method cannot be described.
pub fn trace_args(obj: usize, mid: SomId, ap: usize) -> bool {
    let name = unsafe { somsup::somStringFromId(mid) };
    let Some(desc) = descriptor(obj, name) else {
        return false;
    };
    let mut d = Ds { kind: 0, def: 0, spare: 0, op: core::ptr::null(), callstyle: 3, is_attr: 0, is_get: 0, pad: [0; 2], attr_type: 0 };
    if set_desc(&mut d, &desc) != 0 {
        return false;
    }
    let mut p = (ap + 4) as *const u32;
    if d.callstyle == 2 {
        out(b", \0", &[]);
        out(b"(Environment)0x%08lx\0", &[unsafe { *p }]);
        p = unsafe { p.add(1) };
    }
    for i in 0..num_args(&d) {
        out(b", \0", &[]);
        let Some(info) = arg_info(&d, i) else {
            out(b" ???? cannot print parameters ????\0", &[]);
            return true;
        };
        let k = unsafe { tcKind(info[3], 0) };
        if info[4] == 1 {
            print_value(p as *const u8, info[3]);
            p = unsafe { p.add(if k == 7 || k == 8 { 2 } else { 1 }) };
        } else {
            out(b"%s 0x%08lx\0", &[if info[4] == 2 { b"out\0".as_ptr() } else { b"inout\0".as_ptr() } as u32, unsafe { *p }]);
            p = unsafe { p.add(1) };
        }
    }
    true
}

/// A call's result (`ret` its address); FALSE when it cannot be
/// described.
pub fn trace_result(obj: usize, mid: SomId, ret: usize) -> bool {
    let name = unsafe { somsup::somStringFromId(mid) };
    let Some(desc) = descriptor(obj, name) else { return false };
    let mut d = Ds { kind: 0, def: 0, spare: 0, op: core::ptr::null(), callstyle: 3, is_attr: 0, is_get: 0, pad: [0; 2], attr_type: 0 };
    if set_desc(&mut d, &desc) != 0 {
        return false;
    }
    let (t, _) = ret_info(&d);
    if t != 0 && ret != 0 {
        print_value(ret as *const u8, t);
    }
    true
}

pub fn _unused(_v: Vec<u8>) {}
