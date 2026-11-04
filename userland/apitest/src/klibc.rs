//! What kLIBC (the GCC C library of OS/2, LIBCN0.DLL) and the programs
//! built with it expect of DOSCALLS beyond the documented API.

use core::ptr::null;

use os2::*;

use crate::check;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    // DOSCALLS' constant exports (kLIBC's dos-data.imp): the import's
    // "address" is the value.
    #[link_name = "DOSCALLS.214"]
    static DosMaxPathLen: u8;
    #[link_name = "DOSCALLS.215"]
    static DosPageSize: u8;
    #[link_name = "DOSCALLS.216"]
    static DosLocalInfo: u8;
    #[link_name = "DOSCALLS.217"]
    static DosGlobalInfo: u8;
    #[link_name = "DOSCALLS.369"]
    static DosFlatCS: u8;
    #[link_name = "DOSCALLS.419"]
    static Dos32TIB: u8;
}

unsafe extern "system" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> APIRET;
}

pub fn run() {
    unsafe {
        let v = |p: &u8| p as *const u8 as u32;
        check("DOSCALLS.214 DosMaxPathLen", v(&DosMaxPathLen) == 260);
        check("DOSCALLS.215 DosPageSize", v(&DosPageSize) == 4096);
        let fs: u32;
        let cs: u32;
        core::arch::asm!("mov {0:e}, fs", "mov {1:e}, cs", out(reg) fs, out(reg) cs);
        check("DOSCALLS.419 DOS32TIB is FS", v(&Dos32TIB) == fs & 0xffff);
        check("DOSCALLS.369 DosFlatCS is CS", v(&DosFlatCS) == cs & 0xffff);
        // kLIBC's GETLINFOSEG/GETGINFOSEG: the selector's tile is the
        // segment's flat address.
        let lis = ((v(&DosLocalInfo) & 0xfff8) << 13) as *const u16;
        let gis = ((v(&DosGlobalInfo) & 0xfff8) << 13) as *const u8;
        let (mut tib, mut pib): (*mut Tib, *mut Pib) = (core::ptr::null_mut(), core::ptr::null_mut());
        DosGetInfoBlocks(&mut tib, &mut pib);
        check("DosLocalInfo: LINFOSEG pid", *lis == (*pib).pid as u16);
        check("DosLocalInfo: LINFOSEG tid", *lis.add(3) == 1);
        // GINFOSEG: major/minor version at 21/22.
        check("DosGlobalInfo: GINFOSEG version", *gis.add(21) == 20 && *gis.add(22) == 45);

        // The tiled selector of an address in a 32-bit code object is a
        // code selector (kLIBC's 16-bit thunk RETFs through it).
        let sel = ((run as usize as u32) >> 13) | 7;
        let ar: u32;
        let zf: u8;
        core::arch::asm!("lar {0:e}, {1:e}", "setz {2}", out(reg) ar, in(reg) sel, out(reg_byte) zf);
        check("tile of 32-bit code is a code selector", zf == 1 && ar & 0x0800 != 0);

        // A private event semaphore opened again by its handle in the
        // creating process (kLIBC's heap: _fmutex_open).
        let mut hev = 0u32;
        DosCreateEventSem(null(), &mut hev, 0, 0);
        let mut h2 = hev;
        check("DosOpenEventSem private by handle", DosOpenEventSem(null(), &mut h2) == 0 && h2 == hev);
        check("close opened private sem twice", DosCloseEventSem(hev) == 0 && DosCloseEventSem(hev) == 0);
        check("private sem gone after last close", DosCloseEventSem(hev) == 6);
    }
}
