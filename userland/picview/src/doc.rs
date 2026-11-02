//! The picture: a metafile (or a spool file of the PM_Q_STD kind, which
//! is a metafile whose pages are separated by NEWFRAME escapes), split
//! into one metafile per page.

use alloc::vec::Vec;

use ::gpi::goca::{self, Order};
use ::gpi::metafile::{self, Metafile};
use pmkit::*;

/// pmwin's ESC_DEVESCAPE: a device escape recorded on a printer's
/// PM_Q_STD DC; DEVESC_NEWFRAME ends a page.
const ESC_DEVESCAPE: u8 = 0x80;

pub struct Doc {
    /// The file's full name (empty for the clipboard's picture).
    pub path: Vec<u8>,
    /// One metafile handle per page.
    pub pages: Vec<u32>,
    /// The page shown (0-based).
    pub page: usize,
    /// The page's size in its units (PU_*), from the metafile.
    pub units: u32,
    pub size: (i32, i32),
    /// The metafile's default viewing transform (m11 m12 m21 m22 FIXED,
    /// m41 m42), which drawing applies.
    pub view: Option<[i32; 6]>,
    /// Each page's picture bounds in page units (found when first drawn).
    pub bounds: Vec<Option<RectL>>,
}

impl Doc {
    pub fn hmf(&self) -> u32 {
        self.pages.get(self.page).copied().unwrap_or(0)
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        for &h in &self.pages {
            unsafe { GpiDeleteMetaFile(h) };
        }
    }
}

/// Why a file could not be shown (OS/2's messages).
pub enum Error {
    /// "The file could not be opened."
    Open,
    /// "The file is not a valid metafile."
    NotMetafile,
    /// "The metafile contains no graphics."
    Empty,
}

unsafe extern "C" {
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
}

const OD_METAFILE: i32 = 3;

/// A metafile handle holding these bytes: an empty metafile made on a
/// metafile DC, grown to the bytes with GpiSetMetaFileBits (padding the
/// orders with comments so nothing of the empty one is left over).
fn handle_from_bytes(hab: u32, m: &mut Metafile) -> u32 {
    let dop: [u32; 9] = [0; 9];
    let hdc = unsafe { DevOpenDC(hab, OD_METAFILE, b"*\0".as_ptr(), 0, dop.as_ptr(), 0) };
    if hdc == 0 {
        return 0;
    }
    let size = SizeL { cx: 0, cy: 0 };
    let hps = unsafe { GpiCreatePS(hab, hdc, &size, PU_PELS | GPIA_ASSOC) };
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
    }
    let hmf = unsafe { DevCloseDC(hdc) };
    if hmf == 0 {
        return 0;
    }
    let old = unsafe { GpiQueryMetaFileLength(hmf) }.max(0) as usize;
    let mut bytes = metafile::build(m);
    while bytes.len() < old {
        let pad = (old - bytes.len()).clamp(4, 200);
        goca::encode(&Order::Comment(alloc::vec![0; pad]), &mut m.data);
        bytes = metafile::build(m);
    }
    unsafe { GpiSetMetaFileBits(hmf, 0, bytes.len() as i32, bytes.as_ptr()) };
    hmf
}

/// The metafile's pages: its orders cut at each NEWFRAME. A page after
/// the first starts with a segment of its own and the logical fonts,
/// colour table and code page the earlier pages set up.
pub fn split_pages(m: &Metafile) -> Vec<Vec<Order>> {
    let orders = goca::decode(&m.data, m.long);
    let mut pages: Vec<Vec<Order>> = alloc::vec![Vec::new()];
    let mut setup: Vec<Order> = Vec::new();
    for o in orders {
        match &o {
            Order::Escape { ident: ESC_DEVESCAPE, data } if data.len() >= 4 && i32::from_le_bytes([data[0], data[1], data[2], data[3]]) == DEVESC_NEWFRAME => {
                let mut next = alloc::vec![Order::BeginSegment { id: 1, flags: 0, len: 0 }];
                next.extend(setup.iter().cloned());
                pages.push(next);
                continue;
            }
            Order::Escape { ident, .. } if matches!(*ident, goca::ESC_CREATELOGFONT | goca::ESC_CREATELOGCOLORTABLE | goca::ESC_DELETESETID | goca::ESC_SETCP) => setup.push(o.clone()),
            _ => {}
        }
        pages.last_mut().unwrap().push(o);
    }
    // A last page with nothing drawn on it (the NEWFRAME before ENDDOC)
    // is no page.
    while pages.len() > 1 && !pages.last().unwrap().iter().any(draws) {
        pages.pop();
    }
    pages
}

/// Whether an order draws something.
fn draws(o: &Order) -> bool {
    matches!(
        o,
        Order::Line { .. }
            | Order::RelLine { .. }
            | Order::Box { .. }
            | Order::FullArc { .. }
            | Order::PartialArc { .. }
            | Order::Arc { .. }
            | Order::Bezier { .. }
            | Order::Fillet { .. }
            | Order::SharpFillet { .. }
            | Order::Marker { .. }
            | Order::CharString { .. }
            | Order::CharStringExt { .. }
            | Order::BeginImage { .. }
            | Order::SetPel(_)
            | Order::Unknown(_)
    )
}

/// Open a metafile or spool file.
pub fn open(hab: u32, path: &[u8]) -> Result<Doc, Error> {
    let bytes = read_file(path).ok_or(Error::Open)?;
    let m = metafile::parse(&bytes).ok_or(Error::NotMetafile)?;
    from_metafile(hab, path, m, None)
}

/// A document of a metafile (with its handle, if there is one).
pub fn from_metafile(hab: u32, path: &[u8], m: Metafile, hmf: Option<u32>) -> Result<Doc, Error> {
    let pages = split_pages(&m);
    if !pages.iter().any(|p| p.iter().any(draws)) {
        if let Some(h) = hmf {
            unsafe { GpiDeleteMetaFile(h) };
        }
        return Err(Error::Empty);
    }
    let mut handles = Vec::new();
    if pages.len() == 1 {
        let h = match hmf {
            Some(h) => h,
            None if !path.is_empty() => {
                let z = z(path);
                unsafe { GpiLoadMetaFile(hab, z.as_ptr()) }
            }
            None => {
                let mut pm = m.clone();
                handle_from_bytes(hab, &mut pm)
            }
        };
        if h == 0 {
            return Err(Error::NotMetafile);
        }
        handles.push(h);
    } else {
        if let Some(h) = hmf {
            unsafe { GpiDeleteMetaFile(h) };
        }
        for p in &pages {
            let mut pm = m.clone();
            pm.data = goca::encode_all(p);
            // goca::encode writes the long format.
            pm.long = true;
            let h = handle_from_bytes(hab, &mut pm);
            if h == 0 {
                for &h in &handles {
                    unsafe { GpiDeleteMetaFile(h) };
                }
                return Err(Error::NotMetafile);
            }
            handles.push(h);
        }
    }
    let n = handles.len();
    Ok(Doc { path: path.to_vec(), pages: handles, page: 0, units: m.units, size: m.page, view: m.default_view, bounds: alloc::vec![None; n] })
}

/// A document of a metafile handle (the clipboard's copy).
pub fn from_handle(hab: u32, hmf: u32) -> Result<Doc, Error> {
    let n = unsafe { GpiQueryMetaFileLength(hmf) };
    if n <= 0 {
        unsafe { GpiDeleteMetaFile(hmf) };
        return Err(Error::NotMetafile);
    }
    let mut b = alloc::vec![0u8; n as usize];
    unsafe { GpiQueryMetaFileBits(hmf, 0, n, b.as_mut_ptr()) };
    let Some(m) = metafile::parse(&b) else {
        unsafe { GpiDeleteMetaFile(hmf) };
        return Err(Error::NotMetafile);
    };
    from_metafile(hab, b"", m, Some(hmf))
}
