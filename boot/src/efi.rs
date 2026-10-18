//! Minimal raw UEFI bindings: only what the loader needs.

#![allow(dead_code)]

use core::ffi::c_void;

pub type Handle = *mut c_void;
pub type Status = usize;

pub const SUCCESS: Status = 0;
pub const ERROR_BIT: usize = 1 << (usize::BITS - 1);
pub const BUFFER_TOO_SMALL: Status = ERROR_BIT | 5;
pub const NOT_FOUND: Status = ERROR_BIT | 14;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

pub const LOADED_IMAGE_GUID: Guid =
    Guid(0x5b1b31a1, 0x9562, 0x11d2, [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const SIMPLE_FS_GUID: Guid =
    Guid(0x964e5b22, 0x6459, 0x11d2, [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const FILE_INFO_GUID: Guid =
    Guid(0x09576e92, 0x6d3f, 0x11d2, [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const GOP_GUID: Guid =
    Guid(0x9042a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a]);
pub const ACPI20_GUID: Guid =
    Guid(0x8868e871, 0xe4f1, 0x11d3, [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81]);
pub const ACPI10_GUID: Guid =
    Guid(0xeb9d2d30, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct SystemTable {
    pub hdr: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: Handle,
    pub con_in: *mut c_void,
    pub console_out_handle: Handle,
    pub con_out: *mut TextOutput,
    pub stderr_handle: Handle,
    pub stderr: *mut TextOutput,
    pub runtime_services: *mut c_void,
    pub boot_services: *mut BootServices,
    pub table_count: usize,
    pub config_table: *const ConfigTable,
}

#[repr(C)]
pub struct ConfigTable {
    pub guid: Guid,
    pub table: *const c_void,
}

#[repr(C)]
pub struct TextOutput {
    pub reset: extern "efiapi" fn(*mut TextOutput, bool) -> Status,
    pub output_string: extern "efiapi" fn(*mut TextOutput, *const u16) -> Status,
}

type Unused = usize;

#[repr(C)]
pub struct BootServices {
    pub hdr: TableHeader,
    pub raise_tpl: Unused,
    pub restore_tpl: Unused,
    pub allocate_pages: extern "efiapi" fn(u32, u32, usize, *mut u64) -> Status,
    pub free_pages: extern "efiapi" fn(u64, usize) -> Status,
    pub get_memory_map:
        extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> Status,
    pub allocate_pool: extern "efiapi" fn(u32, usize, *mut *mut u8) -> Status,
    pub free_pool: extern "efiapi" fn(*mut u8) -> Status,
    pub create_event: Unused,
    pub set_timer: Unused,
    pub wait_for_event: Unused,
    pub signal_event: Unused,
    pub close_event: Unused,
    pub check_event: Unused,
    pub install_protocol_interface: Unused,
    pub reinstall_protocol_interface: Unused,
    pub uninstall_protocol_interface: Unused,
    pub handle_protocol: extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status,
    pub reserved: Unused,
    pub register_protocol_notify: Unused,
    pub locate_handle: Unused,
    pub locate_device_path: Unused,
    pub install_configuration_table: Unused,
    pub load_image: Unused,
    pub start_image: Unused,
    pub exit: Unused,
    pub unload_image: Unused,
    pub exit_boot_services: extern "efiapi" fn(Handle, usize) -> Status,
    pub get_next_monotonic_count: Unused,
    pub stall: extern "efiapi" fn(usize) -> Status,
    pub set_watchdog_timer: extern "efiapi" fn(usize, u64, usize, *const u16) -> Status,
    pub connect_controller: Unused,
    pub disconnect_controller: Unused,
    pub open_protocol: Unused,
    pub close_protocol: Unused,
    pub open_protocol_information: Unused,
    pub protocols_per_handle: Unused,
    pub locate_handle_buffer: Unused,
    pub locate_protocol: extern "efiapi" fn(*const Guid, *mut c_void, *mut *mut c_void) -> Status,
}

/// `EFI_MEMORY_DESCRIPTOR`. The firmware's descriptor size may be larger.
#[repr(C)]
pub struct MemoryDescriptor {
    pub ty: u32,
    pub phys_start: u64,
    pub virt_start: u64,
    pub pages: u64,
    pub attribute: u64,
}

pub mod memtype {
    pub const LOADER_CODE: u32 = 1;
    pub const LOADER_DATA: u32 = 2;
    pub const BOOT_CODE: u32 = 3;
    pub const BOOT_DATA: u32 = 4;
    pub const RUNTIME_CODE: u32 = 5;
    pub const RUNTIME_DATA: u32 = 6;
    pub const CONVENTIONAL: u32 = 7;
    pub const ACPI_RECLAIM: u32 = 9;
    pub const PERSISTENT: u32 = 14;
    /// Allocations the kernel must keep (OS-defined range).
    pub const RS2_KEEP: u32 = 0x8000_0001;
}

#[repr(C)]
pub struct LoadedImage {
    pub revision: u32,
    pub parent: Handle,
    pub system_table: *mut SystemTable,
    pub device: Handle,
    pub file_path: *mut c_void,
    pub reserved: *mut c_void,
    pub load_options_size: u32,
    pub load_options: *mut c_void,
    pub image_base: *mut c_void,
    pub image_size: u64,
}

#[repr(C)]
pub struct SimpleFs {
    pub revision: u64,
    pub open_volume: extern "efiapi" fn(*mut SimpleFs, *mut *mut File) -> Status,
}

#[repr(C)]
pub struct File {
    pub revision: u64,
    pub open: extern "efiapi" fn(*mut File, *mut *mut File, *const u16, u64, u64) -> Status,
    pub close: extern "efiapi" fn(*mut File) -> Status,
    pub delete: Unused,
    pub read: extern "efiapi" fn(*mut File, *mut usize, *mut u8) -> Status,
    pub write: Unused,
    pub get_position: Unused,
    pub set_position: extern "efiapi" fn(*mut File, u64) -> Status,
    pub get_info: extern "efiapi" fn(*mut File, *const Guid, *mut usize, *mut u8) -> Status,
}

pub const FILE_MODE_READ: u64 = 1;
pub const FILE_DIRECTORY: u64 = 0x10;

/// Fixed part of `EFI_FILE_INFO`; the UCS-2 file name follows.
#[repr(C)]
pub struct FileInfo {
    pub size: u64,
    pub file_size: u64,
    pub physical_size: u64,
    pub create_time: [u8; 16],
    pub access_time: [u8; 16],
    pub modification_time: [u8; 16],
    pub attribute: u64,
}

#[repr(C)]
pub struct Gop {
    pub query_mode: extern "efiapi" fn(*mut Gop, u32, *mut usize, *mut *mut GopModeInfo) -> Status,
    pub set_mode: extern "efiapi" fn(*mut Gop, u32) -> Status,
    pub blt: Unused,
    pub mode: *mut GopMode,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *mut GopModeInfo,
    pub info_size: usize,
    pub fb_base: u64,
    pub fb_size: usize,
}

#[repr(C)]
pub struct GopModeInfo {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    /// 0 = RGBX, 1 = BGRX, 2 = bitmask, 3 = blt only.
    pub pixel_format: u32,
    pub masks: [u32; 4],
    pub stride: u32,
}
