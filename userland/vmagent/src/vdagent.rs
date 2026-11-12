//! The SPICE agent's channel: the virtio-serial port `com.redhat.spice.0`
//! (`\DEV\VDAGENT$`), protocol in guestagent::vdagent.

use alloc::vec::Vec;

use guestagent::text;
use guestagent::vdagent::{self as vd, Parser};

pub struct Vdagent {
    h: u32,
}

impl Vdagent {
    pub fn open() -> Option<Vdagent> {
        let (mut h, mut action) = (0u32, 0u32);
        // OPEN_ACTION_OPEN_IF_EXISTS; read/write, deny none.
        let rc = unsafe { os2::DosOpen(b"\\DEV\\VDAGENT$\0".as_ptr(), &mut h, &mut action, 0, 0, 0x01, 0x0042, core::ptr::null_mut()) };
        (rc == 0).then_some(Vdagent { h })
    }

    fn send(&self, b: &[u8]) {
        let mut n = 0;
        unsafe { os2::DosWrite(self.h, b.as_ptr(), b.len() as u32, &mut n) };
    }

    pub fn start(&self) {
        self.send(&vd::announce(true));
    }

    pub fn local_changed(&self, t: Option<&[u8]>) {
        match t {
            Some(t) if !t.is_empty() => self.send(&vd::grab_text()),
            _ => self.send(&vd::release()),
        }
    }

    pub fn reply(&self, t: Option<&[u8]>) {
        let utf8 = t.map(|t| text::to_utf8(crate::code_page(), t));
        self.send(&vd::clipboard(utf8.as_deref()));
    }

    pub fn read_loop(&self) {
        let mut p = Parser::new();
        let mut buf = alloc::vec![0u8; 4096];
        loop {
            let mut n = 0;
            let rc = unsafe { os2::DosRead(self.h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
            if rc != 0 || n == 0 {
                unsafe { os2::DosSleep(500) };
                continue;
            }
            for m in p.feed(&buf[..n as usize]) {
                self.message(m);
            }
        }
    }

    fn message(&self, m: vd::Message) {
        let words: Vec<u32> = m.words().collect();
        match m.ty {
            vd::ANNOUNCE_CAPABILITIES => {
                if words.first() == Some(&1) {
                    self.send(&vd::announce(false));
                }
                crate::host_ready();
            }
            vd::CLIPBOARD_GRAB => {
                if words.contains(&vd::TYPE_UTF8_TEXT) {
                    self.send(&vd::request(vd::TYPE_UTF8_TEXT));
                }
            }
            vd::CLIPBOARD => {
                if words.first() == Some(&vd::TYPE_UTF8_TEXT) {
                    crate::host_text(text::from_utf8(crate::code_page(), &m.data[4..]));
                }
            }
            vd::CLIPBOARD_REQUEST => {
                if words.first() == Some(&vd::TYPE_UTF8_TEXT) {
                    crate::host_request();
                } else {
                    self.send(&vd::clipboard(None));
                }
            }
            _ => {}
        }
    }
}
