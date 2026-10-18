//! PMGPI.DLL: the GPI entry points. PMWIN.DLL implements them (it owns the
//! presentation spaces, which are tied to windows), exporting each at
//! ordinal 4000 + its PMGPI ordinal; these entries jump there.

#![no_std]
#![no_main]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

macro_rules! forward {
    ($($name:ident = $ord:literal;)*) => {$(
        #[allow(non_snake_case)]
        mod $name {
            unsafe extern "C" {
                #[link_name = concat!("PMWIN.", stringify!($ord))]
                pub fn target();
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!("jmp {}", sym $name::target);
        }
    )*};
}

forward! {
    GpiAssociate = 4351;
    GpiBox = 4356;
    GpiCharString = 4358;
    GpiCharStringAt = 4359;
    GpiCreatePS = 4369;
    GpiDestroyPS = 4379;
    GpiErase = 4389;
    GpiLine = 4398;
    GpiMove = 4404;
    GpiPolyLine = 4415;
    GpiQueryBackColor = 4424;
    GpiQueryColor = 4437;
    GpiQueryCurrentPosition = 4441;
    GpiQueryPel = 4476;
    GpiSetBackColor = 4504;
    GpiSetBackMix = 4505;
    GpiSetColor = 4517;
    GpiSetCurrentPosition = 4519;
    GpiSetMix = 4537;
    GpiSetPel = 4544;
    GpiCreateLogColorTable = 4592;
}
