//! rs2's multimedia classes, with the names of IBM's MMPARTS.DLL ones so
//! that scripts and programs that name them (MMWAV, MMAudio, ...) find
//! them: the data file classes (MMDataFile and its sound, MIDI, video
//! and image classes), MMFolder, the device objects (MMDevice,
//! MMCDDevice, MMLVDDevice) and MMVolume.
//!
//! The data file classes claim their files by IBM's instance filters and
//! types (their metaclass M_MMDataFile: `*.WAV` and "Digital Audio" for
//! MMWAV, ...), show rs2's multimedia icons and open in rs2's applets in
//! `%MMBASE%` (MPPM, the media player, by default; WEPM, the Digital
//! Audio editor, as a sound's Editor view); the device objects open the
//! CD Player (CDPM), Volume Control (MMVOLUME) and Multimedia Setup
//! (STPM).
//!
//! They load when the shell starts, from the class table (`install.rs`
//! registers them for PMWP, desktop version 13). When IBM's MMPARTS.DLL
//! is registered for these classes instead, the class manager loads
//! IBM's (clsmgr.rs) and rs2's stay unbuilt.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_DEFAULT, OPEN_SETTINGS};
use crate::som;
use crate::util::*;
use crate::wps::{self, Impl};
use pmwin::mmicons as mi;

/// The classes this module provides (the class table's entries).
pub const CLASSES: &[&str] = &[
    "MMDataFile", "MMAudio", "MMWAV", "MMVOC", "MM_AU", "MMAU", "MMAIF", "MMIFF", "MMSNDF", "MMMIDI", "MMVideo", "MMAVI", "MMFLI", "MMMPG", "MMMOV", "MMImage", "MMJPG", "MMGIF", "MMPCX", "MMTIFF", "MMTarga", "MMDIB", "MMPhotoCD", "MMAVC", "MMMMotion", "MMFolder", "MMDevice", "MMCDDevice", "MMLVDDevice", "MMVolume",
];

/// A class of this module once it has been built (0 before: a menu or
/// a check must not build rs2's class where IBM's is registered).
fn built(name: &str) -> usize {
    let cd = wps::cd(name);
    if cd.is_null() { 0 } else { unsafe { *cd } }
}

fn is_a(obj: usize, name: &str) -> bool {
    let c = built(name);
    c != 0 && som::is_a(obj, c)
}

pub fn is_mm_class(name: &[u8]) -> bool {
    CLASSES.iter().any(|c| c.as_bytes().eq_ignore_ascii_case(name))
}

/// (instance filter, instance type) of a data file class, as IBM's
/// classes give them.
fn filter_and_type(name: &[u8]) -> (&'static [u8], &'static [u8]) {
    match name {
        b"MMAudio" => (b"\0", b"Digital Audio\0"),
        b"MMWAV" => (b"*.WAV\0", b"WAV,Digital Audio\0"),
        b"MMVOC" => (b"*.VOC\0", b"VOC\0"),
        b"MM_AU" => (b"*._AU\0", b"_AU\0"),
        b"MMAU" => (b"*.AU\0", b"AU\0"),
        b"MMAIF" => (b"*.AIF,*.AIFF\0", b"AIF\0"),
        b"MMIFF" => (b"*.IFF\0", b"IFF\0"),
        b"MMSNDF" => (b"*.SND\0", b"SND\0"),
        b"MMMIDI" => (b"*.MID,*.RMI,*._MI\0", b"MIDI\0"),
        b"MMVideo" => (b"\0", b"Digital Video\0"),
        b"MMAVI" => (b"*.AVI\0", b"AVI\0"),
        b"MMFLI" => (b"*.FLI,*.FLC\0", b"FLI,FLC\0"),
        b"MMMPG" => (b"*.MPG,*.MPEG\0", b"MPEG\0"),
        b"MMMOV" => (b"*.MOV\0", b"MOV\0"),
        b"MMImage" => (b"\0", b"Image\0"),
        b"MMJPG" => (b"*.JPG,*.JPEG,*.JPE\0", b"JPEG\0"),
        b"MMGIF" => (b"*.GIF\0", b"GIF\0"),
        b"MMPCX" => (b"*.PCX\0", b"PCX\0"),
        b"MMTIFF" => (b"*.TIF,*.TIFF\0", b"TIFF\0"),
        b"MMTarga" => (b"*.TGA\0", b"Targa\0"),
        b"MMDIB" => (b"*.DIB\0", b"DIB\0"),
        b"MMPhotoCD" => (b"*.PCD\0", b"PhotoCD\0"),
        b"MMAVC" => (b"*._IM\0", b"AVC Image\0"),
        b"MMMMotion" => (b"*.VID\0", b"M-Motion Image\0"),
        _ => (b"\0", b"\0"),
    }
}

/// Class titles.
pub fn class_title(name: &[u8]) -> Option<&'static [u8]> {
    Some(match name {
        b"MMDataFile" => b"Multimedia Data File\0",
        b"MMAudio" => b"Digital Audio\0",
        b"MMWAV" => b"Wave Audio\0",
        b"MMVOC" => b"VOC Audio\0",
        b"MM_AU" | b"MMAU" => b"AU Audio\0",
        b"MMAIF" => b"AIFF Audio\0",
        b"MMIFF" => b"IFF Audio\0",
        b"MMSNDF" => b"SND Audio\0",
        b"MMMIDI" => b"MIDI\0",
        b"MMVideo" => b"Digital Video\0",
        b"MMAVI" => b"AVI Movie\0",
        b"MMFLI" => b"FLI/FLC Movie\0",
        b"MMMPG" => b"MPEG Movie\0",
        b"MMMOV" => b"QuickTime Movie\0",
        b"MMImage" => b"Image\0",
        b"MMJPG" => b"JPEG Image\0",
        b"MMGIF" => b"GIF Image\0",
        b"MMPCX" => b"PCX Image\0",
        b"MMTIFF" => b"TIFF Image\0",
        b"MMTarga" => b"Targa Image\0",
        b"MMDIB" => b"DIB Image\0",
        b"MMPhotoCD" => b"Photo CD Image\0",
        b"MMAVC" => b"AVC Image\0",
        b"MMMMotion" => b"M-Motion Image\0",
        b"MMFolder" => b"Multimedia Folder\0",
        b"MMDevice" => b"Multimedia Device\0",
        b"MMCDDevice" => b"CD Player\0",
        b"MMLVDDevice" => b"Videodisc Player\0",
        b"MMVolume" => b"Volume Control\0",
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Media {
    Audio,
    Midi,
    Video,
    Image,
    Other,
}

fn media_of(obj_or_cls: usize, is_class: bool) -> Media {
    let cls = if is_class { obj_or_cls } else { som::class_of(obj_or_cls) };
    let kinds = [("MMAudio", Media::Audio), ("MMMIDI", Media::Midi), ("MMVideo", Media::Video), ("MMImage", Media::Image)];
    for (name, k) in kinds {
        let c = built(name);
        if c != 0 && (cls == c || som::sc_call(som::sc::SOM_DESCENDED_FROM, &[cls, c]) & 0xff != 0) {
            return k;
        }
    }
    Media::Other
}

fn icon_of(m: Media) -> u32 {
    let id = match m {
        Media::Audio => mi::AUDIO,
        Media::Midi => mi::MIDI,
        Media::Video => mi::VIDEO,
        _ => mi::IMAGE,
    };
    icon(id)
}

static mut CACHE: [u32; mi::LAST as usize + 1] = [0; mi::LAST as usize + 1];

/// One of the multimedia icons (made once).
pub fn icon(id: u32) -> u32 {
    if id == 0 || id > mi::LAST {
        return 0;
    }
    let c = unsafe { &mut *(&raw mut CACHE) };
    if c[id as usize] == 0 {
        c[id as usize] = mi::icon(id).map_or(0, |(big, mini)| crate::pm::make_icon(&big, &mini));
    }
    c[id as usize]
}

/// `%MMBASE%\name` (the first directory of MMBASE), else the boot drive's
/// `\MMOS2\name`.
fn applet(name: &[u8]) -> Vec<u8> {
    let mut p: *const u8 = core::ptr::null();
    let mut out = Vec::new();
    if unsafe { os2::DosScanEnv(b"MMBASE\0".as_ptr(), &mut p) } == 0 && !p.is_null() {
        let s = cstr(p);
        out.extend_from_slice(s.split(|&c| c == b';').next().unwrap_or(b""));
    }
    if out.is_empty() {
        out.extend_from_slice(&[crate::install::boot_drive(), b':', b'\\', b'M', b'M', b'O', b'S', b'2']);
    }
    if !out.ends_with(b"\\") {
        out.push(b'\\');
    }
    out.extend_from_slice(name);
    out
}

fn start(prog: &[u8], params: &[u8], title: &[u8]) -> u32 {
    let exe = applet(prog);
    let dir = applet(b"");
    crate::launch::start(&exe, params, &dir[..dir.len() - 1], title, 0)
}

/// The quoted path of a file object.
fn quoted_path(obj: usize) -> Vec<u8> {
    let p = wps::path_of(obj).unwrap_or_default();
    [&b"\""[..], &p, b"\""].concat()
}

// ---- M_MMDataFile ----

extern "C" fn cls_filter(cls: usize) -> usize {
    filter_and_type(som::class_name(cls)).0.as_ptr() as usize
}

extern "C" fn cls_type(cls: usize) -> usize {
    filter_and_type(som::class_name(cls)).1.as_ptr() as usize
}

extern "C" fn cls_icon(cls: usize) -> u32 {
    icon_of(media_of(cls, true))
}

methods! {
    __m_mm_cls_filter => cls_filter, 1;
    __m_mm_cls_type => cls_type, 1;
    __m_mm_cls_icon => cls_icon, 1;
}

pub fn impl_m_data_file() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpclsQueryInstanceFilter", __m_mm_cls_filter as usize),
            ("wpclsQueryInstanceType", __m_mm_cls_type as usize),
            ("wpclsQueryIcon", __m_mm_cls_icon as usize),
        ],
    }
}

// ---- MMDataFile ----

/// The Editor view of a sound (an Open menu item).
pub const MENU_EDIT: u32 = 0x7f10;
/// The Player view (the default).
pub const MENU_PLAY: u32 = 0x7f11;

extern "C" fn df_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("MMDataFile"), wps::cd("WPDataFile"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize]) as u32;
    }
    let title = obj::title(obj);
    let path = quoted_path(obj);
    if view == MENU_EDIT && media_of(obj, false) == Media::Audio {
        start(b"WEPM.EXE", &path, &title);
    } else {
        start(b"MPPM.EXE", &path, &title);
    }
    0
}

extern "C" fn df_icon(obj: usize) -> u32 {
    if obj::d(obj).icon == 0 && ea_read(&wps::path_of(obj).unwrap_or_default(), b".ICON").is_none() {
        return icon_of(media_of(obj, false));
    }
    som::call_parent(wps::ccd("MMDataFile"), wps::cd("WPDataFile"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

extern "C" fn df_menu_selected(obj: usize, frame: u32, id: u32) -> u32 {
    match id {
        MENU_EDIT | MENU_PLAY => {
            df_open(obj, 0, if id == MENU_EDIT { MENU_EDIT } else { OPEN_DEFAULT }, 0);
            1
        }
        _ => som::call_parent(wps::ccd("MMDataFile"), wps::cd("WPDataFile"), m::WPObject::wpMenuItemSelected, &[obj, frame as usize, id as usize]) as u32,
    }
}

methods! {
    __m_mm_df_open => df_open, 4;
    __m_mm_df_icon => df_icon, 1;
    __m_mm_df_menu => df_menu_selected, 3;
}

pub fn impl_data_file() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![("wpOpen", __m_mm_df_open as usize), ("wpQueryIcon", __m_mm_df_icon as usize), ("wpMenuItemSelected", __m_mm_df_menu as usize)],
    }
}

/// The Open submenu's items for a multimedia data file (menu.rs).
pub fn open_items(o: usize) -> Vec<(u16, &'static [u8])> {
    if !is_a(o, "MMDataFile") {
        return Vec::new();
    }
    let mut v = alloc::vec![(MENU_PLAY as u16, if media_of(o, false) == Media::Image { &b"~Viewer"[..] } else { &b"~Player"[..] })];
    if media_of(o, false) == Media::Audio {
        v.push((MENU_EDIT as u16, &b"~Editor"[..]));
    }
    v
}

// ---- MMFolder ----

extern "C" fn folder_icon(obj: usize) -> u32 {
    if obj::d(obj).icon == 0 && ea_read(&wps::path_of(obj).unwrap_or_default(), b".ICON").is_none() {
        return icon(mi::FOLDER);
    }
    som::call_parent(wps::ccd("MMFolder"), wps::cd("WPFolder"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

methods! {
    __m_mm_folder_icon => folder_icon, 1;
}

pub fn impl_folder() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpQueryIcon", __m_mm_folder_icon as usize)] }
}

// ---- The device objects ----

fn device_open(class: &str, obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd(class), wps::cd("WPAbstract"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize]) as u32;
    }
    let title = obj::title(obj);
    if is_a(obj, "MMVolume") {
        start(b"MMVOLUME.EXE", b"", &title)
    } else if is_a(obj, "MMCDDevice") {
        start(b"CDPM.EXE", b"", &title)
    } else {
        start(b"STPM.EXE", b"", &title)
    }
}

fn device_icon(class: &str, obj: usize) -> u32 {
    if obj::d(obj).icon != 0 {
        return som::call_parent(wps::ccd(class), wps::cd("WPAbstract"), m::WPObject::wpQueryIcon, &[obj]) as u32;
    }
    if is_a(obj, "MMVolume") {
        icon(mi::VOLUME)
    } else if is_a(obj, "MMCDDevice") {
        icon(mi::CD_PLAYER)
    } else {
        crate::icons::shell(crate::icons::SOUND)
    }
}

extern "C" fn dev_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    device_open("MMDevice", obj, cnr, view, param)
}

extern "C" fn dev_icon(obj: usize) -> u32 {
    device_icon("MMDevice", obj)
}

extern "C" fn vol_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    device_open("MMVolume", obj, cnr, view, param)
}

extern "C" fn vol_icon(obj: usize) -> u32 {
    device_icon("MMVolume", obj)
}

methods! {
    __m_mm_dev_open => dev_open, 4;
    __m_mm_dev_icon => dev_icon, 1;
    __m_mm_vol_open => vol_open, 4;
    __m_mm_vol_icon => vol_icon, 1;
}

pub fn impl_device() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_mm_dev_open as usize), ("wpQueryIcon", __m_mm_dev_icon as usize)] }
}

pub fn impl_volume() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_mm_vol_open as usize), ("wpQueryIcon", __m_mm_vol_icon as usize)] }
}

/// rs2's multimedia applets' icons (they have no icon resources).
pub fn program_icon(exe: &[u8]) -> Option<u32> {
    let name = exe.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(exe).to_ascii_uppercase();
    let id = match name.as_slice() {
        b"MPPM.EXE" => mi::VIDEO,
        b"WEPM.EXE" => mi::AUDIO,
        b"CDPM.EXE" => mi::CD_PLAYER,
        b"MMVOLUME.EXE" => mi::VOLUME,
        b"STPM.EXE" => mi::FOLDER,
        _ => return None,
    };
    Some(icon(id))
}
