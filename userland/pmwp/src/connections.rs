//! The Connections folder's objects, as far as rs2 has what they stand
//! for:
//!
//! - **Network** (WPNetwork, `<WP_NETWORK>`): IBM's folder of network
//!   groups, which LAN requesters fill. rs2 has no LAN client yet, so it
//!   holds only what is put in it; its Network page says so.
//! - **Printers** (WPPrinter, IBM's WPPRINT.DLL class): print
//!   destinations on rs2's spooler, in printer.rs. The Printers folder
//!   holds a template, as IBM's INI.RC puts one there
//!   (`<WP_NEWPRINTER>`).

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::pm::*;
use crate::setup::add_page;
use crate::som;
use crate::wps::{self, Impl};

// ---- Network ----

extern "C" fn network_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn network_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPNetwork"), wps::cd("WPFolder"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // SS_TEXT | DT_WORDBREAK
    add_page(o, nb, network_page_proc, alloc::vec![
        book::label(b"Network status", (6, 106, 220)),
        item(WC_STATIC, b"No network is available: rs2 has no LAN requester (no network groups to show). Objects you put in this folder stay here.", WS_VISIBLE | 0x0001 | 0x4000, (12, 60, 214, 42), 0xffff),
    ], b"~Network\0");
    1
}

methods! {
    __m_network_add_pages => network_add_pages, 2;
}

pub fn impl_network() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpAddSettingsPages", __m_network_add_pages as usize)] }
}
