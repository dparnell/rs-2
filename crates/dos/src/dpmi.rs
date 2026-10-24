//! The DPMI 0.9 host (INT 2Fh AX=1687h, INT 31h).

use crate::{Cb, Host, Machine};

pub const CB_FIRST: u16 = 0x200;
pub const CB_LAST: u16 = 0x2ff;

#[derive(Default)]
pub struct Dpmi {}

impl Machine {
    pub(crate) fn dpmi_detect<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        // Not installed (yet).
    }

    pub(crate) fn dpmi_callback<H: Host>(&mut self, host: &mut H, n: u16) -> Cb {
        let _ = (host, n);
        Cb::Done
    }

    pub(crate) fn dpmi_process_ended(&mut self, psp: u16) {
        let _ = psp;
    }
}
