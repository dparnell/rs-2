//! The windows a DDEML instance is made of, as IBM's PMDDEML has them:
//! an invisible frame on the desktop whose client is the instance's
//! agent window (its handle is the instance id; DDE clients reach it with
//! WM_DDE_INITIATE like any other frame), and under the agent one window
//! per conversation (its handle is the HCONV: it is the DDE partner of
//! the other side's window) and per conversation list. The agent answers
//! WM_DDE_INITIATE (`AgentWmDdeInitiate`, `AgentIssueResponses`), a
//! conversation window the DDE messages of its conversation
//! (`ConvWmDde*`): a client's transactions and the server's side of
//! requests, pokes, executes and advise loops (IBM's advise windows are
//! a list of links here).
//!
//! Callbacks get (idInst, usType, usFmt, hConv, hsz1, hsz2, hData, ul1,
//! ul2), as IBM's DdeDispatchTransaction passes them.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::ptr::null;

use os2::DosFreeMem;

use crate::handles::*;
use crate::pm::*;
use crate::*;

pub const AGENT_CLASS: &[u8] = b"#101\0";
pub const CONV_CLASS: &[u8] = b"#102\0";
pub const LIST_CLASS: &[u8] = b"#103\0";

/// Private messages (IBM's numbers): abandon a transaction, an advise
/// round (UM_ADVISE posts the data of the links, UM_QUERY counts them),
/// EnableCallback, service registration and replaying a blocked
/// conversation's messages.
pub const UM_ABANDON: u32 = 0x1001;
pub const UM_ADVISE: u32 = 0x1002;
pub const UM_ENABLE: u32 = 0x1003;
pub const UM_QUERY: u32 = 0x1004;
pub const AM_REGISTER: u32 = 0x4b1;
pub const AM_UNREGISTER: u32 = 0x4b2;
pub const CM_REPLAY: u32 = 0x4b4;

/// Conversation status (ST_*).
pub const ST_CONNECTED: u16 = 0x0001;
pub const ST_ADVISE: u16 = 0x0002;
pub const ST_ISLOCAL: u16 = 0x0004;
pub const ST_BLOCKED: u16 = 0x0008;
pub const ST_CLIENT: u16 = 0x0010;
pub const ST_TERMINATED: u16 = 0x0020;
pub const ST_INLIST: u16 = 0x0040;
pub const ST_BLOCKNEXT: u16 = 0x0080;
pub const ST_ISSELF: u16 = 0x0100;
/// A synchronous transaction is waiting (IBM's 0x8000).
pub const ST_INSYNC: u16 = 0x8000;

/// Conversation and transaction states (XST_*).
pub const XST_INCOMPLETE: u16 = 1;
pub const XST_CONNECTED: u16 = 2;
pub const XST_INIT1: u16 = 3;
pub const XST_REQSENT: u16 = 5;
pub const XST_DATARCVD: u16 = 6;
pub const XST_POKESENT: u16 = 7;
pub const XST_POKEACKRCVD: u16 = 8;
pub const XST_EXECSENT: u16 = 9;
pub const XST_EXECACKRCVD: u16 = 10;
pub const XST_ADVSENT: u16 = 11;
pub const XST_UNADVSENT: u16 = 12;
pub const XST_ADVACKRCVD: u16 = 13;
pub const XST_UNADVACKRCVD: u16 = 14;

/// A client's transaction (IBM's 0x1C-byte record).
pub struct Xact {
    /// The transaction id (the time it started), or QID_SYNC.
    pub id: u32,
    pub typ: u16,
    /// What the answer said (fsStatus); for an advise: DDE_FNODATA and
    /// DDE_FACKREQ asked for.
    pub flags: u16,
    pub state: u16,
    pub fmt: u16,
    pub user: u32,
    /// The data handle or TRUE/FALSE.
    pub result: u32,
    pub item: u32,
}

/// A server's advise link (IBM's advise window): item, format, what the
/// client asked for (DDE_FNODATA, DDE_FACKREQ), and whether an
/// acknowledgement is awaited (1) and data changed meanwhile (2).
pub struct Link {
    pub item: u32,
    pub fmt: u16,
    pub flags: u16,
    pub busy: u16,
}

pub struct Conv {
    pub inst: u32,
    pub user: u32,
    pub svc_req: u32,
    pub svc: u32,
    pub topic_req: u32,
    pub topic: u32,
    pub status: u16,
    pub state: u16,
    pub last_err: u32,
    pub list: u32,
    pub cc: [u32; 6],
    pub partner: u32,
    /// The frame a client's WM_DDE_INITIATE went to.
    pub target: u32,
    pub xacts: Vec<Xact>,
    /// Messages held while callbacks are blocked: (msg, from, mp2).
    pub queue: VecDeque<(u32, u32, u32)>,
    pub links: Vec<Link>,
    /// A conversation list's answers: (service, topic, server frame).
    pub answers: Vec<(Vec<u8>, Vec<u8>, u32)>,
}

impl Conv {
    pub fn case(&self) -> bool {
        self.cc[1] & 1 != 0
    }
    pub fn cp(&self) -> u32 {
        self.cc[3]
    }
}

pub fn conv(h: u32) -> Option<&'static mut Conv> {
    if h == 0 || unsafe { WinIsWindow(0, h) } == 0 {
        return None;
    }
    if !is_class(h, CONV_CLASS) && !is_class(h, LIST_CLASS) {
        return None;
    }
    let p = unsafe { WinQueryWindowPtr(h, 0) } as *mut Conv;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

pub fn is_class(h: u32, class: &[u8]) -> bool {
    let mut b = [0u8; 16];
    let n = unsafe { WinQueryClassName(h, 16, b.as_mut_ptr()) } as usize;
    n > 0 && b[..n] == class[..class.len() - 1]
}

/// Both windows belong to one thread (IBM's IsSelf).
pub fn is_self(a: u32, b: u32) -> bool {
    let (mut p1, mut t1, mut p2, mut t2) = (0, 0, 0, 0);
    unsafe { WinQueryWindowProcess(a, &mut p1, &mut t1) != 0 && WinQueryWindowProcess(b, &mut p2, &mut t2) != 0 && p1 == p2 && t1 == t2 }
}

pub fn children(h: u32) -> Vec<u32> {
    let mut v = Vec::new();
    let e = unsafe { WinBeginEnumWindows(h) };
    if e == 0 {
        return v;
    }
    loop {
        let k = unsafe { WinGetNextWindow(e) };
        if k == 0 {
            break;
        }
        v.push(k);
    }
    unsafe { WinEndEnumWindows(e) };
    v
}

/// The conversations under an agent or list window, lists' included.
pub fn convs_of(h: u32) -> Vec<u32> {
    let mut v = Vec::new();
    for k in children(h) {
        if is_class(k, CONV_CLASS) {
            v.push(k);
        } else if is_class(k, LIST_CLASS) {
            v.extend(children(k).into_iter().filter(|&c| is_class(c, CONV_CLASS)));
        }
    }
    v
}

/// Call the instance's callback (IBM's DdeDispatchTransaction); a
/// conversation whose callback returned CBR_BLOCK (or that asked for one
/// callback only) is blocked afterwards.
pub fn dispatch(inst: u32, typ: u32, fmt: u32, hconv: u32, hsz1: u32, hsz2: u32, hdata: u32, ul1: u32, ul2: u32) -> u32 {
    let Some(i) = instance(inst) else { return 0 };
    let pfn = i.pfn;
    if hdata != 0 {
        lock_data(hdata, true);
    }
    let r = pfn(inst, typ, fmt, hconv, hsz1, hsz2, hdata, ul1, ul2);
    if hdata != 0 {
        lock_data(hdata, false);
    }
    if hconv != 0
        && let Some(c) = conv(hconv)
    {
        if c.status & ST_BLOCKNEXT != 0 {
            c.status = c.status & !ST_BLOCKNEXT | ST_BLOCKED;
        } else if r == CBR_BLOCK && typ & XTYPF_NOBLOCK == 0 {
            c.status |= ST_BLOCKED;
        }
    }
    r
}

/// A conversation window (IBM's DdeCreateConvWindow): under `parent`
/// (the agent or a list), its strings kept.
pub fn create_conv(parent: u32, inst: u32, svc_req: u32, topic_req: u32, cc: Option<[u32; 6]>, class: &[u8]) -> u32 {
    keep_string(svc_req);
    keep_string(topic_req);
    let c = Box::new(Conv {
        inst,
        user: 0,
        svc_req,
        svc: 0,
        topic_req,
        topic: 0,
        status: 0,
        state: 0,
        last_err: 0,
        list: 0,
        cc: cc.unwrap_or([0; 6]),
        partner: 0,
        target: 0,
        xacts: Vec::new(),
        queue: VecDeque::new(),
        links: Vec::new(),
        answers: Vec::new(),
    });
    let h = unsafe { WinCreateWindow(parent, class.as_ptr(), null(), 0, 0, 0, 0, 0, inst, 3, 0, null(), null()) };
    if h == 0 {
        free_string(svc_req);
        free_string(topic_req);
        return 0;
    }
    let p = Box::into_raw(c);
    unsafe { WinSetWindowPtr(h, 0, p as u32) };
    if is_class(parent, LIST_CLASS) {
        unsafe {
            (*p).status |= ST_INLIST;
            (*p).list = parent;
        }
    }
    h
}

fn destroy_conv_data(h: u32) {
    let p = unsafe { WinQueryWindowPtr(h, 0) } as *mut Conv;
    if p.is_null() {
        return;
    }
    unsafe { WinSetWindowPtr(h, 0, 0) };
    let c = unsafe { Box::from_raw(p) };
    for s in [c.svc_req, c.svc, c.topic_req, c.topic] {
        free_string(s);
    }
    for x in &c.xacts {
        free_string(x.item);
    }
    for l in &c.links {
        free_string(l.item);
    }
    for &(_, _, p) in &c.queue {
        if p != 0 {
            unsafe { DosFreeMem(p as *mut u8) };
        }
    }
}

/// The DDEINIT block (cb, pszAppName, pszTopic, offConvContext) with the
/// conversation context and the two strings (IBM's DdeMakeDdeInitStruct).
pub fn make_init(svc: &[u8], topic: &[u8], cc: &[u32; 6]) -> *mut u8 {
    let size = 16 + 24 + svc.len() + 1 + topic.len() + 1;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, null(), size as u32, GIVEABLE) } != 0 {
        return p;
    }
    unsafe {
        let base = p as u32;
        let ps = base + 16 + 24;
        let pt = ps + svc.len() as u32 + 1;
        let hdr = [16u32, ps, pt, 16];
        for (i, v) in hdr.iter().chain(cc.iter()).enumerate() {
            (p as *mut u32).add(i).write_unaligned(*v);
        }
        core::ptr::copy_nonoverlapping(svc.as_ptr(), ps as *mut u8, svc.len());
        *((ps as *mut u8).add(svc.len())) = 0;
        core::ptr::copy_nonoverlapping(topic.as_ptr(), pt as *mut u8, topic.len());
        *((pt as *mut u8).add(topic.len())) = 0;
    }
    p
}

/// A DDEINIT's strings and conversation context.
pub fn parse_init(p: *const u8) -> (Vec<u8>, Vec<u8>, Option<[u32; 6]>) {
    let cstr = |q: u32| -> Vec<u8> {
        if q == 0 {
            return Vec::new();
        }
        (0..).map(|i| unsafe { *(q as *const u8).add(i) }).take_while(|&b| b != 0).collect()
    };
    unsafe {
        let w = |i: usize| (p as *const u32).add(i).read_unaligned();
        let off = w(3);
        let cc = if off != 0 {
            let c = p.add(off as usize) as *const u32;
            let mut a = [0u32; 6];
            for (i, v) in a.iter_mut().enumerate() {
                *v = c.add(i).read_unaligned();
            }
            Some(a)
        } else {
            None
        };
        (cstr(w(1)), cstr(w(2)), cc)
    }
}

/// Send WM_DDE_INITIATE to `frame` for conversation window `client`
/// (IBM's DdeConnectRequest): the DDEINIT is given to the frame's
/// process.
pub fn connect_request(client: u32, frame: u32, svc: &[u8], topic: &[u8], cc: &[u32; 6]) {
    let p = make_init(svc, topic, cc);
    if p.is_null() {
        set_error(DMLERR_SYS_ERROR, 0);
        return;
    }
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(frame, &mut pid, &mut tid) };
    let own = pid == my_pid();
    if !own && unsafe { DosGiveSharedMem(p, pid, 3) } != 0 {
        unsafe { DosFreeMem(p) };
        set_error(DMLERR_SYS_ERROR, 0);
        return;
    }
    if let Some(c) = conv(client) {
        c.status |= ST_CLIENT;
        c.state = XST_INIT1;
        c.target = frame;
    }
    unsafe { WinSendMsg(frame, WM_DDE_INITIATE, client, p as u32) };
    if !own {
        unsafe { DosFreeMem(p) };
    }
}

fn post_dde(to: u32, from: u32, msg: u32, p: *mut u8, fl: u32) -> bool {
    unsafe { WinDdePostMsg(to, from, msg, p, fl) != 0 }
}

/// A string handle for a DDE message's name, in the conversation's code
/// page and case rules.
fn name_handle(s: &[u8], cp: u32) -> u32 {
    string_handle(s, cp, 0)
}

// ---- The agent ----

pub extern "C" fn agent_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_DESTROY => {
            let p = unsafe { WinQueryWindowPtr(h, 0) } as *mut Inst;
            if !p.is_null() {
                unsafe { WinSetWindowPtr(h, 0, 0) };
                let i = unsafe { Box::from_raw(p) };
                for s in &i.names {
                    free_string(*s);
                }
            }
            0
        }
        WM_CLOSE => 0,
        WM_DDE_INITIATE => agent_initiate(h, mp1, mp2 as *mut u8),
        AM_REGISTER | AM_UNREGISTER => agent_register(h, msg, mp1, mp2),
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

/// XTYP_REGISTER/XTYP_UNREGISTER for another instance's service: mp1 is
/// that instance, mp2 a system atom of the service's name.
fn agent_register(h: u32, msg: u32, other: u32, atom: u32) -> u32 {
    let Some(i) = instance(h) else { return 0 };
    if other == h {
        return 0;
    }
    if (msg == AM_REGISTER && i.flags & CBF_SKIP_REGISTRATIONS != 0) || (msg == AM_UNREGISTER && i.flags & CBF_SKIP_UNREGISTRATIONS != 0) {
        return 0;
    }
    let mut b = [0u8; 256];
    let n = unsafe { WinQueryAtomName(WinQuerySystemAtomTable(), atom, b.as_mut_ptr(), 256) } as usize;
    if n == 0 {
        return 0;
    }
    let svc = string_handle(&b[..n], 0, 0);
    let isvc = string_handle(&b[..n], 0, other);
    let typ = if msg == AM_REGISTER { XTYP_REGISTER } else { XTYP_UNREGISTER };
    dispatch(h, typ, 0, 0, svc, isvc, 0, 0, 0);
    free_string(svc);
    free_string(isvc);
    0
}

/// WM_DDE_INITIATE (IBM's AgentWmDdeInitiate): XTYP_CONNECT for a
/// service and topic, XTYP_WILDCONNECT when either is empty; each pair
/// the callback agrees to gets a conversation window that answers with
/// WinDdeRespond, then XTYP_CONNECT_CONFIRM.
fn agent_initiate(h: u32, client: u32, p: *mut u8) -> u32 {
    let Some(i) = instance(h) else {
        unsafe { DosFreeMem(p) };
        return 1;
    };
    let me = is_self(h, client);
    let flags = i.flags;
    if flags & APPCMD_CLIENTONLY != 0 || flags & CBF_FAIL_CONNECTIONS != 0 || (flags & CBF_FAIL_SELFCONNECTIONS != 0 && me) {
        unsafe { DosFreeMem(p) };
        return 1;
    }
    let (svc_s, topic_s, cc) = parse_init(p);
    unsafe { DosFreeMem(p) };
    let cc = cc.unwrap_or([24, 0, 0, 0, 0, 0]);
    let cp = cc[3];
    let svc = name_handle(&svc_s, cp);
    let topic = name_handle(&topic_s, cp);
    let wild = svc == 0 || topic == 0;
    // With APPCMD_FILTERINITS only registered services answer.
    if svc != 0 && flags & APPCMD_FILTERINITS != 0 && !i.names.iter().any(|&n| equal(n, svc, cc[1] & 1 != 0)) {
        free_string(svc);
        free_string(topic);
        return 1;
    }
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    if !wild {
        if dispatch(h, XTYP_CONNECT, 0, 0, topic, svc, 0, cc.as_ptr() as u32, me as u32) != 0 {
            keep_string(svc);
            keep_string(topic);
            pairs.push((svc, topic));
        }
    } else {
        let hd = dispatch(h, XTYP_WILDCONNECT, 0, 0, topic, svc, 0, cc.as_ptr() as u32, me as u32);
        if let Some((pd, fl)) = data(hd) {
            let d = parse(pd);
            for k in 0..d.cb as usize / 8 {
                let (s, t) = unsafe { ((d.data as *const u32).add(2 * k).read_unaligned(), (d.data as *const u32).add(2 * k + 1).read_unaligned()) };
                if s == 0 {
                    break;
                }
                keep_string(s);
                keep_string(t);
                pairs.push((s, t));
            }
            if fl & HDATA_APPOWNED == 0 {
                free_data(hd);
            }
        }
    }
    free_string(svc);
    free_string(topic);
    let mut cc_out = cc;
    cc_out[0] = 24;
    for (s, t) in pairs {
        let hc = create_conv(h, h, s, t, Some(cc_out), CONV_CLASS);
        if hc != 0 {
            let ss = string(s).map(|x| x.0).unwrap_or_default();
            let ts = string(t).map(|x| x.0).unwrap_or_default();
            let mut sz = ss.clone();
            sz.push(0);
            let mut tz = ts.clone();
            tz.push(0);
            if let Some(c) = conv(hc) {
                c.partner = client;
                c.status |= ST_CONNECTED;
                c.state = XST_CONNECTED;
                if me {
                    c.status |= ST_ISSELF;
                }
                if is_class(client, CONV_CLASS) || is_class(client, LIST_CLASS) {
                    c.status |= ST_ISLOCAL;
                }
                keep_string(s);
                keep_string(t);
                c.svc = s;
                c.topic = t;
            }
            unsafe { WinDdeRespond(client, hc, sz.as_ptr(), tz.as_ptr(), cc_out.as_ptr()) };
            if flags & CBF_SKIP_CONNECT_CONFIRMS == 0 && conv(hc).is_some() {
                dispatch(h, XTYP_CONNECT_CONFIRM, 0, hc, t, s, 0, 0, me as u32);
            }
        }
        free_string(s);
        free_string(t);
    }
    1
}

// ---- Conversations ----

pub extern "C" fn conv_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_DESTROY => {
            destroy_conv_data(h);
            0
        }
        WM_CLOSE => conv_close(h),
        WM_DDE_INITIATEACK => conv_initiate_ack(h, mp1, mp2 as *mut u8),
        WM_DDE_REQUEST => server_request(h, mp1, mp2 as *mut u8),
        WM_DDE_POKE | WM_DDE_EXECUTE => server_poke_execute(h, msg, mp1, mp2 as *mut u8),
        WM_DDE_ADVISE => server_advise(h, mp1, mp2 as *mut u8),
        WM_DDE_UNADVISE => server_unadvise(h, mp1, mp2 as *mut u8),
        WM_DDE_ACK => conv_ack(h, mp1, mp2 as *mut u8),
        WM_DDE_DATA => conv_data(h, mp1, mp2 as *mut u8),
        WM_DDE_TERMINATE => conv_terminate(h, mp1),
        UM_ABANDON => abandon(h, mp1) as u32,
        UM_ADVISE => {
            advise_round(h, mp1, mp2);
            0
        }
        UM_ENABLE => {
            enable(h, mp1 as u16);
            0
        }
        CM_REPLAY => {
            replay(h);
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

/// A conversation list: its WM_DDE_INITIATEACKs are servers to connect
/// to one by one (IBM's ConvListWmDdeInitiateAck terminates the answer
/// and connects again to that server's instance).
pub extern "C" fn list_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_DESTROY => {
            destroy_conv_data(h);
            0
        }
        WM_CLOSE => {
            for k in children(h) {
                unsafe { WinSendMsg(k, WM_CLOSE, 0, 0) };
            }
            0
        }
        WM_DDE_INITIATEACK => {
            let (svc, topic, _) = parse_init(mp2 as *const u8);
            unsafe { DosFreeMem(mp2 as *mut u8) };
            if let Some(c) = conv(h) {
                let ok_s = c.svc_req == 0 || string(c.svc_req).is_some_and(|s| names_equal(&s.0, &svc, c.case()));
                let ok_t = c.topic_req == 0 || string(c.topic_req).is_some_and(|s| names_equal(&s.0, &topic, c.case()));
                if ok_s && ok_t {
                    c.answers.push((svc, topic, c.target));
                }
            }
            unsafe { WinDdePostMsg(mp1, h, WM_DDE_TERMINATE, core::ptr::null_mut(), DDEPM_RETRY) };
            1
        }
        UM_ADVISE | UM_QUERY | UM_ENABLE => {
            for k in children(h) {
                unsafe { WinSendMsg(k, msg, mp1, mp2) };
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn names_equal(a: &[u8], b: &[u8], case: bool) -> bool {
    if case { a == b } else { a.eq_ignore_ascii_case(b) }
}

/// WM_DDE_INITIATEACK on a client's conversation (IBM's
/// ConvWmDdeInitiateAck): the first server that matches what was asked
/// for is the partner; the others are told to terminate.
fn conv_initiate_ack(h: u32, server: u32, p: *mut u8) -> u32 {
    let (svc_s, topic_s, cc) = parse_init(p);
    unsafe { DosFreeMem(p) };
    let Some(c) = conv(h) else { return 1 };
    if c.status & ST_CONNECTED != 0 {
        unsafe { WinDdePostMsg(server, h, WM_DDE_TERMINATE, core::ptr::null_mut(), DDEPM_RETRY) };
        return 1;
    }
    if c.state == 0 || c.state > XST_INIT1 + 1 {
        return 1;
    }
    let case = cc.map_or(c.case(), |cc| cc[1] & 1 != 0);
    let cp = cc.map_or(c.cp(), |cc| cc[3]);
    let svc = name_handle(&svc_s, cp);
    let topic = name_handle(&topic_s, cp);
    let ok = (c.svc_req == 0 || equal(c.svc_req, svc, case)) && (c.topic_req == 0 || equal(c.topic_req, topic, case));
    if !ok {
        free_string(svc);
        free_string(topic);
        return 1;
    }
    c.svc = svc;
    c.topic = topic;
    if is_self(server, h) {
        c.status |= ST_ISSELF;
    }
    if is_class(server, CONV_CLASS) {
        c.status |= ST_ISLOCAL;
    }
    c.status |= ST_CONNECTED;
    c.partner = server;
    c.state = XST_CONNECTED;
    if let Some(cc) = cc {
        c.cc = cc;
        c.cc[0] = 24;
    }
    1
}

/// WM_CLOSE: DdeDisconnect (IBM's ConvWmClose). A synchronous
/// transaction waiting is ended; the partner gets WM_DDE_TERMINATE and
/// the window goes when it answers.
fn conv_close(h: u32) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    if c.status & ST_INSYNC != 0 {
        if let Some(x) = c.xacts.iter_mut().find(|x| x.id == QID_SYNC && x.state > XST_CONNECTED) {
            x.state = XST_INCOMPLETE;
        }
        unsafe { WinPostMsg(h, WM_CLOSE, 0, 0) };
        return 0;
    }
    if c.status & ST_CONNECTED != 0 {
        c.status = (c.status & !ST_CONNECTED) | ST_TERMINATED;
        let partner = c.partner;
        if c.status & ST_CLIENT != 0 {
            let p = make_struct(0, null(), 0, 0, 0, 0);
            if !p.is_null() {
                post_dde(partner, h, WM_DDE_UNADVISE, p, DDEPM_RETRY);
            }
        }
        if unsafe { WinIsWindow(0, partner) } == 0 || !post_dde(partner, h, WM_DDE_TERMINATE, core::ptr::null_mut(), DDEPM_RETRY) {
            unsafe { WinDestroyWindow(h) };
        }
    } else if c.status & ST_TERMINATED == 0 {
        unsafe { WinDestroyWindow(h) };
    }
    0
}

/// WM_DDE_TERMINATE from the partner (IBM's ConvWmDdeTerminate): it is
/// answered, the callback gets XTYP_DISCONNECT, the conversation goes.
fn conv_terminate(h: u32, from: u32) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    if c.partner != from {
        return 0;
    }
    if c.status & ST_CONNECTED != 0 {
        c.status = (c.status & !ST_CONNECTED) | ST_TERMINATED;
        if c.status & ST_INSYNC != 0
            && let Some(x) = c.xacts.iter_mut().find(|x| x.id == QID_SYNC)
        {
            x.state = XST_INCOMPLETE;
        }
        let me = c.status & ST_ISSELF != 0;
        let inst = c.inst;
        if unsafe { WinIsWindow(0, from) } != 0 {
            post_dde(from, h, WM_DDE_TERMINATE, core::ptr::null_mut(), DDEPM_RETRY);
        }
        if instance(inst).is_some_and(|i| i.flags & CBF_SKIP_DISCONNECTS == 0) {
            dispatch(inst, XTYP_DISCONNECT, 0, h, 0, 0, 0, 0, me as u32);
        }
        // A synchronous transaction still waiting sees the conversation
        // end and destroys it.
        if conv(h).is_some_and(|c| c.status & ST_INSYNC != 0) {
            return 0;
        }
    }
    unsafe { WinDestroyWindow(h) };
    0
}

/// Hold a DDE message while the conversation's callbacks are blocked;
/// `front` puts it back first (the callback returned CBR_BLOCK on it).
fn hold(h: u32, msg: u32, from: u32, p: *mut u8, front: bool) {
    if let Some(c) = conv(h) {
        if front {
            c.queue.push_front((msg, from, p as u32));
        } else {
            c.queue.push_back((msg, from, p as u32));
        }
    }
}

fn blocked(c: &Conv) -> bool {
    c.status & ST_BLOCKED != 0 || !c.queue.is_empty()
}

/// CM_REPLAY: one held message goes round again (and the next after it).
fn replay(h: u32) {
    let Some(c) = conv(h) else { return };
    if c.status & ST_BLOCKED != 0 {
        return;
    }
    if let Some((msg, from, p)) = c.queue.pop_front() {
        unsafe {
            WinPostMsg(h, msg, from, p);
            WinPostMsg(h, CM_REPLAY, 0, 0);
        }
    }
}

/// EnableCallback on one conversation (IBM's ConvUmEnableCallback).
fn enable(h: u32, cmd: u16) {
    let Some(c) = conv(h) else { return };
    match cmd {
        EC_ENABLEALL => {
            c.status &= !(ST_BLOCKED | ST_BLOCKNEXT);
            unsafe { WinPostMsg(h, CM_REPLAY, 0, 0) };
        }
        EC_ENABLEONE => {
            c.status = (c.status & !ST_BLOCKED) | ST_BLOCKNEXT;
            unsafe { WinPostMsg(h, CM_REPLAY, 0, 0) };
        }
        _ => c.status |= cmd,
    }
}

// ---- The server's side ----

/// WM_DDE_REQUEST (IBM's ConvWmDdeRequest): XTYP_REQUEST; the data
/// handle the callback returns is posted as WM_DDE_DATA with
/// DDE_FRESPONSE, none is a negative acknowledgement.
fn server_request(h: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    let Some(i) = instance(c.inst) else { return 0 };
    let inst = c.inst;
    set_status(p, DDE_NOTPROCESSED);
    let mut hd = 0;
    let d = parse(p);
    let item = name_handle(&d.item, c.cp());
    if i.flags & CBF_FAIL_REQUESTS == 0 {
        if blocked(c) {
            free_string(item);
            hold(h, WM_DDE_REQUEST, from, p, false);
            return 0;
        }
        let topic = c.topic_req;
        hd = dispatch(inst, XTYP_REQUEST as u32, d.fmt as u32, h, topic, item, 0, 0, 0);
        if hd == CBR_BLOCK {
            free_string(item);
            hold(h, WM_DDE_REQUEST, from, p, true);
            return 0;
        }
    }
    match data(hd) {
        None => {
            post_dde(from, h, WM_DDE_ACK, p, DDEPM_RETRY);
        }
        Some((pd, fl)) => {
            let dd = parse(pd);
            let np = make_struct(item, dd.data, dd.cb, dd.cb, dd.fmt, dd.status | DDE_FRESPONSE);
            let post = if np.is_null() {
                pd
            } else {
                unsafe { DosFreeMem(pd) };
                set_data(hd, np);
                np
            };
            post_dde(from, h, WM_DDE_DATA, post, fl | DDEPM_RETRY);
            if fl & HDATA_APPOWNED == 0 {
                remove_data(hd);
            }
            unsafe { DosFreeMem(p) };
        }
    }
    free_string(item);
    0
}

/// WM_DDE_POKE and WM_DDE_EXECUTE (IBM's ConvWmDdePoke,
/// ConvWmDdeExecute): XTYP_POKE/XTYP_EXECUTE with the data; what the
/// callback returns (DDE_FACK, DDE_FBUSY, DDE_FNOTPROCESSED) is the
/// acknowledgement, sent back in the same block.
fn server_poke_execute(h: u32, msg: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    let Some(i) = instance(c.inst) else { return 0 };
    let inst = c.inst;
    let fail = if msg == WM_DDE_POKE { CBF_FAIL_POKES } else { CBF_FAIL_EXECUTES };
    set_status(p, DDE_NOTPROCESSED);
    if i.flags & fail == 0 {
        if blocked(c) {
            hold(h, msg, from, p, false);
            return 0;
        }
        set_status(p, 0);
        let d = parse(p);
        let topic = c.topic_req;
        let item = if msg == WM_DDE_POKE { name_handle(&d.item, c.cp()) } else { 0 };
        let hd = data_handle(make_struct(item, d.data, d.cb, d.cb, d.fmt, 0), 0);
        let typ = if msg == WM_DDE_POKE { XTYP_POKE } else { XTYP_EXECUTE };
        let r = dispatch(inst, typ as u32, d.fmt as u32, h, topic, item, hd, 0, 0);
        free_string(item);
        free_data(hd);
        if r == CBR_BLOCK {
            hold(h, msg, from, p, true);
            return 0;
        }
        set_status(p, r as u16);
    }
    post_dde(from, h, WM_DDE_ACK, p, DDEPM_RETRY);
    0
}

/// WM_DDE_ADVISE (IBM's ConvWmDdeAdvise): XTYP_ADVSTART; a link (item,
/// format, the client's DDE_FNODATA/DDE_FACKREQ) if the callback agrees.
fn server_advise(h: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    let Some(i) = instance(c.inst) else { return 0 };
    let inst = c.inst;
    let d = parse(p);
    set_status(p, DDE_NOTPROCESSED);
    if i.flags & CBF_FAIL_ADVISES == 0 {
        if blocked(c) {
            set_status(p, d.status);
            hold(h, WM_DDE_ADVISE, from, p, false);
            return 0;
        }
        let item = name_handle(&d.item, c.cp());
        let topic = c.topic_req;
        let r = dispatch(inst, XTYP_ADVSTART as u32, d.fmt as u32, h, topic, item, 0, 0, 0);
        if r == CBR_BLOCK {
            set_status(p, d.status);
            free_string(item);
            hold(h, WM_DDE_ADVISE, from, p, true);
            return 0;
        }
        if r == 1
            && let Some(c) = conv(h)
        {
            let case = c.case();
            if let Some(l) = c.links.iter_mut().find(|l| l.fmt == d.fmt && equal(l.item, item, case)) {
                l.flags = d.status & (DDE_FNODATA | DDE_FACKREQ);
                free_string(item);
            } else {
                c.links.push(Link { item, fmt: d.fmt, flags: d.status & (DDE_FNODATA | DDE_FACKREQ), busy: 0 });
            }
            c.status |= ST_ADVISE;
            set_status(p, DDE_FACK);
        } else {
            free_string(item);
        }
    }
    post_dde(from, h, WM_DDE_ACK, p, DDEPM_RETRY);
    0
}

/// WM_DDE_UNADVISE (IBM's ConvWmDdeUnAdvise, AdviseWmDdeUnAdvise): the
/// links of the item (all for none) and format (any for 0) end, each
/// with XTYP_ADVSTOP.
fn server_unadvise(h: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else { return 0 };
    let Some(i) = instance(c.inst) else { return 0 };
    let inst = c.inst;
    let d = parse(p);
    set_status(p, 0);
    if i.flags & CBF_FAIL_ADVISES == 0 {
        if blocked(c) {
            hold(h, WM_DDE_UNADVISE, from, p, false);
            return 0;
        }
        let item = name_handle(&d.item, c.cp());
        let case = c.case();
        let topic = c.topic_req;
        let ends: Vec<(u32, u16)> = c.links.iter().filter(|l| (item == 0 || equal(l.item, item, case)) && (d.fmt == 0 || l.fmt == d.fmt)).map(|l| (l.item, l.fmt)).collect();
        for (li, lf) in ends {
            dispatch(inst, XTYP_ADVSTOP as u32, lf as u32, h, topic, li, 0, 0, 0);
            if let Some(c) = conv(h)
                && let Some(k) = c.links.iter().position(|l| l.item == li && l.fmt == lf)
            {
                let l = c.links.remove(k);
                free_string(l.item);
                set_status(p, DDE_FACK);
            }
        }
        free_string(item);
        if let Some(c) = conv(h)
            && c.links.is_empty()
        {
            c.status &= !ST_ADVISE;
        }
    }
    post_dde(from, h, WM_DDE_ACK, p, DDEPM_RETRY);
    0
}

/// UM_ADVISE (WinDdePostAdvise): the data of the links of `item` (all
/// for 0) go to the client (IBM's AdviseUmAdviseItem): XTYP_ADVREQ for
/// each, or an empty WM_DDE_DATA for a DDE_FNODATA link; a link waiting
/// for an acknowledgement is only marked changed.
fn advise_round(h: u32, topic: u32, item: u32) {
    let Some(c) = conv(h) else { return };
    if c.status & ST_CLIENT != 0 || c.status & ST_CONNECTED == 0 {
        return;
    }
    let case = c.case();
    if topic != 0 && !equal(c.topic_req, topic, case) {
        return;
    }
    let mine: Vec<(u32, u16)> = c.links.iter().filter(|l| item == 0 || equal(l.item, item, case)).map(|l| (l.item, l.fmt)).collect();
    let mut left = mine.len() as u32;
    for (li, lf) in mine {
        left = left.saturating_sub(1);
        send_link(h, li, lf, left);
    }
}

fn send_link(h: u32, li: u32, lf: u16, left: u32) {
    let Some(c) = conv(h) else { return };
    let Some(l) = c.links.iter_mut().find(|l| l.item == li && l.fmt == lf) else { return };
    let partner = c.partner;
    let inst = c.inst;
    let topic = c.topic_req;
    let flags = l.flags;
    if flags & DDE_FNODATA != 0 {
        if flags & DDE_FACKREQ != 0 {
            if l.busy & 1 != 0 {
                l.busy |= 2;
                return;
            }
            l.busy |= 1;
        }
        let p = make_struct(li, null(), 0, 0, lf, flags);
        if !p.is_null() {
            post_dde(partner, h, WM_DDE_DATA, p, DDEPM_RETRY);
        }
        return;
    }
    if l.busy & 1 != 0 {
        l.busy |= 2;
        return;
    }
    if flags & DDE_FACKREQ != 0 {
        l.busy |= 1;
    }
    let hd = dispatch(inst, XTYP_ADVREQ, lf as u32, h, topic, li, 0, left, 0);
    if let Some((pd, fl)) = data(hd) {
        set_status(pd, flags);
        set_format(pd, lf);
        post_dde(partner, h, WM_DDE_DATA, pd, fl | DDEPM_RETRY);
        if fl & HDATA_APPOWNED == 0 {
            remove_data(hd);
        }
    } else if let Some(c) = conv(h)
        && let Some(l) = c.links.iter_mut().find(|l| l.item == li && l.fmt == lf)
    {
        l.busy &= !1;
    }
}

// ---- The client's side ----

/// WM_DDE_ACK (IBM's ConvWmDdeAck): the answer to a transaction, or a
/// server's link's acknowledgement of advise data.
fn conv_ack(h: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else {
        unsafe { DosFreeMem(p) };
        return 0;
    };
    let _ = from;
    let d = parse(p);
    let case = c.case();
    let item = name_handle(&d.item, c.cp());
    let k = c.xacts.iter().position(|x| x.fmt == d.fmt && x.state > XST_INIT1 + 1 && equal(x.item, item, case));
    let Some(k) = k else {
        // A link's acknowledgement (IBM's AdviseWmDdeAck).
        if c.status & ST_ADVISE != 0
            && let Some(l) = c.links.iter_mut().find(|l| l.fmt == d.fmt && equal(l.item, item, case))
        {
            let again = l.busy & 2 != 0;
            l.busy = 0;
            let (li, lf) = (l.item, l.fmt);
            free_string(item);
            unsafe { DosFreeMem(p) };
            if again {
                send_link(h, li, lf, 0);
            }
            return 0;
        }
        free_string(item);
        unsafe { DosFreeMem(p) };
        return 0;
    };
    if c.xacts[k].id != QID_SYNC && blocked(c) {
        free_string(item);
        hold(h, WM_DDE_ACK, from, p, false);
        return 0;
    }
    let inst = c.inst;
    let topic = c.topic;
    let x = &mut c.xacts[k];
    x.result = if d.status & DDE_FACK != 0 { 1 } else { 0 };
    if x.result == 0 {
        c.last_err = DMLERR_NOTPROCESSED;
        set_error(DMLERR_NOTPROCESSED, 0);
    }
    let x = &mut c.xacts[k];
    x.state = match x.state {
        XST_REQSENT => XST_DATARCVD,
        XST_POKESENT => XST_POKEACKRCVD,
        XST_EXECSENT => XST_EXECACKRCVD,
        XST_ADVSENT => XST_ADVACKRCVD,
        XST_UNADVSENT => XST_UNADVACKRCVD,
        s => s,
    };
    x.flags = d.status;
    if x.id != QID_SYNC {
        let (id, result, xitem) = (x.id, x.result, x.item);
        dispatch(inst, XTYP_XACT_COMPLETE, d.fmt as u32, h, topic, xitem, result, id, d.status as u32);
        unlink(h, id);
    }
    free_string(item);
    unsafe { DosFreeMem(p) };
    0
}

/// WM_DDE_DATA (IBM's ConvWmDdeData): the answer to a request
/// (DDE_FRESPONSE) or data of an advise loop (XTYP_ADVDATA, whose result
/// is the acknowledgement asked for with DDE_FACKREQ).
fn conv_data(h: u32, from: u32, p: *mut u8) -> u32 {
    let Some(c) = conv(h) else {
        unsafe { DosFreeMem(p) };
        return 0;
    };
    let d = parse(p);
    let case = c.case();
    let item = name_handle(&d.item, c.cp());
    let mut k = None;
    if d.status & DDE_FRESPONSE != 0 {
        k = c.xacts.iter().position(|x| x.state == XST_REQSENT && x.fmt == d.fmt && equal(x.item, item, case));
        if k.is_none() {
            free_string(item);
            unsafe { DosFreeMem(p) };
            return 0;
        }
    }
    let sync = k.is_some_and(|k| c.xacts[k].id == QID_SYNC);
    if !sync && blocked(c) {
        free_string(item);
        hold(h, WM_DDE_DATA, from, p, false);
        return 0;
    }
    let inst = c.inst;
    let topic = c.topic;
    let hd = if d.status & DDE_FNODATA != 0 { 0 } else { data_handle(p, 0) };
    let mut ack = DDE_FACK;
    match k {
        None => {
            let r = dispatch(inst, XTYP_ADVDATA, d.fmt as u32, h, topic, item, hd, 0, 0);
            if r == CBR_BLOCK {
                if hd != 0 {
                    remove_data(hd);
                }
                free_string(item);
                hold(h, WM_DDE_DATA, from, p, true);
                return 0;
            }
            ack = r as u16;
        }
        Some(k) => {
            let x = &mut c.xacts[k];
            x.result = hd;
            x.state = XST_DATARCVD;
            x.flags = d.status;
            if !sync {
                let (id, xitem) = (x.id, x.item);
                dispatch(inst, XTYP_XACT_COMPLETE, d.fmt as u32, h, topic, xitem, hd, id, d.status as u32);
                unlink(h, id);
            }
        }
    }
    if d.status & DDE_FACKREQ != 0 {
        let a = make_struct(item, null(), 0, 0, d.fmt, ack);
        if !a.is_null() {
            post_dde(from, h, WM_DDE_ACK, a, DDEPM_RETRY);
        }
    }
    // A synchronous request's data stays the application's.
    if !sync {
        if hd != 0 {
            remove_data(hd);
        }
        unsafe { DosFreeMem(p) };
    }
    free_string(item);
    0
}

/// Drop transaction `id` (all for 0); false if there is none.
pub fn unlink(h: u32, id: u32) -> bool {
    let Some(c) = conv(h) else { return false };
    let mut found = false;
    c.xacts.retain(|x| {
        if id == 0 || x.id == id {
            free_string(x.item);
            found = true;
            false
        } else {
            true
        }
    });
    found
}

/// UM_ABANDON: AbandonTransaction for an asynchronous transaction.
fn abandon(h: u32, id: u32) -> bool {
    if conv(h).is_some_and(|c| c.xacts.iter().any(|x| x.id == QID_SYNC && (id == 0 || x.id == id))) {
        return false;
    }
    unlink(h, id) || id == 0
}

/// Submit a client transaction (IBM's DdeSubmitTransaction).
#[allow(clippy::too_many_arguments)]
pub fn submit(pdata: u32, cb: u32, h: u32, item: u32, fmt: u16, typ: u16, timeout: u32, presult: *mut u32) -> u32 {
    if !presult.is_null() {
        unsafe { presult.write_unaligned(0) };
    }
    let Some(c) = conv(h) else { return 0 };
    if c.status & ST_CONNECTED == 0 {
        conv_error(h, DMLERR_NO_CONV_ESTABLISHED);
        return 0;
    }
    if unsafe { WinIsWindow(0, c.partner) } == 0 {
        conv_error(h, DMLERR_SERVER_DIED);
        return 0;
    }
    if timeout != TIMEOUT_ASYNC && c.status & ST_INSYNC != 0 {
        conv_error(h, DMLERR_REENTRANCY);
        return 0;
    }
    let (msg, state, err, mut flags) = match typ & !(XTYPF_NODATA | XTYPF_ACKREQ) {
        XTYP_ADVSTART => (WM_DDE_ADVISE, XST_ADVSENT, DMLERR_ADVACKTIMEOUT, 0u16),
        XTYP_REQUEST => (WM_DDE_REQUEST, XST_REQSENT, DMLERR_DATAACKTIMEOUT, 0),
        XTYP_EXECUTE => (WM_DDE_EXECUTE, XST_EXECSENT, DMLERR_EXECACKTIMEOUT, 0),
        XTYP_POKE => (WM_DDE_POKE, XST_POKESENT, DMLERR_POKEACKTIMEOUT, 0),
        XTYP_ADVSTOP => (WM_DDE_UNADVISE, XST_UNADVSENT, DMLERR_UNADVACKTIMEOUT, 0),
        _ => {
            conv_error(h, DMLERR_INVALIDPARAMETER);
            return 0;
        }
    };
    if msg == WM_DDE_ADVISE {
        if typ & XTYPF_NODATA != 0 {
            flags |= DDE_FNODATA;
        }
        if typ & XTYPF_ACKREQ != 0 {
            flags |= DDE_FACKREQ;
        }
    }
    let item = if typ == XTYP_EXECUTE { 0 } else { item };
    keep_string(item);
    let id = if timeout == TIMEOUT_ASYNC { (unsafe { WinGetCurrentTime(0) }) | 1 } else { QID_SYNC };
    let mut post_fl = DDEPM_RETRY;
    let mut handle = 0;
    let p = if cb == 0xffff_ffff {
        handle = pdata;
        match data(pdata) {
            Some((pd, fl)) => {
                post_fl |= fl;
                pd
            }
            None => core::ptr::null_mut(),
        }
    } else {
        make_struct(item, pdata as *const u8, cb, cb, fmt, flags)
    };
    if p.is_null() {
        free_string(item);
        conv_error(h, DMLERR_SYS_ERROR);
        return 0;
    }
    if handle != 0 {
        // The data handle goes out as the transaction's item and format.
        set_format(p, fmt);
    }
    c.xacts.push(Xact { id, typ, flags, state, fmt, user: 0, result: 0, item });
    let partner = c.partner;
    if timeout == TIMEOUT_ASYNC {
        if !presult.is_null() {
            unsafe { presult.write_unaligned(id) };
        }
        if !post_dde(partner, h, msg, p, post_fl) {
            unlink(h, id);
            conv_error(h, DMLERR_POSTMSG_FAILED);
            return 0;
        }
        if handle != 0 && post_fl & DDEPM_NOFREE == 0 {
            remove_data(handle);
        }
        return 1;
    }
    let posted = post_dde(partner, h, msg, p, post_fl);
    if handle != 0 && post_fl & DDEPM_NOFREE == 0 {
        remove_data(handle);
    }
    let mut result = 0;
    if !posted {
        conv_error(h, DMLERR_POSTMSG_FAILED);
    } else {
        if let Some(c) = conv(h) {
            c.status |= ST_INSYNC;
        }
        let end = unsafe { WinGetCurrentTime(0) }.wrapping_add(timeout);
        let mut m = QMsg::default();
        loop {
            let Some(c) = conv(h) else { break };
            let Some(x) = c.xacts.iter().find(|x| x.id == QID_SYNC) else { break };
            if x.state != state {
                if x.state == XST_INCOMPLETE {
                    conv_error(h, err);
                }
                break;
            }
            let now = unsafe { WinGetCurrentTime(0) };
            if end.wrapping_sub(now) as i32 <= 0 {
                conv_error(h, err);
                break;
            }
            if unsafe { WinPeekMsg(0, &mut m, 0, 0, 0, 1) } == 0 {
                unsafe { os2::DosSleep(10) };
                continue;
            }
            if unsafe { WinCallMsgFilter(0, &m, MSGF_DDEMGR) } != 0 {
                continue;
            }
            if m.msg == WM_QUIT {
                unsafe { WinPostMsg(0, WM_QUIT, m.mp1, m.mp2) };
                break;
            }
            unsafe { WinDispatchMsg(0, &m) };
        }
        if let Some(c) = conv(h) {
            c.status &= !ST_INSYNC;
            if let Some(x) = c.xacts.iter().find(|x| x.id == QID_SYNC) {
                result = if x.state == state || x.state == XST_INCOMPLETE { 0 } else { x.result };
                if !presult.is_null() && typ & 0x5000 != 0 {
                    unsafe { presult.write_unaligned(x.flags as u32) };
                }
                // Data that came after the wait ended is not wanted.
                if result == 0 && x.result > 1 {
                    free_data(x.result);
                }
            }
            if c.status & ST_TERMINATED != 0 && c.status & ST_CONNECTED == 0 {
                unlink(h, QID_SYNC);
                unsafe { WinDestroyWindow(h) };
                return result;
            }
        }
    }
    unlink(h, QID_SYNC);
    result
}

pub fn conv_error(h: u32, e: u32) {
    if let Some(c) = conv(h) {
        c.last_err = e;
    }
    set_error(e, 0);
}

/// PostAdvise: every server conversation of the instance runs its links
/// of the topic and item.
pub fn post_advise(inst: u32, topic: u32, item: u32) {
    for h in convs_of(inst) {
        unsafe { WinSendMsg(h, UM_ADVISE, topic, item) };
    }
}
