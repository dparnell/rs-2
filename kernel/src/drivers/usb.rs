//! USB protocol pieces shared by host controller drivers: setup packets,
//! standard and HID class requests, and descriptor parsing.

use alloc::vec::Vec;

// bmRequestType values.
pub const DEV_TO_HOST: u8 = 0x80;
pub const HOST_TO_DEV: u8 = 0x00;
pub const CLASS_IFACE_OUT: u8 = 0x21;

// Standard requests.
pub const GET_DESCRIPTOR: u8 = 6;
pub const SET_CONFIGURATION: u8 = 9;

// HID class requests.
pub const HID_SET_REPORT: u8 = 0x09;
pub const HID_SET_IDLE: u8 = 0x0a;
pub const HID_SET_PROTOCOL: u8 = 0x0b;

// Descriptor types.
pub const DESC_DEVICE: u8 = 1;
pub const DESC_CONFIG: u8 = 2;
pub const DESC_INTERFACE: u8 = 4;
pub const DESC_ENDPOINT: u8 = 5;

pub const CLASS_HID: u8 = 3;
pub const CLASS_HUB: u8 = 9;

/// Port speeds as the xHCI reports them (default protocol speed IDs).
pub mod speed {
    pub const FULL: u8 = 1;
    pub const LOW: u8 = 2;
    pub const HIGH: u8 = 3;
    pub const SUPER: u8 = 4;
}

/// An 8-byte setup packet.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl Setup {
    pub fn get_descriptor(kind: u8, index: u8, length: u16) -> Setup {
        Setup { request_type: DEV_TO_HOST, request: GET_DESCRIPTOR, value: (kind as u16) << 8 | index as u16, index: 0, length }
    }

    pub fn set_configuration(value: u8) -> Setup {
        Setup { request_type: HOST_TO_DEV, request: SET_CONFIGURATION, value: value as u16, index: 0, length: 0 }
    }

    /// SET_PROTOCOL: 0 = boot protocol.
    pub fn set_protocol(iface: u8, protocol: u16) -> Setup {
        Setup { request_type: CLASS_IFACE_OUT, request: HID_SET_PROTOCOL, value: protocol, index: iface as u16, length: 0 }
    }

    /// SET_IDLE with duration 0: report only on change.
    pub fn set_idle(iface: u8) -> Setup {
        Setup { request_type: CLASS_IFACE_OUT, request: HID_SET_IDLE, value: 0, index: iface as u16, length: 0 }
    }

    /// SET_REPORT (output report 0) with a one-byte payload: the LEDs.
    pub fn set_leds(iface: u8) -> Setup {
        Setup { request_type: CLASS_IFACE_OUT, request: HID_SET_REPORT, value: 0x0200, index: iface as u16, length: 1 }
    }

    pub fn is_in(&self) -> bool {
        self.request_type & DEV_TO_HOST != 0
    }

    pub fn bytes(&self) -> [u8; 8] {
        let v = self.value.to_le_bytes();
        let i = self.index.to_le_bytes();
        let l = self.length.to_le_bytes();
        [self.request_type, self.request, v[0], v[1], i[0], i[1], l[0], l[1]]
    }
}

/// The fields of a device descriptor we use.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceDesc {
    pub usb: u16,
    pub class: u8,
    pub max_packet0: u8,
    pub vendor: u16,
    pub product: u16,
    pub configs: u8,
}

pub fn parse_device(d: &[u8]) -> Option<DeviceDesc> {
    if d.len() < 18 || d[1] != DESC_DEVICE {
        return None;
    }
    let w = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    Some(DeviceDesc { usb: w(2), class: d[4], max_packet0: d[7], vendor: w(8), product: w(10), configs: d[17] })
}

/// A HID boot interface with its interrupt-IN endpoint.
#[derive(Clone, Copy, Debug)]
pub struct HidIface {
    pub iface: u8,
    /// Boot protocol: 1 = keyboard, 2 = mouse.
    pub protocol: u8,
    pub endpoint: u8,
    pub max_packet: u16,
    pub interval: u8,
}

/// Configuration value and the HID boot interfaces (alternate setting 0)
/// of a full configuration descriptor.
pub fn parse_config(d: &[u8]) -> (u8, Vec<HidIface>) {
    let mut out = Vec::new();
    if d.len() < 9 || d[1] != DESC_CONFIG {
        return (0, out);
    }
    let value = d[5];
    let mut off = 0;
    // The boot interface being scanned, if any.
    let mut current: Option<(u8, u8)> = None;
    while off + 2 <= d.len() {
        let len = d[off] as usize;
        if len < 2 || off + len > d.len() {
            break;
        }
        let x = &d[off..off + len];
        match x[1] {
            DESC_INTERFACE if len >= 9 => {
                let boot = x[3] == 0 && x[5] == CLASS_HID && x[6] == 1 && matches!(x[7], 1 | 2);
                current = boot.then_some((x[2], x[7]));
            }
            DESC_ENDPOINT if len >= 7 => {
                if let Some((iface, protocol)) = current {
                    // Interrupt IN.
                    if x[2] & 0x80 != 0 && x[3] & 3 == 3 {
                        let max_packet = u16::from_le_bytes([x[4], x[5]]) & 0x7ff;
                        out.push(HidIface { iface, protocol, endpoint: x[2], max_packet, interval: x[6] });
                        current = None;
                    }
                }
            }
            _ => {}
        }
        off += len;
    }
    (value, out)
}
