//! Layout: ODFrame, ODFacet, ODBaseShape/ODShape, ODBaseTransform/
//! ODTransform, ODBaseCanvas/ODCanvas, ODPlatformCanvas and the facet
//! iterators. Coordinates are 16.16 fixed point, y up (OS/2).

use alloc::vec::Vec;

use crate::obj::{acquire, release};
use crate::rt::*;
use crate::storage::{self, new_of};
use crate::*;

// ---- Transforms: p' = p * scale + offset ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Xf {
    pub sx: i32,
    pub sy: i32,
    pub ox: i32,
    pub oy: i32,
}

impl Default for Xf {
    fn default() -> Xf {
        Xf { sx: ONE, sy: ONE, ox: 0, oy: 0 }
    }
}

impl Xf {
    pub fn apply(&self, p: Point) -> Point {
        Point { x: fmul(p.x, self.sx) + self.ox, y: fmul(p.y, self.sy) + self.oy }
    }
    pub fn invert(&self, p: Point) -> Point {
        Point { x: fdiv(p.x - self.ox, self.sx), y: fdiv(p.y - self.oy, self.sy) }
    }
    /// `self` first, then `t`.
    pub fn then(&self, t: &Xf) -> Xf {
        Xf { sx: fmul(self.sx, t.sx), sy: fmul(self.sy, t.sy), ox: fmul(self.ox, t.sx) + t.ox, oy: fmul(self.oy, t.sy) + t.oy }
    }
    pub fn inverse(&self) -> Xf {
        let sx = fdiv(ONE, self.sx);
        let sy = fdiv(ONE, self.sy);
        Xf { sx, sy, ox: -fmul(self.ox, sx), oy: -fmul(self.oy, sy) }
    }
    pub fn kind(&self) -> usize {
        // kODIdentityXform 0, kODTranslateXform 1, kODScaleXform 2, kODScaleTranslateXform 3
        let scaled = self.sx != ONE || self.sy != ONE;
        let moved = self.ox != 0 || self.oy != 0;
        (moved as usize) | ((scaled as usize) << 1)
    }
    pub fn rect(&self, r: Rect) -> Rect {
        let a = self.apply(Point { x: r.left, y: r.bottom });
        let b = self.apply(Point { x: r.right, y: r.top });
        Rect { left: a.x, bottom: a.y, right: b.x, top: b.y }.normalized()
    }
}

pub fn xf(this: usize) -> &'static mut Xf {
    state(cd!(ODBaseTransformCClassData), this)
}

/// A new ODTransform holding `x` (count 1).
pub fn new_transform(x: Xf) -> usize {
    let t = new_of(cd!(ODTransformClassData), "ODTransform");
    if t != 0 {
        *xf(t) = x;
    }
    t
}

/// The transform an ODTransform argument holds (identity for none).
pub fn xf_of(t: usize) -> Xf {
    if t == 0 { Xf::default() } else { *xf(t) }
}

methods! {
    fn XGetType(this: usize, ev: usize) -> usize { xf(this).kind() }
    fn GetOffset(this: usize, ev: usize, p: usize) -> usize {
        write(p, Point { x: xf(this).ox, y: xf(this).oy });
        0
    }
    fn GetPreScaleOffset(this: usize, ev: usize, p: usize) -> usize {
        let x = xf(this);
        write(p, Point { x: fdiv(x.ox, x.sx), y: fdiv(x.oy, x.sy) });
        0
    }
    fn GetScale(this: usize, ev: usize, p: usize) -> usize {
        write(p, Point { x: xf(this).sx, y: xf(this).sy });
        0
    }
    fn GetMatrix(this: usize, ev: usize, m: usize) -> usize {
        let x = *xf(this);
        // ODMatrix: 3x3, the last column ODFract (2.30).
        write::<[i32; 9]>(m, [x.sx, 0, 0, 0, x.sy, 0, x.ox, x.oy, 0x4000_0000]);
        0
    }
    fn HasMatrix(this: usize, ev: usize) -> usize { 1 }
    fn XIsSameAs(this: usize, ev: usize, t: usize) -> usize { (*xf(this) == xf_of(t)) as usize }
    fn XReset(this: usize, ev: usize) -> usize {
        *xf(this) = Xf::default();
        this
    }
    fn SetMatrix(this: usize, ev: usize, m: usize) -> usize {
        if m != 0 {
            let a = read::<[i32; 9]>(m);
            *xf(this) = Xf { sx: a[0], sy: a[4], ox: a[6], oy: a[7] };
        }
        this
    }
    fn XCopy(this: usize, ev: usize) -> usize { new_transform(*xf(this)) }
    fn XCopyFrom(this: usize, ev: usize, t: usize) -> usize {
        *xf(this) = xf_of(t);
        this
    }
    fn SetOffset(this: usize, ev: usize, p: usize) -> usize {
        let p = read::<Point>(p);
        let x = xf(this);
        x.ox = p.x;
        x.oy = p.y;
        this
    }
    fn MoveBy(this: usize, ev: usize, p: usize) -> usize {
        let p = read::<Point>(p);
        let x = xf(this);
        x.ox += p.x;
        x.oy += p.y;
        this
    }
    fn ScaleBy(this: usize, ev: usize, p: usize) -> usize {
        let s = read::<Point>(p);
        let x = xf(this);
        *x = x.then(&Xf { sx: s.x, sy: s.y, ox: 0, oy: 0 });
        this
    }
    fn ScaleDownBy(this: usize, ev: usize, p: usize) -> usize {
        let s = read::<Point>(p);
        let x = xf(this);
        *x = x.then(&Xf { sx: fdiv(ONE, s.x), sy: fdiv(ONE, s.y), ox: 0, oy: 0 });
        this
    }
    fn Invert(this: usize, ev: usize) -> usize {
        let x = xf(this);
        *x = x.inverse();
        this
    }
    fn PreCompose(this: usize, ev: usize, t: usize) -> usize {
        let x = xf(this);
        *x = xf_of(t).then(x);
        this
    }
    fn PostCompose(this: usize, ev: usize, t: usize) -> usize {
        let x = xf(this);
        *x = x.then(&xf_of(t));
        this
    }
    fn TransformPoint(this: usize, ev: usize, p: usize) -> usize {
        if p != 0 {
            write(p, xf(this).apply(read(p)));
        }
        0
    }
    fn InvertPoint(this: usize, ev: usize, p: usize) -> usize {
        if p != 0 {
            write(p, xf(this).invert(read(p)));
        }
        0
    }
    fn TransformShape(this: usize, ev: usize, s: usize) -> usize {
        if s != 0 {
            let sh = shape(s);
            sh.rect = xf(this).rect(sh.rect);
        }
        0
    }
    fn InvertShape(this: usize, ev: usize, s: usize) -> usize {
        if s != 0 {
            let sh = shape(s);
            sh.rect = xf(this).inverse().rect(sh.rect);
        }
        0
    }
    fn NewTransform(this: usize, ev: usize) -> usize { new_transform(Xf::default()) }
    fn XSomUninit(this: usize) -> usize {
        drop_state::<Xf>(cd!(ODBaseTransformCClassData), this);
        call_parent(cd!(ODBaseTransformCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    // ODTransform (OS/2)
    fn InitTransform(this: usize, ev: usize) -> usize {
        *xf(this) = Xf::default();
        0
    }
    fn GetMATRIXLF(this: usize, ev: usize, m: usize) -> usize {
        let x = *xf(this);
        // MATRIXLF: fxM11 fxM12 lM13 fxM21 fxM22 lM23 lM31 lM32 lM33.
        write::<[i32; 9]>(m, [x.sx, 0, 0, 0, x.sy, 0, (x.ox + 0x8000) >> 16, (x.oy + 0x8000) >> 16, 1]);
        0
    }
    fn SetMATRIXLF(this: usize, ev: usize, m: usize) -> usize {
        if m != 0 {
            let a = read::<[i32; 9]>(m);
            *xf(this) = Xf { sx: a[0], sy: a[4], ox: a[6] << 16, oy: a[7] << 16 };
        }
        this
    }
}

pub static BASE_TRANSFORM_M: &[(&str, M)] = &[
    ("GetType", XGetType::t),
    ("GetOffset", GetOffset::t),
    ("GetPreScaleOffset", GetPreScaleOffset::t),
    ("GetScale", GetScale::t),
    ("GetMatrix", GetMatrix::t),
    ("HasMatrix", HasMatrix::t),
    ("IsSameAs", XIsSameAs::t),
    ("Reset", XReset::t),
    ("SetMatrix", SetMatrix::t),
    ("Copy", XCopy::t),
    ("CopyFrom", XCopyFrom::t),
    ("SetOffset", SetOffset::t),
    ("MoveBy", MoveBy::t),
    ("ScaleBy", ScaleBy::t),
    ("ScaleDownBy", ScaleDownBy::t),
    ("Invert", Invert::t),
    ("PreCompose", PreCompose::t),
    ("PostCompose", PostCompose::t),
    ("TransformPoint", TransformPoint::t),
    ("InvertPoint", InvertPoint::t),
    ("TransformShape", TransformShape::t),
    ("InvertShape", InvertShape::t),
    ("NewTransform", NewTransform::t),
];
pub static BASE_TRANSFORM_O: &[(&str, M)] = &[("SOMObject::somUninit", XSomUninit::t)];
pub static TRANSFORM_M: &[(&str, M)] =
    &[("InitTransform", InitTransform::t), ("GetMATRIXLF", GetMATRIXLF::t), ("SetMATRIXLF", SetMATRIXLF::t)];

// ---- Shapes: rectangles ----

#[derive(Default)]
pub struct ShapeState {
    pub rect: Rect,
    pub mode: u32,
    pub region: u32,
}

pub fn shape(this: usize) -> &'static mut ShapeState {
    state(cd!(ODBaseShapeCClassData), this)
}

pub fn new_shape(r: Rect) -> usize {
    let s = new_of(cd!(ODShapeClassData), "ODShape");
    if s != 0 {
        shape(s).rect = r.normalized();
    }
    s
}

pub fn rect_of(s: usize) -> Rect {
    if s == 0 { Rect::default() } else { shape(s).rect }
}

methods! {
    fn GetGeometryMode(this: usize, ev: usize) -> usize { shape(this).mode as usize }
    fn SetGeometryMode(this: usize, ev: usize, m: usize) -> usize {
        shape(this).mode = m as u32;
        0
    }
    fn GetBoundingBox(this: usize, ev: usize, r: usize) -> usize {
        write(r, shape(this).rect);
        0
    }
    fn SetRectangle(this: usize, ev: usize, r: usize) -> usize {
        shape(this).rect = if r == 0 { Rect::default() } else { read::<Rect>(r).normalized() };
        this
    }
    fn CopyPolygon(this: usize, ev: usize, out: usize) -> usize { 0 }
    fn SetPolygon(this: usize, ev: usize, poly: usize) -> usize { this }
    fn GetPlatformShape(this: usize, ev: usize, g: usize) -> usize { shape(this).region as usize }
    fn SetPlatformShape(this: usize, ev: usize, g: usize, s: usize) -> usize {
        shape(this).region = s as u32;
        0
    }
    fn SReset(this: usize, ev: usize) -> usize {
        shape(this).rect = Rect::default();
        0
    }
    fn SIsSameAs(this: usize, ev: usize, s: usize) -> usize { (shape(this).rect == rect_of(s)) as usize }
    fn IsEmpty(this: usize, ev: usize) -> usize { shape(this).rect.is_empty() as usize }
    fn ContainsPoint(this: usize, ev: usize, p: usize) -> usize { (p != 0 && shape(this).rect.contains(read(p))) as usize }
    fn IsRectangular(this: usize, ev: usize) -> usize { 1 }
    fn HasGeometry(this: usize, ev: usize) -> usize { 1 }
    fn NewShape(this: usize, ev: usize) -> usize { new_shape(Rect::default()) }
    fn SCopy(this: usize, ev: usize) -> usize { new_shape(shape(this).rect) }
    fn SCopyFrom(this: usize, ev: usize, s: usize) -> usize {
        shape(this).rect = rect_of(s);
        this
    }
    fn STransform(this: usize, ev: usize, t: usize) -> usize {
        let s = shape(this);
        s.rect = xf_of(t).rect(s.rect);
        this
    }
    fn SInverseTransform(this: usize, ev: usize, t: usize) -> usize {
        let s = shape(this);
        s.rect = xf_of(t).inverse().rect(s.rect);
        this
    }
    fn Subtract(this: usize, ev: usize, o: usize) -> usize {
        // Rectangles only: what is left when the other covers a whole side.
        let s = shape(this);
        let o = rect_of(o);
        let i = s.rect.intersect(&o);
        if i == s.rect {
            s.rect = Rect::default();
        } else if !i.is_empty() {
            let r = &mut s.rect;
            if i.left == r.left && i.right == r.right {
                if i.top == r.top { r.top = i.bottom } else if i.bottom == r.bottom { r.bottom = i.top }
            } else if i.top == r.top && i.bottom == r.bottom {
                if i.left == r.left { r.left = i.right } else if i.right == r.right { r.right = i.left }
            }
        }
        this
    }
    fn Intersect(this: usize, ev: usize, o: usize) -> usize {
        let s = shape(this);
        s.rect = s.rect.intersect(&rect_of(o));
        this
    }
    fn Union(this: usize, ev: usize, o: usize) -> usize {
        let s = shape(this);
        s.rect = s.rect.union(&rect_of(o));
        this
    }
    fn Outset(this: usize, ev: usize, d: usize) -> usize {
        let d = d as i32;
        let r = &mut shape(this).rect;
        *r = Rect { left: r.left - d, right: r.right + d, top: r.top + d, bottom: r.bottom - d }.normalized();
        this
    }
    fn SSomUninit(this: usize) -> usize {
        drop_state::<ShapeState>(cd!(ODBaseShapeCClassData), this);
        call_parent(cd!(ODBaseShapeCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    // ODShape (OS/2): the shape as a GPI region (rs2 does not make one).
    fn InitShape(this: usize, ev: usize) -> usize { 0 }
    fn GetRegion(this: usize, ev: usize) -> usize { shape(this).region as usize }
    fn SetRegion(this: usize, ev: usize, r: usize) -> usize {
        shape(this).region = r as u32;
        this
    }
    fn CopyRegion(this: usize, ev: usize) -> usize { 0 }
}

pub static BASE_SHAPE_M: &[(&str, M)] = &[
    ("GetGeometryMode", GetGeometryMode::t),
    ("SetGeometryMode", SetGeometryMode::t),
    ("GetBoundingBox", GetBoundingBox::t),
    ("SetRectangle", SetRectangle::t),
    ("CopyPolygon", CopyPolygon::t),
    ("SetPolygon", SetPolygon::t),
    ("GetPlatformShape", GetPlatformShape::t),
    ("SetPlatformShape", SetPlatformShape::t),
    ("Reset", SReset::t),
    ("IsSameAs", SIsSameAs::t),
    ("IsEmpty", IsEmpty::t),
    ("ContainsPoint", ContainsPoint::t),
    ("IsRectangular", IsRectangular::t),
    ("HasGeometry", HasGeometry::t),
    ("NewShape", NewShape::t),
    ("Copy", SCopy::t),
    ("CopyFrom", SCopyFrom::t),
    ("Transform", STransform::t),
    ("InverseTransform", SInverseTransform::t),
    ("Subtract", Subtract::t),
    ("Intersect", Intersect::t),
    ("Union", Union::t),
    ("Outset", Outset::t),
];
pub static BASE_SHAPE_O: &[(&str, M)] = &[("SOMObject::somUninit", SSomUninit::t)];
pub static SHAPE_M: &[(&str, M)] =
    &[("InitShape", InitShape::t), ("GetRegion", GetRegion::t), ("SetRegion", SetRegion::t), ("CopyRegion", CopyRegion::t)];

// ---- Canvases ----

#[derive(Default)]
pub struct CanvasState {
    pub platform: usize,
    pub print_job: usize,
    pub owner: usize,
    pub facet: usize,
    pub dynamic: bool,
    pub offscreen: bool,
}

pub fn canvas(this: usize) -> &'static mut CanvasState {
    state(cd!(ODBaseCanvasCClassData), this)
}

#[derive(Default)]
pub struct PlatformCanvasState {
    pub hwnd: u32,
    pub hps: u32,
}

pub fn pcanvas(this: usize) -> &'static mut PlatformCanvasState {
    state(cd!(ODPlatformCanvasCClassData), this)
}

methods! {
    fn HasPlatformCanvas(this: usize, ev: usize, g: usize) -> usize { (canvas(this).platform != 0) as usize }
    fn GetPlatformCanvas(this: usize, ev: usize, g: usize) -> usize { canvas(this).platform }
    fn SetPlatformCanvas(this: usize, ev: usize, g: usize, c: usize) -> usize {
        canvas(this).platform = c;
        0
    }
    fn HasPlatformPrintJob(this: usize, ev: usize, g: usize) -> usize { (canvas(this).print_job != 0) as usize }
    fn GetPlatformPrintJob(this: usize, ev: usize, g: usize) -> usize { canvas(this).print_job }
    fn SetPlatformPrintJob(this: usize, ev: usize, g: usize, j: usize) -> usize {
        canvas(this).print_job = j;
        0
    }
    fn AcquireOwner(this: usize, ev: usize) -> usize { acquire(canvas(this).owner, ev) }
    fn SetOwner(this: usize, ev: usize, o: usize) -> usize {
        canvas(this).owner = o;
        0
    }
    fn CGetFacet(this: usize, ev: usize) -> usize { canvas(this).facet }
    fn CSetFacet(this: usize, ev: usize, f: usize) -> usize {
        canvas(this).facet = f;
        0
    }
    fn IsDynamic(this: usize, ev: usize) -> usize { canvas(this).dynamic as usize }
    fn IsOffscreen(this: usize, ev: usize) -> usize { canvas(this).offscreen as usize }
    fn AcquireUpdateShape(this: usize, ev: usize) -> usize { new_shape(Rect::default()) }
    fn ResetUpdateShape(this: usize, ev: usize) -> usize { 0 }
    fn CInvalidate(this: usize, ev: usize, s: usize) -> usize { 0 }
    fn CValidate(this: usize, ev: usize, s: usize) -> usize { 0 }
    fn CanvasSomUninit(this: usize) -> usize {
        drop_state::<CanvasState>(cd!(ODBaseCanvasCClassData), this);
        call_parent(cd!(ODBaseCanvasCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    fn InitCanvas(this: usize, ev: usize, g: usize, platform: usize, dynamic: usize, offscreen: usize) -> usize {
        let c = canvas(this);
        c.platform = platform;
        c.dynamic = dynamic & 0xff != 0;
        c.offscreen = offscreen & 0xff != 0;
        0
    }
    // ODPlatformCanvas (OS/2): a window's presentation space.
    fn InitPlatformCanvas(this: usize, ev: usize, hwnd: usize, hps: usize) -> usize {
        let p = pcanvas(this);
        p.hwnd = hwnd as u32;
        p.hps = hps as u32;
        0
    }
    fn GetPS(this: usize, ev: usize, facet: usize) -> usize {
        let p = pcanvas(this);
        if p.hps == 0 && p.hwnd != 0 {
            p.hps = unsafe { crate::services::WinGetPS(p.hwnd) };
        }
        p.hps as usize
    }
    fn ReleasePS(this: usize, ev: usize, facet: usize) -> usize { 0 }
    fn GetHWND(this: usize, ev: usize) -> usize { pcanvas(this).hwnd as usize }
    fn PcSomUninit(this: usize) -> usize {
        drop_state::<PlatformCanvasState>(cd!(ODPlatformCanvasCClassData), this);
        call_parent(cd!(ODPlatformCanvasCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static BASE_CANVAS_M: &[(&str, M)] = &[
    ("HasPlatformCanvas", HasPlatformCanvas::t),
    ("GetPlatformCanvas", GetPlatformCanvas::t),
    ("SetPlatformCanvas", SetPlatformCanvas::t),
    ("HasPlatformPrintJob", HasPlatformPrintJob::t),
    ("GetPlatformPrintJob", GetPlatformPrintJob::t),
    ("SetPlatformPrintJob", SetPlatformPrintJob::t),
    ("AcquireOwner", AcquireOwner::t),
    ("SetOwner", SetOwner::t),
    ("GetFacet", CGetFacet::t),
    ("SetFacet", CSetFacet::t),
    ("IsDynamic", IsDynamic::t),
    ("IsOffscreen", IsOffscreen::t),
    ("AcquireUpdateShape", AcquireUpdateShape::t),
    ("ResetUpdateShape", ResetUpdateShape::t),
    ("Invalidate", CInvalidate::t),
    ("Validate", CValidate::t),
];
pub static BASE_CANVAS_O: &[(&str, M)] = &[("SOMObject::somUninit", CanvasSomUninit::t)];
pub static CANVAS_M: &[(&str, M)] = &[("InitCanvas", InitCanvas::t)];
pub static PLATFORM_CANVAS_M: &[(&str, M)] = &[
    ("InitPlatformCanvas", InitPlatformCanvas::t),
    ("GetPS", GetPS::t),
    ("ReleasePS", ReleasePS::t),
    ("GetHWND", GetHWND::t),
];
pub static PLATFORM_CANVAS_O: &[(&str, M)] = &[("SOMObject::somUninit", PcSomUninit::t)];

// ---- ODFrame ----

#[derive(Default)]
pub struct FrameState {
    pub draft: usize,
    pub containing: usize,
    pub containing_id: u32,
    pub window: usize,
    pub part: usize,
    pub part_id: u32,
    pub part_info: usize,
    pub view: usize,
    pub pres: usize,
    pub subframe: bool,
    pub overlaid: bool,
    pub frozen: bool,
    pub propagate: bool,
    pub limbo: bool,
    pub droppable: bool,
    pub dragging: bool,
    pub frame_shape: Rect,
    pub used_shape: Option<Rect>,
    pub internal: Xf,
    pub extent: Point,
    pub facets: Vec<usize>,
    pub seq: u32,
    pub group: u32,
    pub link_status: u32,
}

pub fn frame(this: usize) -> &'static mut FrameState {
    state(cd!(ODFrameCClassData), this)
}

pub fn frame_id(f: usize) -> u32 {
    let su = crate::obj::su_of(f);
    if su == 0 { 0 } else { storage::su_id(su) }
}

pub mod frame_ix {
    pub const ACQUIRE_PART: usize = 18;
    pub const GET_VIEW_TYPE: usize = 22;
    pub const FACET_ADDED: usize = 28;
    pub const CREATE_FACET_ITERATOR: usize = 30;
    pub const ACQUIRE_FRAME_SHAPE: usize = 32;
}

const P_FRAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:Frame:";

fn fprop(name: &[u8]) -> Vec<u8> {
    let mut v = P_FRAME.to_vec();
    v.extend_from_slice(name);
    v
}

fn set_raw(su: usize, prop: &[u8], data: &[u8]) {
    let s = storage::sus(su);
    let v = storage::Value { ty: b"rs2:Bytes".to_vec(), data: data.to_vec() };
    match s.props.iter_mut().find(|p| p.name == prop) {
        Some(p) => p.vals = alloc::vec![v],
        None => s.props.push(storage::Prop { name: prop.to_vec(), vals: alloc::vec![v] }),
    }
}

fn get_raw(su: usize, prop: &[u8]) -> Option<Vec<u8>> {
    let s = storage::sus(su);
    Some(s.props.iter().find(|p| p.name == prop)?.vals.first()?.data.clone())
}

/// Make a new frame for a draft (CreateFrame).
#[allow(clippy::too_many_arguments)]
pub fn new_frame(dr: usize, ev: usize, su: usize, containing: usize, shp: usize, part: usize, view: usize, pres: usize, sub: bool, overlaid: bool) -> usize {
    let f = new_of(cd!(ODFrameClassData), "ODFrame");
    if f == 0 {
        od_error(ev, err::CANNOT_CREATE_FRAME);
        return 0;
    }
    crate::obj::set_su(f, acquire(su, ev));
    let s = frame(f);
    s.draft = dr;
    s.containing = acquire(containing, ev);
    s.part = acquire(part, ev);
    s.view = view;
    s.pres = pres;
    s.subframe = sub;
    s.overlaid = overlaid;
    s.frame_shape = rect_of(shp);
    s.propagate = false;
    storage::set_value(su, storage::PROP_OBJECT_TYPE, storage::TYPE_ISO_STR, storage::OBJ_FRAME);
    storage::draft(dr).frames.insert(storage::su_id(su), f);
    if part != 0 {
        call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_ADDED, &[part, ev, f]);
    }
    f
}

/// A frame read back from its storage unit (AcquireFrame).
pub fn frame_from_storage(dr: usize, ev: usize, su: usize) -> usize {
    let f = new_of(cd!(ODFrameClassData), "ODFrame");
    if f == 0 {
        return 0;
    }
    crate::obj::set_su(f, acquire(su, ev));
    storage::draft(dr).frames.insert(storage::su_id(su), f);
    let s = frame(f);
    s.draft = dr;
    s.part_id = get_raw(su, &fprop(b"Part")).map_or(0, |b| bytes_u32(&b));
    s.containing_id = get_raw(su, &fprop(b"ContainingFrame")).map_or(0, |b| bytes_u32(&b) & 0x7fff_ffff);
    let sess = crate::services::session();
    if let Some(v) = storage::get_value(su, &fprop(b"ViewType")) {
        s.view = crate::services::tokenize(sess, &v);
    }
    if let Some(v) = storage::get_value(su, &fprop(b"Presentation")) {
        s.pres = crate::services::tokenize(sess, &v);
    }
    if let Some(b) = get_raw(su, &fprop(b"FrameShape")) {
        if b.len() >= 16 {
            let w = |i: usize| bytes_u32(&b[i..]) as i32;
            s.frame_shape = Rect { left: w(0), top: w(4), right: w(8), bottom: w(12) };
        }
    }
    if let Some(b) = get_raw(su, &fprop(b"Flags")) {
        let fl = bytes_u32(&b);
        s.subframe = fl & 1 != 0;
        s.overlaid = fl & 2 != 0;
        s.frozen = fl & 4 != 0;
    }
    f
}

/// What a frame holds goes when its draft closes.
pub fn frame_close(f: usize) {
    let s = frame(f);
    s.facets.clear();
    s.part = 0;
    s.containing = 0;
}

fn frame_part(this: usize, ev: usize) -> usize {
    let s = frame(this);
    if s.part == 0 && s.part_id != 0 {
        let p = call(cd!(ODDraftClassData), storage::draft_ix::ACQUIRE_PART, &[s.draft, ev, s.part_id as usize]);
        frame(this).part = p;
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_CONNECTED, &[p, ev, this]);
        }
    }
    frame(this).part
}

fn type_name(token: usize) -> Vec<u8> {
    crate::services::type_of(crate::services::session(), token).unwrap_or_default()
}

methods! {
    fn AcquireContainingFrame(this: usize, ev: usize) -> usize {
        let s = frame(this);
        if s.containing == 0 && s.containing_id != 0 {
            s.containing = call(cd!(ODDraftClassData), storage::draft_ix::ACQUIRE_FRAME, &[s.draft, ev, s.containing_id as usize]);
            return acquire(frame(this).containing, ev);
        }
        acquire(s.containing, ev)
    }
    fn SetContainingFrame(this: usize, ev: usize, f: usize) -> usize {
        frame(this).containing = f;
        0
    }
    fn FAcquireWindow(this: usize, ev: usize) -> usize {
        let s = frame(this);
        if s.window == 0 && s.containing != 0 {
            return FAcquireWindow(s.containing, ev);
        }
        acquire(s.window, ev)
    }
    fn FSetWindow(this: usize, ev: usize, w: usize) -> usize {
        frame(this).window = w;
        0
    }
    fn GetFrameGroup(this: usize, ev: usize) -> usize { frame(this).group as usize }
    fn SetFrameGroup(this: usize, ev: usize, g: usize) -> usize {
        frame(this).group = g as u32;
        0
    }
    fn GetSequenceNumber(this: usize, ev: usize) -> usize { frame(this).seq as usize }
    fn ChangeSequenceNumber(this: usize, ev: usize, n: usize) -> usize {
        frame(this).seq = n as u32;
        0
    }
    fn IsRoot(this: usize, ev: usize) -> usize { (frame(this).containing == 0 && frame(this).containing_id == 0) as usize }
    fn IsSubframe(this: usize, ev: usize) -> usize { frame(this).subframe as usize }
    fn SetSubframe(this: usize, ev: usize, b: usize) -> usize {
        frame(this).subframe = b & 0xff != 0;
        0
    }
    fn IsOverlaid(this: usize, ev: usize) -> usize { frame(this).overlaid as usize }
    fn IsFrozen(this: usize, ev: usize) -> usize { frame(this).frozen as usize }
    fn SetFrozen(this: usize, ev: usize, b: usize) -> usize {
        frame(this).frozen = b & 0xff != 0;
        0
    }
    fn DoesPropagateEvents(this: usize, ev: usize) -> usize { frame(this).propagate as usize }
    fn SetPropagateEvents(this: usize, ev: usize, b: usize) -> usize {
        frame(this).propagate = b & 0xff != 0;
        0
    }
    fn IsInLimbo(this: usize, ev: usize) -> usize { frame(this).limbo as usize }
    fn SetInLimbo(this: usize, ev: usize, b: usize) -> usize {
        frame(this).limbo = b & 0xff != 0;
        0
    }
    fn FAcquirePart(this: usize, ev: usize) -> usize { acquire(frame_part(this, ev), ev) }
    fn ChangePart(this: usize, ev: usize, p: usize) -> usize {
        let old = frame(this).part;
        frame(this).part = acquire(p, ev);
        if old != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_REMOVED, &[old, ev, this]);
            release(old, ev);
        }
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_ADDED, &[p, ev, this]);
        }
        0
    }
    fn FGetPartInfo(this: usize, ev: usize) -> usize { frame(this).part_info }
    fn FSetPartInfo(this: usize, ev: usize, i: usize) -> usize {
        frame(this).part_info = i;
        0
    }
    fn GetViewType(this: usize, ev: usize) -> usize { frame(this).view }
    fn SetViewType(this: usize, ev: usize, v: usize) -> usize {
        frame(this).view = v;
        0
    }
    fn ChangeViewType(this: usize, ev: usize, v: usize) -> usize {
        frame(this).view = v;
        let p = frame_part(this, ev);
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::VIEW_TYPE_CHANGED, &[p, ev, this]);
        }
        0
    }
    fn GetPresentation(this: usize, ev: usize) -> usize { frame(this).pres }
    fn SetPresentation(this: usize, ev: usize, p: usize) -> usize {
        frame(this).pres = p;
        0
    }
    fn ChangePresentation(this: usize, ev: usize, v: usize) -> usize {
        frame(this).pres = v;
        let p = frame_part(this, ev);
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::PRESENTATION_CHANGED, &[p, ev, this]);
        }
        0
    }
    fn FFacetAdded(this: usize, ev: usize, facet: usize) -> usize {
        frame(this).facets.push(facet);
        let p = frame_part(this, ev);
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::FACET_ADDED, &[p, ev, facet]);
        }
        0
    }
    fn FFacetRemoved(this: usize, ev: usize, facet: usize) -> usize {
        frame(this).facets.retain(|&f| f != facet);
        let p = frame_part(this, ev);
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::FACET_REMOVED, &[p, ev, facet]);
        }
        0
    }
    fn FCreateFacetIterator(this: usize, ev: usize) -> usize {
        let it = new_of(cd!(ODFrameFacetIteratorClassData), "ODFrameFacetIterator");
        if it != 0 {
            iter(it).items = frame(this).facets.clone();
        }
        it
    }
    fn FCreateShape(this: usize, ev: usize) -> usize { new_shape(Rect::default()) }
    fn AcquireFrameShape(this: usize, ev: usize, bias: usize) -> usize { new_shape(frame(this).frame_shape) }
    fn ChangeFrameShape(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        frame(this).frame_shape = rect_of(s);
        let p = frame_part(this, ev);
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::FRAME_SHAPE_CHANGED, &[p, ev, this]);
        }
        0
    }
    fn RequestFrameShape(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        // The containing part would negotiate; rs2 grants the request.
        frame(this).frame_shape = rect_of(s);
        acquire(s, ev)
    }
    fn AcquireUsedShape(this: usize, ev: usize, bias: usize) -> usize {
        let s = frame(this);
        new_shape(s.used_shape.unwrap_or(s.frame_shape))
    }
    fn ChangeUsedShape(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        frame(this).used_shape = if s == 0 { None } else { Some(rect_of(s)) };
        0
    }
    fn FCreateTransform(this: usize, ev: usize) -> usize { new_transform(Xf::default()) }
    fn AcquireInternalTransform(this: usize, ev: usize, bias: usize) -> usize { new_transform(frame(this).internal) }
    fn ChangeInternalTransform(this: usize, ev: usize, t: usize, bias: usize) -> usize {
        frame(this).internal = xf_of(t);
        0
    }
    fn GetContentExtent(this: usize, ev: usize, p: usize) -> usize {
        write(p, frame(this).extent);
        0
    }
    fn ChangeContentExtent(this: usize, ev: usize, p: usize) -> usize {
        if p != 0 {
            frame(this).extent = read(p);
        }
        0
    }
    fn IsDroppable(this: usize, ev: usize) -> usize { frame(this).droppable as usize }
    fn SetDroppable(this: usize, ev: usize, b: usize) -> usize {
        frame(this).droppable = b & 0xff != 0;
        0
    }
    fn IsDragging(this: usize, ev: usize) -> usize { frame(this).dragging as usize }
    fn SetDragging(this: usize, ev: usize, b: usize) -> usize {
        frame(this).dragging = b & 0xff != 0;
        0
    }
    fn ContentUpdated(this: usize, ev: usize, id: usize) -> usize { 0 }
    fn ChangeLinkStatus(this: usize, ev: usize, s: usize) -> usize {
        frame(this).link_status = s as u32;
        0
    }
    fn GetLinkStatus(this: usize, ev: usize) -> usize { frame(this).link_status as usize }
    fn EditInLink(this: usize, ev: usize) -> usize { 0 }
    fn FInvalidate(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        // Every facet's window area is invalidated in PM.
        for &f in &frame(this).facets {
            let w = facet(f).hwnd;
            if w != 0 {
                unsafe { crate::services::WinInvalidateRect(w, 0, 1) };
            }
        }
        0
    }
    fn FValidate(this: usize, ev: usize, s: usize, bias: usize) -> usize { 0 }
    fn InvalidateActiveBorder(this: usize, ev: usize) -> usize { 0 }
    fn DrawActiveBorder(this: usize, ev: usize) -> usize { 0 }
    fn FClose(this: usize, ev: usize) -> usize {
        let p = frame(this).part;
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_CLOSED, &[p, ev, this]);
        }
        0
    }
    fn FRemove(this: usize, ev: usize) -> usize {
        let p = frame(this).part;
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DISPLAY_FRAME_REMOVED, &[p, ev, this]);
        }
        let s = frame(this);
        let id = frame_id(this);
        storage::draft(s.draft).frames.remove(&id);
        0
    }
    fn InitFrame(this: usize, ev: usize, su: usize, containing: usize, shp: usize, bias: usize, part: usize, view: usize, pres: usize, sub: usize, overlaid: usize) -> usize {
        crate::obj::set_su(this, acquire(su, ev));
        let s = frame(this);
        s.containing = containing;
        s.part = part;
        s.view = view;
        s.pres = pres;
        s.subframe = sub & 0xff != 0;
        s.overlaid = overlaid & 0xff != 0;
        s.frame_shape = rect_of(shp);
        0
    }
    fn InitFrameFromStorage(this: usize, ev: usize, su: usize) -> usize {
        crate::obj::set_su(this, acquire(su, ev));
        0
    }
    fn FExternalize(this: usize, ev: usize) -> usize {
        let su = crate::obj::su_of(this);
        if su == 0 {
            return 0;
        }
        let s = frame(this);
        let part_id = if s.part != 0 {
            call(cd!(ODPersistentObjectClassData), crate::obj::ix::GET_ID, &[s.part, ev]) as u32
        } else {
            s.part_id
        };
        let cont_id = if s.containing != 0 { frame_id(s.containing) } else { s.containing_id };
        set_raw(su, &fprop(b"Part"), &part_id.to_le_bytes());
        set_raw(su, &fprop(b"ContainingFrame"), &(cont_id | 0x8000_0000 * (cont_id != 0) as u32).to_le_bytes());
        let r = s.frame_shape;
        let mut b = Vec::new();
        for v in [r.left, r.top, r.right, r.bottom] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        set_raw(su, &fprop(b"FrameShape"), &b);
        let fl = s.subframe as u32 | (s.overlaid as u32) << 1 | (s.frozen as u32) << 2;
        set_raw(su, &fprop(b"Flags"), &fl.to_le_bytes());
        let (v, p) = (type_name(s.view), type_name(s.pres));
        storage::set_value(su, &fprop(b"ViewType"), storage::TYPE_ISO_STR, &v);
        storage::set_value(su, &fprop(b"Presentation"), storage::TYPE_ISO_STR, &p);
        0
    }
    fn FRelease(this: usize, ev: usize) -> usize {
        // The draft keeps its frames until it closes.
        let c: &mut i32 = somsup::data(cd!(ODRefCntObjectCClassData), this);
        if *c > 0 {
            *c -= 1;
        }
        0
    }
    fn FSomUninit(this: usize) -> usize {
        drop_state::<FrameState>(cd!(ODFrameCClassData), this);
        call_parent(cd!(ODFrameCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static FRAME_M: &[(&str, M)] = &[
    ("AcquireContainingFrame", AcquireContainingFrame::t),
    ("SetContainingFrame", SetContainingFrame::t),
    ("AcquireWindow", FAcquireWindow::t),
    ("SetWindow", FSetWindow::t),
    ("GetFrameGroup", GetFrameGroup::t),
    ("SetFrameGroup", SetFrameGroup::t),
    ("GetSequenceNumber", GetSequenceNumber::t),
    ("ChangeSequenceNumber", ChangeSequenceNumber::t),
    ("IsRoot", IsRoot::t),
    ("IsSubframe", IsSubframe::t),
    ("SetSubframe", SetSubframe::t),
    ("IsOverlaid", IsOverlaid::t),
    ("IsFrozen", IsFrozen::t),
    ("SetFrozen", SetFrozen::t),
    ("DoesPropagateEvents", DoesPropagateEvents::t),
    ("SetPropagateEvents", SetPropagateEvents::t),
    ("IsInLimbo", IsInLimbo::t),
    ("SetInLimbo", SetInLimbo::t),
    ("AcquirePart", FAcquirePart::t),
    ("ChangePart", ChangePart::t),
    ("GetPartInfo", FGetPartInfo::t),
    ("SetPartInfo", FSetPartInfo::t),
    ("GetViewType", GetViewType::t),
    ("SetViewType", SetViewType::t),
    ("ChangeViewType", ChangeViewType::t),
    ("GetPresentation", GetPresentation::t),
    ("SetPresentation", SetPresentation::t),
    ("ChangePresentation", ChangePresentation::t),
    ("FacetAdded", FFacetAdded::t),
    ("FacetRemoved", FFacetRemoved::t),
    ("CreateFacetIterator", FCreateFacetIterator::t),
    ("CreateShape", FCreateShape::t),
    ("AcquireFrameShape", AcquireFrameShape::t),
    ("ChangeFrameShape", ChangeFrameShape::t),
    ("RequestFrameShape", RequestFrameShape::t),
    ("AcquireUsedShape", AcquireUsedShape::t),
    ("ChangeUsedShape", ChangeUsedShape::t),
    ("CreateTransform", FCreateTransform::t),
    ("AcquireInternalTransform", AcquireInternalTransform::t),
    ("ChangeInternalTransform", ChangeInternalTransform::t),
    ("GetContentExtent", GetContentExtent::t),
    ("ChangeContentExtent", ChangeContentExtent::t),
    ("IsDroppable", IsDroppable::t),
    ("SetDroppable", SetDroppable::t),
    ("IsDragging", IsDragging::t),
    ("SetDragging", SetDragging::t),
    ("ContentUpdated", ContentUpdated::t),
    ("ChangeLinkStatus", ChangeLinkStatus::t),
    ("GetLinkStatus", GetLinkStatus::t),
    ("EditInLink", EditInLink::t),
    ("Invalidate", FInvalidate::t),
    ("Validate", FValidate::t),
    ("InvalidateActiveBorder", InvalidateActiveBorder::t),
    ("DrawActiveBorder", DrawActiveBorder::t),
    ("Close", FClose::t),
    ("Remove", FRemove::t),
    ("InitFrame", InitFrame::t),
    ("InitFrameFromStorage", InitFrameFromStorage::t),
];
pub static FRAME_O: &[(&str, M)] = &[
    ("ODPersistentObject::Externalize", FExternalize::t),
    ("ODRefCntObject::Release", FRelease::t),
    ("SOMObject::somUninit", FSomUninit::t),
];

// ---- ODFacet ----

#[derive(Default)]
pub struct FacetState {
    pub frame: usize,
    pub containing: usize,
    pub children: Vec<usize>,
    pub clip: Option<Rect>,
    pub active: Option<Rect>,
    pub external: Xf,
    pub canvas: usize,
    pub window: usize,
    pub part_info: usize,
    pub highlight: u32,
    pub selected: bool,
    pub hwnd: u32,
}

pub fn facet(this: usize) -> &'static mut FacetState {
    state(cd!(ODFacetCClassData), this)
}

fn frame_internal(f: usize) -> Xf {
    if f == 0 { Xf::default() } else { frame(f).internal }
}

/// Frame coordinates to window coordinates.
pub fn window_frame_xf(fc: usize) -> Xf {
    let s = facet(fc);
    match s.containing {
        0 => s.external,
        c => s.external.then(&window_content_xf(c)),
    }
}

pub fn window_content_xf(fc: usize) -> Xf {
    frame_internal(facet(fc).frame).then(&window_frame_xf(fc))
}

pub fn new_facet(ev: usize, frm: usize, clip: usize, ext: usize, cnv: usize, containing: usize) -> usize {
    let f = new_of(cd!(ODFacetClassData), "ODFacet");
    if f == 0 {
        return 0;
    }
    let s = facet(f);
    s.frame = frm;
    s.clip = if clip == 0 { None } else { Some(rect_of(clip)) };
    s.external = xf_of(ext);
    s.containing = containing;
    s.canvas = cnv;
    if containing != 0 {
        let c = facet(containing);
        if s.canvas == 0 {
            s.canvas = c.canvas;
        }
        s.window = c.window;
        s.hwnd = c.hwnd;
        c.children.push(f);
    }
    f
}

methods! {
    fn GetFrame(this: usize, ev: usize) -> usize { facet(this).frame }
    fn CreateEmbeddedFacet(this: usize, ev: usize, frm: usize, clip: usize, ext: usize, cnv: usize, bias: usize, sibling: usize, posn: usize) -> usize {
        let f = new_facet(ev, frm, clip, ext, cnv, this);
        if f != 0 && frm != 0 {
            FFacetAdded(frm, ev, f);
        }
        f
    }
    fn RemoveFacet(this: usize, ev: usize, f: usize) -> usize {
        facet(this).children.retain(|&c| c != f);
        let fr = facet(f).frame;
        if fr != 0 {
            FFacetRemoved(fr, ev, f);
        }
        0
    }
    fn MoveBefore(this: usize, ev: usize, child: usize, sib: usize) -> usize { 0 }
    fn MoveBehind(this: usize, ev: usize, child: usize, sib: usize) -> usize { 0 }
    fn GetContainingFacet(this: usize, ev: usize) -> usize { facet(this).containing }
    fn CreateFacetIterator(this: usize, ev: usize, trav: usize, order: usize) -> usize {
        let it = new_of(cd!(ODFacetIteratorClassData), "ODFacetIterator");
        if it != 0 {
            // Top-down: this facet, then its descendants.
            let mut items = alloc::vec![this];
            let mut i = 0;
            while i < items.len() {
                let c = facet(items[i]).children.clone();
                items.extend(c);
                i += 1;
            }
            iter(it).items = items;
        }
        it
    }
    fn CreateShape(this: usize, ev: usize) -> usize { new_shape(Rect::default()) }
    fn CreateTransform(this: usize, ev: usize) -> usize { new_transform(Xf::default()) }
    fn ChangeGeometry(this: usize, ev: usize, clip: usize, ext: usize, bias: usize) -> usize {
        let s = facet(this);
        if clip != 0 {
            s.clip = Some(rect_of(clip));
        }
        if ext != 0 {
            s.external = xf_of(ext);
        }
        let fr = s.frame;
        let p = if fr != 0 { frame_part(fr, ev) } else { 0 };
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::GEOMETRY_CHANGED, &[p, ev, this, (clip != 0) as usize, (ext != 0) as usize]);
        }
        0
    }
    fn AcquireClipShape(this: usize, ev: usize, bias: usize) -> usize {
        let s = facet(this);
        new_shape(s.clip.unwrap_or_else(|| if s.frame != 0 { frame(s.frame).frame_shape } else { Rect::default() }))
    }
    fn GetPartInfo(this: usize, ev: usize) -> usize { facet(this).part_info }
    fn SetPartInfo(this: usize, ev: usize, i: usize) -> usize {
        facet(this).part_info = i;
        0
    }
    fn ChangeActiveShape(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        facet(this).active = if s == 0 { None } else { Some(rect_of(s)) };
        0
    }
    fn AcquireActiveShape(this: usize, ev: usize, bias: usize) -> usize {
        let s = facet(this);
        let r = s.active.or(s.clip).unwrap_or_else(|| if s.frame != 0 { frame(s.frame).frame_shape } else { Rect::default() });
        new_shape(r)
    }
    fn AcquireExternalTransform(this: usize, ev: usize, bias: usize) -> usize { new_transform(facet(this).external) }
    fn AcquireFrameTransform(this: usize, ev: usize, bias: usize) -> usize { new_transform(facet(this).external) }
    fn AcquireContentTransform(this: usize, ev: usize, bias: usize) -> usize {
        new_transform(frame_internal(facet(this).frame).then(&facet(this).external))
    }
    fn AcquireWindowFrameTransform(this: usize, ev: usize, bias: usize) -> usize { new_transform(window_frame_xf(this)) }
    fn AcquireWindowContentTransform(this: usize, ev: usize, bias: usize) -> usize { new_transform(window_content_xf(this)) }
    fn HasCanvas(this: usize, ev: usize) -> usize { (facet(this).canvas != 0) as usize }
    fn ChangeCanvas(this: usize, ev: usize, c: usize) -> usize {
        facet(this).canvas = c;
        0
    }
    fn GetCanvas(this: usize, ev: usize) -> usize {
        let mut f = this;
        while f != 0 {
            let s = facet(f);
            if s.canvas != 0 {
                return s.canvas;
            }
            f = s.containing;
        }
        0
    }
    fn ChangeHighlight(this: usize, ev: usize, h: usize) -> usize {
        facet(this).highlight = h as u32;
        0
    }
    fn GetWindow(this: usize, ev: usize) -> usize { facet(this).window }
    fn GetHighlight(this: usize, ev: usize) -> usize { facet(this).highlight as usize }
    fn SetWindow(this: usize, ev: usize, w: usize) -> usize {
        facet(this).window = w;
        0
    }
    fn DragEnter(this: usize, ev: usize, p: usize, info: usize, src: usize) -> usize { 0 }
    fn DragWithin(this: usize, ev: usize, p: usize, info: usize, src: usize) -> usize { 0 }
    fn DragLeave(this: usize, ev: usize, p: usize, src: usize) -> usize { 0 }
    fn Drop(this: usize, ev: usize, p: usize, info: usize, src: usize) -> usize { 0 }
    fn Update(this: usize, ev: usize, s: usize, bias: usize) -> usize { 0 }
    fn Invalidate(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        let w = facet(this).hwnd;
        if w != 0 {
            unsafe { crate::services::WinInvalidateRect(w, 0, 1) };
        }
        0
    }
    fn Validate(this: usize, ev: usize, s: usize, bias: usize) -> usize { 0 }
    fn Draw(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        let fr = facet(this).frame;
        let p = if fr != 0 { frame_part(fr, ev) } else { 0 };
        if p != 0 {
            call(cd!(ODPartClassData), crate::part::ix::DRAW, &[p, ev, this, s]);
        }
        0
    }
    fn DrawChildren(this: usize, ev: usize, s: usize, bias: usize) -> usize {
        for c in facet(this).children.clone() {
            Draw(c, ev, s, bias);
            DrawChildren(c, ev, s, bias);
        }
        1
    }
    fn DrawChildrenAlways(this: usize, ev: usize, s: usize, bias: usize) -> usize { DrawChildren(this, ev, s, bias) }
    fn DrawnIn(this: usize, ev: usize, s: usize, bias: usize) -> usize { 0 }
    fn FcInvalidateActiveBorder(this: usize, ev: usize) -> usize { 0 }
    fn FcDrawActiveBorder(this: usize, ev: usize) -> usize { 0 }
    fn FcContainsPoint(this: usize, ev: usize, p: usize, bias: usize) -> usize {
        if p == 0 {
            return 0;
        }
        let pt = window_frame_xf(this).invert(read(p));
        let s = facet(this);
        shown_rect(this).contains(pt) as usize
    }
    fn ActiveBorderContainsPoint(this: usize, ev: usize, p: usize, bias: usize) -> usize { 0 }
    fn IsSelected(this: usize, ev: usize) -> usize { facet(this).selected as usize }
    fn SetSelected(this: usize, ev: usize, b: usize) -> usize {
        facet(this).selected = b & 0xff != 0;
        0
    }
    fn GetFacetHWND(this: usize, ev: usize) -> usize { facet(this).hwnd as usize }
    fn AcquireAggregateClipShape(this: usize, ev: usize, bias: usize) -> usize {
        // The clip shape in the containing frame's coordinates, cut by the
        // containing facets'.
        let mut r = facet(this).external.rect(rect_of_clip(this));
        let mut f = facet(this).containing;
        while f != 0 {
            r = r.intersect(&facet(f).external.rect(rect_of_clip(f)));
            f = facet(f).containing;
        }
        new_shape(r)
    }
    fn AcquireWindowAggregateClipShape(this: usize, ev: usize, bias: usize) -> usize {
        let mut r = window_frame_xf(this).rect(rect_of_clip(this));
        let mut f = facet(this).containing;
        while f != 0 {
            r = r.intersect(&window_frame_xf(f).rect(rect_of_clip(f)));
            f = facet(f).containing;
        }
        new_shape(r)
    }
    fn InitFacet(this: usize, ev: usize, frm: usize, clip: usize, ext: usize, cnv: usize, bias: usize) -> usize {
        let s = facet(this);
        s.frame = frm;
        s.clip = if clip == 0 { None } else { Some(rect_of(clip)) };
        s.external = xf_of(ext);
        s.canvas = cnv;
        0
    }
    fn SetFacetHWND(this: usize, ev: usize, hwnd: usize) -> usize {
        facet(this).hwnd = hwnd as u32;
        0
    }
    fn FcSomUninit(this: usize) -> usize {
        drop_state::<FacetState>(cd!(ODFacetCClassData), this);
        call_parent(cd!(ODFacetCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

fn rect_of_clip(f: usize) -> Rect {
    shown_rect(f)
}

/// What a facet shows, in frame coordinates: its clip shape, else its
/// frame's shape placed at the frame's origin. IBM's OS/2 parts lay out
/// and draw from the frame origin whatever the shape's own origin is
/// (MMODPTS asks for frame shapes away from it), so hit-testing and the
/// active border follow what they draw.
pub fn shown_rect(f: usize) -> Rect {
    let s = facet(f);
    if let Some(c) = s.clip {
        return c;
    }
    if s.frame == 0 {
        return Rect::default();
    }
    let r = frame(s.frame).frame_shape;
    Rect { left: 0, bottom: 0, right: r.right - r.left, top: r.top - r.bottom }
}

pub static FACET_M: &[(&str, M)] = &[
    ("GetFrame", GetFrame::t),
    ("CreateEmbeddedFacet", CreateEmbeddedFacet::t),
    ("RemoveFacet", RemoveFacet::t),
    ("MoveBefore", MoveBefore::t),
    ("MoveBehind", MoveBehind::t),
    ("GetContainingFacet", GetContainingFacet::t),
    ("CreateFacetIterator", CreateFacetIterator::t),
    ("CreateShape", CreateShape::t),
    ("CreateTransform", CreateTransform::t),
    ("ChangeGeometry", ChangeGeometry::t),
    ("AcquireClipShape", AcquireClipShape::t),
    ("GetPartInfo", GetPartInfo::t),
    ("SetPartInfo", SetPartInfo::t),
    ("ChangeActiveShape", ChangeActiveShape::t),
    ("AcquireActiveShape", AcquireActiveShape::t),
    ("AcquireExternalTransform", AcquireExternalTransform::t),
    ("AcquireFrameTransform", AcquireFrameTransform::t),
    ("AcquireContentTransform", AcquireContentTransform::t),
    ("AcquireWindowFrameTransform", AcquireWindowFrameTransform::t),
    ("AcquireWindowContentTransform", AcquireWindowContentTransform::t),
    ("HasCanvas", HasCanvas::t),
    ("ChangeCanvas", ChangeCanvas::t),
    ("GetCanvas", GetCanvas::t),
    ("ChangeHighlight", ChangeHighlight::t),
    ("GetWindow", GetWindow::t),
    ("GetHighlight", GetHighlight::t),
    ("SetWindow", SetWindow::t),
    ("DragEnter", DragEnter::t),
    ("DragWithin", DragWithin::t),
    ("DragLeave", DragLeave::t),
    ("Drop", Drop::t),
    ("Update", Update::t),
    ("Invalidate", Invalidate::t),
    ("Validate", Validate::t),
    ("Draw", Draw::t),
    ("DrawChildren", DrawChildren::t),
    ("DrawChildrenAlways", DrawChildrenAlways::t),
    ("DrawnIn", DrawnIn::t),
    ("InvalidateActiveBorder", FcInvalidateActiveBorder::t),
    ("DrawActiveBorder", FcDrawActiveBorder::t),
    ("ContainsPoint", FcContainsPoint::t),
    ("ActiveBorderContainsPoint", ActiveBorderContainsPoint::t),
    ("ODFacet_reserved42", ret0),
    ("ODFacet_reserved43", ret0),
    ("IsSelected", IsSelected::t),
    ("SetSelected", SetSelected::t),
    ("GetFacetHWND", GetFacetHWND::t),
    ("AcquireAggregateClipShape", AcquireAggregateClipShape::t),
    ("AcquireWindowAggregateClipShape", AcquireWindowAggregateClipShape::t),
    ("InitFacet", InitFacet::t),
    ("SetFacetHWND", SetFacetHWND::t),
];
pub static FACET_O: &[(&str, M)] = &[("SOMObject::somUninit", FcSomUninit::t)];

pub mod facet_ix {
    pub const SET_WINDOW: usize = 26;
    pub const DRAW: usize = 34;
    pub const INIT_FACET: usize = 49;
    pub const SET_FACET_HWND: usize = 50;
}

// ---- Iterators over a list made when they are created ----

#[derive(Default)]
pub struct IterState {
    pub items: Vec<usize>,
    pub next: usize,
}

pub fn iter(this: usize) -> &'static mut IterState {
    state(cd!(ODFrameFacetIteratorCClassData), this)
}

pub fn fiter(this: usize) -> &'static mut IterState {
    state(cd!(ODFacetIteratorCClassData), this)
}

methods! {
    fn IFirst(this: usize, ev: usize) -> usize {
        let s = iter(this);
        s.next = 1;
        s.items.first().copied().unwrap_or(0)
    }
    fn INext(this: usize, ev: usize) -> usize {
        let s = iter(this);
        let r = s.items.get(s.next).copied().unwrap_or(0);
        s.next += 1;
        r
    }
    fn IIsNotComplete(this: usize, ev: usize) -> usize {
        let s = iter(this);
        (s.next <= s.items.len() && s.next > 0) as usize
    }
    fn ISomUninit(this: usize) -> usize {
        drop_state::<IterState>(cd!(ODFrameFacetIteratorCClassData), this);
        call_parent(cd!(ODFrameFacetIteratorCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    fn FIFirst(this: usize, ev: usize) -> usize {
        let s = fiter(this);
        s.next = 1;
        s.items.first().copied().unwrap_or(0)
    }
    fn FINext(this: usize, ev: usize) -> usize {
        let s = fiter(this);
        let r = s.items.get(s.next).copied().unwrap_or(0);
        s.next += 1;
        r
    }
    fn FISkipChildren(this: usize, ev: usize) -> usize { 0 }
    fn FIIsNotComplete(this: usize, ev: usize) -> usize {
        let s = fiter(this);
        (s.next <= s.items.len() && s.next > 0) as usize
    }
    fn FISomUninit(this: usize) -> usize {
        drop_state::<IterState>(cd!(ODFacetIteratorCClassData), this);
        call_parent(cd!(ODFacetIteratorCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static FRAME_FACET_ITER_M: &[(&str, M)] =
    &[("First", IFirst::t), ("Next", INext::t), ("IsNotComplete", IIsNotComplete::t)];
pub static FRAME_FACET_ITER_O: &[(&str, M)] = &[("SOMObject::somUninit", ISomUninit::t)];
pub static FACET_ITER_M: &[(&str, M)] =
    &[("First", FIFirst::t), ("Next", FINext::t), ("SkipChildren", FISkipChildren::t), ("IsNotComplete", FIIsNotComplete::t)];
pub static FACET_ITER_O: &[(&str, M)] = &[("SOMObject::somUninit", FISomUninit::t)];
