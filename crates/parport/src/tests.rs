//! The protocol code against the simulated port (`sim.rs`).

use crate::sim::{Dev, Sim};
use crate::*;

const ID: &str = "MFG:Hewlett-Packard;MDL:LaserJet 4;CMD:PJL,PCL,POSTSCRIPT;CLS:PRINTER;";

fn pattern() -> Vec<u8> {
    let mut v = b"RS2 LPT TEST\r\n".to_vec();
    for _ in 0..16 {
        v.extend(0..=255u8);
    }
    v
}

#[test]
fn detect_spp_port() {
    // QEMU's: the latch reads back, no ECR, no EPP, not bidirectional.
    let mut s = Sim::spp(Dev::printer());
    let p = Port { base: 0x378, ecr: 0 };
    assert!(latch_ok(&mut s, &p));
    assert!(!ecr_ok(&mut s, 0x378, 0x77a));
    assert_eq!(capabilities(&mut s, &p), 0);
    assert!(s.dev.printed.is_empty(), "probing prints nothing");
}

#[test]
fn detect_nothing() {
    let mut s = Sim::spp(Dev::printer());
    s.present = false;
    let p0 = s.port();
    assert!(!latch_ok(&mut s, &p0));
    assert!(!ecr_ok(&mut s, 0x378, 0x77a));
}

#[test]
fn detect_ecr_mirror_is_not_ecp() {
    let mut s = Sim::spp(Dev::printer());
    s.ecr_mirror = true;
    assert!(!ecr_ok(&mut s, 0x378, 0x77a));
}

#[test]
fn detect_ecp_chip() {
    let mut s = Sim::ecp(Dev::printer());
    assert!(ecr_ok(&mut s, 0x378, 0x77a));
    let p0 = s.port();
    let caps = capabilities(&mut s, &p0);
    assert_eq!(caps, CAP_ECP | CAP_EPP | CAP_BYTE);
    // Probing never waits on the peripheral.
    assert!(s.t_us < 1000, "{} us", s.t_us);
}

#[test]
fn detect_epp_and_byte_without_ecr() {
    let mut s = Sim::spp(Dev::printer());
    s.has_epp = true;
    s.bidir = true;
    let p0 = s.port();
    assert_eq!(capabilities(&mut s, &p0), CAP_EPP | CAP_BYTE);
}

#[test]
fn spp_write_prints() {
    let mut s = Sim::spp(Dev::printer());
    let data = pattern();
    let p0 = s.port();
    let (n, r) = spp_write(&mut s, &p0, CTR_IDLE, &data, 1000, false);
    assert_eq!((n, r), (data.len(), Ok(())));
    assert_eq!(s.dev.printed, data);
}

#[test]
fn spp_out_of_paper_times_out() {
    let mut d = Dev::printer();
    d.set_paper_out(true);
    let mut s = Sim::spp(d);
    let p = s.port();
    let t0 = s.t_us;
    let (n, r) = spp_write(&mut s, &p, CTR_IDLE, b"abc", 50, false);
    assert_eq!((n, r), (0, Err(OUT_OF_PAPER)));
    assert!(s.t_us - t0 >= 50_000);
    // Paper again: the write goes through.
    s.dev.set_paper_out(false);
    assert_eq!(spp_write(&mut s, &p, CTR_IDLE, b"abc", 50, false), (3, Ok(())));
    assert_eq!(s.dev.printed, b"abc");
}

#[test]
fn stop_errors() {
    // PRINT01's prterp: out of paper, busy, selected, no fault.
    assert_eq!(stop_error(STS_PAPER | STS_SELECT | STS_ERROR | STS_ACK), OUT_OF_PAPER);
    // Nothing answering: everything low.
    assert_eq!(stop_error(0x00 | STS_ACK | STS_ERROR), WRITE_FAULT);
    // Not busy, paper out, selected: unknown unit.
    assert_eq!(stop_error(STS_BUSY | STS_PAPER | STS_SELECT | STS_ACK | STS_ERROR), BAD_UNIT);
    assert!(ready(STS_BUSY | STS_SELECT | STS_ACK | STS_ERROR, b'a'));
    assert!(!ready(STS_SELECT | STS_ACK | STS_ERROR, b'a'));
    assert!(ready(STS_BUSY | STS_ACK | STS_ERROR, 0x11));
}

#[test]
fn fifo_write_prints() {
    let mut s = Sim::ecp(Dev::printer());
    let p = s.port();
    let data = pattern();
    let (n, r) = fifo_write(&mut s, &p, &data, 1000);
    assert_eq!((n, r), (data.len(), Ok(())));
    assert_eq!(s.dev.printed, data);
}

#[test]
fn ecp_write_to_ecp_printer() {
    let mut s = Sim::ecp(Dev::ieee1284(ID, true, true));
    let p = s.port();
    let data = pattern();
    let (n, r) = ecp_write(&mut s, &p, &data, 1000);
    assert_eq!((n, r), (data.len(), Ok(())));
    assert_eq!(s.dev.ecp_data, data);
    assert!(s.dev.printed.is_empty());
    assert_eq!(s.dev.requests, [XFLAG_ECP]);
    assert!(s.dev.in_compat(), "terminated after");
    // Compatibility mode works after it.
    let (n, r) = spp_write(&mut s, &p, CTR_IDLE, b"after", 1000, false);
    assert_eq!((n, r), (5, Ok(())));
    assert_eq!(s.dev.printed, b"after");
}

#[test]
fn ecp_refused_by_1284_printer() {
    // A 1284 printer without ECP: negotiation refused, terminated.
    let mut s = Sim::ecp(Dev::ieee1284(ID, false, false));
    let p = s.port();
    let (n, r) = ecp_write(&mut s, &p, b"x", 1000);
    assert_eq!((n, r), (0, Err(BAD_COMMAND)));
    assert!(s.dev.in_compat());
    assert!(s.dev.ecp_data.is_empty() && s.dev.printed.is_empty());
}

#[test]
fn ecp_to_plain_printer_does_not_hang() {
    let mut s = Sim::ecp(Dev::printer());
    let p = s.port();
    let t0 = s.t_us;
    let (n, r) = ecp_write(&mut s, &p, b"x", 120_000);
    assert_eq!((n, r), (0, Err(BAD_COMMAND)));
    // About 35 ms (IEEE 1284's response time), not the write timeout.
    assert!(s.t_us - t0 < 40_000, "{} us", s.t_us - t0);
    // And the printer still prints.
    assert_eq!(fifo_write(&mut s, &p, b"ok", 1000), (2, Ok(())));
    assert_eq!(s.dev.printed, b"ok");
}

#[test]
fn epp_write_to_device() {
    let mut d = Dev::printer();
    d.epp = true;
    let mut s = Sim::ecp(d);
    let p = s.port();
    assert_eq!(epp_write(&mut s, &p, b"epp data"), (8, Ok(())));
    assert_eq!(s.dev.epp_data, b"epp data");
}

#[test]
fn epp_write_without_device() {
    let mut s = Sim::ecp(Dev::printer());
    let p = s.port();
    assert_eq!(epp_write(&mut s, &p, b"x"), (0, Err(WRITE_FAULT)));
    // The timeout bit is cleared again.
    assert_eq!(s.inb(p.status()) & STS_EPP_TIMEOUT, 0);
}

#[test]
fn device_id_nibble() {
    let mut s = Sim::spp(Dev::ieee1284(ID, false, false));
    let p = s.port();
    let id = read_device_id(&mut s, &p, Reverse::Nibble).unwrap();
    assert_eq!(&id[2..], ID.as_bytes());
    assert_eq!(u16::from_be_bytes([id[0], id[1]]) as usize, id.len());
    assert_eq!(s.dev.requests, [XFLAG_DEVICE_ID]);
    assert!(s.dev.in_compat());
    assert_eq!(id_field(&id, &["MFG", "MANUFACTURER"]), Some(&b"Hewlett-Packard"[..]));
    assert_eq!(id_field(&id, &["MDL", "MODEL"]), Some(&b"LaserJet 4"[..]));
    assert_eq!(id_field(&id, &["DES"]), None);
}

#[test]
fn device_id_byte_mode() {
    let mut s = Sim::ecp(Dev::ieee1284(ID, true, true));
    let p = s.port();
    let id = read_device_id(&mut s, &p, Reverse::Byte).unwrap();
    assert_eq!(&id[2..], ID.as_bytes());
    assert_eq!(s.dev.requests, [XFLAG_DEVICE_ID | XFLAG_BYTE]);
    assert!(s.dev.in_compat());
}

#[test]
fn device_id_best_mode() {
    // Byte mode on a bidirectional port.
    let mut s = Sim::ecp(Dev::ieee1284(ID, false, true));
    let p = s.port();
    let caps = capabilities(&mut s, &p);
    assert_eq!(&device_id(&mut s, &p, caps).unwrap()[2..], ID.as_bytes());
    assert_eq!(s.dev.requests, [0x05]);
    // A peripheral without byte mode: refused, then nibble mode.
    let mut s = Sim::ecp(Dev::ieee1284(ID, false, false));
    let caps = capabilities(&mut s, &p);
    assert_eq!(&device_id(&mut s, &p, caps).unwrap()[2..], ID.as_bytes());
    assert_eq!(s.dev.requests, [0x05, 0x04]);
    // An SPP port: nibble mode.
    let mut s = Sim::spp(Dev::ieee1284(ID, false, true));
    let p = s.port();
    let caps = capabilities(&mut s, &p);
    assert_eq!(&device_id(&mut s, &p, caps).unwrap()[2..], ID.as_bytes());
    assert_eq!(s.dev.requests, [0x04]);
}

#[test]
fn device_id_short_length() {
    // A peripheral whose length is less than the data: cut there.
    let mut d = Dev::ieee1284(ID, false, false);
    d.id[1] = 12;
    let mut s = Sim::spp(d);
    let p = s.port();
    let id = read_device_id(&mut s, &p, Reverse::Nibble).unwrap();
    assert_eq!(id.len(), 12);
    assert!(s.dev.in_compat());
}

#[test]
fn device_id_long_length() {
    // A length past the data: reading stops when the peripheral says it
    // has no more (nDataAvail high).
    let mut d = Dev::ieee1284(ID, false, false);
    d.id[0] = 0x01;
    let mut s = Sim::spp(d);
    let p = s.port();
    let id = read_device_id(&mut s, &p, Reverse::Nibble).unwrap();
    assert_eq!(&id[2..], ID.as_bytes());
}

#[test]
fn device_id_without_1284_peripheral() {
    // QEMU's case: nothing negotiates, BAD_COMMAND after the response
    // time, and the printer prints as before.
    let mut s = Sim::spp(Dev::printer());
    let p = s.port();
    let t0 = s.t_us;
    assert_eq!(device_id(&mut s, &p, 0), Err(BAD_COMMAND));
    assert!(s.t_us - t0 < 40_000);
    assert!(s.dev.printed.is_empty());
    assert_eq!(spp_write(&mut s, &p, CTR_IDLE, b"ok", 1000, false), (2, Ok(())));
    assert_eq!(s.dev.printed, b"ok");
}

#[test]
fn device_id_without_id() {
    // A 1284 peripheral without a device ID refuses request 04h.
    let mut d = Dev::ieee1284("", false, false);
    d.id.clear();
    let mut s = Sim::spp(d);
    let p = s.port();
    assert_eq!(device_id(&mut s, &p, 0), Err(BAD_COMMAND));
    assert!(s.dev.in_compat());
}

#[test]
fn nibble_decoding() {
    // nFault, Select, PError, then Busy (inverted in the register).
    assert_eq!(status_to_nibble(STS_BUSY), 0);
    assert_eq!(status_to_nibble(0), 8);
    assert_eq!(status_to_nibble(STS_ERROR | STS_SELECT | STS_PAPER | STS_BUSY), 7);
    assert_eq!(status_to_nibble(STS_SELECT), 0x0a);
}

#[test]
fn pci_ports() {
    // A NetMos 9865 parallel function: I/O BARs 0 and 1.
    assert_eq!(pci_port(7, 1, 2, 0xe001, 0xe101), Ok((0xe000, 0xe102)));
    // SPP only, no second BAR.
    assert_eq!(pci_port(7, 1, 0, 0xd001, 0), Ok((0xd000, 0)));
    // A memory BAR 1 is not the ECP registers.
    assert_eq!(pci_port(7, 1, 1, 0xd001, 0xfe00_0000), Ok((0xd000, 0)));
    assert!(pci_port(7, 1, 3, 0xd001, 0).is_err());
    assert!(pci_port(7, 1, 0xfe, 0xd001, 0).is_err());
    assert!(pci_port(7, 1, 0, 0xfe00_0000, 0).is_err());
    assert!(pci_port(7, 1, 0, 1, 0).is_err());
    assert!(pci_port(7, 0, 2, 0xe001, 0xe101).is_err());
}

#[test]
fn ecp_chip_on_pci_bars() {
    // A card's port at BAR 0, its ECP registers at BAR 1 (the simulated
    // chip decodes them at base+400h).
    let mut s = Sim::ecp(Dev::ieee1284(ID, true, true));
    s.base = 0xe000;
    let (base, ecr) = pci_port(7, 1, 2, 0xe001, 0xe401).unwrap();
    assert!(latch_ok(&mut s, &Port { base, ecr: 0 }));
    assert!(ecr_ok(&mut s, base, ecr));
    let p = Port { base, ecr };
    assert_eq!(capabilities(&mut s, &p), CAP_ECP | CAP_EPP | CAP_BYTE);
    assert_eq!(ecp_write(&mut s, &p, b"pci", 1000), (3, Ok(())));
    assert_eq!(s.dev.ecp_data, b"pci");
    assert_eq!(&device_id(&mut s, &p, CAP_BYTE).unwrap()[2..], ID.as_bytes());
}
