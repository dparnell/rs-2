//! VirtualBox's shared clipboard through the kernel's VMMDev driver
//! (`\DEV\VBOXGST$`, IOCtl category 0x80; kernel/src/drivers/vbox.rs):
//! the legacy protocol of the clipboard service, text as UTF-16LE with
//! CR LF. The host shares the clipboard only when the machine's "Shared
//! Clipboard" setting allows it (Disabled by default).

use alloc::vec::Vec;

use guestagent::text;

const FMT_UNICODETEXT: u32 = 1;
const MSG_QUIT: u32 = 1;
const MSG_READ_DATA: u32 = 2;
const MSG_FORMATS_REPORT: u32 = 3;
const MAX_TEXT: usize = 1000 * 1024;

pub struct VBox {
    h: u32,
}

struct Out {
    rc: i32,
    a: u32,
    b: u32,
}

impl VBox {
    pub fn open() -> Option<VBox> {
        let (mut h, mut action) = (0u32, 0u32);
        let rc = unsafe { os2::DosOpen(b"\\DEV\\VBOXGST$\0".as_ptr(), &mut h, &mut action, 0, 0, 0x01, 0x0042, core::ptr::null_mut()) };
        (rc == 0).then_some(VBox { h })
    }

    fn ioctl(&self, f: u32, format: u32, data: &mut [u8]) -> Option<Out> {
        let mut p = [0u8; 16];
        p[0..4].copy_from_slice(&format.to_le_bytes());
        let (mut cbp, mut cbd) = (16u32, data.len() as u32);
        let rc = unsafe {
            os2::DosDevIOCtl(self.h, 0x80, f, p.as_mut_ptr(), 16, &mut cbp, data.as_mut_ptr(), data.len() as u32, &mut cbd)
        };
        let w = |o: usize| u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        (rc == 0).then(|| Out { rc: w(4) as i32, a: w(8), b: w(12) })
    }

    pub fn start(&self) {}

    pub fn local_changed(&self, t: Option<&[u8]>) {
        let fmts = if t.is_some_and(|t| !t.is_empty()) { FMT_UNICODETEXT } else { 0 };
        let _ = self.ioctl(2, fmts, &mut []);
    }

    pub fn reply(&self, t: Option<&[u8]>) {
        let mut d = t.map(|t| text::to_utf16le(crate::code_page(), t)).unwrap_or_default();
        let _ = self.ioctl(4, FMT_UNICODETEXT, &mut d);
    }

    /// The host's text, if it has some.
    fn read_text(&self) -> Option<Vec<u8>> {
        let mut size = 16 * 1024;
        loop {
            let mut buf = alloc::vec![0u8; size];
            let o = self.ioctl(3, FMT_UNICODETEXT, &mut buf)?;
            if o.rc < 0 {
                return None;
            }
            if o.b > 0 || o.a == 0 {
                buf.truncate(o.b as usize);
                return Some(text::from_utf16le(crate::code_page(), &buf));
            }
            let want = (o.a as usize + 4096).next_multiple_of(4096);
            if want <= size || want > MAX_TEXT {
                return None;
            }
            size = want;
        }
    }

    pub fn read_loop(&self) {
        let mut ready = false;
        loop {
            let Some(o) = self.ioctl(1, 0, &mut []) else {
                unsafe { os2::DosSleep(1000) };
                continue;
            };
            if o.rc < 0 {
                unsafe { os2::DosSleep(1000) };
                continue;
            }
            if !ready {
                ready = true;
                crate::host_ready();
            }
            match o.a {
                MSG_FORMATS_REPORT if o.b & FMT_UNICODETEXT != 0 => {
                    if let Some(t) = self.read_text() {
                        crate::host_text(t);
                    }
                }
                MSG_READ_DATA => {
                    if o.b == FMT_UNICODETEXT {
                        crate::host_request();
                    } else {
                        let _ = self.ioctl(4, o.b, &mut []);
                    }
                }
                MSG_QUIT => {
                    unsafe { os2::DosSleep(1000) };
                }
                _ => {}
            }
        }
    }
}
