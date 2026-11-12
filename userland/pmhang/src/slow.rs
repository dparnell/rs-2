//! RS2\PMSLOW.EXE: a PM program that takes eight seconds before it makes
//! its message queue and window (an entry field). With the System object's
//! type-ahead on, what is typed while it starts ends up in the entry
//! field (docs/WPS.md, "The Input page in the window manager").

#![no_std]
#![no_main]

os2::entry!(main);

#[repr(C)]
#[derive(Default)]
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
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: u32, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

fn main(_s: &os2::Startup) -> u32 {
    unsafe {
        DosSleep(8000);
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        if hab == 0 || hmq == 0 {
            return 1;
        }
        // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX |
        // FCF_SHELLPOSITION | FCF_TASKLIST
        let mut fcf = 0x1 | 0x2 | 0x8 | 0x30 | 0x400 | 0x800;
        let mut client = 0;
        let frame = WinCreateStdWindow(1, 0x8000_0000, &mut fcf, core::ptr::null(), b"Slow start\0".as_ptr(), 0, 0, 1, &mut client);
        // WC_ENTRYFIELD (WS_VISIBLE | ES_MARGIN) in the frame.
        let e = WinCreateWindow(frame, 0xffff_0006, b"\0".as_ptr(), 0x8000_0000 | 0x8, 20, 20, 360, 28, frame, 3, 0x8008, core::ptr::null(), core::ptr::null());
        WinSetFocus(1, e);
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
