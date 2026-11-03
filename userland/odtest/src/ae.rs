//! Checks of OPENDOC.DLL's Apple Event Manager (ordinals 1-71).

use crate::check;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Desc {
    ty: u32,
    h: usize,
}

unsafe extern "C" {
    #[link_name = "OPENDOC.3"]
    fn AECoerceDesc(d: *const Desc, to: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.5"]
    fn AECountItems(l: *const Desc, n: *mut i32) -> i32;
    #[link_name = "OPENDOC.7"]
    fn AECreateDesc(ty: u32, p: *const u8, n: usize, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.8"]
    fn AECreateList(f: usize, fs: usize, rec: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.10"]
    fn AECreateAppleEvent(class: u32, id: u32, target: *const Desc, rid: i32, tid: i32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.11"]
    fn CreateObjSpecifier(class: u32, cont: *mut Desc, form: u32, key: *mut Desc, dispose: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.16"]
    fn AEDisposeDesc(d: *mut Desc) -> i32;
    #[link_name = "OPENDOC.22"]
    fn AEGetAttributePtr(e: *const Desc, key: u32, ty: u32, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.24"]
    fn AEGetDescData(d: *const Desc, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.28"]
    fn AEGetNthDesc(l: *const Desc, i: i32, ty: u32, key: *mut u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.32"]
    fn AEGetParamDesc(e: *const Desc, key: u32, ty: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.33"]
    fn AEGetParamPtr(e: *const Desc, key: u32, ty: u32, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.38"]
    fn AEInstallEventHandler(class: u32, id: u32, f: usize, refcon: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.42"]
    fn AEGetProcessID(name: *const u8, timeout: i32, pid: *mut u32) -> i32;
    #[link_name = "OPENDOC.50"]
    fn AEPutDesc(l: *const Desc, i: i32, d: *const Desc) -> i32;
    #[link_name = "OPENDOC.51"]
    fn AEPutParamDesc(e: *const Desc, key: u32, d: *const Desc) -> i32;
    #[link_name = "OPENDOC.55"]
    fn AERemoveEventHandler(class: u32, id: u32, f: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.61"]
    fn AESend(e: *const Desc, reply: *mut Desc, mode: u32, prio: i32, timeout: i32, idle: usize, filter: usize) -> i32;
    #[link_name = "OPENDOC.66"]
    fn AEGetDescDataSize(d: *const Desc, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.67"]
    fn AESizeOfNthItem(l: *const Desc, i: i32, tyo: *mut u32, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.71"]
    fn AEGetProcessName(pid: u32, buf: *mut u8, info: usize) -> i32;
}

const fn c(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

fn long(v: i32) -> Desc {
    let mut d = Desc::default();
    unsafe { AECreateDesc(c(b"long"), &v as *const i32 as *const u8, 4, &mut d) };
    d
}

fn text(s: &[u8]) -> Desc {
    let mut d = Desc::default();
    unsafe { AECreateDesc(c(b"TEXT"), s.as_ptr(), s.len(), &mut d) };
    d
}

fn data(d: &Desc) -> (u32, alloc::vec::Vec<u8>) {
    let mut ty = 0;
    let mut n = 0;
    let mut buf = [0u8; 64];
    unsafe { AEGetDescData(d, &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    (ty, buf[..(n as usize).min(64)].to_vec())
}

fn long_of(d: &Desc) -> i32 {
    let (_, b) = data(d);
    if b.len() >= 4 { i32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { i32::MIN }
}

/// An event handler: the reply's direct parameter is twice the event's.
extern "C" fn double_handler(event: *const Desc, reply: *mut Desc, refcon: usize) -> i32 {
    let mut v = 0i32;
    let mut ty = 0;
    let mut n = 0;
    let err = unsafe { AEGetParamPtr(event, c(b"----"), c(b"long"), &mut ty, &mut v as *mut i32 as *mut u8, 4, &mut n) };
    if err != 0 {
        return err;
    }
    let mut r = long(v * 2 + refcon as i32);
    unsafe {
        AEPutParamDesc(reply, c(b"----"), &r);
        AEDisposeDesc(&mut r);
    }
    0
}

pub fn ae_test() {
    unsafe {
        let mut d = long(42);
        let mut n = 0u32;
        AEGetDescDataSize(&d, &mut n);
        check("AE: AECreateDesc and the data back (ordinals 24, 66)", data(&d) == (c(b"long"), 42i32.to_le_bytes().to_vec()) && n == 4);
        let mut t = Desc::default();
        let r1 = AECoerceDesc(&d, c(b"TEXT"), &mut t);
        let mut t2 = text(b" 17");
        let mut l = Desc::default();
        let r2 = AECoerceDesc(&t2, c(b"long"), &mut l);
        check("AECoerceDesc: long to TEXT and TEXT to long", r1 == 0 && data(&t).1 == b"42" && r2 == 0 && long_of(&l) == 17);
        let code = c(b"abcd").to_le_bytes();
        let mut en = Desc::default();
        AECreateDesc(c(b"enum"), code.as_ptr(), 4, &mut en);
        let mut et = Desc::default();
        AECoerceDesc(&en, c(b"TEXT"), &mut et);
        let mut bad = Desc::default();
        let r3 = AECoerceDesc(&t2, c(b"obj "), &mut bad);
        check("an enumeration as TEXT; an impossible coercion fails", data(&et).1 == b"abcd\0" && r3 == -1700);
        // lists
        let mut list = Desc::default();
        AECreateList(0, 0, 0, &mut list);
        AEPutDesc(&list, 0, &d);
        AEPutDesc(&list, 0, &t2);
        let mut count = 0;
        AECountItems(&list, &mut count);
        let mut nth = Desc::default();
        let mut key = 0;
        let r4 = AEGetNthDesc(&list, 1, c(b"TEXT"), &mut key, &mut nth);
        let (mut ty, mut sz) = (0, 0);
        AESizeOfNthItem(&list, 2, &mut ty, &mut sz);
        check("AE lists: AEPutDesc, AECountItems, AEGetNthDesc coerced, AESizeOfNthItem", count == 2 && r4 == 0 && data(&nth).1 == b"42" && ty == c(b"TEXT") && sz == 3);
        // records and object specifiers
        let mut rec = Desc::default();
        AECreateList(0, 0, 1, &mut rec);
        AEPutParamDesc(&rec, c(b"kwd1"), &d);
        let mut v = 0i32;
        let r5 = AEGetParamPtr(&rec, c(b"kwd1"), c(b"long"), &mut ty, &mut v as *mut i32 as *mut u8, 4, &mut sz);
        let r6 = AEGetParamPtr(&rec, c(b"none"), c(b"long"), &mut ty, &mut v as *mut i32 as *mut u8, 4, &mut sz);
        check("AE records: parameters by keyword, a missing one", r5 == 0 && v == 42 && r6 == -1701);
        let mut null = Desc { ty: c(b"null"), h: 0 };
        let mut name = text(b"Fred");
        let mut obj = Desc::default();
        CreateObjSpecifier(c(b"cwin"), &mut null, c(b"name"), &mut name, 0, &mut obj);
        let mut asrec = Desc::default();
        AECoerceDesc(&obj, c(b"reco"), &mut asrec);
        let mut want = Desc::default();
        AEGetParamDesc(&asrec, c(b"want"), c(b"type"), &mut want);
        let mut seld = Desc::default();
        AEGetParamDesc(&asrec, c(b"seld"), c(b"TEXT"), &mut seld);
        check("CreateObjSpecifier: an 'obj ' record with want, form, seld", obj.ty == c(b"obj ") && long_of(&want) as u32 == c(b"cwin") && data(&seld).1 == b"Fred");
        // Apple events to this process
        let mut pid = 0u32;
        let r7 = AEGetProcessID(b"ODTEST\0".as_ptr(), -1, &mut pid);
        let mut pname = [0u8; 64];
        let r8 = AEGetProcessName(pid, pname.as_mut_ptr(), 0);
        check("this process by name and back (ordinals 42, 71)", r7 == 0 && pid != 0 && r8 == 0 && pname.starts_with(b"ODTEST.EXE"));
        let mut target = Desc::default();
        AECreateDesc(c(b"PID "), &pid as *const u32 as *const u8, 4, &mut target);
        let mut ev = Desc::default();
        AECreateAppleEvent(c(b"RS2T"), c(b"dubl"), &target, -1, 0, &mut ev);
        AEPutParamDesc(&ev, c(b"----"), &long(21));
        let mut id = 0u32;
        AEGetAttributePtr(&ev, c(b"evid"), c(b"type"), &mut ty, &mut id as *mut u32 as *mut u8, 4, &mut sz);
        check("AECreateAppleEvent: class and ID attributes", ev.ty == c(b"aevt") && id == c(b"dubl"));
        AEInstallEventHandler(c(b"RS2T"), c(b"dubl"), double_handler as usize, 0, 0);
        let mut reply = Desc::default();
        let r9 = AESend(&ev, &mut reply, 0x23, 0, -1, 0, 0);
        let mut ans = Desc::default();
        AEGetParamDesc(&reply, c(b"----"), c(b"long"), &mut ans);
        check("AESend: the installed handler answers in the reply", r9 == 0 && long_of(&ans) == 42);
        AERemoveEventHandler(c(b"RS2T"), c(b"dubl"), double_handler as usize, 0);
        let mut reply2 = Desc::default();
        let r10 = AESend(&ev, &mut reply2, 0x23, 0, -1, 0, 0);
        check("without a handler: errAEEventNotHandled", r10 == -1708);
        for x in [&mut d, &mut t, &mut t2, &mut l, &mut en, &mut et, &mut list, &mut nth, &mut rec, &mut obj, &mut asrec, &mut want, &mut seld, &mut target, &mut ev, &mut reply, &mut ans, &mut reply2, &mut name] {
            AEDisposeDesc(x);
        }
    }
}
