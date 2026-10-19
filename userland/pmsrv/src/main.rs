//! PMSRV.EXE: the Presentation Manager server. PMWIN.DLL starts it with
//! the first PM program; all of it is in PMWIN (`Rs2PmServer`, see
//! userland/pmwin/src/server.rs), which runs until the last PM program
//! has ended.

#![no_std]
#![no_main]

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.9000"]
    fn Rs2PmServer() -> u32;
}

fn main(_s: &os2::Startup) -> u32 {
    unsafe { Rs2PmServer() }
}
