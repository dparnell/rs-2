//! The Apple-event terminology classes (ordinals 1000-1046), as OREXXSC's
//! scriptable-application support uses them (docs/OPENDOC.md). IBM's
//! names for them are not known; rs2's are its own. They are built on
//! SOMUC's somf_MCollectible:
//!
//! - **ODAETEDictionary** (1000 CClassData, 1001 ClassData, 1002
//!   NewClass): an application's terminology. Method 2,
//!   `Load(appName)`, asks the application for it: the Apple event
//!   'ascr'/'gdte' to the process of that name, whose reply's direct
//!   parameter is an 'aete' (see below). Method 12,
//!   `FindEvent(name, *event)`, gives the event of that name (any
//!   case).
//! - **ODAETEEvent** (1019): method 2, `GetInfo(*info)`, gives the
//!   event's record as OREXXSC reads it: name[256] at 0, description[256]
//!   at 0x100, class at 0x200, ID at 0x204, reply type at 0x208, reply
//!   description[256] at 0x20C, reply flags (16 bits) at 0x30C, direct
//!   parameter type at 0x310, its description[256] at 0x314 and flags at
//!   0x414. Method 6, `FindParameter(name, *param)`.
//! - **ODAETEParameter** (1040): method 1, `GetInfo(*info)`: name[256] at
//!   0, keyword at 0x100, type at 0x104, description[256] at 0x108, flags
//!   at 0x208.
//! - The other class data OREXXSC imports (1004, 1013, 1016, 1021, 1030,
//!   1033, 1034, 1043, 1046) belong to classes whose methods fail
//!   (classes, properties, enumerations, comparisons: not yet).
//!
//! The 'aete' rs2 reads is Apple's layout, little-endian, with OS/2
//! OpenDoc's four-character codes: version (2 bytes), language and script
//! (2 each), the suites (count, 2 bytes), each a name and description
//! (Pascal strings, then padded to an even offset), ID (4), level and
//! version (2 each) and its events (count): name, description, class and
//! ID (4 each), reply type, description and flags (2), direct parameter
//! type, description and flags, then the other parameters (count): name,
//! keyword, type, description, flags. A suite's classes, comparisons and
//! enumerations follow its events; rs2 reads none yet.

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
        pub static mut $cd: [usize; 24] = [0; 24];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

aete_class_data! {
    ODAETEDictionaryClassData, ODAETEDictionaryCClassData;
    ODAETEEventClassData, ODAETEEventCClassData;
    ODAETEParameterClassData, ODAETEParameterCClassData;
    ODAETETerm1004ClassData, ODAETETerm1004CClassData;
    ODAETETerm1013ClassData, ODAETETerm1013CClassData;
    ODAETETerm1016ClassData, ODAETETerm1016CClassData;
    ODAETETerm1021ClassData, ODAETETerm1021CClassData;
    ODAETETerm1030ClassData, ODAETETerm1030CClassData;
    ODAETETerm1034ClassData, ODAETETerm1034CClassData;
    ODAETETerm1043ClassData, ODAETETerm1043CClassData;
    ODAETETerm1046ClassData, ODAETETerm1046CClassData;
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

pub struct Event {
    info: Box<EventInfo>,
    params: Vec<usize>,
}

#[derive(Default)]
struct Dict {
    events: Vec<usize>,
}

fn dict(o: usize) -> &'static mut Dict {
    state(p!(ODAETEDictionaryCClassData) as *const usize, o)
}

#[derive(Default)]
struct EventState(Option<Event>);

fn event(o: usize) -> &'static mut EventState {
    state(p!(ODAETEEventCClassData) as *const usize, o)
}

#[derive(Default)]
struct ParamState(Option<Box<ParamInfo>>);

fn param(o: usize) -> &'static mut ParamState {
    state(p!(ODAETEParameterCClassData) as *const usize, o)
}

fn fill(dst: &mut [u8; 256], s: &[u8]) {
    let n = s.len().min(255);
    dst[..n].copy_from_slice(&s[..n]);
    dst[n] = 0;
}

fn name_of(b: &[u8; 256]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(256);
    &b[..n]
}

/// Reading an 'aete'.
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
    fn align(&mut self) {
        self.p += self.p & 1;
    }
}

fn new_object_of(cd: *mut usize, make: fn()) -> usize {
    unsafe {
        if *cd == 0 {
            make();
        }
        somsup::new_object(*cd)
    }
}

/// The events of an 'aete' as ODAETEEvent objects.
fn parse(b: &[u8]) -> Option<Vec<usize>> {
    let mut r = Rd { b, p: 0 };
    r.u16()?; // version
    r.u16()?; // language
    r.u16()?; // script
    let suites = r.u16()?;
    let mut out = Vec::new();
    for _ in 0..suites {
        r.pstr()?;
        r.pstr()?;
        r.align();
        r.u32()?;
        r.u16()?;
        r.u16()?;
        let events = r.u16()?;
        for _ in 0..events {
            let mut info: Box<EventInfo> = Box::new(unsafe { core::mem::zeroed() });
            let name = r.pstr()?;
            let desc = r.pstr()?;
            r.align();
            fill(&mut info.name, &name);
            fill(&mut info.desc, &desc);
            info.class = r.u32()?;
            info.id = r.u32()?;
            info.reply_type = r.u32()?;
            let rd = r.pstr()?;
            r.align();
            fill(&mut info.reply_desc, &rd);
            info.reply_flags = r.u16()?;
            info.direct_type = r.u32()?;
            let dd = r.pstr()?;
            r.align();
            fill(&mut info.direct_desc, &dd);
            info.direct_flags = r.u16()?;
            let n = r.u16()?;
            let mut params = Vec::new();
            for _ in 0..n {
                let mut pi: Box<ParamInfo> = Box::new(unsafe { core::mem::zeroed() });
                let pn = r.pstr()?;
                r.align();
                fill(&mut pi.name, &pn);
                pi.keyword = r.u32()?;
                pi.ty = r.u32()?;
                let pd = r.pstr()?;
                r.align();
                fill(&mut pi.desc, &pd);
                pi.flags = r.u16()?;
                let po = new_object_of(p!(ODAETEParameterClassData), build_param);
                param(po).0 = Some(pi);
                params.push(po);
            }
            let eo = new_object_of(p!(ODAETEEventClassData), build_event);
            event(eo).0 = Some(Event { info, params });
            out.push(eo);
        }
        // the suite's classes: rs2 reads no further
        if r.u16().unwrap_or(0) != 0 {
            break;
        }
        r.u16();
        r.u16();
    }
    Some(out)
}

methods! {
    // ODAETEDictionary::Load(appName): the application's 'aete'
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
                    Some(evs) => dict(this).events = evs,
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
    // ODAETEDictionary::FindEvent(name, *event)
    fn DictFindEvent(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        for &e in &dict(this).events {
            if let Some(x) = &event(e).0 {
                if name_of(&x.info.name).eq_ignore_ascii_case(want) {
                    write::<usize>(out, e);
                    return 0;
                }
            }
        }
        ERR_NOT_FOUND as usize
    }
    // ODAETEEvent::GetInfo(*info)
    fn EventGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match &event(this).0 {
            Some(x) => {
                write::<usize>(out, &*x.info as *const EventInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    // ODAETEEvent::FindParameter(name, *param)
    fn EventFindParam(this: usize, ev: usize, name: usize, out: usize) -> usize {
        let want = cstr(name);
        if let Some(x) = &event(this).0 {
            for &p in &x.params {
                if param(p).0.as_ref().is_some_and(|pi| name_of(&pi.name).eq_ignore_ascii_case(want)) {
                    write::<usize>(out, p);
                    return 0;
                }
            }
        }
        ERR_NOT_FOUND as usize
    }
    // ODAETEParameter::GetInfo(*info)
    fn ParamGetInfo(this: usize, ev: usize, out: usize) -> usize {
        match &param(this).0 {
            Some(x) => {
                write::<usize>(out, &**x as *const ParamInfo as usize);
                0
            }
            None => ERR_NOT_FOUND as usize,
        }
    }
    fn NotFound1(this: usize, ev: usize, a: usize) -> usize { ERR_NOT_FOUND as usize }
    fn NotFound2(this: usize, ev: usize, a: usize, b: usize) -> usize { ERR_NOT_FOUND as usize }
    fn NotFound3(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { ERR_NOT_FOUND as usize }
}

fn methods_with(n: usize, set: &[(usize, M)]) -> Vec<(&'static str, usize)> {
    const NAMES: [&str; 20] = [
        "Term_m0", "Term_m1", "Term_m2", "Term_m3", "Term_m4", "Term_m5", "Term_m6", "Term_m7", "Term_m8", "Term_m9", "Term_m10",
        "Term_m11", "Term_m12", "Term_m13", "Term_m14", "Term_m15", "Term_m16", "Term_m17", "Term_m18", "Term_m19",
    ];
    (0..n)
        .map(|i| (NAMES[i], set.iter().find(|x| x.0 == i).map_or(NotFound3::t as usize, |x| x.1 as usize)))
        .collect()
}

fn build(name: &str, n: usize, set: &[(usize, M)], cd: *mut usize, ccd: *mut usize) -> usize {
    unsafe { somf_MCollectibleNewClass(2, 1) };
    let m = methods_with(n, set);
    somsup::build_class(&somsup::ClassSpec {
        name,
        parents: &["somf_MCollectible"],
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

fn build_param() {
    build("ODAETEParameter", 4, &[(1, ParamGetInfo::t)], p!(ODAETEParameterClassData), p!(ODAETEParameterCClassData));
}

fn build_event() {
    build("ODAETEEvent", 8, &[(2, EventGetInfo::t), (3, NotFound1::t), (6, EventFindParam::t)], p!(ODAETEEventClassData), p!(ODAETEEventCClassData));
}

/// ODAETEDictionaryNewClass (1002): the terminology classes.
pub extern "C" fn dictionary_new_class(_major: i32, _minor: i32) -> usize {
    unsafe {
        if ODAETEDictionaryClassData[0] != 0 {
            return ODAETEDictionaryClassData[0];
        }
    }
    build_param();
    build_event();
    for (name, cd, ccd) in [
        ("ODAETETerm1004", p!(ODAETETerm1004ClassData), p!(ODAETETerm1004CClassData)),
        ("ODAETETerm1013", p!(ODAETETerm1013ClassData), p!(ODAETETerm1013CClassData)),
        ("ODAETETerm1016", p!(ODAETETerm1016ClassData), p!(ODAETETerm1016CClassData)),
        ("ODAETETerm1021", p!(ODAETETerm1021ClassData), p!(ODAETETerm1021CClassData)),
        ("ODAETETerm1030", p!(ODAETETerm1030ClassData), p!(ODAETETerm1030CClassData)),
        ("ODAETETerm1034", p!(ODAETETerm1034ClassData), p!(ODAETETerm1034CClassData)),
        ("ODAETETerm1043", p!(ODAETETerm1043ClassData), p!(ODAETETerm1043CClassData)),
        ("ODAETETerm1046", p!(ODAETETerm1046ClassData), p!(ODAETETerm1046CClassData)),
    ] {
        build(name, 20, &[], cd, ccd);
    }
    build("ODAETEDictionary", 15, &[(2, DictLoad::t), (12, DictFindEvent::t)], p!(ODAETEDictionaryClassData), p!(ODAETEDictionaryCClassData))
}

#[allow(dead_code)]
fn unused() -> usize {
    NotFound2::t as usize
}
