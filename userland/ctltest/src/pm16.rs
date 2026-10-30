//! The 16-bit PM API (OS/2 1.x programs): PMWIN's 16-bit entries called
//! through their 16:16 aliases (thunk16's call16), a window procedure that
//! is 16-bit code (`ct16_proc` below) called by 32-bit PMWIN, structure
//! conversion both ways, and 16-bit callers of 32-bit procedures.

use thunk16::{far, PascalCall};

use crate::check;

unsafe extern "C" {
    #[link_name = "PMWIN.3"]
    fn W16RegisterClass();
    #[link_name = "PMWIN.6"]
    fn W16CreateWindow();
    #[link_name = "PMWIN.7"]
    fn W16DestroyWindow();
    #[link_name = "PMWIN.8"]
    fn W16SetWindowPos();
    #[link_name = "PMWIN.10"]
    fn W16QueryWindowPos();
    #[link_name = "PMWIN.16"]
    fn W16SetWindowText();
    #[link_name = "PMWIN.17"]
    fn W16QueryWindowText();
    #[link_name = "PMWIN.37"]
    fn W16SubclassWindow();
    #[link_name = "PMWIN.61"]
    fn W16SendMsg();
    #[link_name = "PMWIN.269"]
    fn W16QueryWindowPtr();
    #[link_name = "PMWIN.248"]
    fn W16Catch();
    #[link_name = "PMWIN.249"]
    fn W16Throw();
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(h: u32, p: u32) -> u32;
    #[link_name = "PMWIN.839"]
    fn WinQueryWindowPtr(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.837"]
    fn WinQueryWindowPos(h: u32, swp: *mut i32) -> u32;
    static ct16_proc: u8;
    static ct16_catch: u8;
}

// A 16-bit window procedure (_pascal far: hwnd, msg, mp1, mp2; the stack
// after `push bp`: mp2 at bp+6, mp1 at bp+10, msg at bp+14, hwnd at
// bp+16). WM_USER + 9: WinThrow(mp2, 1) through the 16:16 procedure in
// mp1 (out of the window procedure, past 32-bit PMWIN). WM_USER and up:
// returns mp1 + msg. WM_QUERYWINDOWPARAMS: returns fsStatus read through
// the 16:16 WNDPARAMS pointer and sets cchText to 77. Anything else: 0.
core::arch::global_asm!(
    ".pushsection .text.ct16,\"ax\"",
    ".p2align 4",
    ".code16",
    ".globl ct16_proc",
    "ct16_proc:",
    "push bp",
    "mov bp, sp",
    "xor ax, ax",
    "xor dx, dx",
    "mov cx, word ptr [bp + 14]",
    "cmp cx, 0x1009",
    "jne 4f",
    "push word ptr [bp + 8]",
    "push word ptr [bp + 6]",
    "push 1",
    // call far [bp + 10]
    ".byte 0xff, 0x5e, 10",
    "4:",
    "cmp cx, 0x000b",
    "jne 2f",
    "push es",
    "push bx",
    "les bx, dword ptr [bp + 10]",
    "mov ax, word ptr es:[bx]",
    "mov word ptr es:[bx + 2], 77",
    "pop bx",
    "pop es",
    "jmp 3f",
    "2:",
    "cmp cx, 0x1000",
    "jb 3f",
    "mov ax, word ptr [bp + 10]",
    "mov dx, word ptr [bp + 12]",
    "add ax, cx",
    "adc dx, 0",
    "3:",
    "pop bp",
    // retf 14
    ".byte 0xca, 14, 0",
    // ct16_catch(pfnCatch, pfnThrow, pfnSendMsg, hwnd, pcatchbuf) (_pascal
    // far; after `push bp`: pcatchbuf at bp+6, hwnd bp+10, pfnSendMsg
    // bp+14, pfnThrow bp+18, pfnCatch bp+22): WinCatch, then with SI and
    // DI changed WinThrow(pcatchbuf, 0) (hwnd 0) or WinSendMsg(hwnd,
    // WM_USER + 9, pfnThrow, pcatchbuf), whose procedure throws. Returns
    // what WinCatch returned the second time; 0xFFFE if SI/DI did not come
    // back, 0xFFFF if nothing threw.
    ".globl ct16_catch",
    "ct16_catch:",
    "push bp",
    "mov bp, sp",
    "push si",
    "push di",
    "mov si, 0x1234",
    "mov di, 0x5678",
    "push word ptr [bp + 8]",
    "push word ptr [bp + 6]",
    ".byte 0xff, 0x5e, 22",
    "or ax, ax",
    "jnz 1f",
    "xor si, si",
    "xor di, di",
    "mov ax, word ptr [bp + 10]",
    "or ax, word ptr [bp + 12]",
    "jnz 2f",
    "push word ptr [bp + 8]",
    "push word ptr [bp + 6]",
    "push 0",
    ".byte 0xff, 0x5e, 18",
    "2:",
    "push word ptr [bp + 12]",
    "push word ptr [bp + 10]",
    "push 0x1009",
    "push word ptr [bp + 20]",
    "push word ptr [bp + 18]",
    "push word ptr [bp + 8]",
    "push word ptr [bp + 6]",
    ".byte 0xff, 0x5e, 14",
    "mov ax, 0xffff",
    "jmp 3f",
    "1:",
    "cmp si, 0x1234",
    "jne 4f",
    "cmp di, 0x5678",
    "je 3f",
    "4:",
    "mov ax, 0xfffe",
    "3:",
    "xor dx, dx",
    "pop di",
    "pop si",
    "pop bp",
    ".byte 0xca, 20, 0",
    ".code32",
    ".popsection",
);

fn f16(f: unsafe extern "C" fn()) -> u32 {
    far(f as *const u8)
}

const WM_USER: u32 = 0x1000;
const WM_QUERYWINDOWPARAMS: u32 = 0x000b;
const QWP_PFNWP: u32 = -3i32 as u32;

pub fn pm16_checks(hab: u32, client: u32) {
    let proc16 = far(&raw const ct16_proc);
    let class = b"Ct16Class\0";
    let r = PascalCall::new().l(hab).l(far(class.as_ptr())).l(proc16).l(0).w(0).call(f16(W16RegisterClass));
    check("16-bit WinRegisterClass with a 16-bit window procedure", r & 0xffff != 0);
    let name = b"sixteen\0";
    // WinCreateWindow(parent, class, name, style, x, y, cx, cy, owner,
    // behind, id, ctldata, presparams)
    let h = PascalCall::new()
        .l(client)
        .l(far(class.as_ptr()))
        .l(far(name.as_ptr()))
        .l(0)
        .w(1)
        .w(2)
        .w(30)
        .w(40)
        .l(client)
        .l(3)
        .w(77)
        .l(0)
        .l(0)
        .call(f16(W16CreateWindow));
    check("16-bit WinCreateWindow", h != 0);
    let mut buf = [0u8; 32];
    let n = unsafe { WinQueryWindowText(h, 32, buf.as_mut_ptr()) };
    check("its text, set through a 16:16 pointer", &buf[..n.max(0) as usize] == b"sixteen");

    // 32-bit PMWIN calls the 16-bit procedure.
    let r = unsafe { WinSendMsg(h, WM_USER + 5, 0x0001_0010, 0) };
    check("WinSendMsg to a 16-bit window procedure", r == 0x0001_1015);
    // WNDPARAMS has 16-bit fields for it: converted there and back.
    let mut wp = [0x1234u32, 5, 0, 0, 0, 0, 0];
    let r = unsafe { WinSendMsg(h, WM_QUERYWINDOWPARAMS, wp.as_mut_ptr() as u32, 0) };
    check("WM_QUERYWINDOWPARAMS converted for the 16-bit procedure", r == 0x1234 && wp[1] == 77);
    // 16-bit WinSendMsg to it.
    let r = PascalCall::new().l(h).w((WM_USER + 1) as u16).l(0x22).l(0).call(f16(W16SendMsg));
    check("16-bit WinSendMsg to it", r == 0x1023);

    // SWP with 16-bit fields.
    PascalCall::new().l(h).l(3).w(5).w(6).w(50).w(60).w(3).call(f16(W16SetWindowPos));
    let mut swp = [0u8; 18];
    let ok = PascalCall::new().l(h).l(far(swp.as_mut_ptr())).call(f16(W16QueryWindowPos));
    let w = |o: usize| i16::from_le_bytes([swp[o], swp[o + 1]]) as i32;
    check("16-bit WinSetWindowPos / WinQueryWindowPos (16-bit SWP)", ok & 0xffff != 0 && w(2) == 60 && w(4) == 50 && w(6) == 6 && w(8) == 5);
    let mut s32 = [0i32; 9];
    unsafe { WinQueryWindowPos(h, s32.as_mut_ptr()) };
    check("the same position seen by 32-bit code", s32[1] == 60 && s32[2] == 50 && s32[3] == 6 && s32[4] == 5);

    let mut t = [0u8; 16];
    let n = PascalCall::new().l(h).w(16).l(far(t.as_mut_ptr())).call(f16(W16QueryWindowText)) & 0xffff;
    check("16-bit WinQueryWindowText", &t[..n as usize] == b"sixteen");
    PascalCall::new().l(h).l(far(b"changed\0".as_ptr())).call(f16(W16SetWindowText));
    let n = unsafe { WinQueryWindowText(h, 32, buf.as_mut_ptr()) };
    check("16-bit WinSetWindowText", &buf[..n.max(0) as usize] == b"changed");

    // A 32-bit window subclassed with a 16-bit procedure: the old one,
    // handed to 16-bit code, is a 16:16 procedure that calls it.
    let orig = unsafe { WinQueryWindowPtr(client, QWP_PFNWP as i32) };
    let old16 = PascalCall::new().l(client).l(proc16).call(f16(W16SubclassWindow));
    check("16-bit WinSubclassWindow returns a 16:16 procedure", old16 != 0 && old16 >> 16 & 7 == 7);
    let r = unsafe { WinSendMsg(client, WM_USER + 2, 0x40, 0) };
    check("the subclassed window runs the 16-bit procedure", r == 0x1042);
    let back = PascalCall::new().l(h).w(0).l(0).l(0).call(old16);
    check("16-bit code calls the old 32-bit procedure", back == 0);
    let p16 = PascalCall::new().l(client).w(QWP_PFNWP as u16).call(f16(W16QueryWindowPtr));
    check("16-bit QWP_PFNWP is the 16-bit procedure", p16 == proc16);
    PascalCall::new().l(client).l(old16).call(f16(W16SubclassWindow));
    let now = unsafe { WinQueryWindowPtr(client, QWP_PFNWP as i32) };
    check("putting the old one back restores the 32-bit procedure", now == orig);
    unsafe { WinSubclassWindow(client, orig) };

    // WinCatch/WinThrow: back to where WinCatch returned, with its SI, DI,
    // BP and SP. (A throw out of a window procedure that PMWIN called
    // works in a 16-bit program, whose outermost code is 16-bit; here the
    // caller is 32-bit code, which a throw past PMWIN would not return
    // to.)
    let catch = far(&raw const ct16_catch);
    let mut cb = [0u8; 16];
    let r = PascalCall::new().l(f16(W16Catch)).l(f16(W16Throw)).l(f16(W16SendMsg)).l(0).l(far(cb.as_mut_ptr())).call(catch) & 0xffff;
    check("WinThrow(.., 0): WinCatch returns 1, registers as they were", r == 1);
    PascalCall::new().l(h).call(f16(W16DestroyWindow));
}
