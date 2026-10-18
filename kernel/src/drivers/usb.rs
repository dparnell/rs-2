//! USB protocol pieces shared by host controller drivers: setup packets,
//! standard, hub, HID and mass storage class requests, and descriptor
//! parsing.

use alloc::vec::Vec;

// bmRequestType values.
pub const DEV_TO_HOST: u8 = 0x80;
pub const HOST_TO_DEV: u8 = 0x00;
pub const CLASS_IFACE_OUT: u8 = 0x21;
pub const CLASS_IFACE_IN: u8 = 0xa1;
pub const CLASS_DEV_OUT: u8 = 0x20;
pub const CLASS_DEV_IN: u8 = 0xa0;
/// Class request to "other" (a hub port).
pub const CLASS_OTHER_OUT: u8 = 0x23;
pub const CLASS_OTHER_IN: u8 = 0xa3;
pub const STD_EP_OUT: u8 = 0x02;

// Standard requests.
pub const GET_STATUS: u8 = 0;
pub const CLEAR_FEATURE: u8 = 1;
pub const SET_FEATURE: u8 = 3;
pub const GET_DESCRIPTOR: u8 = 6;
pub const SET_CONFIGURATION: u8 = 9;
/// Feature selector ENDPOINT_HALT.
pub const ENDPOINT_HALT: u16 = 0;

// Hub class requests.
pub const HUB_SET_DEPTH: u8 = 12;

// Mass storage class requests (Bulk-Only Transport).
pub const MSC_RESET: u8 = 0xff;
pub const MSC_GET_MAX_LUN: u8 = 0xfe;

// HID class requests.
pub const HID_SET_REPORT: u8 = 0x09;
pub const HID_SET_IDLE: u8 = 0x0a;
pub const HID_SET_PROTOCOL: u8 = 0x0b;

// Descriptor types.
pub const DESC_DEVICE: u8 = 1;
pub const DESC_CONFIG: u8 = 2;
pub const DESC_INTERFACE: u8 = 4;
pub const DESC_ENDPOINT: u8 = 5;
pub const DESC_HUB: u8 = 0x29;
pub const DESC_SS_HUB: u8 = 0x2a;
pub const DESC_SS_EP_COMPANION: u8 = 0x30;

pub const CLASS_HID: u8 = 3;
pub const CLASS_STORAGE: u8 = 8;
pub const CLASS_HUB: u8 = 9;

/// Mass storage subclass SCSI transparent command set, protocol Bulk-Only.
pub const MSC_SCSI: u8 = 6;
pub const MSC_BOT: u8 = 0x50;

// Endpoint transfer types (bmAttributes bits 0-1).
pub const EP_BULK: u8 = 2;
pub const EP_INTERRUPT: u8 = 3;

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

    /// GET_DESCRIPTOR for the hub class descriptor (USB 2 or USB 3 form).
    pub fn hub_descriptor(usb3: bool) -> Setup {
        let kind = if usb3 { DESC_SS_HUB } else { DESC_HUB };
        Setup { request_type: CLASS_DEV_IN, request: GET_DESCRIPTOR, value: (kind as u16) << 8, index: 0, length: 32 }
    }

    pub fn set_port_feature(port: u8, feature: u16) -> Setup {
        Setup { request_type: CLASS_OTHER_OUT, request: SET_FEATURE, value: feature, index: port as u16, length: 0 }
    }

    pub fn clear_port_feature(port: u8, feature: u16) -> Setup {
        Setup { request_type: CLASS_OTHER_OUT, request: CLEAR_FEATURE, value: feature, index: port as u16, length: 0 }
    }

    /// GET_STATUS of a hub port: wPortStatus, wPortChange.
    pub fn port_status(port: u8) -> Setup {
        Setup { request_type: CLASS_OTHER_IN, request: GET_STATUS, value: 0, index: port as u16, length: 4 }
    }

    /// GET_STATUS of the hub itself: wHubStatus, wHubChange.
    pub fn hub_status() -> Setup {
        Setup { request_type: CLASS_DEV_IN, request: GET_STATUS, value: 0, index: 0, length: 4 }
    }

    pub fn clear_hub_feature(feature: u16) -> Setup {
        Setup { request_type: CLASS_DEV_OUT, request: CLEAR_FEATURE, value: feature, index: 0, length: 0 }
    }

    /// SET_HUB_DEPTH (USB 3 hubs): the hub's tier below the root, from 0.
    pub fn set_hub_depth(depth: u8) -> Setup {
        Setup { request_type: CLASS_DEV_OUT, request: HUB_SET_DEPTH, value: depth as u16, index: 0, length: 0 }
    }

    /// CLEAR_FEATURE(ENDPOINT_HALT) for endpoint address `ep`.
    pub fn clear_halt(ep: u8) -> Setup {
        Setup { request_type: STD_EP_OUT, request: CLEAR_FEATURE, value: ENDPOINT_HALT, index: ep as u16, length: 0 }
    }

    /// Bulk-Only: Get Max LUN (one byte back).
    pub fn get_max_lun(iface: u8) -> Setup {
        Setup { request_type: CLASS_IFACE_IN, request: MSC_GET_MAX_LUN, value: 0, index: iface as u16, length: 1 }
    }

    /// Bulk-Only Mass Storage Reset.
    pub fn msc_reset(iface: u8) -> Setup {
        Setup { request_type: CLASS_IFACE_OUT, request: MSC_RESET, value: 0, index: iface as u16, length: 0 }
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
    pub protocol: u8,
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
    Some(DeviceDesc {
        usb: w(2),
        class: d[4],
        protocol: d[6],
        max_packet0: d[7],
        vendor: w(8),
        product: w(10),
        configs: d[17],
    })
}

/// An endpoint descriptor (with the SuperSpeed companion's burst size).
#[derive(Clone, Copy, Debug, Default)]
pub struct Endpoint {
    pub address: u8,
    pub attributes: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub max_burst: u8,
}

impl Endpoint {
    pub fn is_in(&self) -> bool {
        self.address & 0x80 != 0
    }

    pub fn kind(&self) -> u8 {
        self.attributes & 3
    }

    /// xHCI device context index: endpoint number * 2, plus 1 for IN.
    pub fn dci(&self) -> u8 {
        (self.address & 0xf) * 2 + self.is_in() as u8
    }

    /// The first endpoint of `kind` in direction `is_in` among `eps`.
    pub fn find(eps: &[Endpoint], kind: u8, is_in: bool) -> Option<Endpoint> {
        eps.iter().copied().find(|e| e.kind() == kind && e.is_in() == is_in)
    }
}

/// An interface (one alternate setting) with its endpoints.
#[derive(Clone, Debug, Default)]
pub struct Interface {
    pub number: u8,
    pub alt: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<Endpoint>,
}

/// Configuration value and all interfaces of a full configuration
/// descriptor.
pub fn parse_interfaces(d: &[u8]) -> (u8, Vec<Interface>) {
    let mut out: Vec<Interface> = Vec::new();
    if d.len() < 9 || d[1] != DESC_CONFIG {
        return (0, out);
    }
    let mut off = 0;
    while off + 2 <= d.len() {
        let len = d[off] as usize;
        if len < 2 || off + len > d.len() {
            break;
        }
        let x = &d[off..off + len];
        match x[1] {
            DESC_INTERFACE if len >= 9 => out.push(Interface {
                number: x[2],
                alt: x[3],
                class: x[5],
                subclass: x[6],
                protocol: x[7],
                endpoints: Vec::new(),
            }),
            DESC_ENDPOINT if len >= 7 => {
                if let Some(i) = out.last_mut() {
                    let max_packet = u16::from_le_bytes([x[4], x[5]]) & 0x7ff;
                    i.endpoints.push(Endpoint { address: x[2], attributes: x[3], max_packet, interval: x[6], max_burst: 0 });
                }
            }
            DESC_SS_EP_COMPANION if len >= 6 => {
                if let Some(e) = out.last_mut().and_then(|i| i.endpoints.last_mut()) {
                    e.max_burst = x[2].min(15);
                }
            }
            _ => {}
        }
        off += len;
    }
    (d[5], out)
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

/// The HID boot interfaces (alternate setting 0, keyboard or mouse) with
/// their interrupt-IN endpoints.
pub fn hid_boot_interfaces(ifaces: &[Interface]) -> Vec<HidIface> {
    ifaces
        .iter()
        .filter(|i| i.alt == 0 && i.class == CLASS_HID && i.subclass == 1 && matches!(i.protocol, 1 | 2))
        .filter_map(|i| {
            let e = Endpoint::find(&i.endpoints, EP_INTERRUPT, true)?;
            Some(HidIface { iface: i.number, protocol: i.protocol, endpoint: e.address, max_packet: e.max_packet, interval: e.interval })
        })
        .collect()
}
