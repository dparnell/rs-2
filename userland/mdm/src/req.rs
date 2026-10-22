#![allow(dead_code)]

//! Commands as MDM carries them out: mciSendCommand's messages and
//! mciSendString's strings both become a `Req`, which the device answers
//! with a `Reply`.

use alloc::string::String;

pub const MCI_OPEN: u32 = 1;
pub const MCI_CLOSE: u32 = 2;
pub const MCI_PLAY: u32 = 4;
pub const MCI_SEEK: u32 = 5;
pub const MCI_STOP: u32 = 6;
pub const MCI_PAUSE: u32 = 7;
pub const MCI_INFO: u32 = 8;
pub const MCI_GETDEVCAPS: u32 = 9;
pub const MCI_STATUS: u32 = 10;
pub const MCI_SET: u32 = 12;
pub const MCI_SYSINFO: u32 = 15;
pub const MCI_CUE: u32 = 17;
pub const MCI_LOAD: u32 = 22;
pub const MCI_ACQUIREDEVICE: u32 = 23;
pub const MCI_RELEASEDEVICE: u32 = 24;
pub const MCI_MASTERAUDIO: u32 = 25;
pub const MCI_RESUME: u32 = 29;
pub const MCI_PUT: u32 = 44;
pub const MCI_WHERE: u32 = 51;
pub const MCI_WINDOW: u32 = 52;

pub use mci::{MCI_NOTIFY, MCI_WAIT};
pub const MCI_FROM: u32 = 0x4;
pub const MCI_TO: u32 = 0x8;

pub const MCI_TO_START: u32 = 0x100;
pub const MCI_TO_END: u32 = 0x200;

pub const MCI_OPEN_ELEMENT: u32 = 0x100;
pub const MCI_OPEN_ALIAS: u32 = 0x200;
pub const MCI_OPEN_ELEMENT_ID: u32 = 0x400;
pub const MCI_OPEN_PLAYLIST: u32 = 0x800;
pub const MCI_OPEN_TYPE_ID: u32 = 0x1000;
pub const MCI_OPEN_SHAREABLE: u32 = 0x2000;
pub const MCI_OPEN_MMIO: u32 = 0x4000;
pub const MCI_READONLY: u32 = 0x8000;
pub const MCI_DGV_OPEN_PARENT: u32 = 0x0100_0000;

pub const MCI_STATUS_ITEM: u32 = 0x100;
pub const MCI_STATUS_LENGTH: u32 = 2;
pub const MCI_STATUS_MODE: u32 = 3;
pub const MCI_STATUS_NUMBER_OF_TRACKS: u32 = 4;
pub const MCI_STATUS_POSITION: u32 = 5;
pub const MCI_STATUS_MEDIA_PRESENT: u32 = 7;
pub const MCI_STATUS_VOLUME: u32 = 8;
pub const MCI_STATUS_READY: u32 = 9;
pub const MCI_STATUS_TIME_FORMAT: u32 = 10;
pub const MCI_STATUS_SPEED_FORMAT: u32 = 11;
pub const MCI_STATUS_AUDIO: u32 = 13;

pub const MCI_SET_ON: u32 = 0x200;
pub const MCI_SET_OFF: u32 = 0x400;
pub const MCI_SET_VIDEO: u32 = 0x800;
pub const MCI_SET_AUDIO: u32 = 0x1000;
pub const MCI_SET_SPEED_FORMAT: u32 = 0x8000;
pub const MCI_SET_TIME_FORMAT: u32 = 0x0001_0000;
pub const MCI_SET_VOLUME: u32 = 0x0008_0000;

pub const MCI_WAVE_SET_SAMPLESPERSEC: u32 = 0x0100_0000;
pub const MCI_WAVE_SET_AVGBYTESPERSEC: u32 = 0x0200_0000;
pub const MCI_WAVE_SET_BLOCKALIGN: u32 = 0x0400_0000;
pub const MCI_WAVE_SET_FORMATTAG: u32 = 0x0800_0000;
pub const MCI_WAVE_SET_CHANNELS: u32 = 0x1000_0000;
pub const MCI_WAVE_SET_BITSPERSAMPLE: u32 = 0x8000_0000;

pub const MCI_FORMAT_MILLISECONDS: u32 = 1;
pub const MCI_FORMAT_MMTIME: u32 = 2;
pub const MCI_FORMAT_MSF: u32 = 5;
pub const MCI_FORMAT_TMSF: u32 = 6;
pub const MCI_FORMAT_FRAMES: u32 = 8;
pub const MCI_FORMAT_BYTES: u32 = 11;
pub const MCI_FORMAT_SAMPLES: u32 = 12;

pub const MCI_SYSINFO_QUANTITY: u32 = 0x100;
pub const MCI_SYSINFO_OPEN: u32 = 0x200;
pub const MCI_SYSINFO_NAME: u32 = 0x400;
pub const MCI_SYSINFO_INSTALLNAME: u32 = 0x800;
pub const MCI_SYSINFO_ITEM: u32 = 0x1000;

pub const MCI_GETDEVCAPS_MESSAGE: u32 = 0x100;
pub const MCI_GETDEVCAPS_ITEM: u32 = 0x200;
pub const MCI_GETDEVCAPS_CAN_EJECT: u32 = 1;
pub const MCI_GETDEVCAPS_CAN_LOCKEJECT: u32 = 2;
pub const MCI_GETDEVCAPS_CAN_PLAY: u32 = 3;
pub const MCI_GETDEVCAPS_CAN_PROCESS_INTERNAL: u32 = 4;
pub const MCI_GETDEVCAPS_CAN_RECORD: u32 = 5;
pub const MCI_GETDEVCAPS_CAN_RECORD_INSERT: u32 = 6;
pub const MCI_GETDEVCAPS_CAN_SAVE: u32 = 7;
pub const MCI_GETDEVCAPS_CAN_SETVOLUME: u32 = 8;
pub const MCI_GETDEVCAPS_CAN_STREAM: u32 = 9;
pub const MCI_GETDEVCAPS_DEVICE_TYPE: u32 = 10;
pub const MCI_GETDEVCAPS_HAS_AUDIO: u32 = 11;
pub const MCI_GETDEVCAPS_HAS_VIDEO: u32 = 12;
pub const MCI_GETDEVCAPS_USES_FILES: u32 = 15;
pub const MCI_GETDEVCAPS_HAS_IMAGE: u32 = 16;

pub const MCI_INFO_PRODUCT: u32 = 0x100;
pub const MCI_INFO_FILE: u32 = 0x200;

pub const MCI_CUE_INPUT: u32 = 0x100;
pub const MCI_CUE_OUTPUT: u32 = 0x200;

pub const MCI_DGV_PLAY_FAST: u32 = 0x2000;
pub const MCI_DGV_PLAY_SLOW: u32 = 0x1_0000;
pub const MCI_DGV_PLAY_REPEAT: u32 = 0x2_0000;

pub const MCI_VID_PUT_RECT: u32 = 0x100;
pub const MCI_VID_PUT_DESTINATION: u32 = 0x200;
pub const MCI_VID_PUT_SOURCE: u32 = 0x400;
pub const MCI_VID_PUT_WINDOW_MOVE: u32 = 0x800;
pub const MCI_VID_PUT_WINDOW_SIZE: u32 = 0x1000;
pub const MCI_VID_WHERE_DESTINATION: u32 = 0x100;
pub const MCI_VID_WHERE_SOURCE: u32 = 0x200;
pub const MCI_VID_WHERE_WINDOW: u32 = 0x400;
pub const MCI_VID_WINDOW_HWND: u32 = 0x100;
pub const MCI_VID_WINDOW_STATE: u32 = 0x200;
pub const MCI_VID_WINDOW_TEXT: u32 = 0x400;
pub const MCI_VID_WINDOW_DEFAULT: u32 = 0x800;

pub const MCI_VID_STATUS_HWND: u32 = 0x7000;
pub const MCI_VID_STATUS_NORMAL_RATE: u32 = 0x7000 + 10;
pub const MCI_VID_STATUS_VIDEO_X_EXTENT: u32 = 0x7000 + 11;
pub const MCI_VID_STATUS_VIDEO_Y_EXTENT: u32 = 0x7000 + 12;
pub const MCI_VID_GETDEVCAPS_VIDEO_X_EXTENT: u32 = 0x7000 + 2;
pub const MCI_VID_GETDEVCAPS_VIDEO_Y_EXTENT: u32 = 0x7000 + 3;
pub const MCI_VID_GETDEVCAPS_NORMAL_RATE: u32 = 0x7000 + 12;

/// SWP_ values for WINDOW ... STATE.
pub const SWP_SHOW: u32 = 0x8;
pub const SWP_HIDE: u32 = 0x10;
pub const SWP_MINIMIZE: u32 = 0x400;
pub const SWP_MAXIMIZE: u32 = 0x800;
pub const SWP_RESTORE: u32 = 0x1000;
pub const SWP_ACTIVATE: u32 = 0x80;
pub const SWP_DEACTIVATE: u32 = 0x200;

/// A command for a device.
#[derive(Default, Clone)]
pub struct Req {
    pub msg: u32,
    /// The message flags (MCI_WAIT, MCI_NOTIFY and the message's own).
    pub flags: u32,
    pub callback: u32,
    pub user: u16,
    /// MCI_FROM / MCI_TO, MCI_SEEK's position, in the time format.
    pub from: u32,
    pub to: u32,
    /// STATUS, GETDEVCAPS, SYSINFO item; SET's time format.
    pub item: u32,
    pub value: u32,
    pub element: Option<String>,
    /// PUT's rectangle (x, y, cx, cy); WINDOW's handle.
    pub rect: [i32; 4],
    pub hwnd: u32,
    pub text: Option<String>,
    /// MCI_SET of a wave format: tag, channels, rate, avg bytes, block
    /// align, bits.
    pub wave: [u32; 6],
    /// From the string interface (status texts, TRUE/FALSE).
    pub from_string: bool,
}

/// What a command returns.
pub enum Reply {
    None,
    Num(u32),
    /// A value that the string interface shows as a name (STATUS MODE,
    /// TIME FORMAT): the number for mciSendCommand, the name for strings.
    Named(u32, &'static str),
    Bool(bool),
    Text(String),
    Rect([i32; 4]),
}

impl Reply {
    pub fn text(&self) -> String {
        match self {
            Reply::None => String::new(),
            Reply::Num(n) => alloc::format!("{n}"),
            Reply::Named(_, s) => String::from(*s),
            Reply::Bool(b) => mci::bool_text(*b),
            Reply::Text(t) => t.clone(),
            Reply::Rect(r) => alloc::format!("{} {} {} {}", r[0], r[1], r[2], r[3]),
        }
    }

    pub fn num(&self) -> u32 {
        match self {
            Reply::Num(n) | Reply::Named(n, _) => *n,
            Reply::Bool(b) => *b as u32,
            _ => 0,
        }
    }
}

pub fn time_format_name(f: u32) -> &'static str {
    match f {
        MCI_FORMAT_MILLISECONDS => "milliseconds",
        MCI_FORMAT_MMTIME => "mmtime",
        MCI_FORMAT_MSF => "msf",
        MCI_FORMAT_TMSF => "tmsf",
        MCI_FORMAT_FRAMES => "frames",
        MCI_FORMAT_BYTES => "bytes",
        MCI_FORMAT_SAMPLES => "samples",
        _ => "unknown",
    }
}
