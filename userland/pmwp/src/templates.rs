//! Templates, as in IBM's shell: an object with the TEMPLATE style
//! (setup TEMPLATE=YES, the General page's Template check box) makes new
//! objects. Opening it makes one beside it, dragging it to a folder makes
//! one there, and Create another... (any object) makes one where the
//! user says. The new object is a copy of the template (a file system
//! template's file or directory with its EAs; an abstract object's
//! state) without the TEMPLATE style; its title is edited in place when
//! its folder is open. The Templates folder (`<WP_TEMPS>`) holds a Data
//! File, a Folder and a Program template from the first start on. Views
//! show a template as its icon on a notepad (PMWIN's Rs2TemplateIcon).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::fileops::{self, join};
use crate::obj::{self, OBJSTYLE_TEMPLATE};
use crate::util::*;
use crate::{view, wps};

pub fn is_template(o: usize) -> bool {
    o != 0 && obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & OBJSTYLE_TEMPLATE != 0
}

/// A new object from `o` in `folder`, titled `title` (else the
/// original's title); not a template. Returns it (0 if it could not be
/// made).
pub fn create_from(o: usize, folder: usize, title: Option<&[u8]>) -> usize {
    if folder == 0 {
        return 0;
    }
    let t = title.filter(|t| !t.is_empty()).map(|t| t.to_vec()).unwrap_or_else(|| obj::title(o));
    let made = match (wps::path_of(o), wps::path_of(folder)) {
        (Some(src), Some(dir)) if src.len() > 3 => {
            // A name for the copy (FAT: a short one, the title in
            // .LONGNAME).
            let clean: Vec<u8> = t.iter().map(|&c| if c == b'\n' || c == b'\r' { b' ' } else if b"\\/:*?\"<>|".contains(&c) { b'_' } else { c }).collect();
            let long = fileops::long_names(&dir);
            let name = if long { pmwin::drag::unique_name(&clean, true, |n| fileops::exists(&join(&dir, n))) } else { crate::fs::short_name(&dir, &clean) };
            let dst = join(&dir, &name);
            let rc = fileops::copy_tree(&src, &dst, false, &mut |_, _| true);
            if rc != 0 {
                debug(&alloc::format!("template: copying {:?} to {:?}: error {}", core::str::from_utf8(&src), core::str::from_utf8(&dst), rc));
                return 0;
            }
            wps::drop_copied_id(&dst);
            if !long || name != clean {
                ea_write(&dst, b".LONGNAME", Some((EAT_ASCII, &t)));
            }
            let n = wps::object_from_path(&dst);
            if n != 0 {
                obj::d(n).folder = folder;
                wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, n]);
            }
            n
        }
        _ => {
            let n = obj::call(m::WPObject::wpCopyObject, &[o, folder, 0]);
            if n != 0 && title.is_some() {
                let zt = z(&t);
                obj::call(m::WPObject::wpSetTitle, &[n, zt.as_ptr() as usize]);
            }
            n
        }
    };
    if made == 0 {
        return 0;
    }
    let style = obj::call(m::WPObject::wpQueryStyle, &[made]) as u32;
    if style & OBJSTYLE_TEMPLATE != 0 {
        obj::call(m::WPObject::wpSetStyle, &[made, (style & !OBJSTYLE_TEMPLATE) as usize]);
    }
    obj::call(m::WPObject::wpSaveImmediate, &[made]);
    // Its title, edited where the folder is open.
    if let Some(w) = view::wins().iter().position(|w| w.obj == folder && w.view == obj::OPEN_CONTENTS) {
        view::rename(w, made);
    }
    made
}

unsafe extern "C" {
    #[link_name = "PMWIN.9008"]
    fn Rs2TemplateIcon(h: u32) -> u32;
}

/// Template icons made so far: (icon, its template look).
static mut LOOKS: Vec<(u32, u32)> = Vec::new();

/// The icon a view shows for an object: a template's on a notepad.
pub fn view_icon(o: usize) -> u32 {
    let icon = obj::call(m::WPObject::wpQueryIcon, &[o]) as u32;
    if icon == 0 || !is_template(o) {
        return icon;
    }
    #[allow(static_mut_refs)]
    let looks = unsafe { &mut LOOKS };
    if let Some(&(_, t)) = looks.iter().find(|l| l.0 == icon) {
        return t;
    }
    let t = unsafe { Rs2TemplateIcon(icon) };
    if t == 0 {
        return icon;
    }
    looks.push((icon, t));
    t
}
