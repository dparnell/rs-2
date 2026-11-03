//! The Apple-event terminology classes (ordinals 1000-1046), as OREXXSC's
//! scriptable-application support uses them (docs/OPENDOC.md). IBM's
//! names for them are not known; rs2's are its own. All are built on
//! SOMUC's somf_MCollectible; the method numbers are the tokens OREXXSC
//! calls (from its disassembly):
//!
//! - **ODAETEList** (1033 CClassData, 1034 ClassData): what the
//!   dictionary and a suite have in common: 2 `First(*cursor, *item)`,
//!   3 `Next(*cursor, *item)` over a cursor (an ODAETECursor object the
//!   caller frees) one of their own methods made.
//! - **ODAETEDictionary** (1000-1002), an ODAETEList: an application's
//!   terminology. 2 `Load(appName)`: the Apple event 'ascr'/'gdte' to
//!   the process of that name, whose reply's direct parameter is an
//!   'aete'; 9 `SuiteCursor(*cursor)`; 12 `FindEvent(name, *event)`;
//!   14 `FindClass(name, *class)` (names in any case).
//! - **ODAETESuite** (1046), an ODAETEList: 7 `ClassCursor(*cursor)`.
//! - **ODAETEClass** (1004): 3 `GetInfo(*info)` (name[256] at 0, code at
//!   0x100, description[256] at 0x104); 10 `FindProperty(name, *prop)`.
//! - **ODAETEProperty** (1043): 0 `GetInfo(*info)` (name[256], code at
//!   0x100, type at 0x104, description[256] at 0x108, flags at 0x208).
//! - **ODAETEEvent** (1019): 2 `GetInfo(*info)` (name[256] at 0,
//!   description[256] at 0x100, class 0x200, ID 0x204, reply type 0x208,
//!   reply description[256] 0x20C, reply flags 0x30C, direct parameter
//!   type 0x310, its description[256] 0x314, flags 0x414); 6
//!   `FindParameter(name, *param)`.
//! - **ODAETEParameter** (1040): 1 `GetInfo(*info)` (name[256], keyword
//!   0x100, type 0x104, description[256] 0x108, flags 0x208).
//! - The class data of 1013, 1016, 1021 and 1030 belong to classes whose
//!   methods fail (they serve OSA recording's event-to-REXX text).
//!
//! The 'aete' is Apple's layout, little-endian, with OS/2 OpenDoc's
//! four-character codes; every Pascal string that a number follows is
//! padded to an even offset: version, language, script (2 bytes each),
//! suites (count): name, description, ID (4), level, version (2 each),
//! events (count: name, description, class, ID, reply type, description,
//! flags, direct parameter type, description, flags, parameters (count:
//! name, keyword, type, description, flags)), classes (count: name, code,
//! description, properties (count: name, code, type, description,
//! flags), elements (count: class, key forms (count, codes))),
//! comparisons (count: name, code, description), enumerations (count:
//! code, items (count: name, code, description)).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ae::{self, Desc};
use crate::rt::*;
use crate::*;

unsafe extern "C" {
    #[link_name = "SOMUC.26"]
    fn somf_MCollectibleNewClass(major: i32, minor: i32) -> usize;
}

macro_rules! aete_class_data {
    ($($cd:ident, $ccd:ident;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; 32] = [0; 32];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

aete_class_data! {
    ODAETEListClassData, ODAETEListCClassData;
    ODAETEDictionaryClassData, ODAETEDictionaryCClassData;
    ODAETESuiteClassData, ODAETESuiteCClassData;
    ODAETEClassClassData, ODAETEClassCClassData;
    ODAETEPropertyClassData, ODAETEPropertyCClassData;
    ODAETEEventClassData, ODAETEEventCClassData;
    ODAETEParameterClassData, ODAETEParameterCClassData;
    ODAETECursorClassData, ODAETECursorCClassData;
    ODAETETerm1013ClassData, ODAETETerm1013CClassData;
    ODAETETerm1016ClassData, ODAETETerm1016CClassData;
    ODAETETerm1021ClassData, ODAETETerm1021CClassData;
    ODAETETerm1030ClassData, ODAETETerm1030CClassData;
}

pub const ERR_NOT_FOUND: i32 = -1701;

#[repr(C)]
pub struct EventInfo {
    pub name: [u8; 256],
    pub desc: [u8; 256],
    pub class: u32,
    pub id: u32,
    pub reply_type: u32,
    pub reply_desc: [u8; 256],
    pub reply_flags: u16,
    pub pad: u16,
    pub direct_type: u32,
    pub direct_desc: [u8; 256],
    pub direct_flags: u16,
    pub pad2: u16,
}

#[repr(C)]
pub struct ParamInfo {
    pub name: [u8; 256],
    pub keyword: u32,
    pub ty: u32,
    pub desc: [u8; 256],
    pub flags: u16,
    pub pad: u16,
}

#[repr(C)]
pub struct ClassInfo {
    pub name: [u8; 256],
    pub code: u32,
    pub desc: [u8; 256],
}

// ---- object state ----

macro_rules! st {
    ($f:ident, $t:ty, $ccd:ident) => {
        fn $f(o: usize) -> &'static mut $t {
            state(p!($ccd) as *const usize, o)
        }
    };
}

#[derive(Default)]
struct Dict {
    suites: Vec<usize>,
}

#[derive(Default)]
struct Suite {
    events: Vec<usize>,
    classes: Vec<usize>,
}

#[derive(Default)]
struct Class {
    info: Option<Box<ClassInfo>>,
    props: Vec<usize>,
}

#[derive(Default)]
struct EventState {
    info: Option<Box<EventInfo>>,
    params: Vec<usize>,
}

#[derive(Default)]
struct Cursor {
    items: Vec<usize>,
    pos: usize,
}

st!(dict, Dict, ODAETEDictionaryCClassData);
st!(suite, Suite, ODAETESuiteCClassData);
st!(class, Class, ODAETEClassCClassData);
st!(event, EventState, ODAETEEventCClassData);
st!(param, Option<Box<ParamInfo>>, ODAETEParameterCClassData);
st!(prop, Option<Box<ParamInfo>>, ODAETEPropertyCClassData);
st!(cursor, Cursor, ODAETECursorCClassData);

fn fill(dst: &mut [u8; 256], s: &[u8]) {
    let n = s.len().min(255);
    dst[..n].copy_from_slice(&s[..n]);
    dst[n] = 0;
}

fn name_of(b: &[u8; 256]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(256);
    &b[..n]
}

fn same_name(a: &[u8; 256], b: &[u8]) -> bool {
    name_of(a).eq_ignore_ascii_case(b)
}

fn new_of(cd: *mut usize) -> usize {
    unsafe {
        if *cd == 0 {
            dictionary_new_class(1, 0);
        }
        somsup::new_object(*cd)
    }
}

// ---- reading an 'aete' ----

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(self.u8()? as u16 | (self.u8()? as u16) << 8)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(self.u16()? as u32 | (self.u16()? as u32) << 16)
    }
    fn pstr(&mut self) -> Option<Vec<u8>> {
        let n = self.u8()? as usize;
        let s = self.b.get(self.p..self.p + n)?.to_vec();
        self.p += n;
        Some(s)
    }
    /// A Pascal string a number follows.
    fn pstr_a(&mut self) -> Option<Vec<u8>> {
        let s = self.pstr()?;
        self.p += self.p & 1;
        Some(s)
    }
}

fn param_info(r: &mut Rd) -> Option<Box<ParamInfo>> {
    let mut pi: Box<ParamInfo> = Box::new(unsafe { core::mem::zeroed() });
    let n = r.pstr_a()?;
    fill(&mut pi.name, &n);
    pi.keyword = r.u32()?;
    pi.ty = r.u32()?;
    let d = r.pstr_a()?;
    fill(&mut pi.desc, &d);
    pi.flags = r.u16()?;
    Some(pi)
}

fn read_event(r: &mut Rd) -> Option<usize> {
    let mut info: Box<EventInfo> = Box::new(unsafe { core::mem::zeroed() });
    let name = r.pstr()?;
    let desc = r.pstr_a()?;
    fill(&mut info.name, &name);
    fill(&mut info.desc, &desc);
    info.class = r.u32()?;
    info.id = r.u32()?;
    info.reply_type = r.u32()?;
    let rd = r.pstr_a()?;
    fill(&mut info.reply_desc, &rd);
    info.reply_flags = r.u16()?;
    info.direct_type = r.u32()?;
    let dd = r.pstr_a()?;
    fill(&mut info.direct_desc, &dd);
    info.direct_flags = r.u16()?;
    let mut params = Vec::new();
    for _ in 0..r.u16()? {
        let pi = param_info(r)?;
        let po = new_of(p!(ODAETEParameterClassData));
        *param(po) = Some(pi);
        params.push(po);
    }
    let eo = new_of(p!(ODAETEEventClassData));
    *event(eo) = EventState { info: Some(info), params };
    Some(eo)
}

fn read_class(r: &mut Rd) -> Option<usize> {
    let mut ci: Box<ClassInfo> = Box::new(unsafe { core::mem::zeroed() });
    let n = r.pstr_a()?;
    fill(&mut ci.name, &n);
    ci.code = r.u32()?;
    let d = r.pstr_a()?;
    fill(&mut ci.desc, &d);
    let mut props = Vec::new();
    for _ in 0..r.u16()? {
        let pi = param_info(r)?;
        let po = new_of(p!(ODAETEPropertyClassData));
        *prop(po) = Some(pi);
        props.push(po);
    }
    for _ in 0..r.u16()? {
        r.u32()?;
        for _ in 0..r.u16()? {
            r.u32()?;
        }
    }
    let co = new_of(p!(ODAETEClassClassData));
    *class(co) = Class { info: Some(ci), props };
    Some(co)
}

fn parse(b: &[u8]) -> Option<Vec<usize>> {
    let mut r = Rd { b, p: 0 };
    r.u16()?;
    r.u16()?;
    r.u16()?;
    let mut suites = Vec::new();
    for _ in 0..r.u16()? {
        r.pstr()?;
        r.pstr_a()?;
        r.u32()?;
        r.u16()?;
        r.u16()?;
        let mut s = Suite::default();
        for _ in 0..r.u16()? {
            s.events.push(read_event(&mut r)?);
        }
        for _ in 0..r.u16()? {
            s.classes.push(read_class(&mut r)?);
        }
        for _ in 0..r.u16()? {
            r.pstr_a()?;
            r.u32()?;
            r.pstr_a()?;
        }
        for _ in 0..r.u16()? {
            r.u32()?;
            for _ in 0..r.u16()? {
                r.pstr_a()?;
                r.u32()?;
                r.pstr_a()?;
            }
        }
        let so = new_of(p!(ODAETESuiteClassData));
        *suite(so) = s;
        suites.push(so);
    }
    Some(suites)
}

fn make_cursor(items: Vec<usize>, out: usize) {
    let c = new_of(p!(ODAETECursorClassData));
    *cursor(c) = Cursor { items, pos: 0 };
    write::<usize>(out, c);
}

fn all_events(d: usize) -> Vec<usize> {
    dict(d).suites.iter().flat_map(|&s| suite(s).events.clone()).collect()
}

fn all_classes(d: usize) -> Vec<usize> {
    dict(d).suites.iter().flat_map(|&s| suite(s).classes.clone()).collect()
}

methods! {
    // ODAETEList::First(*cursor, *item), ::Next(*cursor, *item)
    fn ListFirst(this: usize, ev: usize, cur: usize, out: usize) -> usize {
        let c = if cur == 0 { 0 } else { read::<usize>(cur) };
        let item = if c == 0 { 0 } else {
            let x = cursor(c);
            x.pos = 0;
            x.items.first().copied().unwrap_or(0)
        };
        write::<usize>(out, item);
        0
    }
    fn ListNext(this: usize, ev: usize, cur: usize, out: usize) -> usize {
        let c = if cur == 0 { 0 } else { read::<usize>(cur) };
        let item = if c == 0 { 0 } else {
            let x = cursor(c);
            x.pos += 1;
            x.items.get(x.pos).copied().unwrap_or(0)
        };
        write::<usize>(out, item);
        0
    }
    // ODAETEDictionary::Load(appName)
    fn DictLoad(this: usize, ev: usize, app: usize) -> usize {
        let mut pid = 0u32;
        if ae::AEGetProcessID(app, -1, &mut pid as *mut u32 as usize) != 0 {
            return ae::ERR_PROC_NOT_FOUND as usize;
        }
        let mut target = Desc::default();
        let mut e = Desc::default();
        let mut reply = Desc::default();
        let tp = &mut target as *mut Desc as usize;
        ae::AECreateDesc(ae::code(b"PID "), &pid as *const u32 as usize, 4, tp);
        ae::AECreateAppleEvent(ae::code(b"ascr"), ae::code(b"gdte"), tp, -1, 0, &mut e as *mut Desc as usize);
        let mut rc = ae::AESend(&e as *const Desc as usize, &mut reply as *mut Desc as usize, 0x23, 0, -1, 0, 0);
        if rc == 0 {
            let mut d = Desc::default();
            rc = ae::AEGetParamDesc(&reply as *const Desc as usize, ae::code(b"----"), ae::code(b"aete"), &mut d as *mut Desc as usize);
            if rc == 0 {
                let mut n = 0u32;
                ae::AEGetDescDataSize(&d as *const Desc as usize, &mut n as *mut u32 as usize);
                let mut buf = alloc::vec![0u8; n as usize];
                ae::AEGetDescData(&d as *const Desc as usize, 0, buf.as_mut_ptr() as usize, n as usize, 0);
                match parse(&buf) {
                    Some(s) => dict(this).suites = s,
                    None => rc = -1702,
                }
                ae::AEDisposeDesc(&mut d as *mut Desc as usize);
            }
        }
        for d in [&mut target, &mut e, &mut reply] {
            ae::AEDisposeDesc(d as *mut Desc as usize);
        }
        rc as usize
    }
    // ODAETEDictionary::SuiteCursor(*cursor)
    fn DictSuiteCursor(this: usize, ev: usize, out: usize) -> usize {
        make_cursor(dict(this).suites.clone(), out);
        0
    }
    // ODAETEDictionary::EventCursor(*cursor) (method 6)
    fn DictEventCursor(this: usize, ev: usize, out: usize) -> usize {
        make_cursor(all_events(this), out);
        0
    }
    // ODAETEDictionary::FindEvent(name, *event)
    fn DictFindEvent(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        match all_events(this).into_iter().find(|&e| event(e).info.as_ref().is_some_and(|i| same_name(&i.name, want))) {
            Some(e) => {
                write::<usize>(out, e);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEDictionary::FindClass(name, *class)
    fn DictFindClass(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        match all_classes(this).into_iter().find(|&c| class(c).info.as_ref().is_some_and(|i| same_name(&i.name, want))) {
            Some(c) => {
                write::<usize>(out, c);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETESuite::ClassCursor(*cursor)
    fn SuiteClassCursor(this: usize, ev: usize, out: usize) -> usize {
        make_cursor(suite(this).classes.clone(), out);
        0
    }
    // ODAETEClass::GetInfo(*info)
    fn ClassGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match &class(this).info {
            Some(i) => {
                write::<usize>(out, &**i as *const ClassInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEClass::FindProperty(name, *property)
    fn ClassFindProperty(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        match class(this).props.iter().copied().find(|&p| prop(p).as_ref().is_some_and(|i| same_name(&i.name, want))) {
            Some(p) => {
                write::<usize>(out, p);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEProperty::GetInfo(*info)
    fn PropGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match prop(this) {
            Some(i) => {
                write::<usize>(out, &**i as *const ParamInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEEvent::GetInfo(*info)
    fn EventGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match &event(this).info {
            Some(i) => {
                write::<usize>(out, &**i as *const EventInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEEvent::ParamCursor(*cursor) (method 3)
    fn EventParamCursor(this: usize, ev: usize, out: usize) -> usize {
        make_cursor(event(this).params.clone(), out);
        0
    }
    // ODAETEEvent::FindParameter(name, *param)
    fn EventFindParam(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        match event(this).params.iter().copied().find(|&p| param(p).as_ref().is_some_and(|i| same_name(&i.name, want))) {
            Some(p) => {
                write::<usize>(out, p);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEParameter::GetInfo(*info)
    fn ParamGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match param(this) {
            Some(i) => {
                write::<usize>(out, &**i as *const ParamInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    fn NotFound3(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { ERR_NOT_FOUND as usize }
}

/// The methods of a class, in release order: each name is the class's
/// own (a subclass may not introduce a name its parent has).
fn methods_with(class: &str, n: usize, set: &[(usize, M)]) -> Vec<(&'static str, usize)> {
    (0..n)
        .map(|i| {
            let name: &'static str = alloc::boxed::Box::leak(alloc::format!("{class}_m{i}").into_boxed_str());
            (name, set.iter().find(|x| x.0 == i).map_or(NotFound3::t as usize, |x| x.1 as usize))
        })
        .collect()
}

fn build(name: &str, parent: &str, n: usize, set: &[(usize, M)], cd: *mut usize, ccd: *mut usize) -> usize {
    let m = methods_with(name, n, set);
    somsup::build_class(&somsup::ClassSpec {
        name,
        parents: &[parent],
        meta: None,
        major: 1,
        minor: 0,
        data: 4,
        align: 4,
        methods: &m,
        overrides: &[],
        cd,
        ccd,
    })
}

/// ODAETEDictionaryNewClass (1002): the terminology classes.
pub extern "C" fn dictionary_new_class(_major: i32, _minor: i32) -> usize {
    unsafe {
        if ODAETEDictionaryClassData[0] != 0 {
            return ODAETEDictionaryClassData[0];
        }
        somf_MCollectibleNewClass(2, 1);
    }
    const MC: &str = "somf_MCollectible";
    build("ODAETECursor", MC, 0, &[], p!(ODAETECursorClassData), p!(ODAETECursorCClassData));
    build("ODAETEList", MC, 4, &[(2, ListFirst::t), (3, ListNext::t)], p!(ODAETEListClassData), p!(ODAETEListCClassData));
    build("ODAETEParameter", MC, 4, &[(1, ParamGetInfo::t)], p!(ODAETEParameterClassData), p!(ODAETEParameterCClassData));
    build("ODAETEProperty", MC, 4, &[(0, PropGetInfo::t)], p!(ODAETEPropertyClassData), p!(ODAETEPropertyCClassData));
    build("ODAETEEvent", MC, 8, &[(2, EventGetInfo::t), (3, EventParamCursor::t), (6, EventFindParam::t)], p!(ODAETEEventClassData), p!(ODAETEEventCClassData));
    build("ODAETEClass", MC, 12, &[(3, ClassGetInfo::t), (10, ClassFindProperty::t)], p!(ODAETEClassClassData), p!(ODAETEClassCClassData));
    build("ODAETESuite", "ODAETEList", 26, &[(7, SuiteClassCursor::t)], p!(ODAETESuiteClassData), p!(ODAETESuiteCClassData));
    for (name, cd, ccd) in [
        ("ODAETETerm1013", p!(ODAETETerm1013ClassData), p!(ODAETETerm1013CClassData)),
        ("ODAETETerm1016", p!(ODAETETerm1016ClassData), p!(ODAETETerm1016CClassData)),
        ("ODAETETerm1021", p!(ODAETETerm1021ClassData), p!(ODAETETerm1021CClassData)),
        ("ODAETETerm1030", p!(ODAETETerm1030ClassData), p!(ODAETETerm1030CClassData)),
    ] {
        build(name, MC, 12, &[], cd, ccd);
    }
    build(
        "ODAETEDictionary",
        "ODAETEList",
        16,
        &[(2, DictLoad::t), (6, DictEventCursor::t), (9, DictSuiteCursor::t), (12, DictFindEvent::t), (14, DictFindClass::t)],
        p!(ODAETEDictionaryClassData),
        p!(ODAETEDictionaryCClassData),
    )
}
