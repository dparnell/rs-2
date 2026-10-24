//! rs2's COMMAND.COM.

use crate::{Cb, Host, Machine};

pub const CB_SHELL: u16 = 0x102;

pub struct Shell {
    pub psp: u16,
}

impl Machine {
    pub(crate) fn shell_step<H: Host>(&mut self, host: &mut H) -> Cb {
        let _ = host;
        Cb::Done
    }

    pub(crate) fn shell_child_ended<H: Host>(&mut self, host: &mut H, psp: u16) {
        let _ = (host, psp);
    }
}
