//! VIEW.EXE: shows OS/2 books, as `VIEW book[+book...] [topic]` does on
//! OS/2: the books (found along BOOKSHELF, `.INF` added when there is no
//! extension) open in the help manager's window at the first panel whose
//! title or index entry has the topic words, else at the contents. Closing
//! the window ends the program. (IBM's VIEW hands books to a VIEWDOC
//! server; here the help manager shows them, through its `Rs2ViewBook`.)

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

os2::entry!(main);

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "HELPMGR.52"]
    fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.200"]
    fn Rs2ViewBook(hab: u32, names: *const u8, topic: *const u8, rc: *mut u32) -> u32;
}

const MB_OK: u32 = 0;
const MB_ERROR: u32 = 0x0040;
const MB_MOVEABLE: u32 = 0x4000;

fn message(hab: u32, text: &[u8]) {
    let _ = hab;
    let mut t = text.to_vec();
    t.push(0);
    unsafe { WinMessageBox(1, 1, t.as_ptr(), b"Help\0".as_ptr(), 0, MB_OK | MB_ERROR | MB_MOVEABLE) };
}

fn main(s: &os2::Startup) -> u32 {
    let args = s.args();
    // The books (the first word; options such as /S are left out), then
    // the topic.
    let mut words = args.split(|&c| c == b' ').filter(|w| !w.is_empty() && !w.starts_with(b"/"));
    let books: Vec<u8> = words.next().unwrap_or(b"").to_vec();
    let mut topic: Vec<u8> = Vec::new();
    for w in words {
        if !topic.is_empty() {
            topic.push(b' ');
        }
        topic.extend_from_slice(w);
    }
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if books.is_empty() {
        message(hab, b"Specify the name of a document to view, for example: VIEW CMDREF");
        return 1;
    }
    let (b, t) = (z(&books), z(&topic));
    let mut rc = 0u32;
    let h = unsafe { Rs2ViewBook(hab, b.as_ptr(), t.as_ptr(), &mut rc) };
    if h == 0 {
        let mut m = b"Cannot open the document ".to_vec();
        m.extend_from_slice(&books);
        m.push(b'.');
        message(hab, &m);
    } else {
        let mut q = QMsg::default();
        while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(hab, &q) };
        }
        unsafe { WinDestroyHelpInstance(h) };
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    (h == 0) as u32
}

fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}
